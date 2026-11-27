//! Image loading: local files at once, remote files on `remote`'s worker threads.
use crate::render::doc::{Block, Inline};
use crate::render::media::is_remote;
use crate::render::page::Bitmap;
use resvg::usvg;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

#[derive(Clone)]
pub enum Image {
    Raster(Rc<Bitmap>),
    Svg(Rc<usvg::Tree>),
}

impl Image {
    pub fn size(&self) -> (f32, f32) {
        match self {
            Image::Raster(b) => (b.w as f32, b.h as f32),
            Image::Svg(t) => (t.size().width(), t.size().height()),
        }
    }
}

pub enum Status {
    Pending,
    Failed,
    Ready(Image),
}

enum LoadState {
    Loading,
    Failed,
    Ready(Image),
}

pub struct Images {
    pub dir: PathBuf,
    map: HashMap<String, LoadState>,
    tx: Sender<(String, Option<Arc<Vec<u8>>>)>,
    rx: Receiver<(String, Option<Arc<Vec<u8>>>)>,
    /// Stops this page's queued and running fetches when a reload replaces it.
    stops: Vec<Arc<std::sync::atomic::AtomicBool>>,
    /// See `Engine::set_offline`.
    pub offline: bool,
    /// Local images bigger than this many bytes show their alt text.
    pub max_local: Option<u64>,
    /// Decoded bytes left for raster images. Past it, images show their alt text.
    pub budget: Option<u64>,
    fitted: HashMap<String, Vec<(u32, u32, Rc<Bitmap>)>>,
}

impl Drop for Images {
    fn drop(&mut self) {
        for s in &self.stops {
            s.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

/// A decoded image before it is shared. `Rc` can't cross threads, so `prefetch`'s workers return this.
enum Decoded {
    Raster(Bitmap),
    Svg(Box<usvg::Tree>),
}

impl From<Decoded> for Image {
    fn from(d: Decoded) -> Image {
        match d {
            Decoded::Raster(b) => Image::Raster(Rc::new(b)),
            Decoded::Svg(t) => Image::Svg(Rc::from(t)),
        }
    }
}

pub fn decode(data: &[u8]) -> Option<Image> {
    decoded(data, false).map(Image::from)
}

/// `capped` limits the decoded size, for a preview nobody asked to load.
fn decoded(data: &[u8], capped: bool) -> Option<Decoded> {
    let head = &data[..data.len().min(512)];
    let is_svg = std::str::from_utf8(head).map(|s| s.contains("<svg")).unwrap_or(false) || String::from_utf8_lossy(head).contains("<svg");
    if is_svg {
        if capped && data.len() > 1 << 20 {
            return None;
        }
        return crate::render::svg::svg_tree(data).map(|t| Decoded::Svg(Box::new(t)));
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format().ok()?;
    if capped {
        let mut limits = image::Limits::default();
        (limits.max_image_width, limits.max_image_height, limits.max_alloc) = (Some(8192), Some(8192), Some(128 << 20));
        reader.limits(limits);
    }
    let im = reader.decode().ok()?.to_rgba8();
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

/// Confined to the repo, else home, else the folder; `/` starts at that root, and the query is dropped.
fn local(dir: &Path, src: &str, max: Option<u64>) -> Option<Decoded> {
    let path = crate::render::media::confined_media(src, dir);
    let fits = |m: std::fs::Metadata| m.is_file() && max.is_none_or(|max| m.len() <= max);
    path.filter(|p| std::fs::metadata(p).is_ok_and(fits)).and_then(|p| std::fs::read(p).ok()).and_then(|d| decoded(&d, max.is_some()))
}

/// The image sources layout will ask for: all but those in closed `<details>`, which it lays out only once opened.
pub fn sources(blocks: &[Block], out: &mut Vec<String>) {
    fn imgs(is: &[Inline], out: &mut Vec<String>) {
        for i in is {
            match i {
                Inline::Img { src, .. } => out.push(src.clone()),
                Inline::Ruby { base, rt } => {
                    imgs(base, out);
                    imgs(rt, out);
                }
                _ => {}
            }
        }
    }
    let mut inls = |is: &[Inline]| imgs(is, out);
    crate::render::doc::walk_shown(blocks, &mut |b| match b {
        Block::Para(is) | Block::Text(is) | Block::Heading(_, is, _) | Block::Details { summary: is, .. } => inls(is),
        Block::Table { head, rows, .. } => head.iter().chain(rows.iter().flatten()).for_each(|c| inls(c)),
        _ => {}
    });
}

impl Images {
    pub fn new(dir: PathBuf) -> Images {
        let (tx, rx) = channel();
        Images { dir, map: HashMap::new(), tx, rx, stops: vec![], offline: false, max_local: None, budget: None, fitted: HashMap::new() }
    }

    pub fn fitted(&mut self, src: &str, b: &Rc<Bitmap>, w: f32, h: f32) -> Rc<Bitmap> {
        if cfg!(target_os = "macos") || (b.w as f32 <= w * 1.25 && b.h as f32 <= h * 1.25) {
            return b.clone();
        }
        let (w, h) = (w.round().max(1.0) as u32, h.round().max(1.0) as u32);
        let sizes = self.fitted.entry(src.to_string()).or_default();
        if let Some((_, _, f)) = sizes.iter().find(|(fw, fh, _)| (*fw, *fh) == (w, h)) {
            return f.clone();
        }
        let Some(view) = image::ImageBuffer::<image::Rgba<u8>, &[u8]>::from_raw(b.w, b.h, &b.px) else { return b.clone() };
        let px = image::imageops::thumbnail(&view, w, h).into_raw();
        let f = Rc::new(Bitmap { w, h, px });
        if sizes.len() == 2 {
            sizes.remove(0);
        }
        sizes.push((w, h, f.clone()));
        f
    }

    pub fn get(&mut self, src: &str, wanted: &mut Vec<String>) -> Status {
        if let Some(s) = self.map.get(src) {
            return match s {
                LoadState::Loading => Status::Pending,
                LoadState::Failed => Status::Failed,
                LoadState::Ready(i) => Status::Ready(i.clone()),
            };
        }
        if is_remote(src) && self.offline {
            let d = crate::render::remote::cached_only(src).and_then(|d| decoded(&d, self.max_local.is_some()));
            let state = self.spend(d).map_or(LoadState::Failed, |d| LoadState::Ready(d.into()));
            self.map.insert(src.to_string(), state);
            return self.get(src, wanted);
        }
        if is_remote(src) {
            self.map.insert(src.to_string(), LoadState::Loading);
            wanted.push(src.to_string());
            return Status::Pending;
        }
        let d = local(&self.dir, src, self.max_local);
        match self.spend(d).map(Image::from) {
            Some(i) => {
                self.map.insert(src.to_string(), LoadState::Ready(i.clone()));
                Status::Ready(i)
            }
            None => {
                self.map.insert(src.to_string(), LoadState::Failed);
                Status::Failed
            }
        }
    }

    /// Reads and decodes the local images in `srcs` on several threads, so layout finds them ready instead of
    /// decoding each in turn. A large photo takes tens of milliseconds to decode.
    pub fn prefetch(&mut self, mut srcs: Vec<String>) {
        srcs.sort_unstable();
        srcs.dedup();
        srcs.retain(|s| !is_remote(s) && !self.map.contains_key(s));
        if srcs.is_empty() {
            return;
        }
        let cap = if self.max_local.is_some() { 2 } else { usize::MAX };
        let workers = std::thread::available_parallelism().map_or(4, usize::from).min(srcs.len()).min(cap);
        let next = AtomicUsize::new(0);
        let left = std::sync::atomic::AtomicU64::new(self.budget.unwrap_or(u64::MAX));
        let (dir, srcs_ref, next_ref, max, left_ref) = (&self.dir, &srcs, &next, self.max_local, &left);
        let done: Vec<(usize, Option<Decoded>)> = std::thread::scope(|s| {
            let work = move || {
                let mut got = vec![];
                loop {
                    let i = next_ref.fetch_add(1, Ordering::Relaxed);
                    let Some(src) = srcs_ref.get(i) else { break got };
                    let d = local(dir, src, max).filter(|d| {
                        let cost = if let Decoded::Raster(b) = d { b.px.len() as u64 } else { 0 };
                        left_ref.try_update(Ordering::Relaxed, Ordering::Relaxed, |l| l.checked_sub(cost)).is_ok()
                    });
                    got.push((i, d));
                }
            };
            // A worker that failed to start leaves its share to the others, and to layout if none started.
            let handles: Vec<_> = (0..workers).filter_map(|_| std::thread::Builder::new().stack_size(crate::render::STACK).spawn_scoped(s, work).ok()).collect();
            handles.into_iter().flat_map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).collect()
        });
        if self.budget.is_some() {
            self.budget = Some(left.into_inner());
        }
        for (i, d) in done {
            self.map.insert(srcs[i].clone(), d.map_or(LoadState::Failed, |d| LoadState::Ready(d.into())));
        }
    }

    fn spend(&mut self, d: Option<Decoded>) -> Option<Decoded> {
        let d = d?;
        if let (Some(left), Decoded::Raster(b)) = (self.budget.as_mut(), &d) {
            *left = left.checked_sub(b.px.len() as u64)?;
        }
        Some(d)
    }

    /// Fetches through mddog's remote fetcher: its worker pool, cache, and private-address checks.
    pub fn fetch(&mut self, urls: Vec<String>) {
        use crate::render::remote;
        for u in urls {
            let tx = self.tx.clone();
            let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            self.stops.push(stop.clone());
            let (host, url) = (remote::host(&u), u.clone());
            let started = remote::spawn(host, stop.clone(), move || {
                let body = remote::fetch(remote::agent(), &url, &stop).and_then(Result::ok).filter(|b| !b.is_empty());
                let _ = tx.send((url, body));
            });
            if !started {
                let _ = self.tx.send((u, None));
            }
        }
    }

    /// How many remote images are still loading.
    pub fn loading(&self) -> usize {
        self.map.values().filter(|s| matches!(s, LoadState::Loading)).count()
    }

    /// Take finished fetches. True when any image changed.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok((u, d)) = self.rx.try_recv() {
            // a failed fetch looks the same as a pending one: no relayout
            match d.and_then(|d| decode(&d)) {
                Some(i) => {
                    self.map.insert(u, LoadState::Ready(i));
                    any = true;
                }
                None => {
                    self.map.insert(u, LoadState::Failed);
                }
            }
        }
        any
    }
}

/// The folder a page's relative image paths start from: the file's, or for standard input the working folder.
pub fn base_dir(file: &Path, stdin: bool) -> PathBuf {
    if stdin { std::env::current_dir().ok() } else { std::fs::canonicalize(file).ok().and_then(|p| p.parent().map(Path::to_path_buf)) }.unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    fn png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let mut enc = ::png::Encoder::new(&mut out, width, height);
        enc.set_color(::png::ColorType::Rgba);
        enc.set_depth(::png::BitDepth::Eight);
        enc.write_header().ok()?.write_image_data(rgba).ok()?;
        Some(out)
    }

    #[test]
    fn prefetch_takes_the_images_layout_will_draw() {
        let md = "![a](a.png)\n\n| ![b](b.png) |\n|---|\n\n> - ![c](c.svg)\n\n<details open><summary>s</summary>\n\n![d](d.png)\n\n</details>\n\n<details><summary>s</summary>\n\n![e](e.png)\n\n</details>\n\n![a](a.png) ![r](https://x.test/r.png)\n";
        let mut srcs = vec![];
        super::sources(&crate::render::doc::parse(md).blocks, &mut srcs);
        assert_eq!(srcs, ["a.png", "b.png", "c.svg", "d.png", "a.png", "https://x.test/r.png"]);

        let dir = std::env::temp_dir().join(format!("mddog-prefetch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("c.svg"), r#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2"/>"#).unwrap();
        let mut images = super::Images::new(dir.clone());
        images.prefetch(srcs);
        // Ready or failed before layout asks, and the remote one is left to `fetch`.
        assert!(matches!(images.map.get("c.svg"), Some(super::LoadState::Ready(i)) if i.size() == (4.0, 2.0)));
        assert!(matches!(images.map.get("a.png"), Some(super::LoadState::Failed)));
        assert!(!images.map.contains_key("https://x.test/r.png"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn offline_skips_local_images_over_the_cap() {
        use super::{Images, Status};
        let dir = std::env::temp_dir().join(format!("mddog-imgcap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("dot.png"), png(2, 2, &[255; 16]).unwrap()).unwrap();
        let mut wanted = vec![];
        let mut online = Images::new(dir.clone());
        assert!(matches!(online.get("dot.png", &mut wanted), Status::Ready(_)));
        let mut capped = Images::new(dir.clone());
        capped.max_local = Some(10);
        assert!(matches!(capped.get("dot.png", &mut wanted), Status::Failed));
        capped.prefetch(vec!["dot.png".into()]);
        assert!(matches!(capped.get("dot.png", &mut wanted), Status::Failed));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_capped_load_refuses_an_image_too_big_to_decode() {
        // 20000x1 px compresses to almost nothing but is far wider than any preview needs.
        let wide = png(20_000, 1, &vec![255; 80_000]).unwrap();
        assert!(wide.len() < 1 << 20);
        assert!(super::decoded(&wide, true).is_none());
        assert!(super::decoded(&wide, false).is_some());
    }

    #[test]
    fn a_capped_load_stops_decoding_past_its_budget() {
        use super::{Images, Status};
        let dir = std::env::temp_dir().join(format!("mddog-imgbudget-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = png(64, 64, &[255; 64 * 64 * 4]).unwrap();
        std::fs::write(dir.join("a.png"), &png).unwrap();
        std::fs::write(dir.join("b.png"), &png).unwrap();
        let mut wanted = vec![];
        let mut capped = Images::new(dir.clone());
        // Room for one 64x64 image.
        (capped.max_local, capped.budget) = (Some(1 << 20), Some(64 * 64 * 4));
        assert!(matches!(capped.get("a.png", &mut wanted), Status::Ready(_)));
        assert!(matches!(capped.get("b.png", &mut wanted), Status::Failed));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn prefetch_spends_the_budget_while_it_decodes() {
        use super::{Images, Status};
        let dir = std::env::temp_dir().join(format!("mddog-imgprefetch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = png(64, 64, &[255; 64 * 64 * 4]).unwrap();
        let names: Vec<String> = (0..6).map(|i| format!("{i}.png")).collect();
        for n in &names {
            std::fs::write(dir.join(n), &png).unwrap();
        }
        let mut capped = Images::new(dir.clone());
        (capped.max_local, capped.budget) = (Some(1 << 20), Some(2 * 64 * 64 * 4));
        capped.prefetch(names.clone());
        let mut wanted = vec![];
        let ready = names.iter().filter(|n| matches!(capped.get(n, &mut wanted), Status::Ready(_))).count();
        assert_eq!(ready, 2);
        assert_eq!(capped.budget, Some(0));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_capped_load_refuses_a_big_svg() {
        let mut svg = String::from("<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'>");
        while svg.len() < (1 << 20) + 10 {
            svg.push_str("<rect width='1' height='1'/>");
        }
        svg.push_str("</svg>");
        assert!(super::decoded(svg.as_bytes(), true).is_none());
        assert!(super::decoded(svg.as_bytes(), false).is_some());
    }
}
