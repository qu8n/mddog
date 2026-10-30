//! Fetches remote images and media for the web view, so WebKit never reaches the network for them. The page asks
//! for `mddog-https://…` or `mddog-http://…` (see `mddog.loadRemote`), and the scheme handler in `engine::webview`
//! calls `fetch` on `spawn`'s small pool of worker threads. Each connection goes only to addresses checked here, so
//! a redirect to a name that resolves to a private address, or a DNS answer that changes between check and connect,
//! can't reach one.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::Read;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use ureq::unversioned::resolver::{ResolvedSocketAddrs, Resolver};

use crate::engine::Engine;

/// The page asks for `PREFIX` + the remote URL, one scheme for each of http and https.
pub const PREFIX: &str = "mddog-";
pub const SCHEMES: [&str; 2] = ["mddog-http", "mddog-https"];
const HOPS: u32 = 5;
const PER_HOST: usize = 6;
const MAX_BODY: u64 = 32 << 20;
const CACHE_BYTES: usize = 64 << 20;
const CHUNK: usize = 256 << 10;
const BLOCK: u64 = 2 << 20;

/// The http(s) URL behind the page's `mddog-http(s)://` URL, without the fragment, which is never sent. READMEs
/// often add one, as in GitHub's `#gh-dark-mode-only`.
pub fn target(url: &str) -> &str {
    let url = url.strip_prefix(PREFIX).unwrap_or(url);
    url.split_once('#').map_or(url, |(u, _)| u)
}

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

pub enum Event {
    Head { status: u16, headers: Vec<(&'static str, String)> },
    Data(Vec<u8>),
    Done,
    Fail(Fail),
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
    /// Where `body` starts in the whole resource, and the resource's length.
    start: u64,
    total: u64,
    body: Arc<Vec<u8>>,
    until: Instant,
}

impl Entry {
    fn covers(&self, want: Option<(u64, Option<u64>)>) -> bool {
        let end = self.start + self.body.len() as u64;
        match want {
            None => self.start == 0 && end == self.total,
            // An open range (`bytes=a-`) takes what the block holds, as a server may answer one in parts.
            Some((a, b)) => self.start <= a && a < end && (end == self.total || b.is_none_or(|b| b < end)),
        }
    }
}

/// Whole responses and blocks of media, oldest first, so a reload or a reopen in the same process skips the
/// network, and a video's many small range requests are answered from one block.
static CACHE: Mutex<Vec<Arc<Entry>>> = Mutex::new(Vec::new());

fn cached(url: &str, want: Option<(u64, Option<u64>)>) -> Option<Arc<Entry>> {
    let mut cache = lock(&CACHE);
    let now = Instant::now();
    cache.retain(|e| e.until > now);
    let i = cache.iter().position(|e| e.url == url && e.covers(want))?;
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
    cache.retain(|e| e.url != entry.url || e.start != entry.start);
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
    crate::assets::cache_dir().join("remote")
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
    Some(Arc::new(Entry { url: url.to_owned(), mime, start: 0, total: body.len() as u64, body: Arc::new(body), until: Instant::now() + Duration::from_secs(left) }))
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

/// The first and last byte of a `Range: bytes=a-b` header, where `b` may be left out.
fn parse_range(range: &str) -> Option<(u64, Option<u64>)> {
    let (a, b) = range.trim().strip_prefix("bytes=")?.split_once('-')?;
    let a = a.trim().parse().ok()?;
    let b = if b.trim().is_empty() { None } else { Some(b.trim().parse().ok()?) };
    b.is_none_or(|b| b >= a).then_some((a, b))
}

/// Where a `Content-Range: bytes s-e/t` body starts, and the whole length when the server gives it.
fn parse_content_range(v: &str) -> Option<(u64, Option<u64>)> {
    let (span, total) = v.trim().strip_prefix("bytes ")?.split_once('/')?;
    Some((span.split_once('-')?.0.trim().parse().ok()?, total.trim().parse().ok()))
}

/// Answers the page's request from `e`: the whole body, or for a range the part of it asked for.
fn answer(e: &Entry, want: Option<(u64, Option<u64>)>, on: &mut dyn FnMut(Event)) {
    if let Some(m) = lock(&OUTCOMES).as_mut() {
        m.remove(&e.url);
    }
    let Some((a, b)) = want else {
        on(Event::Head { status: 200, headers: vec![("Content-Type", e.mime.clone()), ("Content-Length", e.body.len().to_string())] });
        on(Event::Data(e.body.to_vec()));
        return on(Event::Done);
    };
    let last = b.unwrap_or(u64::MAX).min(e.start + e.body.len() as u64 - 1);
    let part = e.body[(a - e.start) as usize..=(last - e.start) as usize].to_vec();
    let headers = vec![("Content-Type", e.mime.clone()), ("Content-Range", format!("bytes {a}-{last}/{}", e.total)), ("Content-Length", part.len().to_string()), ("Accept-Ranges", "bytes".into())];
    on(Event::Head { status: 206, headers });
    on(Event::Data(part));
    on(Event::Done);
}

static OUTCOMES: Mutex<Option<HashMap<String, Fail>>> = Mutex::new(None);

/// Why the last fetch of `url` failed, for the page's placeholder.
pub fn outcome(url: &str) -> Option<Fail> {
    lock(&OUTCOMES).as_ref()?.get(url).copied()
}

fn failed(url: &str, why: Fail) -> Event {
    let mut outcomes = lock(&OUTCOMES);
    let map = outcomes.get_or_insert_with(HashMap::new);
    if map.len() > 4096 {
        map.clear();
    }
    map.insert(url.to_owned(), why);
    Event::Fail(why)
}

fn media_type(mime: &str) -> bool {
    let m = mime.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
    ["image/", "video/", "audio/"].iter().any(|p| m.starts_with(p)) || m == "application/octet-stream" || m == "binary/octet-stream"
}

/// Answers from the memory cache alone, with no disk or network, so a hit never waits for a worker: a live reload
/// re-requests every image, and a video's range requests come from one block. Returns false on a miss.
pub fn answer_cached(url: &str, range: Option<&str>, on: &mut dyn FnMut(Event)) -> bool {
    let want = range.and_then(parse_range);
    cached(url, want).map(|e| answer(&e, want, on)).is_some()
}

/// Fetches `url` (http or https) and reports its response through `on`, ending with `Done` or `Fail`. Stops
/// early, silently, once `stop` is set. `range` is the page's `Range` header, which video playback sends; it
/// fetches a block of at least `BLOCK` bytes, so the next small ranges come from the cache.
pub fn fetch(agent: &ureq::Agent, url: &str, range: Option<&str>, stop: &AtomicBool, on: &mut dyn FnMut(Event)) {
    let want = range.and_then(parse_range);
    let disk = (!cfg!(test)).then(disk_dir);
    if let Some(e) = cached(url, want) {
        return answer(&e, want, on);
    }
    if let Some(e) = disk.as_deref().and_then(|d| read_disk(d, url)).filter(|e| e.covers(want)) {
        answer(&e, want, on);
        return store(e);
    }
    if stop.load(Ordering::Relaxed) {
        return;
    }
    let mut req = agent.get(url).header("Accept", "image/*,video/*,*/*;q=0.8");
    if let Some((a, b)) = want {
        let len = b.map_or(0, |b| (b - a).saturating_add(1)).clamp(BLOCK, MAX_BODY);
        req = req.header("Range", format!("bytes={a}-{}", a.saturating_add(len - 1)));
    }
    let resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::Other(e)) if e.is::<Blocked>() => return on(failed(url, Fail::Blocked)),
        Err(_) => return on(failed(url, Fail::Failed)),
    };
    let header = |k: &str| resp.headers().get(k).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let mime = header("content-type").unwrap_or_default();
    if !media_type(&mime) {
        return on(failed(url, Fail::Failed));
    }
    let (start, total) = if resp.status().as_u16() == 206 { header("content-range").as_deref().and_then(parse_content_range).unwrap_or((0, None)) } else { (0, None) };
    // A part with no stated length can't be told from the whole, so it is not kept.
    let keep = keep_for(header("cache-control").as_deref()).filter(|_| resp.status().as_u16() != 206 || total.is_some());
    let mut body = Vec::new();
    let mut reader = resp.into_body().into_with_config().limit(MAX_BODY).reader();
    let mut buf = vec![0; CHUNK];
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => body.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return on(failed(url, Fail::Failed)),
        }
    }
    let Some(end) = start.checked_add(body.len() as u64) else { return on(failed(url, Fail::Failed)) };
    let total = total.unwrap_or(end);
    let entry = Arc::new(Entry { url: url.to_owned(), mime, start, total, body: Arc::new(body), until: Instant::now() + keep.unwrap_or_default() });
    if !entry.covers(want) {
        return on(failed(url, Fail::Failed));
    }
    answer(&entry, want, on);
    if let Some(keep) = keep {
        if let Some(d) = disk.as_deref().filter(|_| entry.covers(None) && entry.body.len() as u64 <= DISK_BYTES / 8) {
            write_disk(d, &entry, keep);
        }
        store(entry);
    }
}

/// A WebKit content rule that blocks every http and https image and media load, so the page can reach remote
/// media only through the scheme handler, even if a remote URL got past `markdown::render_in`'s deferral.
pub fn content_rules() -> String {
    serde_json::json!([{ "trigger": { "url-filter": "^https?:", "resource-type": ["image", "media"] }, "action": { "type": "block" } }]).to_string()
}

/// Starts loading the page's remote images and media, each given up after `timeout`. Returns false when it has none.
pub fn load(engine: &Engine, timeout: Duration) -> bool {
    if engine.eval("String(document.querySelectorAll('img[data-mddog-src], video[data-mddog-poster], [data-mddog-media]').length)").is_ok_and(|n| n == "0") {
        return false;
    }
    let _ = engine.eval(&format!("mddog.loadRemote({})", timeout.as_millis()));
    true
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
    fn targets() {
        assert_eq!(target("mddog-https://x.test/a.svg#gh-dark-mode-only"), "https://x.test/a.svg");
        assert_eq!(target("mddog-http://x.test/a.png?s=1"), "http://x.test/a.png?s=1");
    }

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-1"), Some((0, Some(1))));
        assert_eq!(parse_range("bytes=5-"), Some((5, None)));
        assert_eq!(parse_range("bytes=5-1"), None);
        assert_eq!(parse_range("bytes=-5"), None);
        assert_eq!(parse_content_range("bytes 0-1/10"), Some((0, Some(10))));
        assert_eq!(parse_content_range("bytes 4-7/*"), Some((4, None)));
    }

    #[test]
    fn disk_cache_round_trip() {
        let dir = std::env::temp_dir().join(format!("mddog-remote-test-{}", std::process::id()));
        let e = Entry { url: "https://x.test/a.png".into(), mime: "image/png".into(), start: 0, total: 3, body: Arc::new(b"PNG".to_vec()), until: Instant::now() };
        write_disk(&dir, &e, Duration::from_secs(60));
        let back = read_disk(&dir, "https://x.test/a.png").unwrap();
        assert_eq!((back.mime.as_str(), back.body.as_slice(), back.covers(None)), ("image/png", &b"PNG"[..], true));
        assert!(read_disk(&dir, "https://x.test/b.png").is_none());
        write_disk(&dir, &e, Duration::ZERO);
        assert!(read_disk(&dir, "https://x.test/a.png").is_none(), "expired");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn same_url_writes_never_mix() {
        let dir = std::env::temp_dir().join(format!("mddog-remote-race-{}", std::process::id()));
        let entry = |byte: u8, len: usize| Entry { url: "https://x.test/a.png".into(), mime: "image/png".into(), start: 0, total: len as u64, body: Arc::new(vec![byte; len]), until: Instant::now() };
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

    fn get(agent: &ureq::Agent, url: &str) -> (Option<u16>, usize, Option<Fail>) {
        get_range(agent, url, None)
    }

    fn get_range(agent: &ureq::Agent, url: &str, range: Option<&str>) -> (Option<u16>, usize, Option<Fail>) {
        let (mut status, mut bytes, mut fail) = (None, 0, None);
        fetch(agent, url, range, &AtomicBool::new(false), &mut |e| match e {
            Event::Head { status: s, .. } => status = Some(s),
            Event::Data(d) => bytes += d.len(),
            Event::Done => {}
            Event::Fail(f) => fail = Some(f),
        });
        (status, bytes, fail)
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
        ]));
        let (port, hits) = serve(routes);
        PUBLIC.store(port, Ordering::SeqCst);
        let agent = agent_with(test_lookup, test_allow);
        let url = |host: &str, path: &str| format!("http://{host}:{port}{path}");

        assert_eq!(get(&agent, &url("img.test", "/a.png")), (Some(200), 3, None));
        assert_eq!(get(&agent, &url("img.test", "/hop.png")), (Some(200), 3, None));
        for path in ["/to-name.png", "/to-local.png", "/to-ip.png"] {
            assert_eq!(get(&agent, &url("img.test", path)), (None, 0, Some(Fail::Blocked)), "{path}");
            assert_eq!(outcome(&url("img.test", path)), Some(Fail::Blocked));
        }
        assert_eq!(get(&agent, &format!("http://127.0.0.1.nip.io:{private}/secret.png")), (None, 0, Some(Fail::Blocked)));
        assert_eq!(private_hits.load(Ordering::SeqCst), 0, "a request reached the private server");
        assert_eq!(get(&agent, &url("img.test", "/page")), (None, 0, Some(Fail::Failed)));
        assert_eq!(get(&agent, &url("img.test", "/loop.png")), (None, 0, Some(Fail::Failed)));

        // The first range fetches a block, and the next ranges in it come from the cache.
        let before = hits.load(Ordering::SeqCst);
        assert_eq!(get_range(&agent, &url("img.test", "/v.mp4"), Some("bytes=0-1")), (Some(206), 2, None));
        assert_eq!(get_range(&agent, &url("img.test", "/v.mp4"), Some("bytes=4-")), (Some(206), 6, None));
        assert_eq!(get_range(&agent, &url("img.test", "/v.mp4"), Some("bytes=9-20")), (Some(206), 1, None));
        assert_eq!(get_range(&agent, &url("img.test", "/v.mp4"), Some("bytes=10-11")), (None, 0, Some(Fail::Failed)));
        assert_eq!(hits.load(Ordering::SeqCst), before + 2);

        // Rebinding: the one lookup is public, and the connection goes to that address. A later lookup is
        // private, so a fresh connection is refused rather than sent to it.
        let fresh = agent_with(test_lookup, test_allow);
        assert_eq!(get(&fresh, &format!("http://flip.test:{port}/a.png")), (Some(200), 3, None));
        assert_eq!(LOOKUPS.load(Ordering::SeqCst), 1);
        let fresh = agent_with(test_lookup, test_allow);
        assert_eq!(get(&fresh, &format!("http://flip.test:{port}/hop.png")), (None, 0, Some(Fail::Blocked)));
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
                let mut ok = false;
                fetch(agent, &url, None, &stop, &mut |e| ok |= matches!(e, Event::Done));
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
    fn stop_ends_a_fetch_without_events() {
        let agent = agent_with(test_lookup, |_| true);
        let mut events = 0;
        fetch(&agent, "http://img.test:9/x.png", None, &AtomicBool::new(true), &mut |_| events += 1);
        assert_eq!(events, 0);
    }
}
