use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSImage, NSScreen, NSWindow,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CFRetained, CFString, CFType, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGBitmapContextCreate, CGColorSpace, CGContext, CGDataProvider, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo, kCGColorSpaceSRGB};
use objc2_foundation::{NSArray, NSData, NSDate, NSDictionary, NSError, NSHTTPURLResponse, NSNumber, NSPoint, NSRect, NSRunLoop, NSSize, NSString, NSURL};
use objc2_web_kit::{
    WKContentRuleList, WKContentRuleListStore, WKPreferences, WKSnapshotConfiguration, WKURLSchemeHandler, WKURLSchemeTask, WKUserContentController, WKWebView, WKWebViewConfiguration,
    WKWebsiteDataStore,
};

use super::{Bitmap, CALL_LIMIT, snapshot_size};
use crate::page::remote::{self, Event};
use crate::term::kitty::{self, Shm};

const PARKED: NSPoint = NSPoint::new(-100_000.0, 0.0);

pub struct Engine {
    window: Retained<NSWindow>,
    view: Retained<WKWebView>,
    mtm: MainThreadMarker,
    pub width: f64,
    pub height: f64,
    pub(super) stalled: Cell<bool>,
    rules: Rc<Cell<Option<bool>>>,
    _remote: Retained<RemoteHandler>,
}

fn disable_feature(prefs: &WKPreferences, name: &str) {
    let Some(cls) = AnyClass::get(c"WKPreferences") else { return };
    // SAFETY: respondsToSelector: takes a selector and returns a BOOL on any class.
    let listed: bool = unsafe { msg_send![cls, respondsToSelector: sel!(_features)] };
    if !listed || !prefs.respondsToSelector(sel!(_setEnabled:forFeature:)) {
        return;
    }
    // SAFETY: the class answers the private +_features (checked above), which returns an NSArray or nil.
    let Some(features): Option<Retained<NSArray>> = (unsafe { msg_send![cls, _features] }) else { return };
    for i in 0..features.count() {
        let feature = features.objectAtIndex(i);
        // SAFETY: each _features element is a _WKFeature, whose -key returns an NSString or nil.
        let key: Option<Retained<NSString>> = unsafe { msg_send![&*feature, key] };
        if key.is_some_and(|k| k.to_string() == name) {
            // SAFETY: prefs answers _setEnabled:forFeature: (checked above), and feature came from _features.
            let _: () = unsafe { msg_send![prefs, _setEnabled: false, forFeature: &*feature] };
        }
    }
}

type Task = Retained<ProtocolObject<dyn WKURLSchemeTask>>;

thread_local! {
    /// The scheme tasks still running, by id, with the flag that tells their fetch to stop.
    static TASKS: RefCell<HashMap<u64, (Task, Arc<AtomicBool>)>> = RefCell::new(HashMap::new());
}

static NEXT_TASK: AtomicU64 = AtomicU64::new(0);

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and RemoteHandler has no Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MddogRemoteHandler"]
    /// Serves the page's `mddog-http(s)://` requests through `remote::fetch` (see `page::remote`).
    struct RemoteHandler;

    unsafe impl NSObjectProtocol for RemoteHandler {}

    // SAFETY: both methods have the signatures the protocol declares.
    unsafe impl WKURLSchemeHandler for RemoteHandler {
        #[unsafe(method(webView:startURLSchemeTask:))]
        fn start(&self, _view: &WKWebView, task: &ProtocolObject<dyn WKURLSchemeTask>) {
            start_task(task);
        }

        /// WebKit throws if a stopped task gets any later call, so the task leaves `TASKS` here, on the main
        /// thread where every call to it is made.
        #[unsafe(method(webView:stopURLSchemeTask:))]
        fn stop(&self, _view: &WKWebView, task: &ProtocolObject<dyn WKURLSchemeTask>) {
            TASKS.with_borrow_mut(|tasks| {
                tasks.retain(|_, (t, stop)| {
                    let same = std::ptr::eq::<ProtocolObject<dyn WKURLSchemeTask>>(&**t, task);
                    if same {
                        stop.store(true, Ordering::Relaxed);
                    }
                    !same
                });
            });
        }
    }
);

fn start_task(task: &ProtocolObject<dyn WKURLSchemeTask>) {
    // SAFETY: a getter on the live task WebKit passed in.
    let req = unsafe { task.request() };
    let url = req.URL().and_then(|u| u.absoluteString()).map(|s| s.to_string()).unwrap_or_default();
    let url = remote::target(&url).to_owned();
    let head = req.HTTPMethod().is_some_and(|m| m.to_string() == "HEAD");
    let range = req.valueForHTTPHeaderField(&NSString::from_str("Range")).map(|r| r.to_string());
    let id = NEXT_TASK.fetch_add(1, Ordering::Relaxed);
    let stop = Arc::new(AtomicBool::new(false));
    TASKS.with_borrow_mut(|t| t.insert(id, (task.retain(), stop.clone())));
    // The page asks with HEAD after a load fails, to learn whether it was blocked.
    if head {
        let status = match remote::outcome(&url) {
            Some(remote::Fail::Blocked) => 403,
            Some(remote::Fail::Failed) => 502,
            None => 200,
        };
        deliver(id, Event::Head { status, headers: Vec::new() });
        deliver(id, Event::Done);
        return;
    }
    if remote::answer_cached(&url, range.as_deref(), &mut |e| deliver(id, e)) {
        if crate::trace::enabled() {
            crate::trace::mark(&format!("remote done in 0 ms: {url} {}", range.unwrap_or_default()));
        }
        return;
    }
    let start = Instant::now();
    let queued = remote::spawn(remote::host(&url), stop.clone(), move || {
        let mut end = "stopped";
        remote::fetch(remote::agent(), &url, range.as_deref(), &stop, &mut |e| {
            end = match e {
                Event::Done => "done",
                Event::Fail(remote::Fail::Blocked) => "blocked",
                Event::Fail(remote::Fail::Failed) => "failed",
                _ => end,
            };
            DispatchQueue::main().exec_async(move || deliver(id, e));
        });
        if crate::trace::enabled() {
            crate::trace::mark(&format!("remote {end} in {} ms: {url} {}", start.elapsed().as_millis(), range.unwrap_or_default()));
        }
    });
    if !queued {
        deliver(id, Event::Fail(remote::Fail::Failed));
    }
}

/// Passes one fetch event to its task, on the main thread, unless WebKit has stopped the task.
fn deliver(id: u64, event: Event) {
    let Some(task) = TASKS.with_borrow(|t| t.get(&id).map(|(task, _)| task.clone())) else { return };
    let end = matches!(event, Event::Done | Event::Fail(_));
    match event {
        Event::Head { status, headers } => {
            // SAFETY: a getter on a live task.
            let Some(url) = (unsafe { task.request() }).URL() else { return deliver(id, Event::Fail(remote::Fail::Failed)) };
            // The page's HEAD probe is a cross-origin fetch, which needs this header to read the status.
            let all: Vec<_> = headers.into_iter().chain([("Access-Control-Allow-Origin", "*".into())]).collect();
            let keys: Vec<_> = all.iter().map(|(k, _)| NSString::from_str(k)).collect();
            let values: Vec<_> = all.iter().map(|(_, v)| NSString::from_str(v)).collect();
            let fields = NSDictionary::from_slices(&keys.iter().map(|k| &**k).collect::<Vec<_>>(), &values.iter().map(|v| &**v).collect::<Vec<_>>());
            let version = NSString::from_str("HTTP/1.1");
            let Some(resp) = NSHTTPURLResponse::initWithURL_statusCode_HTTPVersion_headerFields(NSHTTPURLResponse::alloc(), &url, status as isize, Some(&version), Some(&fields)) else {
                // A task with no response would throw on the data that follows, so it ends here instead.
                return deliver(id, Event::Fail(remote::Fail::Failed));
            };
            // SAFETY: a running task's first reply, on the main thread.
            unsafe { task.didReceiveResponse(&resp) };
        }
        // SAFETY: the task has its response, since `fetch` sends `Head` before any data.
        Event::Data(d) => unsafe { task.didReceiveData(&NSData::with_bytes(&d)) },
        // SAFETY: a running task, which ends here.
        Event::Done => unsafe { task.didFinish() },
        // SAFETY: a running task, which ends here.
        Event::Fail(_) => unsafe { task.didFailWithError(&NSError::new(-1, &NSString::from_str("mddog"))) },
    }
    if end {
        TASKS.with_borrow_mut(|t| t.remove(&id));
    }
}

/// Compiles `remote::content_rules`, adds them to `controller`, and then sets `done` to whether that worked. The
/// compile is async and takes a few ms, so it ends while the page loads and the first paint never waits for it.
fn add_content_rules(controller: Retained<WKUserContentController>, done: Rc<Cell<Option<bool>>>, mtm: MainThreadMarker) {
    let dir = NSURL::fileURLWithPath(&NSString::from_str(&crate::assets::cache_dir().join("content-rules").to_string_lossy()));
    // SAFETY: a file URL on the main thread (mtm).
    let Some(store) = (unsafe { WKContentRuleListStore::storeWithURL(Some(&dir), mtm) }) else {
        done.set(Some(false));
        return;
    };
    let block = RcBlock::new(move |list: *mut WKContentRuleList, err: *mut NSError| {
        // SAFETY: WebKit passes a valid rule list or nil, live for this call.
        if let Some(list) = unsafe { list.as_ref() } {
            // SAFETY: a live controller and rule list on the main thread, where WebKit calls this handler.
            unsafe { controller.addContentRuleList(list) };
            crate::trace::mark("content rules added");
            done.set(Some(true));
        } else {
            // SAFETY: WebKit passes a valid NSError or nil, live for this call.
            let why = unsafe { err.as_ref() }.map(|e| e.localizedDescription().to_string());
            crate::trace::mark(&format!("content rules failed: {}", why.unwrap_or_default()));
            done.set(Some(false));
        }
    });
    let (id, json) = (NSString::from_str("remote"), NSString::from_str(&crate::page::remote::content_rules()));
    // SAFETY: the block's argument types match the handler WebKit calls, and WebKit copies the block.
    unsafe { store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(Some(&id), Some(&json), Some(&block)) };
}

/// The system's named appearance, dark or light.
fn appearance(dark: bool) -> Option<Retained<NSAppearance>> {
    // SAFETY: AppKit's appearance name statics are initialized before main and never written.
    NSAppearance::appearanceNamed(unsafe { if dark { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua } })
}

fn srgb_name() -> &'static CFString {
    // SAFETY: kCGColorSpaceSRGB is a CoreGraphics constant, initialized before main and never written.
    unsafe { kCGColorSpaceSRGB }
}

fn srgb() -> Option<CFRetained<CGColorSpace>> {
    CGColorSpace::with_name(Some(srgb_name()))
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

    pub fn new(width: f64) -> Engine {
        let mtm = MainThreadMarker::new().expect("main thread");
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        // SAFETY: plain init on the main thread (mtm); nothing else holds the configuration yet.
        let config = unsafe { WKWebViewConfiguration::new(mtm) };
        // SAFETY: a getter on a live configuration; the preferences it returns are retained.
        let prefs = unsafe { config.preferences() };
        disable_feature(&prefs, "UseGPUProcessForDOMRenderingEnabled");
        // SAFETY: the store is a fresh retained object, and the configuration is not yet in a web view.
        unsafe { config.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(mtm)) };
        let rules = Rc::new(Cell::new(None));
        // SAFETY: a getter on a live configuration; the controller it returns is retained.
        add_content_rules(unsafe { config.userContentController() }, rules.clone(), mtm);
        // SAFETY: plain init on the main thread; the class has no other initializer.
        let remote: Retained<RemoteHandler> = unsafe { msg_send![RemoteHandler::alloc(mtm), init] };
        for scheme in remote::SCHEMES {
            // SAFETY: a live handler and a custom scheme, set before the configuration is in a web view.
            unsafe { config.setURLSchemeHandler_forURLScheme(Some(ProtocolObject::from_ref(&*remote)), &NSString::from_str(scheme)) };
        }
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, 800.0));
        // SAFETY: alloc'd on the main thread with a finite frame and a fully set-up configuration.
        let view = unsafe { WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &config) };
        let dark = appearance(true);
        app.setAppearance(dark.as_deref());
        view.setAppearance(dark.as_deref());
        let parked = NSRect::new(PARKED, NSSize::new(width, 100.0));
        // SAFETY: alloc'd on the main thread with a finite rect and valid style and backing values.
        let window = unsafe { NSWindow::initWithContentRect_styleMask_backing_defer(NSWindow::alloc(mtm), parked, NSWindowStyleMask::Borderless, NSBackingStoreType::Buffered, false) };
        // SAFETY: Retained owns the window, so AppKit must not also release it on close.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setIgnoresMouseEvents(true);
        window.setCollectionBehavior(NSWindowCollectionBehavior::Transient | NSWindowCollectionBehavior::IgnoresCycle);
        window.setContentView(Some(&view));
        view.setFrame(frame);
        if view.respondsToSelector(sel!(_setWindowOcclusionDetectionEnabled:)) {
            // SAFETY: the view answers this private selector (checked above), which takes a BOOL.
            let _: () = unsafe { msg_send![&*view, _setWindowOcclusionDetectionEnabled: false] };
        }
        window.orderFrontRegardless();
        Engine { window, view, mtm, width, height: 800.0, stalled: Cell::new(false), rules, _remote: remote }
    }

    /// Whether the content rules are in place, waiting up to 2 s for WebKit to look them up or compile them.
    pub fn content_rules(&self) -> bool {
        let start = Instant::now();
        while self.rules.get().is_none() && start.elapsed() < Duration::from_secs(2) {
            spin(1.0);
        }
        self.rules.get() == Some(true)
    }

    pub fn load_blank(&self) {
        // SAFETY: a live view on the main thread and a valid string; a nil base URL is allowed.
        unsafe { self.view.loadHTMLString_baseURL(&NSString::from_str("<!DOCTYPE html><html></html>"), None) };
    }

    pub fn load_file(&self, html: &Path, dark: bool) {
        let _ = self.eval("document.documentElement.dataset.stale = '1'");
        let look = appearance(dark);
        self.view.setAppearance(look.as_deref());
        let url = NSURL::fileURLWithPath(&NSString::from_str(&html.to_string_lossy()));
        let root = NSURL::fileURLWithPath(&NSString::from_str("/"));
        // SAFETY: both URLs are file URLs, as this method requires.
        unsafe { self.view.loadFileURL_allowingReadAccessToURL(&url, &root) };
    }

    fn wait_for<T>(&self, out: &RefCell<Option<T>>, limit: Duration) -> Option<T> {
        let deadline = Instant::now() + limit;
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
        self.eval_within(js, CALL_LIMIT)
    }

    /// Like `eval`, but a reply later than `limit` marks the engine stalled.
    pub fn eval_within(&self, js: &str, limit: Duration) -> Result<String, String> {
        if self.stalled.get() {
            return Err("WebKit stopped responding".into());
        }
        let out: Rc<RefCell<Option<Result<String, String>>>> = Rc::new(RefCell::new(None));
        let slot = out.clone();
        let block = RcBlock::new(move |res: *mut AnyObject, err: *mut NSError| {
            let r = if !err.is_null() {
                // SAFETY: WebKit passes a valid NSError or nil, and it is not nil here.
                Err(unsafe { (*err).localizedDescription() }.to_string())
            } else if res.is_null() {
                Ok(String::new())
            } else {
                // SAFETY: WebKit passes a valid object or nil, live for this call, and it is not nil here.
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
        // SAFETY: the block's argument types match the handler WebKit calls, and WebKit copies the block.
        unsafe { self.view.evaluateJavaScript_completionHandler(&NSString::from_str(js), Some(&block)) };
        self.wait_for(&out, limit).unwrap_or_else(|| Err("WebKit stopped responding".into()))
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
        // The snapshot's image is autoreleased, and nothing else drains a pool on this thread, so without this
        // pool every snapshot stayed in memory until exit: 8 MiB a tile in both mdd and WebContent.
        objc2::rc::autoreleasepool(|_| self.snap_in(y, h, width, height, dst))
    }

    fn snap_in(&self, y: f64, h: f64, width: usize, height: usize, dst: &mut [u8]) -> Option<()> {
        // SAFETY: plain init and setters on the main thread, on a configuration nothing else holds yet.
        let config = unsafe {
            let c = WKSnapshotConfiguration::new(self.mtm);
            c.setRect(NSRect::new(NSPoint::new(0.0, y), NSSize::new(self.width, h)));
            c.setAfterScreenUpdates(true);
            c
        };
        let out: Rc<RefCell<Option<Option<Retained<NSImage>>>>> = Rc::new(RefCell::new(None));
        let slot = out.clone();
        let block = RcBlock::new(move |img: *mut NSImage, _err: *mut NSError| {
            // SAFETY: WebKit passes a valid NSImage or nil, live for this call; retain keeps it after.
            *slot.borrow_mut() = Some(unsafe { Retained::retain(img) });
        });
        // SAFETY: the block's argument types match the handler WebKit calls, and WebKit copies the block.
        unsafe { self.view.takeSnapshotWithConfiguration_completionHandler(Some(&config), &block) };
        let image = self.wait_for(&out, CALL_LIMIT)??;
        // SAFETY: a null proposed rect and nil context and hints are documented as allowed.
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
    let srgb = CGImage::color_space(Some(cg)).and_then(|c| CGColorSpace::name(Some(&c))).is_some_and(|n| AsRef::<CFType>::as_ref(&*n) == srgb_name().as_ref());
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
    assert!(dst.len() >= width * height * 4, "bitmap too small");
    let space = srgb();
    // SAFETY: dst holds width * height * 4 bytes (asserted above) and outlives ctx.
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
        let space = srgb();
        let info = CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0;
        // SAFETY: bgra holds row * height bytes and outlives ctx.
        let ctx = unsafe { CGBitmapContextCreate(bgra.as_mut_ptr().cast(), width, height, 8, row, space.as_deref(), info) }.unwrap();
        let cg = CGBitmapContextCreateImage(Some(&ctx)).unwrap();
        let (mut swapped, mut drawn) = (vec![0; width * height * 4], vec![0; width * height * 4]);
        assert!(swap_channels(&cg, width, height, &mut swapped));
        draw(&cg, width, height, &mut drawn);
        assert_eq!(swapped, drawn);
    }
}
