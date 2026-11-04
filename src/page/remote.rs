//! Fetches remote images for the page. `native::images` calls `fetch` on `spawn`'s small pool of worker threads. Each connection goes only to addresses checked here, so
//! a redirect to a name that resolves to a private address, or a DNS answer that changes between check and connect,
//! can't reach one.

use std::collections::{BTreeMap, VecDeque};
use std::io::Read;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use ureq::unversioned::resolver::{ResolvedSocketAddrs, Resolver};

const HOPS: u32 = 5;
const PER_HOST: usize = 6;
const MAX_BODY: u64 = 32 << 20;
const CACHE_BYTES: usize = 64 << 20;
const CHUNK: usize = 256 << 10;

fn private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_broadcast()
                || v.is_multicast()
                || matches!(v.octets(), [0, ..] | [100, 64..=127, ..] | [198, 18 | 19, ..] | [240..=255, ..])
        }
        IpAddr::V6(v) => {
            let s = v.segments();
            // An IPv4 address inside: IPv4-compatible ::a.b.c.d, NAT64 64:ff9b::a.b.c.d, and 6to4 2002:aabb:ccdd::.
            let v4 = |hi: u16, lo: u16| private_ip(std::net::Ipv4Addr::from((u32::from(hi) << 16) | u32::from(lo)).into());
            v.to_ipv4_mapped().is_some_and(|m| private_ip(m.into()))
                || (s[..6] == [0; 6] && v4(s[6], s[7]))
                || (s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] && v4(s[6], s[7]))
                || (s[0] == 0x2002 && v4(s[1], s[2]))
                || s[0] & 0xffc0 == 0xfec0
                || v.is_loopback()
                || v.is_unspecified()
                || v.is_multicast()
                || v.is_unique_local()
                || v.is_unicast_link_local()
        }
    }
}

const RESERVED: [&str; 8] = [".local", ".localhost", ".internal", ".lan", ".home", ".home.arpa", ".intranet", ".corp"];

fn private_name(host: &str) -> bool {
    !host.contains('.') || RESERVED.iter().any(|s| host.ends_with(s))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fail {
    /// The host is private, or resolves to a private address.
    Blocked,
    Failed,
}

type Lookup = fn(&str, u16) -> std::io::Result<Vec<SocketAddr>>;
type Allow = fn(SocketAddr) -> bool;

/// Resolves a host and returns its addresses only when every one is allowed. ureq connects only to what this
/// returns, on the first request and on each redirect, so the address checked is the address used.
#[derive(Debug)]
struct Checked {
    lookup: Lookup,
    allow: Allow,
}

#[derive(Debug)]
struct Blocked;

impl std::fmt::Display for Blocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("private host")
    }
}

impl std::error::Error for Blocked {}

impl Resolver for Checked {
    fn resolve(&self, uri: &ureq::http::Uri, _: &ureq::config::Config, _: ureq::unversioned::transport::NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let host = uri.host().unwrap_or_default().trim_start_matches('[').trim_end_matches(']').trim_end_matches('.').to_ascii_lowercase();
        let port = uri.port_u16().unwrap_or(if uri.scheme_str() == Some("http") { 80 } else { 443 });
        if host.parse::<IpAddr>().is_err() && private_name(&host) {
            return Err(ureq::Error::Other(Box::new(Blocked)));
        }
        let addrs = (self.lookup)(&host, port).map_err(|_| ureq::Error::HostNotFound)?;
        if addrs.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        if !addrs.iter().all(|a| (self.allow)(*a)) {
            return Err(ureq::Error::Other(Box::new(Blocked)));
        }
        let mut out = self.empty();
        for a in addrs.into_iter().take(16) {
            out.push(a);
        }
        Ok(out)
    }
}

fn system_lookup(host: &str, port: u16) -> std::io::Result<Vec<SocketAddr>> {
    Ok((host, port).to_socket_addrs()?.collect())
}

fn agent_with(lookup: Lookup, allow: Allow) -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
    let tls = TlsConfig::builder().provider(TlsProvider::NativeTls).root_certs(RootCerts::PlatformVerifier).build();
    let config = ureq::Agent::config_builder()
        // A proxy would be resolved instead of the image host.
        .proxy(None)
        .max_redirects(HOPS)
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(30)))
        // Wikimedia answers 429 to a user agent with no contact URL.
        .user_agent(concat!("mddog/", env!("CARGO_PKG_VERSION"), " (+https://github.com/qu8n/mddog)"))
        .tls_config(tls)
        .build();
    ureq::Agent::with_parts(config, ureq::unversioned::transport::DefaultConnector::default(), Checked { lookup, allow })
}

pub fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| agent_with(system_lookup, |a| !private_ip(a.ip())))
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

const WORKERS: usize = 16;

struct Job {
    host: String,
    stop: Arc<AtomicBool>,
    run: Box<dyn FnOnce() + Send>,
}

/// The queued fetches, and how many run on each host.
struct Pool {
    jobs: VecDeque<Job>,
    hosts: BTreeMap<String, usize>,
    workers: usize,
    idle: usize,
}

static POOL: Mutex<Pool> = Mutex::new(Pool { jobs: VecDeque::new(), hosts: BTreeMap::new(), workers: 0, idle: 0 });
static WAKE: Condvar = Condvar::new();

/// The host and port that `url` connects to, which `spawn` limits to `PER_HOST` fetches at once.
pub fn host(url: &str) -> String {
    url.split_once("://").map_or("", |(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or_default()).to_ascii_lowercase()
}

/// Runs `run` on one of `WORKERS` threads, oldest first, once fewer than `PER_HOST` jobs run on `host`. A job whose
/// `stop` is set while it waits is dropped. Returns false when no thread could start to run it.
pub fn spawn(host: String, stop: Arc<AtomicBool>, run: impl FnOnce() + Send + 'static) -> bool {
    let mut pool = lock(&POOL);
    pool.jobs.push_back(Job { host, stop, run: Box::new(run) });
    // A woken worker counts as idle until it takes the lock, so compare against the queue, not just `idle`.
    if pool.jobs.len() > pool.idle && pool.workers < WORKERS && std::thread::Builder::new().name("remote".into()).spawn(work).is_ok() {
        pool.workers += 1;
    }
    if pool.workers == 0 {
        pool.jobs.pop_back();
        return false;
    }
    WAKE.notify_one();
    true
}

fn work() {
    let mut pool = lock(&POOL);
    loop {
        let Pool { jobs, hosts, .. } = &mut *pool;
        jobs.retain(|j| !j.stop.load(Ordering::Relaxed));
        let Some(job) = jobs.iter().position(|j| hosts.get(&j.host).is_none_or(|&n| n < PER_HOST)).and_then(|i| jobs.remove(i)) else {
            pool.idle += 1;
            pool = WAKE.wait(pool).unwrap_or_else(PoisonError::into_inner);
            pool.idle -= 1;
            continue;
        };
        *hosts.entry(job.host.clone()).or_default() += 1;
        drop(pool);
        // A panic in one fetch must not take the worker, or its host's count, with it.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job.run));
        pool = lock(&POOL);
        if let Some(n) = pool.hosts.get_mut(&job.host) {
            *n -= 1;
            if *n == 0 {
                pool.hosts.remove(&job.host);
            }
        }
    }
}

struct Entry {
    url: String,
    mime: String,
    body: Arc<Vec<u8>>,
    until: Instant,
}

/// Whole responses, oldest first, so a reload or a reopen in the same process skips the network.
static CACHE: Mutex<Vec<Arc<Entry>>> = Mutex::new(Vec::new());

fn cached(url: &str) -> Option<Arc<Entry>> {
    let mut cache = lock(&CACHE);
    let now = Instant::now();
    cache.retain(|e| e.until > now);
    let i = cache.iter().position(|e| e.url == url)?;
    let e = cache.remove(i);
    cache.push(e.clone());
    Some(e)
}

/// How long to keep a response: its `max-age`, between 1 minute and 1 hour, else 5 minutes. A live reload
/// re-requests every image on each save, so even a `max-age=0` image is kept a minute; `no-store` is not kept.
fn keep_for(cache_control: Option<&str>) -> Option<Duration> {
    let cc = cache_control.unwrap_or_default().to_ascii_lowercase();
    if cc.contains("no-store") {
        return None;
    }
    let age = cc.split(',').find_map(|d| d.trim().strip_prefix("max-age=")?.parse::<u64>().ok()).unwrap_or(300);
    Some(Duration::from_secs(age).clamp(Duration::from_secs(60), KEEP_MAX))
}

fn store(entry: Arc<Entry>) {
    if entry.body.len() > CACHE_BYTES / 8 {
        return;
    }
    let mut cache = lock(&CACHE);
    cache.retain(|e| e.url != entry.url);
    cache.push(entry);
    let mut total: usize = cache.iter().map(|e| e.body.len()).sum();
    while total > CACHE_BYTES {
        total -= cache.remove(0).body.len();
    }
}

/// The disk cache, so a reopen in a new process skips the network too: one file per whole response, named by a
/// hash of its URL, holding a JSON line of the URL, type, and expiry, then the body. The sweep removes files past
/// the longest lifetime and keeps the folder under `DISK_BYTES`.
pub fn disk_dir() -> PathBuf {
    cache_dir().join("remote")
}

/// The cache root: `MDDOG_CACHE_DIR` when set, so tests keep out of the real one, else `~/Library/Caches/mddog`.
fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MDDOG_CACHE_DIR").filter(|d| !d.is_empty()) {
        return std::path::absolute(&dir).unwrap_or_else(|_| PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    home.join("Library/Caches/mddog")
}

/// The longest any response is kept; also the age at which the sweep removes a file.
pub const KEEP_MAX: Duration = Duration::from_secs(3600);
pub const DISK_BYTES: u64 = 64 << 20;

fn disk_path(dir: &Path, url: &str) -> PathBuf {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    url.hash(&mut h);
    dir.join(format!("{:016x}", h.finish()))
}

fn epoch() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn read_disk(dir: &Path, url: &str) -> Option<Arc<Entry>> {
    let mut bytes = std::fs::read(disk_path(dir, url)).ok()?;
    let line = bytes.iter().position(|&b| b == b'\n')?;
    let head: serde_json::Value = serde_json::from_slice(&bytes[..line]).ok()?;
    let left = head["until"].as_u64()?.checked_sub(epoch()).filter(|&s| s > 0)?;
    if head["url"].as_str() != Some(url) {
        return None;
    }
    let mime = head["mime"].as_str()?.to_owned();
    let body = bytes.split_off(line + 1);
    Some(Arc::new(Entry { url: url.to_owned(), mime, body: Arc::new(body), until: Instant::now() + Duration::from_secs(left) }))
}

/// Writes through a temp file and a rename, so a reader never sees half a file.
/// Each write gets its own temp file, since two workers can fetch the same URL at once.
fn write_disk(dir: &Path, e: &Entry, keep: Duration) {
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let path = disk_path(dir, &e.url);
    let tmp = path.with_extension(format!("tmp{}-{}", std::process::id(), WRITES.fetch_add(1, Ordering::Relaxed)));
    let head = serde_json::json!({ "url": e.url, "mime": e.mime, "until": epoch() + keep.as_secs() });
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let mut f = std::fs::File::create(&tmp)?;
        std::io::Write::write_all(&mut f, format!("{head}\n").as_bytes())?;
        std::io::Write::write_all(&mut f, &e.body)?;
        std::fs::rename(&tmp, &path)
    };
    if write().is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Images, and the generic binary types some servers send images as. The native engine draws nothing else.
fn image_type(mime: &str) -> bool {
    let m = mime.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    m.starts_with("image/") || m == "application/octet-stream" || m == "binary/octet-stream"
}

/// Fetches `url` (http or https) and returns its body. Returns `None`, early and silently, once `stop` is set.
pub fn fetch(agent: &ureq::Agent, url: &str, stop: &AtomicBool) -> Option<Result<Arc<Vec<u8>>, Fail>> {
    let disk = (!cfg!(test)).then(disk_dir);
    if let Some(e) = cached(url) {
        return Some(Ok(e.body.clone()));
    }
    if let Some(e) = disk.as_deref().and_then(|d| read_disk(d, url)) {
        let body = e.body.clone();
        store(e);
        return Some(Ok(body));
    }
    if stop.load(Ordering::Relaxed) {
        return None;
    }
    let resp = match agent.get(url).header("Accept", "image/*,*/*;q=0.8").call() {
        Ok(r) => r,
        Err(ureq::Error::Other(e)) if e.is::<Blocked>() => return Some(Err(Fail::Blocked)),
        Err(_) => return Some(Err(Fail::Failed)),
    };
    let header = |k: &str| resp.headers().get(k).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let mime = header("content-type").unwrap_or_default();
    // A 206 to a GET with no Range is a part that would be cached as the whole image.
    if resp.status().as_u16() != 200 || !image_type(&mime) {
        return Some(Err(Fail::Failed));
    }
    let keep = keep_for(header("cache-control").as_deref());
    let mut body = Vec::new();
    let mut reader = resp.into_body().into_with_config().limit(MAX_BODY).reader();
    let mut buf = vec![0; CHUNK];
    loop {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => body.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Some(Err(Fail::Failed)),
        }
    }
    let entry = Arc::new(Entry { url: url.to_owned(), mime, body: Arc::new(body), until: Instant::now() + keep.unwrap_or_default() });
    if let Some(keep) = keep {
        if let Some(d) = disk.as_deref().filter(|_| entry.body.len() as u64 <= DISK_BYTES / 8) {
            write_disk(d, &entry, keep);
        }
        store(entry.clone());
    }
    Some(Ok(entry.body.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicU16, AtomicUsize};

    #[test]
    fn private_hosts_are_refused() {
        for ip in ["10.1.2.3", "127.0.0.1", "169.254.1.1", "100.100.1.1", "0.1.2.3", "::1", "fd00::1", "fe80::1", "::ffff:192.168.1.1", "::127.0.0.1", "64:ff9b::a00:1", "2002:c0a8:101::1", "fec0::1"]
        {
            assert!(private_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "140.82.112.3", "2606:4700::1111", "64:ff9b::808:808", "2002:808:808::1"] {
            assert!(!private_ip(ip.parse().unwrap()), "{ip}");
        }
        assert!(private_name("router") && private_name("nas.home.arpa") && private_name("wiki.internal"));
        assert!(!private_name("img.shields.io"));
    }

    #[test]
    fn disk_cache_round_trip() {
        let dir = std::env::temp_dir().join(format!("mddog-remote-test-{}", std::process::id()));
        let e = Entry { url: "https://x.test/a.png".into(), mime: "image/png".into(), body: Arc::new(b"PNG".to_vec()), until: Instant::now() };
        write_disk(&dir, &e, Duration::from_secs(60));
        let back = read_disk(&dir, "https://x.test/a.png").unwrap();
        assert_eq!((back.mime.as_str(), back.body.as_slice()), ("image/png", &b"PNG"[..]));
        assert!(read_disk(&dir, "https://x.test/b.png").is_none());
        write_disk(&dir, &e, Duration::ZERO);
        assert!(read_disk(&dir, "https://x.test/a.png").is_none(), "expired");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn same_url_writes_never_mix() {
        let dir = std::env::temp_dir().join(format!("mddog-remote-race-{}", std::process::id()));
        let entry = |byte: u8, len: usize| Entry { url: "https://x.test/a.png".into(), mime: "image/png".into(), body: Arc::new(vec![byte; len]), until: Instant::now() };
        let (a, b) = (entry(b'a', 4 << 20), entry(b'b', 1 << 20));
        for _ in 0..20 {
            std::thread::scope(|s| {
                s.spawn(|| write_disk(&dir, &a, Duration::from_secs(60)));
                s.spawn(|| write_disk(&dir, &b, Duration::from_secs(60)));
            });
            let back = read_disk(&dir, "https://x.test/a.png").unwrap();
            assert!(back.body == a.body || back.body == b.body, "a mixed or cut body of {} bytes", back.body.len());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cache_lifetimes() {
        assert_eq!(keep_for(None), Some(Duration::from_secs(300)));
        assert_eq!(keep_for(Some("public, max-age=0")), Some(Duration::from_secs(60)));
        assert_eq!(keep_for(Some("max-age=86400")), Some(Duration::from_secs(3600)));
        assert_eq!(keep_for(Some("private, no-store")), None);
    }

    /// A one-thread HTTP server on 127.0.0.1 that answers each path from `routes` and counts requests.
    fn serve(routes: &'static [(&'static str, &'static str)]) -> (u16, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let count = hits.clone();
        std::thread::spawn(move || {
            for mut s in listener.incoming().flatten() {
                count.fetch_add(1, Ordering::SeqCst);
                let mut line = String::new();
                let mut r = BufReader::new(s.try_clone().unwrap());
                r.read_line(&mut line).unwrap();
                while r.read_line(&mut String::new()).is_ok_and(|n| n > 2) {}
                let path = line.split(' ').nth(1).unwrap_or_default().split('?').next().unwrap_or_default().to_owned();
                let reply = routes.iter().find(|(p, _)| *p == path).map_or("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n", |(_, r)| r);
                let _ = s.write_all(reply.replacen("\r\n", "\r\nConnection: close\r\n", 1).as_bytes());
            }
        });
        (port, hits)
    }

    // Only the test server's own port counts as public here, so 127.0.0.1 stands in for a public host.
    static PUBLIC: AtomicU16 = AtomicU16::new(0);
    static LOOKUPS: AtomicUsize = AtomicUsize::new(0);

    fn test_allow(a: SocketAddr) -> bool {
        a.ip() == IpAddr::from([127, 0, 0, 1]) && a.port() == PUBLIC.load(Ordering::SeqCst)
    }

    /// `img.test` resolves to the public server. `flip.test` answers public once and private after, as a
    /// rebinding DNS server would. Every other name resolves to 127.0.0.1, as `127.0.0.1.nip.io` does.
    #[expect(clippy::unnecessary_wraps, reason = "it stands in for the system lookup, which can fail")]
    fn test_lookup(host: &str, port: u16) -> std::io::Result<Vec<SocketAddr>> {
        let public = SocketAddr::from(([127, 0, 0, 1], port));
        Ok(match host {
            "flip.test" if LOOKUPS.fetch_add(1, Ordering::SeqCst) > 0 => vec![SocketAddr::from(([10, 0, 0, 1], port))],
            _ => vec![public],
        })
    }

    /// The body's length, or why the fetch failed.
    fn get(agent: &ureq::Agent, url: &str) -> Result<usize, Fail> {
        fetch(agent, url, &AtomicBool::new(false)).unwrap().map(|b| b.len())
    }

    #[test]
    fn redirects_and_rebinding_never_reach_a_private_address() {
        // The private server shares 127.0.0.1 with the public one, so it is refused by its port alone.
        let (private, private_hits) = serve(&[("/secret.png", "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 4\r\n\r\nPRIV")]);
        let routes: &'static [(&str, &str)] = Box::leak(Box::new([
            ("/a.png", "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 3\r\nCache-Control: no-store\r\n\r\nPNG"),
            ("/page", "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 2\r\n\r\nhi"),
            ("/hop.png", "HTTP/1.1 302 Found\r\nLocation: /a.png\r\nContent-Length: 0\r\n\r\n"),
            ("/to-name.png", Box::leak(format!("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1.nip.io:{private}/secret.png\r\nContent-Length: 0\r\n\r\n").into_boxed_str())),
            ("/to-local.png", Box::leak(format!("HTTP/1.1 302 Found\r\nLocation: http://printer.local:{private}/secret.png\r\nContent-Length: 0\r\n\r\n").into_boxed_str())),
            ("/to-ip.png", Box::leak(format!("HTTP/1.1 301 Moved\r\nLocation: http://[::ffff:127.0.0.1]:{private}/secret.png\r\nContent-Length: 0\r\n\r\n").into_boxed_str())),
            ("/loop.png", "HTTP/1.1 302 Found\r\nLocation: /loop.png\r\nContent-Length: 0\r\n\r\n"),
            ("/v.mp4", "HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: 10\r\n\r\n0123456789"),
            ("/part.png", "HTTP/1.1 206 Partial Content\r\nContent-Type: image/png\r\nContent-Range: bytes 0-2/10\r\nContent-Length: 3\r\n\r\nPNG"),
            ("/raw", "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: 3\r\nCache-Control: no-store\r\n\r\nPNG"),
        ]));
        let (port, _) = serve(routes);
        PUBLIC.store(port, Ordering::SeqCst);
        let agent = agent_with(test_lookup, test_allow);
        let url = |host: &str, path: &str| format!("http://{host}:{port}{path}");

        assert_eq!(get(&agent, &url("img.test", "/a.png")), Ok(3));
        assert_eq!(get(&agent, &url("img.test", "/hop.png")), Ok(3));
        for path in ["/to-name.png", "/to-local.png", "/to-ip.png"] {
            assert_eq!(get(&agent, &url("img.test", path)), Err(Fail::Blocked), "{path}");
        }
        assert_eq!(get(&agent, &format!("http://127.0.0.1.nip.io:{private}/secret.png")), Err(Fail::Blocked));
        assert_eq!(private_hits.load(Ordering::SeqCst), 0, "a request reached the private server");
        assert_eq!(get(&agent, &url("img.test", "/page")), Err(Fail::Failed));
        assert_eq!(get(&agent, &url("img.test", "/loop.png")), Err(Fail::Failed));
        // Only images are drawn, so only images, or bytes of no stated type, are read.
        assert_eq!(get(&agent, &url("img.test", "/v.mp4")), Err(Fail::Failed));
        assert_eq!(get(&agent, &url("img.test", "/raw")), Ok(3));
        assert_eq!(get(&agent, &url("img.test", "/part.png")), Err(Fail::Failed));

        // Rebinding: the one lookup is public, and the connection goes to that address. A later lookup is
        // private, so a fresh connection is refused rather than sent to it.
        let fresh = agent_with(test_lookup, test_allow);
        assert_eq!(get(&fresh, &format!("http://flip.test:{port}/a.png")), Ok(3));
        assert_eq!(LOOKUPS.load(Ordering::SeqCst), 1);
        let fresh = agent_with(test_lookup, test_allow);
        assert_eq!(get(&fresh, &format!("http://flip.test:{port}/hop.png")), Err(Fail::Blocked));
    }

    #[test]
    fn many_fetches_share_a_few_threads() {
        let (port, _) = serve(&[("/a.png", "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 3\r\nCache-Control: no-store\r\n\r\nPNG")]);
        static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
        let agent = AGENT.get_or_init(|| agent_with(test_lookup, |_| true));
        let (now, most) = (Arc::new(Mutex::new(BTreeMap::<String, usize>::new())), Arc::new(Mutex::new((0, 0))));
        let (tx, rx) = std::sync::mpsc::channel();
        // Stopped while it waits, so it never runs.
        let stopped = Arc::new(AtomicBool::new(false));
        for i in 0..301 {
            let url = format!("http://h{}.test:{port}/a.png?{i}", i % 10);
            let (now, most, tx) = (now.clone(), most.clone(), tx.clone());
            if i == 300 {
                stopped.store(true, Ordering::Relaxed);
            }
            let stop = if i == 300 { stopped.clone() } else { Arc::new(AtomicBool::new(false)) };
            assert!(spawn(host(&url), stop.clone(), move || {
                let h = host(&url);
                {
                    let mut now = lock(&now);
                    *now.entry(h.clone()).or_default() += 1;
                    let mut most = lock(&most);
                    *most = (most.0.max(now.values().sum()), most.1.max(now[&h]));
                }
                let ok = fetch(agent, &url, &stop).is_some_and(|r| r.is_ok());
                *lock(&now).get_mut(&h).unwrap() -= 1;
                tx.send(ok).unwrap();
            }));
        }
        drop(tx);
        let done: Vec<bool> = rx.iter().collect();
        assert_eq!((done.len(), done.iter().all(|&ok| ok)), (300, true));
        let (all, per_host) = *lock(&most);
        assert!(all <= WORKERS && per_host <= PER_HOST, "{all} at once, {per_host} on one host");
        assert!(lock(&POOL).workers <= WORKERS);
    }

    #[test]
    fn stop_ends_a_fetch_without_a_result() {
        let agent = agent_with(test_lookup, |_| true);
        assert!(fetch(&agent, "http://img.test:9/x.png", &AtomicBool::new(true)).is_none());
    }
}
