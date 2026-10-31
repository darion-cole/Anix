//! The macOS side of the embedded player (the counterpart of the GTK `layer.rs`): an OpenGL
//! view placed directly under the WKWebView, in the same parent view, which mpv draws into
//! through its render API. The page is transparent where the video shows, as on Linux.
//!
//! Everything here runs on the main thread. mpv announces a new frame through its update
//! callback, which may come from any thread; that only schedules one draw on the main thread
//! (several announcements before it runs are merged). A window resize schedules a draw too, so
//! a paused picture follows the window. The swap interval is 0, so a draw never blocks the main
//! thread waiting for the display.
//!
//! Fullscreen: WebKit shows an element fullscreen by moving the WKWebView into a window of its
//! own. Before each draw the video view checks the webview's parent and moves in under it when it
//! changed (entering or leaving fullscreen changes the video's place on the page, so mpv draws).
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
use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_int, c_void};
use std::ptr::{null_mut, NonNull};
use std::sync::atomic::{AtomicBool, Ordering};

// NSOpenGLPixelFormatAttribute values (NSOpenGL.h).
const PFA_DOUBLE_BUFFER: NSOpenGLPixelFormatAttribute = 5;
const PFA_COLOR_SIZE: NSOpenGLPixelFormatAttribute = 8;
const PFA_ACCELERATED: NSOpenGLPixelFormatAttribute = 73;
const PFA_OPENGL_PROFILE: NSOpenGLPixelFormatAttribute = 99;
const OPENGL_PROFILE_3_2_CORE: NSOpenGLPixelFormatAttribute = 0x3200;

const OPENGL_FRAMEWORK: &str = "/System/Library/Frameworks/OpenGL.framework/OpenGL";

/// The OpenGL framework, opened once; mpv looks its functions up through it.
static OPENGL: OnceCell<usize> = OnceCell::new();
/// A draw is already scheduled on the main thread.
static DRAW_PENDING: AtomicBool = AtomicBool::new(false);
static FIRST_FRAME: AtomicBool = AtomicBool::new(false);
static LIB: OnceCell<&'static Lib> = OnceCell::new();

thread_local! {
    static WEBVIEW: RefCell<Option<Retained<NSView>>> = const { RefCell::new(None) };
    static VIEW: RefCell<Option<Retained<NSView>>> = const { RefCell::new(None) };
    static GL: RefCell<Option<Retained<NSOpenGLContext>>> = const { RefCell::new(None) };
    static CONTEXT: Cell<RenderContext> = const { Cell::new(null_mut()) };
    /// The drawable size the context was last updated for, in pixels.
    static SIZE: Cell<(c_int, c_int)> = const { Cell::new((0, 0)) };
}

unsafe extern "C" fn get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    match OPENGL.get() {
        Some(&handle) => unsafe { libc::dlsym(handle as *mut c_void, name) },
        None => null_mut(),
    }
}

/// Called by mpv (any thread) when a new frame is ready.
unsafe extern "C" fn on_update(_ctx: *mut c_void) {
    schedule_draw();
}

/// Draws once on the main thread, soon; repeated calls before then are merged.
pub(super) fn schedule_draw() {
    if DRAW_PENDING.swap(true, Ordering::AcqRel) {
        return;
    }
    let Some(app) = super::APP.get() else {
        DRAW_PENDING.store(false, Ordering::Release);
        return;
    };
    if app.run_on_main_thread(draw).is_err() {
        DRAW_PENDING.store(false, Ordering::Release);
    }
}

/// The size of `view` in pixels (Retina screens have 2 pixels per point).
fn backing_size(view: &NSView) -> (c_int, c_int) {
    let rect = view.convertRectToBacking(view.bounds());
    (rect.size.width.round() as c_int, rect.size.height.round() as c_int)
}

fn draw() {
    DRAW_PENDING.store(false, Ordering::Release);
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(lib) = LIB.get().copied() else { return };
    let context = CONTEXT.with(Cell::get);
    if context.is_null() {
        return;
    }
    let Some((view, gl)) = VIEW.with(|v| v.borrow().clone()).zip(GL.with(|g| g.borrow().clone())) else {
        return;
    };
    follow_webview(&view, &gl, mtm);
    gl.makeCurrentContext();
    let (w, h) = backing_size(&view);
    if w < 1 || h < 1 {
        return;
    }
    if SIZE.with(Cell::get) != (w, h) {
        // The context's drawable follows the view only when told.
        gl.update(mtm);
        SIZE.with(|s| s.set((w, h)));
    }
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
    if !FIRST_FRAME.swap(true, Ordering::Relaxed) {
        tracing::info!("embedded mpv drew its first frame on macOS: {}x{} ({})", w, h, code);
    }
}

/// Keeps `view` directly under the webview when WebKit moves the webview (fullscreen).
fn follow_webview(view: &NSView, gl: &NSOpenGLContext, mtm: MainThreadMarker) {
    let Some(webview) = WEBVIEW.with(|w| w.borrow().clone()) else { return };
    let Some(parent) = (unsafe { webview.superview() }) else { return };
    let current = unsafe { view.superview() };
    if current.as_ref().is_some_and(|c| Retained::as_ptr(c) == Retained::as_ptr(&parent)) {
        return;
    }
    view.removeFromSuperview();
    view.setFrame(parent.bounds());
    parent.addSubview_positioned_relativeTo(view, NSWindowOrderingMode::Below, Some(&webview));
    gl.setView(Some(view), mtm);
    // The drawable changed with the window; the next draw updates the context.
    SIZE.with(|s| s.set((0, 0)));
    tracing::info!("embedded mpv video view followed the webview into another window");
}

/// An OpenGL 3.2 core context for `view`, made current, with vsync off.
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
    gl.makeCurrentContext();
    let off: c_int = 0;
    // setValues:forParameter: takes a GLint pointer.
    let _: () = unsafe {
        msg_send![&*gl, setValues: &off as *const c_int, forParameter: NSOpenGLContextParameter::SwapInterval]
    };
    Ok(gl)
}

fn create_render_context(lib: &'static Lib, handle: Handle) -> Result<RenderContext, String> {
    let framework = OPENGL.get_or_try_init(|| ffi::open(OPENGL_FRAMEWORK).map(|h| h as usize))?;
    if *framework == 0 {
        return Err("The OpenGL framework could not be opened".to_string());
    }
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

/// mpv's handle, carried into the main-thread callback; mpv's client API is thread-safe.
struct SendHandle(Handle);
unsafe impl Send for SendHandle {}

impl SendHandle {
    /// Taking `self` makes a closure capture the whole wrapper, not its raw pointer.
    fn get(self) -> Handle {
        self.0
    }
}

/// Builds the view, context and render context under the WKWebView `webview`.
fn attach_under(webview: *mut c_void, lib: &'static Lib, handle: Handle) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("The video layer must be set up on the main thread")?;
    let webview = unsafe { (webview as *mut NSView).as_ref() }.ok_or("The webview was not found")?;
    let parent = unsafe { webview.superview() }.ok_or("The webview has no parent view")?;

    let view = NSView::initWithFrame(NSView::alloc(mtm), parent.bounds());
    // Layer-backed, so the system composites it under the (layer-backed) webview.
    view.setWantsLayer(true);
    view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
    // Draw at the screen's full resolution on Retina displays.
    let _: () = unsafe { msg_send![&*view, setWantsBestResolutionOpenGLSurface: true] };
    parent.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, Some(webview));

    let gl = create_gl(&view, mtm)?;
    let context = create_render_context(lib, handle)?;
    let _ = LIB.set(lib);
    WEBVIEW.with(|w| *w.borrow_mut() = Some(webview.retain()));
    VIEW.with(|v| *v.borrow_mut() = Some(view));
    GL.with(|g| *g.borrow_mut() = Some(gl));
    CONTEXT.with(|c| c.set(context));
    schedule_draw();
    Ok(())
}

/// Frees mpv's render context; it must go before mpv itself (called when the app exits).
pub fn detach() {
    let Some(lib) = LIB.get().copied() else { return };
    if MainThreadMarker::new().is_none() {
        return;
    }
    if let Some(gl) = GL.with(|g| g.borrow().clone()) {
        gl.makeCurrentContext();
    }
    let context = CONTEXT.with(|c| c.replace(null_mut()));
    if !context.is_null() {
        unsafe { (lib.render_context_free)(context) };
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
            schedule_draw();
        }
    });
    Ok(())
}

