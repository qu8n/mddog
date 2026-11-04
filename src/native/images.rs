//! Image loading: local files at once, remote files in a background thread through curl.
use crate::native::doc::{Block, Inl};
use crate::native::page::Bitmap;
use resvg::usvg;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, OnceLock};

pub static WAKE_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

/// Wake the main loop from another thread.
pub fn wake() {
    let fd = WAKE_FD.load(std::sync::atomic::Ordering::Relaxed);
    if fd >= 0 {
        // SAFETY: the buffer is a one-byte static, and a bad fd only makes write fail.
        unsafe {
            libc::write(fd, b"x".as_ptr().cast(), 1);
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
    tx: Sender<(String, Option<Arc<Vec<u8>>>)>,
    rx: Receiver<(String, Option<Arc<Vec<u8>>>)>,
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
        for f in [
            "/System/Library/Fonts/SFNS.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
            "/System/Library/Fonts/Menlo.ttc",
        ] {
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

/// The SF family's name in `fontdb`: macOS's system font file calls itself this.
const SYSTEM_FONT: &str = "System Font";

/// A theme's font list for SVG text: CSS's names for the system font become SF's family, which `fontdb` knows,
/// so diagram labels draw in SF as pele's did in WebKit, not in the Helvetica fallback.
/// The names after the system font never draw, so they are left out.
pub fn svg_families(fonts: &[String]) -> String {
    let mut out = Vec::new();
    for f in fonts {
        match f.to_ascii_lowercase().as_str() {
            "system-ui" | "-apple-system" | "blinkmacsystemfont" | "ui-sans-serif" | "sans-serif" | "sf pro" | "sf pro text" | "sf pro display" => {
                out.push(SYSTEM_FONT.to_string());
                break;
            }
            _ => out.push(f.clone()),
        }
    }
    out.join(", ")
}

pub fn svg_tree(data: &[u8]) -> Option<usvg::Tree> {
    let o = usvg::Options { fontdb: fontdb(), font_family: "Helvetica".into(), ..usvg::Options::default() };
    usvg::Tree::from_data(data, &o).ok()
}

pub fn render_svg(t: &usvg::Tree, w: u32, h: u32) -> Option<Bitmap> {
    let mut pm = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    let s = t.size();
    let tf = resvg::tiny_skia::Transform::from_scale(w as f32 / s.width(), h as f32 / s.height());
    resvg::render(t, tf, &mut pm.as_mut());
    Some(Bitmap { w: pm.width(), h: pm.height(), px: pm.take() })
}

/// A `w` by `h` window onto `t` drawn at `z` pixels per SVG unit, with SVG point (`x`, `y`) at its top left.
pub fn render_view(t: &usvg::Tree, w: u32, h: u32, z: f32, x: f32, y: f32) -> Option<Bitmap> {
    let mut pm = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    resvg::render(t, resvg::tiny_skia::Transform::from_row(z, 0.0, 0.0, z, -x * z, -y * z), &mut pm.as_mut());
    Some(Bitmap { w: pm.width(), h: pm.height(), px: pm.take() })
}

/// A decoded image before it is shared. `Rc` can't cross threads, so `prefetch`'s workers return this.
enum Decoded {
    Raster(Bitmap),
    Svg(Box<usvg::Tree>),
}

impl From<Decoded> for Img {
    fn from(d: Decoded) -> Img {
        match d {
            Decoded::Raster(b) => Img::Raster(Rc::new(b)),
            Decoded::Svg(t) => Img::Svg(Rc::from(t)),
        }
    }
}

pub fn decode(data: &[u8]) -> Option<Img> {
    decoded(data).map(Img::from)
}

fn decoded(data: &[u8]) -> Option<Decoded> {
    let head = &data[..data.len().min(512)];
    let is_svg = std::str::from_utf8(head).map(|s| s.contains("<svg")).unwrap_or(false) || String::from_utf8_lossy(head).contains("<svg");
    if is_svg {
        return svg_tree(data).map(|t| Decoded::Svg(Box::new(t)));
    }
    let im = image::load_from_memory(data).ok()?.to_rgba8();
    let (w, h) = im.dimensions();
    let mut px = im.into_raw();
    for p in px.as_chunks_mut::<4>().0 {
        let a = u32::from(p[3]);
        for c in &mut p[..3] {
            *c = ((u32::from(*c) * a + 127) / 255) as u8;
        }
    }
    Some(Decoded::Raster(Bitmap { w, h, px }))
}

fn remote(src: &str) -> bool {
    src.split_once("://").is_some_and(|(s, _)| s.eq_ignore_ascii_case("http") || s.eq_ignore_ascii_case("https"))
}

/// Confined to the repo, else home, else the folder; `/` starts at that root, and the query is dropped.
fn local(dir: &Path, src: &str) -> Option<Decoded> {
    let path = crate::render::markdown::confined_media(src, dir);
    path.filter(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file())).and_then(|p| std::fs::read(p).ok()).and_then(|d| decoded(&d))
}

/// The image sources layout will ask for: all but those in closed `<details>`, which it lays out only once opened.
pub fn sources(blocks: &[Block], out: &mut Vec<String>) {
    let inls = |is: &[Inl], out: &mut Vec<String>| {
        out.extend(is.iter().filter_map(|i| if let Inl::Img { src, .. } = i { Some(src.clone()) } else { None }));
    };
    for b in blocks {
        match b {
            Block::Para(is) | Block::Text(is) | Block::Heading(_, is, _) => inls(is, out),
            Block::Table { head, rows, .. } => head.iter().chain(rows.iter().flatten()).for_each(|c| inls(c, out)),
            Block::Details { summary, open, body, .. } => {
                inls(summary, out);
                if *open {
                    sources(body, out);
                }
            }
            Block::Quote(bs) | Block::Alert(_, bs) | Block::Align(_, bs) => sources(bs, out),
            Block::List { items, .. } => items.iter().for_each(|i| sources(&i.blocks, out)),
            Block::Footnotes(notes) => notes.iter().for_each(|n| sources(&n.2, out)),
            Block::Code { .. } | Block::Rule | Block::Math(_) | Block::Mermaid(_) => {}
        }
    }
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
        if remote(src) {
            self.map.insert(src.to_string(), St::Loading);
            wanted.push(src.to_string());
            return Status::Pending;
        }
        match local(&self.dir, src).map(Img::from) {
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

    /// Reads and decodes the local images in `srcs` on several threads, so layout finds them ready instead of
    /// decoding each in turn. A large photo takes tens of milliseconds to decode.
    pub fn prefetch(&mut self, mut srcs: Vec<String>) {
        srcs.sort_unstable();
        srcs.dedup();
        srcs.retain(|s| !remote(s) && !self.map.contains_key(s));
        if srcs.is_empty() {
            return;
        }
        let workers = std::thread::available_parallelism().map_or(4, usize::from).min(srcs.len());
        let next = AtomicUsize::new(0);
        let (dir, srcs_ref, next_ref) = (&self.dir, &srcs, &next);
        let done: Vec<(usize, Option<Decoded>)> = std::thread::scope(|s| {
            let work = move || {
                let mut got = vec![];
                loop {
                    let i = next_ref.fetch_add(1, Ordering::Relaxed);
                    let Some(src) = srcs_ref.get(i) else { break got };
                    got.push((i, local(dir, src)));
                }
            };
            // A worker that failed to start leaves its share to the others, and to layout if none started.
            let handles: Vec<_> = (0..workers).filter_map(|_| std::thread::Builder::new().stack_size(crate::render::STACK).spawn_scoped(s, work).ok()).collect();
            handles.into_iter().flat_map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).collect()
        });
        for (i, d) in done {
            self.map.insert(srcs[i].clone(), d.map_or(St::Failed, |d| St::Ready(d.into())));
        }
    }

    /// Start background fetches for the urls the layout asked for.
    /// Fetches through mddog's remote fetcher: its worker pool, cache, and private-address checks.
    pub fn fetch(&mut self, urls: Vec<String>) {
        use crate::page::remote;
        for u in urls {
            let tx = self.tx.clone();
            let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            self.stops.push(stop.clone());
            let (host, url) = (remote::host(&u), u.clone());
            let started = remote::spawn(host, stop.clone(), move || {
                let body = remote::fetch(remote::agent(), &url, &stop).and_then(Result::ok).filter(|b| !b.is_empty());
                let _ = tx.send((url, body));
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

#[cfg(test)]
mod tests {
    #[test]
    fn prefetch_takes_the_images_layout_will_draw() {
        let md = "![a](a.png)\n\n| ![b](b.png) |\n|---|\n\n> - ![c](c.svg)\n\n<details open><summary>s</summary>\n\n![d](d.png)\n\n</details>\n\n<details><summary>s</summary>\n\n![e](e.png)\n\n</details>\n\n![a](a.png) ![r](https://x.test/r.png)\n";
        let mut srcs = vec![];
        super::sources(&crate::native::doc::parse(md).blocks, &mut srcs);
        assert_eq!(srcs, ["a.png", "b.png", "c.svg", "d.png", "a.png", "https://x.test/r.png"]);

        let dir = std::env::temp_dir().join(format!("mddog-prefetch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("c.svg"), r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2"/>"#).unwrap();
        let mut images = super::Images::new(dir.clone());
        images.prefetch(srcs);
        // Ready or failed before layout asks, and the remote one is left to `fetch`.
        assert!(matches!(images.map.get("c.svg"), Some(super::St::Ready(i)) if i.size() == (4.0, 2.0)));
        assert!(matches!(images.map.get("a.png"), Some(super::St::Failed)));
        assert!(!images.map.contains_key("https://x.test/r.png"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_system_font_names_reach_sf() {
        let fonts = ["system-ui", "-apple-system", "Avenir Next"].map(String::from);
        assert_eq!(super::svg_families(&fonts), "System Font");
        let fonts = ["Avenir Next", "ui-sans-serif", "Inter"].map(String::from);
        assert_eq!(super::svg_families(&fonts), "Avenir Next, System Font");
        // SF is in the font set under that name, so a label in it doesn't fall back to Helvetica.
        assert!(super::fontdb().faces().any(|f| f.families.iter().any(|(n, _)| n == super::SYSTEM_FONT)));
    }
}
