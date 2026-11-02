use std::cell::{Cell, RefCell};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::{Bitmap, CALL_LIMIT, Engine, snapshot_size};
use crate::term::kitty::{self, Shm};

const IDLE: Duration = Duration::from_secs(12 * 60 * 60);
/// How often an unused spare checks that a client can still reach it.
const CHECK: Duration = Duration::from_secs(3);
const HELLO: &[u8] = b"mddog";
const WARM_MD: &str =
    "```js\nx\n```\n```ts\nx\n```\n```tsx\nx\n```\n```python\nx\n```\n```rust\nx\n```\n```go\nx\n```\n```ruby\nx\n```\n```css\nx\n```\n```html\nx\n```\n```php\nx\n```\n```sh\nx\n```\n";

fn path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let meta = std::fs::metadata(&exe).ok()?;
    let mut h = DefaultHasher::new();
    // A spare serves only clients with its cache root, since it reads its runtime folder from there.
    (exe, meta.len(), meta.modified().ok(), crate::assets::cache_dir()).hash(&mut h);
    Some(std::env::temp_dir().join(format!("mddog-{}-{:016x}.sock", crate::sweep::uid(), h.finish())))
}

fn off() -> bool {
    std::env::var_os("MDDOG_NO_SPARE").is_some() || !crate::config::get().prewarm
}

fn write_frame(s: &mut UnixStream, b: &[u8]) -> io::Result<()> {
    s.write_all(&(b.len() as u32).to_le_bytes())?;
    s.write_all(b)
}

const MAX_FRAME: usize = 1 << 30;

fn read_frame(s: &mut UnixStream) -> io::Result<Vec<u8>> {
    let mut n = [0u8; 4];
    s.read_exact(&mut n)?;
    let n = u32::from_le_bytes(n) as usize;
    if n > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut b = vec![0u8; n];
    s.read_exact(&mut b)?;
    Ok(b)
}

const AGENT: &str = "dev.mddog.prewarm";

fn agent_plist() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    Ok(PathBuf::from(home).join(format!("Library/LaunchAgents/{AGENT}.plist")))
}

fn agent_target() -> String {
    format!("gui/{}", crate::sweep::uid())
}

pub fn prewarm_at_login() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("can't find the mdd executable: {e}"))?;
    let plist = agent_plist()?;
    let esc = |p: &Path| p.to_string_lossy().replace('&', "&amp;").replace('<', "&lt;");
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>{AGENT}</string><key>ProgramArguments</key><array><string>{}</string><string>--spare</string></array><key>EnvironmentVariables</key><dict><key>TMPDIR</key><string>{}</string>{}{}</dict><key>RunAtLoad</key><true/></dict></plist>\n",
        esc(&exe),
        esc(&std::env::temp_dir()),
        // The spare's socket name holds its cache root, so a moved root must reach it too.
        std::env::var_os("MDDOG_CACHE_DIR").map_or_else(String::new, |_| format!("<key>MDDOG_CACHE_DIR</key><string>{}</string>", esc(&crate::assets::cache_dir()))),
        // launchd doesn't read the shell's environment, so the spare would miss a config under XDG_CONFIG_HOME.
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .and_then(|v| std::path::absolute(v).ok())
            .map_or_else(String::new, |d| format!("<key>XDG_CONFIG_HOME</key><string>{}</string>", esc(&d)))
    );
    let dir = plist.parent().unwrap_or(Path::new("/"));
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), crate::term::io_error(&e)))?;
    std::fs::write(&plist, body).map_err(|e| format!("{}: {e}", plist.display()))?;
    let target = agent_target();
    let service = format!("{target}/{AGENT}");
    let _ = Command::new("launchctl").args(["bootout", &service]).stderr(Stdio::null()).status();
    // bootout returns while launchd is still tearing the job down, and a bootstrap then fails with error 5.
    for _ in 0..100 {
        let gone = !Command::new("launchctl").args(["print", &service]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
        if gone {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let ok = Command::new("launchctl").args(["bootstrap", &target]).arg(&plist).status().is_ok_and(|s| s.success());
    if ok { Ok(plist) } else { Err(format!("launchctl couldn't load {}", plist.display())) }
}

/// Stops the launchd job and removes its plist. Returns false when there was none.
pub fn no_prewarm_at_login() -> Result<bool, String> {
    let plist = agent_plist()?;
    let _ = Command::new("launchctl").args(["bootout", &format!("{}/{AGENT}", agent_target())]).stderr(Stdio::null()).status();
    match std::fs::remove_file(&plist) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("{}: {e}", plist.display())),
    }
}

pub fn spawn() {
    if off() || super::native() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let mut cmd = Command::new(exe);
    cmd.arg("--spare").current_dir("/").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    for k in ["MDDOG_TRACE", "MDDOG_DUMP_VIEW", "MDDOG_DUMP_TILES", "MDDOG_DEBUG"] {
        cmd.env_remove(k);
    }
    // The spare runs in `/`, so a relative root would resolve elsewhere there.
    if std::env::var_os("MDDOG_CACHE_DIR").is_some() {
        cmd.env("MDDOG_CACHE_DIR", crate::assets::cache_dir());
    }
    // SAFETY: the hook runs in the forked child, so it calls only setsid, fork, and _exit (async-signal-safe) and allocates nothing.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            match libc::fork() {
                -1 => Err(io::Error::last_os_error()),
                0 => Ok(()),
                _ => libc::_exit(0),
            }
        });
    }
    if let Ok(mut middle) = cmd.spawn() {
        let _ = middle.wait();
    }
}

pub fn serve() -> i32 {
    if off() {
        return 0;
    }
    let Some(path) = path() else { return 1 };
    // SAFETY: umask only sets the process file mode mask and cannot fail.
    unsafe { libc::umask(0o077) };
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(_) if UnixStream::connect(&path).is_ok() => return 0,
        Err(_) => {
            let _ = std::fs::remove_file(&path);
            match UnixListener::bind(&path) {
                Ok(l) => l,
                Err(_) => return 1,
            }
        }
    };
    // Another spare can replace a stale socket under this one, so remove only the socket bound here.
    let bound = std::fs::symlink_metadata(&path).map(|m| (m.dev(), m.ino())).ok();
    let remove = || remove_if(&path, bound);
    let (src_tx, src_rx) = mpsc::channel::<(String, PathBuf)>();
    let (doc_tx, doc_rx) = mpsc::channel();
    let _ = std::thread::Builder::new().stack_size(crate::render::STACK).spawn(move || {
        crate::render(WARM_MD);
        for (src, dir) in src_rx {
            if doc_tx.send(crate::render::markdown::render_in(&src, Some(&dir))).is_err() {
                break;
            }
        }
    });
    let mut engine = Engine::new(800.0);
    engine.load_blank();
    warm(&engine);
    if !engine.content_rules() {
        remove();
        return 1;
    }
    let deadline = Instant::now() + IDLE;
    let client = loop {
        let left = deadline.saturating_duration_since(Instant::now());
        // A rebuilt binary or moved socket means no client can reach this spare, and a newer spare serves them.
        if left.is_zero() || !reachable(&path, bound) {
            remove();
            return 0;
        }
        let mut fds = libc::pollfd { fd: listener.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        // SAFETY: fds is one valid pollfd, and the count passed is 1.
        match unsafe { libc::poll(&mut fds, 1, left.min(CHECK).as_millis() as i32) } {
            0 => continue,
            n if n < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted => continue,
            n if n < 0 => {
                remove();
                return 1;
            }
            _ => {}
        }
        let Ok((mut s, _)) = listener.accept() else { continue };
        if !same_user(&s) {
            continue;
        }
        let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
        if read_frame(&mut s).is_ok_and(|f| f == HELLO) {
            break s;
        }
    };
    remove();
    drop(listener);
    serve_on(&mut engine, client, |src, dir| {
        src_tx.send((src.to_string(), dir.to_path_buf())).ok()?;
        doc_rx.recv().ok()
    });
    0
}

/// Whether clients still find this spare: the binary is unchanged, so they hash the same `path`, and the socket there is still the one `id` names.
fn reachable(path: &Path, id: Option<(u64, u64)>) -> bool {
    self::path().as_deref() == Some(path) && is(path, id)
}

/// Whether the file at `path` is the one `id` (device, inode) names.
fn is(path: &Path, id: Option<(u64, u64)>) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| Some((m.dev(), m.ino())) == id)
}

/// Removes the file at `path` when it is still the one `id` (device, inode) names.
fn remove_if(path: &Path, id: Option<(u64, u64)>) {
    if is(path, id) {
        let _ = std::fs::remove_file(path);
    }
}

fn same_user(s: &UnixStream) -> bool {
    let (mut uid, mut gid) = (0, 0);
    // SAFETY: the socket fd is open for this call, and uid and gid are valid out-pointers.
    unsafe { libc::getpeereid(s.as_raw_fd(), &mut uid, &mut gid) == 0 && uid == libc::geteuid() }
}

pub fn serve_on(engine: &mut Engine, mut client: UnixStream, render: impl Fn(&str, &Path) -> Option<crate::Doc>) {
    let _ = client.set_read_timeout(None);
    let hello = json!({ "scale": engine.scale() });
    if write_frame(&mut client, hello.to_string().as_bytes()).is_err() {
        return;
    }
    while let Ok(req) = read_frame(&mut client) {
        let v: Value = serde_json::from_slice(&req).unwrap_or_default();
        let (s, n) = (|i: usize| v[i].as_str().unwrap_or(""), |i: usize| v[i].as_f64().unwrap_or(0.0));
        let mut reply = match s(0) {
            "eval" => match engine.eval(s(1)) {
                Ok(r) => json!({ "ok": r }),
                Err(e) => json!({ "err": e }),
            },
            "load" => {
                engine.load_file(Path::new(s(1)), v[2].as_bool().unwrap_or(true));
                json!({})
            }
            "resize" => {
                engine.resize(n(1), n(2));
                json!({})
            }
            "render" => match render(s(1), Path::new(s(2))) {
                Some(d) => json!({ "html": d.html, "math": d.math, "mermaid": d.mermaid }),
                None => json!({}),
            },
            "wait" => json!({ "ok": engine.wait_js(s(1), Duration::from_secs_f64(n(2))) }),
            "snap" => json!({ "ok": Shm::open(s(4)).and_then(|mut dst| engine.snapshot_into(n(1), n(2), n(3), &mut dst)).is_some() }),
            _ => json!({}),
        };
        reply["stalled"] = engine.stalled().into();
        if write_frame(&mut client, reply.to_string().as_bytes()).is_err() {
            break;
        }
    }
}

fn warm(engine: &Engine) {
    let theme = crate::config::theme();
    let runtime = crate::assets::runtime_dir();
    let opts = crate::page::Opts { theme, base: Path::new("/"), pad_x: 0.0, pad_y: 0.0, math: true, mermaid: true, engine: crate::config::mermaid(theme), wrap: false, runtime: Some(&runtime) };
    let html = crate::page::build("<p>$x$</p><section data-type=\"mermaid\"><pre lang=\"mermaid\">graph TD; a-->b</pre></section>", &opts);
    let file = crate::sweep::temp_path("mddog-", "-warm.html");
    if crate::page::load(engine, &file, &html, theme.dark, Duration::from_secs(5)).is_ok() {
        let _ = engine.eval(crate::page::script::START_MERMAID);
        engine.wait_js("document.fonts.status === 'loaded' && mddog.state.math === true && mddog.state.mermaid === true", Duration::from_secs(5));
    }
    let _ = std::fs::remove_file(&file);
}

pub struct Client {
    stream: RefCell<UnixStream>,
    pub width: Cell<f64>,
    pub stalled: Cell<bool>,
    pub scale: f64,
}

pub fn connect() -> Option<Client> {
    if std::env::var_os("MDDOG_NO_SPARE").is_some() {
        return None;
    }
    let path = path()?;
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if !crate::sweep::mine(&meta) || !meta.file_type().is_socket() {
        return None;
    }
    let mut s = UnixStream::connect(&path).ok()?;
    // The owner check above and the connect are two steps, so check who answered too.
    if !same_user(&s) {
        return None;
    }
    write_frame(&mut s, HELLO).ok()?;
    // A spare from before prewarm was turned off serves this client, finds it gone, and exits.
    if off() {
        return None;
    }
    Client::over(s)
}

impl Client {
    pub fn over(mut s: UnixStream) -> Option<Client> {
        s.set_read_timeout(Some(Duration::from_secs(1))).ok()?;
        let hello: Value = serde_json::from_slice(&read_frame(&mut s).ok()?).ok()?;
        Some(Client { stream: RefCell::new(s), width: Cell::new(800.0), stalled: Cell::new(false), scale: hello["scale"].as_f64()? })
    }
}

fn doc(v: Value) -> Option<crate::Doc> {
    // `v["html"].take()` would panic on a reply that is not an object.
    let Value::Object(mut m) = v else { return None };
    let Some(Value::String(html)) = m.remove("html") else { return None };
    Some(crate::Doc { html, math: m.get("math")?.as_bool()?, mermaid: m.get("mermaid")?.as_bool()? })
}

pub fn render(s: &mut UnixStream, src: &str, dir: &Path) -> Option<crate::Doc> {
    let reply = s.set_read_timeout(Some(CALL_LIMIT)).and_then(|()| write_frame(s, json!(["render", src, dir.to_string_lossy()]).to_string().as_bytes())).and_then(|()| read_frame(s));
    match reply {
        Ok(b) => doc(serde_json::from_slice(&b).ok()?),
        Err(_) => {
            let _ = s.shutdown(std::net::Shutdown::Both);
            None
        }
    }
}

impl Client {
    pub fn renderer(&self) -> Option<UnixStream> {
        self.stream.borrow().try_clone().ok()
    }

    pub fn render(&self, src: &str, dir: &Path) -> Option<crate::Doc> {
        doc(self.call(json!(["render", src, dir.to_string_lossy()]), Duration::ZERO)?)
    }

    fn call(&self, req: Value, wait: Duration) -> Option<Value> {
        self.call_within(req, wait + CALL_LIMIT + Duration::from_secs(1))
    }

    /// A reply later than `limit` leaves the stream out of step, so it marks the spare stalled.
    fn call_within(&self, req: Value, limit: Duration) -> Option<Value> {
        if self.stalled.get() {
            return None;
        }
        let mut s = self.stream.borrow_mut();
        let _ = s.set_read_timeout(Some(limit));
        let r = (|| {
            write_frame(&mut s, req.to_string().as_bytes()).ok()?;
            serde_json::from_slice::<Value>(&read_frame(&mut s).ok()?).ok()
        })();
        if r.as_ref().is_none_or(|v| v["stalled"] == true) {
            self.stalled.set(true);
        }
        r
    }

    pub fn eval(&self, js: &str) -> Result<String, String> {
        self.eval_within(js, CALL_LIMIT + Duration::from_secs(1))
    }

    pub fn eval_within(&self, js: &str, limit: Duration) -> Result<String, String> {
        let v = self.call_within(json!(["eval", js]), limit).ok_or("WebKit stopped responding")?;
        match v["ok"].as_str() {
            Some(r) => Ok(r.to_string()),
            None => Err(v["err"].as_str().unwrap_or("WebKit stopped responding").to_string()),
        }
    }

    pub fn load_file(&self, html: &Path, dark: bool) {
        self.call(json!(["load", html.to_string_lossy(), dark]), Duration::ZERO);
    }

    pub fn resize(&self, width: f64, height: f64) {
        self.width.set(width);
        self.call(json!(["resize", width, height]), Duration::ZERO);
    }

    pub fn wait_js(&self, js: &str, timeout: Duration) -> bool {
        self.call(json!(["wait", js, timeout.as_secs_f64()]), timeout).is_some_and(|v| v["ok"] == true)
    }

    pub fn snapshot(&self, y: f64, h: f64, scale: f64) -> Option<Bitmap> {
        let (width, height) = snapshot_size(self.width.get(), h, scale);
        let rgba = Shm::create(kitty::shm_name(), width * height * 4)?;
        (self.call(json!(["snap", y, h, scale, rgba.name()]), Duration::ZERO)?["ok"] == true).then_some(Bitmap { width, height, rgba })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_refuses_a_reply_that_is_not_a_page() {
        for v in [json!([]), json!("error"), json!(null), json!({"html": 1, "math": false, "mermaid": false})] {
            assert!(doc(v).is_none());
        }
        let page = doc(json!({"html": "<p>x</p>", "math": true, "mermaid": false})).unwrap();
        assert_eq!((page.html.as_str(), page.math, page.mermaid), ("<p>x</p>", true, false));
    }

    #[test]
    fn frames_round_trip_and_refuse_huge_lengths() {
        let (mut a, mut b) = UnixStream::pair().unwrap();
        write_frame(&mut a, b"hello").unwrap();
        assert_eq!(read_frame(&mut b).unwrap(), b"hello");
        a.write_all(&u32::MAX.to_le_bytes()).unwrap();
        assert_eq!(read_frame(&mut b).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert!(same_user(&a));
    }

    #[test]
    fn a_spare_removes_only_its_own_socket() {
        let path = crate::sweep::temp_path("mddog-test-", ".sock");
        let mine = UnixListener::bind(&path).unwrap();
        let id = std::fs::symlink_metadata(&path).map(|m| (m.dev(), m.ino())).ok();
        std::fs::remove_file(&path).unwrap();
        let theirs = UnixListener::bind(&path).unwrap();
        remove_if(&path, id);
        assert!(path.exists(), "removed another spare's socket");
        let id = std::fs::symlink_metadata(&path).map(|m| (m.dev(), m.ino())).ok();
        remove_if(&path, id);
        assert!(!path.exists());
        drop((mine, theirs));
    }
}
