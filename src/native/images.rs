//! Image loading: local files at once, remote files in a background thread through curl.
use crate::native::page::Bitmap;
use resvg::usvg;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, OnceLock};

pub static WAKE_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

/// Wake the main loop from another thread.
pub fn wake() {
    let fd = WAKE_FD.load(std::sync::atomic::Ordering::Relaxed);
    if fd >= 0 {
        unsafe {
            libc::write(fd, b"x".as_ptr() as *const _, 1);
        }
    }
}

#[derive(Clone)]
pub enum Img {
    Raster(Rc<Bitmap>),
    Svg(Rc<usvg::Tree>),
}

impl Img {
    pub fn size(&self) -> (f32, f32) {
        match self {
            Img::Raster(b) => (b.w as f32, b.h as f32),
            Img::Svg(t) => (t.size().width(), t.size().height()),
        }
    }
}

pub enum Status {
    Pending,
    Failed,
    Ready(Img),
}

enum St {
    Loading,
    Failed,
    Ready(Img),
}

pub struct Images {
    pub dir: PathBuf,
    map: HashMap<String, St>,
    tx: Sender<(String, Option<Vec<u8>>)>,
    rx: Receiver<(String, Option<Vec<u8>>)>,
    /// Stops this page's queued and running fetches when a reload replaces it.
    stops: Vec<Arc<std::sync::atomic::AtomicBool>>,
}

impl Drop for Images {
    fn drop(&mut self) {
        for s in &self.stops {
            s.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

fn fontdb() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        for f in ["/System/Library/Fonts/Helvetica.ttc", "/System/Library/Fonts/Supplemental/Arial.ttf", "/System/Library/Fonts/Supplemental/Arial Bold.ttf", "/System/Library/Fonts/Menlo.ttc"] {
            if let Ok(d) = std::fs::read(f) {
                db.load_font_data(d);
            }
        }
        db.set_sans_serif_family("Helvetica");
        db.set_serif_family("Helvetica");
        db.set_monospace_family("Menlo");
        Arc::new(db)
    })
    .clone()
}

pub fn svg_tree(data: &[u8]) -> Option<usvg::Tree> {
    let mut o = usvg::Options::default();
    o.fontdb = fontdb();
    o.font_family = "Helvetica".into();
    usvg::Tree::from_data(data, &o).ok()
}

pub fn render_svg(t: &usvg::Tree, w: u32, h: u32) -> Option<Bitmap> {
    let mut pm = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    let s = t.size();
    let tf = resvg::tiny_skia::Transform::from_scale(w as f32 / s.width(), h as f32 / s.height());
    resvg::render(t, tf, &mut pm.as_mut());
    Some(Bitmap { w: pm.width(), h: pm.height(), px: pm.take() })
}

pub fn decode(data: &[u8]) -> Option<Img> {
    let head = &data[..data.len().min(512)];
    let is_svg = std::str::from_utf8(head).map(|s| s.contains("<svg")).unwrap_or(false) || String::from_utf8_lossy(head).contains("<svg");
    if is_svg {
        return svg_tree(data).map(|t| Img::Svg(Rc::new(t)));
    }
    let im = image::load_from_memory(data).ok()?.to_rgba8();
    let (w, h) = im.dimensions();
    let mut px = im.into_raw();
    for p in px.chunks_exact_mut(4) {
        let a = p[3] as u32;
        for c in 0..3 {
            p[c] = ((p[c] as u32 * a + 127) / 255) as u8;
        }
    }
    Some(Img::Raster(Rc::new(Bitmap { w, h, px })))
}

impl Images {
    pub fn new(dir: PathBuf) -> Images {
        let (tx, rx) = channel();
        Images { dir, map: HashMap::new(), tx, rx, stops: vec![] }
    }

    pub fn get(&mut self, src: &str, wanted: &mut Vec<String>) -> Status {
        if let Some(s) = self.map.get(src) {
            return match s {
                St::Loading => Status::Pending,
                St::Failed => Status::Failed,
                St::Ready(i) => Status::Ready(i.clone()),
            };
        }
        let scheme = src.split_once("://").map(|(s, _)| s.to_ascii_lowercase());
        if matches!(scheme.as_deref(), Some("http" | "https")) {
            self.map.insert(src.to_string(), St::Loading);
            wanted.push(src.to_string());
            return Status::Pending;
        }
        // Confined to the repo, else home, else the folder; `/` starts at that root, and the query is dropped.
        let path = crate::render::markdown::confined_media(src, &self.dir);
        let r = path.filter(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file())).and_then(|p| std::fs::read(p).ok()).and_then(|d| decode(&d));
        match r {
            Some(i) => {
                self.map.insert(src.to_string(), St::Ready(i.clone()));
                Status::Ready(i)
            }
            None => {
                self.map.insert(src.to_string(), St::Failed);
                Status::Failed
            }
        }
    }

    /// Start background fetches for the urls the layout asked for.
    /// Fetches through mddog's remote fetcher: its worker pool, cache, and private-address checks.
    pub fn fetch(&mut self, urls: Vec<String>) {
        use crate::page::remote::{self, Event};
        for u in urls {
            let tx = self.tx.clone();
            let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            self.stops.push(stop.clone());
            let (host, url) = (remote::host(&u), u.clone());
            let started = remote::spawn(host, stop.clone(), move || {
                let mut body = Vec::new();
                let mut ok = false;
                remote::fetch(remote::agent(), &url, None, &stop, &mut |e| match e {
                    Event::Data(d) => body.extend_from_slice(&d),
                    Event::Done => ok = true,
                    _ => {}
                });
                let _ = tx.send((url, (ok && !body.is_empty()).then_some(body)));
                wake();
            });
            if !started {
                let _ = self.tx.send((u, None));
            }
        }
    }

    /// How many remote images are still loading.
    pub fn loading(&self) -> usize {
        self.map.values().filter(|s| matches!(s, St::Loading)).count()
    }

    /// Take finished fetches. True when any image changed.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok((u, d)) = self.rx.try_recv() {
            // a failed fetch looks the same as a pending one: no relayout
            match d.and_then(|d| decode(&d)) {
                Some(i) => {
                    self.map.insert(u, St::Ready(i));
                    any = true;
                }
                None => {
                    self.map.insert(u, St::Failed);
                }
            }
        }
        any
    }
}
