//! The macOS side of the embedded player (the counterpart of the GTK `layer.rs`): an OpenGL
//! view placed directly under the WKWebView, in the same parent view, which mpv draws into
//! through its render API. The page is transparent where the video shows, as on Linux.
//!
//! Drawing happens on a render thread of its own, as mpv recommends for GUI toolkits: a draw
//! waits inside mpv until the frame's display time, and on the main thread that starved the
//! app of input (the spinning cursor). The main thread only owns the view: it builds it, tells
//! the OpenGL context when the view's size or window changes (`refresh`), and nothing else.
//! The two share the context under CGL's context lock.
//!
//! mpv announces a frame through its update callback (any thread), which wakes the render
//! thread. A window resize calls `refresh` on the main thread, which wakes it too, so a paused
//! picture follows the window.
//!
//! Fullscreen: WebKit shows an element fullscreen by moving the WKWebView into a window of its
//! own, without a window event. While frames are drawn, the render thread asks the main thread to
//! `refresh` a few times a second; that moves the video view in under the webview when the
//! webview's parent changed.
//!
//! macOS still ships OpenGL (on Apple Silicon it runs on Metal), and NSOpenGLContext is the
//! simplest surface mpv's OpenGL render API can draw into. Hardware decoding stays on copy-back
//! (VideoToolbox decodes, the frame is copied into OpenGL), which needs no interop checks.
// Apple marks all of OpenGL deprecated; it is still supported, and mpv's render API needs it.
#![allow(deprecated)]

use super::ffi::{self, Handle, Lib, OpenGlFbo, OpenGlInitParams, RenderContext, RenderParam};
use objc2::rc::Retained;
use objc2::{msg_send, AllocAnyThread, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSOpenGLContext, NSOpenGLContextParameter, NSOpenGLPixelFormat,
    NSOpenGLPixelFormatAttribute, NSView, NSWindowOrderingMode,
};
use once_cell::sync::OnceCell;
use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void};
use std::ptr::{null_mut, NonNull};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

// NSOpenGLPixelFormatAttribute values (NSOpenGL.h).
const PFA_DOUBLE_BUFFER: NSOpenGLPixelFormatAttribute = 5;
const PFA_COLOR_SIZE: NSOpenGLPixelFormatAttribute = 8;
const PFA_ACCELERATED: NSOpenGLPixelFormatAttribute = 73;
const PFA_OPENGL_PROFILE: NSOpenGLPixelFormatAttribute = 99;
const OPENGL_PROFILE_3_2_CORE: NSOpenGLPixelFormatAttribute = 0x3200;

const OPENGL_FRAMEWORK: &str = "/System/Library/Frameworks/OpenGL.framework/OpenGL";
/// How long the render thread sleeps when nothing asks for a draw.
const IDLE_WAIT: Duration = Duration::from_millis(500);
/// How often, while drawing, the main thread checks whether the webview changed windows.
const FOLLOW_EVERY: Duration = Duration::from_millis(300);
/// How long quitting waits for the render thread to free mpv's render context.
const STOP_WAIT: Duration = Duration::from_secs(2);

type CglLock = unsafe extern "C" fn(*mut c_void) -> c_int;

/// The OpenGL framework, opened once; mpv looks its functions up through it.
static OPENGL: OnceCell<usize> = OnceCell::new();
static CGL: OnceCell<(CglLock, CglLock)> = OnceCell::new();
static LIB: OnceCell<&'static Lib> = OnceCell::new();
/// The NSOpenGLContext (kept alive by `GL` on the main thread) and its CGLContextObj.
static GL_PTR: AtomicUsize = AtomicUsize::new(0);
static CGL_PTR: AtomicUsize = AtomicUsize::new(0);
/// The view's size in pixels as `width << 32 | height`, written by the main thread.
static SIZE: AtomicU64 = AtomicU64::new(0);
static FIRST_FRAME: AtomicBool = AtomicBool::new(false);
/// A main-thread `refresh` is already scheduled.
static REFRESH_PENDING: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct Wake {
    draw: bool,
    stop: bool,
    stopped: bool,
}
static WAKE: Mutex<Wake> = Mutex::new(Wake { draw: false, stop: false, stopped: false });
static WAKE_CV: Condvar = Condvar::new();

thread_local! {
    // Main thread only.
    static WEBVIEW: RefCell<Option<Retained<NSView>>> = const { RefCell::new(None) };
    static VIEW: RefCell<Option<Retained<NSView>>> = const { RefCell::new(None) };
    static GL: RefCell<Option<Retained<NSOpenGLContext>>> = const { RefCell::new(None) };
}

unsafe extern "C" fn get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    match OPENGL.get() {
        Some(&handle) => unsafe { libc::dlsym(handle as *mut c_void, name) },
        None => null_mut(),
    }
}

/// Called by mpv (any thread) when a new frame is ready.
unsafe extern "C" fn on_update(_ctx: *mut c_void) {
    request_draw();
}

/// Wakes the render thread for one draw; repeated calls before it runs are merged.
fn request_draw() {
    let mut wake = WAKE.lock().unwrap_or_else(|p| p.into_inner());
    wake.draw = true;
    WAKE_CV.notify_one();
}

fn opengl() -> Result<usize, String> {
    let handle = *OPENGL.get_or_try_init(|| ffi::open(OPENGL_FRAMEWORK).map(|h| h as usize))?;
    if handle == 0 {
        return Err("The OpenGL framework could not be opened".to_string());
    }
    Ok(handle)
}

/// Holds CGL's lock on the shared context while alive, so the render thread and the main
/// thread never use the context at the same time.
struct CglGuard(*mut c_void);

impl CglGuard {
    fn lock() -> Option<Self> {
        let ctx = CGL_PTR.load(Ordering::Acquire) as *mut c_void;
        let (lock, _) = CGL.get()?;
        if ctx.is_null() {
            return None;
        }
        unsafe { lock(ctx) };
        Some(CglGuard(ctx))
    }
}

impl Drop for CglGuard {
    fn drop(&mut self) {
        if let Some((_, unlock)) = CGL.get() {
            unsafe { unlock(self.0) };
        }
    }
}

/// The shared context, usable from any thread (NSOpenGLContext is not tied to the main thread).
fn gl_context() -> Option<&'static NSOpenGLContext> {
    let ptr = GL_PTR.load(Ordering::Acquire) as *const NSOpenGLContext;
    // SAFETY: `GL` on the main thread keeps the context alive for the life of the app.
    unsafe { ptr.as_ref() }
}

fn unpack(size: u64) -> (c_int, c_int) {
    ((size >> 32) as c_int, (size & 0xffff_ffff) as c_int)
}

/// The render thread: creates mpv's render context with the shared context current, then
/// draws whenever woken, until asked to stop.
fn render_thread(lib: &'static Lib, handle: Handle) {
    let Some(gl) = gl_context() else { return };
    let context = {
        let _guard = CglGuard::lock();
        gl.makeCurrentContext();
        match create_render_context(lib, handle) {
            Ok(context) => context,
            Err(e) => {
                super::render_failed(e);
                return;
            }
        }
    };
    let mut last_follow = Instant::now();
    loop {
        {
            let mut wake = WAKE.lock().unwrap_or_else(|p| p.into_inner());
            if !wake.draw && !wake.stop {
                wake = WAKE_CV.wait_timeout(wake, IDLE_WAIT).unwrap_or_else(|p| p.into_inner()).0;
            }
            if wake.stop {
                break;
            }
            if !wake.draw {
                continue;
            }
            wake.draw = false;
        }
        let (w, h) = unpack(SIZE.load(Ordering::Acquire));
        if w < 1 || h < 1 {
            continue;
        }
        let code = {
            let _guard = CglGuard::lock();
            gl.makeCurrentContext();
            let mut target = OpenGlFbo { fbo: 0, w, h, internal_format: 0 };
            // The default framebuffer has its origin at the bottom left.
            let mut flip: c_int = 1;
            let mut params = [
                RenderParam { kind: ffi::RENDER_PARAM_OPENGL_FBO, data: &mut target as *mut _ as *mut c_void },
                RenderParam { kind: ffi::RENDER_PARAM_FLIP_Y, data: &mut flip as *mut _ as *mut c_void },
                RenderParam { kind: ffi::RENDER_PARAM_INVALID, data: null_mut() },
            ];
            let code = unsafe {
                (lib.render_context_update)(context);
                (lib.render_context_render)(context, params.as_mut_ptr())
            };
            gl.flushBuffer();
            code
        };
        if !FIRST_FRAME.swap(true, Ordering::Relaxed) {
            tracing::info!("embedded mpv drew its first frame on macOS: {}x{} ({})", w, h, code);
        }
        if last_follow.elapsed() >= FOLLOW_EVERY {
            last_follow = Instant::now();
            schedule_refresh();
        }
    }
    {
        let _guard = CglGuard::lock();
        gl.makeCurrentContext();
        unsafe { (lib.render_context_free)(context) };
    }
    let mut wake = WAKE.lock().unwrap_or_else(|p| p.into_inner());
    wake.stopped = true;
    WAKE_CV.notify_all();
}

/// Runs `refresh` on the main thread soon; repeated calls before then are merged.
fn schedule_refresh() {
    if REFRESH_PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    let scheduled = super::APP.get().is_some_and(|app| app.run_on_main_thread(refresh).is_ok());
    if !scheduled {
        REFRESH_PENDING.store(false, Ordering::Release);
    }
}

/// The size of `view` in pixels (Retina screens have 2 pixels per point).
fn backing_size(view: &NSView) -> (c_int, c_int) {
    let rect = view.convertRectToBacking(view.bounds());
    (rect.size.width.round() as c_int, rect.size.height.round() as c_int)
}

/// Main thread: keeps the view under the webview (fullscreen moves the webview to another
/// window) and the context's drawable at the view's size, then asks for a draw when either changed.
fn refresh() {
    REFRESH_PENDING.store(false, Ordering::Release);
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some((view, gl)) = VIEW.with(|v| v.borrow().clone()).zip(GL.with(|g| g.borrow().clone())) else {
        return;
    };
    let moved = follow_webview(&view, &gl, mtm);
    let (w, h) = backing_size(&view);
    let size = ((w.max(0) as u64) << 32) | h.max(0) as u64;
    if moved || SIZE.load(Ordering::Acquire) != size {
        let _guard = CglGuard::lock();
        // The context's drawable follows the view only when told, on the main thread.
        gl.update(mtm);
        SIZE.store(size, Ordering::Release);
        drop(_guard);
        request_draw();
    }
}

/// Moves `view` in directly under the webview when WebKit moved the webview. Returns whether it
/// moved.
fn follow_webview(view: &NSView, gl: &NSOpenGLContext, mtm: MainThreadMarker) -> bool {
    let Some(webview) = WEBVIEW.with(|w| w.borrow().clone()) else { return false };
    let Some(parent) = (unsafe { webview.superview() }) else { return false };
    let current = unsafe { view.superview() };
    if current.as_ref().is_some_and(|c| Retained::as_ptr(c) == Retained::as_ptr(&parent)) {
        return false;
    }
    view.removeFromSuperview();
    view.setFrame(parent.bounds());
    parent.addSubview_positioned_relativeTo(view, NSWindowOrderingMode::Below, Some(&webview));
    let _guard = CglGuard::lock();
    gl.setView(Some(view), mtm);
    tracing::info!("embedded mpv video view followed the webview into another window");
    true
}

/// An OpenGL 3.2 core context for `view`, synced to the display.
fn create_gl(view: &NSView, mtm: MainThreadMarker) -> Result<Retained<NSOpenGLContext>, String> {
    let attributes: [NSOpenGLPixelFormatAttribute; 7] =
        [PFA_OPENGL_PROFILE, OPENGL_PROFILE_3_2_CORE, PFA_DOUBLE_BUFFER, PFA_ACCELERATED, PFA_COLOR_SIZE, 24, 0];
    let format = unsafe {
        NSOpenGLPixelFormat::initWithAttributes(NSOpenGLPixelFormat::alloc(), NonNull::from(&attributes[0]))
    }
    .ok_or("No OpenGL pixel format is available")?;
    let gl = NSOpenGLContext::initWithFormat_shareContext(NSOpenGLContext::alloc(), &format, None)
        .ok_or("The OpenGL context could not be created")?;
    gl.setView(Some(view), mtm);
    // Off the main thread, waiting for the display is harmless and avoids tearing.
    let on: c_int = 1;
    let _: () = unsafe {
        msg_send![&*gl, setValues: &on as *const c_int, forParameter: NSOpenGLContextParameter::SwapInterval]
    };
    Ok(gl)
}

fn create_render_context(lib: &'static Lib, handle: Handle) -> Result<RenderContext, String> {
    let mut init = OpenGlInitParams { get_proc_address, get_proc_address_ctx: null_mut() };
    let mut params = [
        RenderParam { kind: ffi::RENDER_PARAM_API_TYPE, data: c"opengl".as_ptr() as *mut c_void },
        RenderParam { kind: ffi::RENDER_PARAM_OPENGL_INIT_PARAMS, data: &mut init as *mut _ as *mut c_void },
        RenderParam { kind: ffi::RENDER_PARAM_INVALID, data: null_mut() },
    ];
    let mut context: RenderContext = null_mut();
    let code = unsafe { (lib.render_context_create)(&mut context, handle, params.as_mut_ptr()) };
    if code < 0 || context.is_null() {
        return Err(format!("mpv could not draw with OpenGL: {}", lib.error_text(code)));
    }
    unsafe { (lib.render_context_set_update_callback)(context, Some(on_update), null_mut()) };
    Ok(context)
}

/// mpv's handle, carried to other threads; mpv's client API is thread-safe.
struct SendHandle(Handle);
unsafe impl Send for SendHandle {}

impl SendHandle {
    /// Taking `self` makes a closure capture the whole wrapper, not its raw pointer.
    fn get(self) -> Handle {
        self.0
    }
}

/// Main thread: builds the view and the shared context under the WKWebView `webview`, then
/// starts the render thread.
fn attach_under(webview: *mut c_void, lib: &'static Lib, handle: Handle) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("The video layer must be set up on the main thread")?;
    let webview = unsafe { (webview as *mut NSView).as_ref() }.ok_or("The webview was not found")?;
    let parent = unsafe { webview.superview() }.ok_or("The webview has no parent view")?;

    let opengl = opengl()?;
    let lock = unsafe { libc::dlsym(opengl as *mut c_void, c"CGLLockContext".as_ptr()) };
    let unlock = unsafe { libc::dlsym(opengl as *mut c_void, c"CGLUnlockContext".as_ptr()) };
    if lock.is_null() || unlock.is_null() {
        return Err("CGL's context lock was not found".to_string());
    }
    let _ = CGL.set(unsafe { (std::mem::transmute::<*mut c_void, CglLock>(lock), std::mem::transmute::<*mut c_void, CglLock>(unlock)) });

    let view = NSView::initWithFrame(NSView::alloc(mtm), parent.bounds());
    // Layer-backed, so the system composites it under the (layer-backed) webview.
    view.setWantsLayer(true);
    view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
    // Draw at the screen's full resolution on Retina displays.
    let _: () = unsafe { msg_send![&*view, setWantsBestResolutionOpenGLSurface: true] };
    parent.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, Some(webview));

    let gl = create_gl(&view, mtm)?;
    let cgl: *mut c_void = unsafe { msg_send![&*gl, CGLContextObj] };
    if cgl.is_null() {
        return Err("The OpenGL context has no CGL context".to_string());
    }
    let (w, h) = backing_size(&view);
    SIZE.store(((w.max(0) as u64) << 32) | h.max(0) as u64, Ordering::Release);
    GL_PTR.store(Retained::as_ptr(&gl) as usize, Ordering::Release);
    CGL_PTR.store(cgl as usize, Ordering::Release);
    let _ = LIB.set(lib);
    WEBVIEW.with(|w| *w.borrow_mut() = Some(webview.retain()));
    VIEW.with(|v| *v.borrow_mut() = Some(view));
    GL.with(|g| *g.borrow_mut() = Some(gl));

    let handle = SendHandle(handle);
    std::thread::Builder::new()
        .name("embedded-mpv-render".into())
        .spawn(move || render_thread(lib, handle.get()))
        .map_err(|e| e.to_string())?;
    request_draw();
    Ok(())
}

/// Stops the render thread, which frees mpv's render context; that must happen before mpv itself
/// goes (called when the app exits).
pub fn detach() {
    if LIB.get().is_none() {
        return;
    }
    let mut wake = WAKE.lock().unwrap_or_else(|p| p.into_inner());
    wake.stop = true;
    WAKE_CV.notify_all();
    let deadline = Instant::now() + STOP_WAIT;
    while !wake.stopped {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            tracing::warn!("The embedded mpv render thread did not stop in time");
            break;
        }
        wake = WAKE_CV.wait_timeout(wake, left).unwrap_or_else(|p| p.into_inner()).0;
    }
}

/// Puts the video view under the main webview and makes the webview's background transparent.
/// The view is built once the webview's native object is at hand (on the main thread); a failure
/// there is reported through `render_failed`, which goes back to the web player.
pub fn attach(window: &tauri::WebviewWindow, lib: &'static Lib, handle: Handle) -> Result<(), String> {
    AsRef::<tauri::Webview<_>>::as_ref(window)
        .set_background_color(Some(tauri::window::Color(0, 0, 0, 0)))
        .map_err(|e| e.to_string())?;
    let handle = SendHandle(handle);
    window
        .with_webview(move |webview| {
            if let Err(e) = attach_under(webview.inner(), lib, handle.get()) {
                super::render_failed(e);
            }
        })
        .map_err(|e| e.to_string())?;
    // A resize changes the drawable; a paused video draws no new frame by itself.
    window.on_window_event(|event| {
        if matches!(event, tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. }) {
            schedule_refresh();
        }
    });
    Ok(())
}
