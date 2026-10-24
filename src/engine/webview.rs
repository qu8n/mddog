use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, NSObjectProtocol};
use objc2::{MainThreadMarker, MainThreadOnly, msg_send, sel};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSImage, NSScreen, NSWindow,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CFType, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGDataProvider, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo, kCGColorSpaceSRGB};
use objc2_foundation::{NSDate, NSError, NSNumber, NSPoint, NSRect, NSRunLoop, NSSize, NSString, NSURL};
use objc2_web_kit::{WKPreferences, WKSnapshotConfiguration, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore};

use super::{Bitmap, CALL_LIMIT, snapshot_size};
use crate::term::kitty::{self, Shm};

const PARKED: NSPoint = NSPoint::new(-100_000.0, 0.0);

pub struct Engine {
    window: Retained<NSWindow>,
    view: Retained<WKWebView>,
    mtm: MainThreadMarker,
    pub width: f64,
    pub height: f64,
    pub(super) stalled: Cell<bool>,
}

fn disable_feature(prefs: &WKPreferences, name: &str) {
    let Some(cls) = AnyClass::get(c"WKPreferences") else { return };
    let listed: bool = unsafe { msg_send![cls, respondsToSelector: sel!(_features)] };
    if !listed || !prefs.respondsToSelector(sel!(_setEnabled:forFeature:)) {
        return;
    }
    let features: *mut AnyObject = unsafe { msg_send![cls, _features] };
    if features.is_null() {
        return;
    }
    let count: usize = unsafe { msg_send![features, count] };
    for i in 0..count {
        let feature: *mut AnyObject = unsafe { msg_send![features, objectAtIndex: i] };
        let key: *mut NSString = unsafe { msg_send![feature, key] };
        if !key.is_null() && unsafe { (*key).to_string() } == name {
            let _: () = unsafe { msg_send![prefs, _setEnabled: false, forFeature: feature] };
        }
    }
}

pub fn spin(ms: f64) {
    NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(ms / 1000.0));
}

pub fn backing_scale() -> f64 {
    let mtm = MainThreadMarker::new().expect("main thread");
    NSScreen::mainScreen(mtm).map_or(2.0, |s| s.backingScaleFactor())
}

impl Engine {
    pub fn wait_js(&self, js: &str, timeout: Duration) -> bool {
        let start = Instant::now();
        loop {
            if self.eval(&format!("String(!!({js}))")).as_deref() == Ok("true") {
                return true;
            }
            if self.stalled.get() {
                return false;
            }
            if start.elapsed() > timeout {
                return false;
            }
            spin(2.0);
        }
    }

    pub fn wait_frame(&self) {
        let _ = self.eval("window.mddogFrame = false; requestAnimationFrame(() => { window.mddogFrame = true; }); ''");
        self.wait_js("window.mddogFrame", Duration::from_millis(100));
    }

    pub fn new(width: f64) -> Engine {
        let mtm = MainThreadMarker::new().expect("main thread");
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        let config = unsafe { WKWebViewConfiguration::new(mtm) };
        let prefs = unsafe { config.preferences() };
        disable_feature(&prefs, "UseGPUProcessForDOMRenderingEnabled");
        unsafe { config.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(mtm)) };
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, 800.0));
        let view = unsafe { WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &config) };
        let dark = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua });
        app.setAppearance(dark.as_deref());
        view.setAppearance(dark.as_deref());
        let parked = NSRect::new(PARKED, NSSize::new(width, 100.0));
        let window = unsafe { NSWindow::initWithContentRect_styleMask_backing_defer(NSWindow::alloc(mtm), parked, NSWindowStyleMask::Borderless, NSBackingStoreType::Buffered, false) };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setIgnoresMouseEvents(true);
        window.setCollectionBehavior(NSWindowCollectionBehavior::Transient | NSWindowCollectionBehavior::IgnoresCycle);
        window.setContentView(Some(&view));
        view.setFrame(frame);
        if view.respondsToSelector(sel!(_setWindowOcclusionDetectionEnabled:)) {
            let _: () = unsafe { msg_send![&*view, _setWindowOcclusionDetectionEnabled: false] };
        }
        window.orderFrontRegardless();
        Engine { window, view, mtm, width, height: 800.0, stalled: Cell::new(false) }
    }

    pub fn load_blank(&self) {
        unsafe { self.view.loadHTMLString_baseURL(&NSString::from_str("<!DOCTYPE html><html></html>"), None) };
    }

    pub fn load_file(&self, html: &Path, dark: bool) {
        let _ = self.eval("document.documentElement.dataset.stale = '1'");
        let look = NSAppearance::appearanceNamed(unsafe { if dark { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua } });
        self.view.setAppearance(look.as_deref());
        let url = NSURL::fileURLWithPath(&NSString::from_str(&html.to_string_lossy()));
        let root = NSURL::fileURLWithPath(&NSString::from_str("/"));
        unsafe { self.view.loadFileURL_allowingReadAccessToURL(&url, &root) };
    }

    fn wait_for<T>(&self, out: &RefCell<Option<T>>) -> Option<T> {
        let deadline = Instant::now() + CALL_LIMIT;
        loop {
            if let Some(r) = out.borrow_mut().take() {
                return Some(r);
            }
            if Instant::now() > deadline {
                self.stalled.set(true);
                return None;
            }
            spin(0.5);
        }
    }

    pub fn eval(&self, js: &str) -> Result<String, String> {
        if self.stalled.get() {
            return Err("WebKit stopped responding".into());
        }
        let out: Rc<RefCell<Option<Result<String, String>>>> = Rc::new(RefCell::new(None));
        let slot = out.clone();
        let block = RcBlock::new(move |res: *mut AnyObject, err: *mut NSError| {
            let r = if !err.is_null() {
                Err(unsafe { (*err).localizedDescription() }.to_string())
            } else if res.is_null() {
                Ok(String::new())
            } else {
                let obj: &AnyObject = unsafe { &*res };
                if let Some(s) = obj.downcast_ref::<NSString>() {
                    Ok(s.to_string())
                } else if let Some(n) = obj.downcast_ref::<NSNumber>() {
                    Ok(n.stringValue().to_string())
                } else {
                    Ok(String::new())
                }
            };
            *slot.borrow_mut() = Some(r);
        });
        unsafe { self.view.evaluateJavaScript_completionHandler(&NSString::from_str(js), Some(&block)) };
        self.wait_for(&out).unwrap_or_else(|| Err("WebKit stopped responding".into()))
    }

    pub fn resize(&mut self, width: f64, height: f64) {
        self.width = width;
        self.height = height;
        self.window.setFrame_display(NSRect::new(PARKED, NSSize::new(width, 100.0)), false);
        self.view.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height)));
    }

    pub fn snapshot(&self, y: f64, h: f64, scale: f64) -> Option<Bitmap> {
        let (width, height) = snapshot_size(self.width, h, scale);
        let mut rgba = Shm::create(kitty::shm_name(), width * height * 4)?;
        self.snapshot_into(y, h, scale, &mut rgba)?;
        Some(Bitmap { width, height, rgba })
    }

    pub fn snapshot_into(&self, y: f64, h: f64, scale: f64, dst: &mut [u8]) -> Option<()> {
        let (width, height) = snapshot_size(self.width, h, scale);
        let dst = dst.get_mut(..width * height * 4).filter(|_| !self.stalled.get())?;
        // WebKit snaps a rect that starts or ends between CSS pixels, which shifts the rows and rasterizes the
        // text differently. A tile starts on a half CSS pixel whenever the cell height is odd at scale 2, so
        // take whole CSS pixels around it and keep the rows it asked for.
        let (from, to) = (y.floor(), (y + h).ceil());
        if from == y && to == y + h {
            return self.snap(y, h, width, height, dst);
        }
        let skip = ((y - from) * scale).round() as usize;
        let (_, all) = snapshot_size(self.width, to - from, scale);
        let mut buf = vec![0; width * all * 4];
        self.snap(from, to - from, width, all, &mut buf)?;
        dst.copy_from_slice(buf.get(skip * width * 4..)?.get(..dst.len())?);
        Some(())
    }

    fn snap(&self, y: f64, h: f64, width: usize, height: usize, dst: &mut [u8]) -> Option<()> {
        let config = unsafe { WKSnapshotConfiguration::new(self.mtm) };
        unsafe { config.setRect(NSRect::new(NSPoint::new(0.0, y), NSSize::new(self.width, h))) };
        unsafe { config.setAfterScreenUpdates(true) };
        let out: Rc<RefCell<Option<Option<Retained<NSImage>>>>> = Rc::new(RefCell::new(None));
        let slot = out.clone();
        let block = RcBlock::new(move |img: *mut NSImage, _err: *mut NSError| {
            *slot.borrow_mut() = Some(unsafe { Retained::retain(img) });
        });
        unsafe { self.view.takeSnapshotWithConfiguration_completionHandler(Some(&config), &block) };
        let image = self.wait_for(&out)??;
        let cg = unsafe { image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None) }?;
        if !swap_channels(&cg, width, height, dst) {
            draw(&cg, width, height, dst);
        }
        Some(())
    }
}

// WebKit's snapshot is premultiplied sRGB BGRA, so drawing it into the RGBA context is a channel
// swap. Swapping the bytes directly takes about half the time of the draw.
fn swap_channels(cg: &CGImage, width: usize, height: usize, dst: &mut [u8]) -> bool {
    let srgb = CGImage::color_space(Some(cg)).and_then(|c| CGColorSpace::name(Some(&c))).is_some_and(|n| AsRef::<CFType>::as_ref(&*n) == unsafe { kCGColorSpaceSRGB }.as_ref());
    let bgra = CGImage::alpha_info(Some(cg)) == CGImageAlphaInfo::PremultipliedFirst && CGImage::byte_order_info(Some(cg)) == CGImageByteOrderInfo::Order32Little;
    if !srgb || !bgra || height == 0 || CGImage::width(Some(cg)) != width || CGImage::height(Some(cg)) != height || CGImage::bits_per_pixel(Some(cg)) != 32 {
        return false;
    }
    let Some(data) = CGImage::data_provider(Some(cg)).and_then(|p| CGDataProvider::data(Some(&p))) else { return false };
    // SAFETY: the data is a private copy that nothing else mutates.
    let src = unsafe { data.as_bytes_unchecked() };
    let row = CGImage::bytes_per_row(Some(cg));
    if src.len() < (height - 1) * row + width * 4 {
        return false;
    }
    for (d, s) in dst.chunks_exact_mut(width * 4).zip(src.chunks(row)).take(height) {
        for (d, s) in d.as_chunks_mut::<4>().0.iter_mut().zip(s.as_chunks::<4>().0) {
            let p = u32::from_le_bytes(*s);
            *d = ((p & 0xff00_ff00) | ((p >> 16) & 0xff) | ((p & 0xff) << 16)).to_le_bytes();
        }
    }
    true
}

fn draw(cg: &CGImage, width: usize, height: usize, dst: &mut [u8]) {
    let space = unsafe { CGColorSpace::with_name(Some(kCGColorSpaceSRGB)) };
    let ctx = unsafe { CGBitmapContextCreate(dst.as_mut_ptr().cast(), width, height, 8, width * 4, space.as_deref(), CGImageAlphaInfo::PremultipliedLast.0) }.expect("bitmap context");
    CGContext::draw_image(Some(&ctx), CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(width as f64, height as f64)), Some(cg));
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_core_graphics::{CGBitmapContextCreateImage, CGImageByteOrderInfo};

    #[test]
    fn swapping_channels_matches_the_draw() {
        // WebKit pads its rows, so this one does too.
        let (width, height, row) = (5, 3, 32);
        let mut bgra: Vec<u8> = (0..row * height).map(|i| (i * 37 % 256) as u8).collect();
        for px in bgra.as_chunks_mut::<4>().0 {
            px[3] = 255;
        }
        let space = unsafe { CGColorSpace::with_name(Some(kCGColorSpaceSRGB)) };
        let info = CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0;
        let ctx = unsafe { CGBitmapContextCreate(bgra.as_mut_ptr().cast(), width, height, 8, row, space.as_deref(), info) }.unwrap();
        let cg = CGBitmapContextCreateImage(Some(&ctx)).unwrap();
        let (mut swapped, mut drawn) = (vec![0; width * height * 4], vec![0; width * height * 4]);
        assert!(swap_channels(&cg, width, height, &mut swapped));
        draw(&cg, width, height, &mut drawn);
        assert_eq!(swapped, drawn);
    }
}
