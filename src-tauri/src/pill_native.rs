// The pill, drawn natively (Windows).
//
// A small per-pixel-alpha layered window painted with Direct2D. Windows
// hit-tests a layered window by its alpha, so every transparent pixel lets
// clicks straight through to whatever is underneath: no cursor polling, no
// click-through toggling. The window never takes focus.
//
// It costs nothing when idle. The pill's thread renders frames only while
// something on it is moving (a transition, the ribbon, the processing
// slider), paced to the display with DwmFlush. The rest of the time it sleeps
// on its message queue, or on a single timeout when a state is due to end.
//
// It is a faithful port of overlay/pill.css and pill.js, which remain the
// pill on other platforms. Sizes are in logical pixels; the render target's
// DPI maps them to the screen.

#![cfg(windows)]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::AppHandle;
use windows::core::w;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Foundation::{BOOL, COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN, D2D1_PIXEL_FORMAT,
    D2D_POINT_2F, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush, ID2D1StrokeStyle,
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_CAP_STYLE_ROUND, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_LINE_JOIN_ROUND,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_MEDIUM,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE,
    IDWriteFactory5, IDWriteFontCollection, IDWriteFontFileStream, DWRITE_CONTAINER_TYPE_WOFF2,
};
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetMonitorInfoW, MonitorFromPoint, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC,
    HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::WTSRegisterSessionNotification;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, KillTimer, LoadCursorW, SetTimer, WM_TIMER, MsgWaitForMultipleObjects, PeekMessageW,
    PostMessageW, RegisterClassW, SetCursor, SetWindowPos, ShowWindow, TranslateMessage, UpdateLayeredWindow,
    WaitMessage, HTCLIENT, HWND_TOPMOST, IDC_HAND, MA_NOACTIVATE, MSG, PM_REMOVE, QS_ALLINPUT, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA, WINDOW_EX_STYLE, WM_APP, WM_DISPLAYCHANGE,
    WM_DPICHANGED, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_QUIT,
    WM_CLOSE, WM_SETCURSOR, WM_SETTINGCHANGE, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP,
};

use crate::overlay::State;
use crate::session::{self, Input};

const WIDTH: f32 = 440.0;
const HEIGHT: f32 = 120.0;
const PAD_BOTTOM: f32 = 8.0;
const WM_WAKE: u32 = WM_APP + 1;
/// A press on the idle pill that lasts this long is a hold (hold-to-talk);
/// anything shorter is a click (hands-free on, then off).
const HOLD_MS: u32 = 260;
const HOLD_TIMER: usize = 1;
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
const WTS_SESSION_UNLOCK: usize = 0x8;
const WM_POWERBROADCAST: u32 = 0x0218;
const PBT_APMRESUMEAUTOMATIC: usize = 0x12;
const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
const WINEVENT_OUTOFCONTEXT: u32 = 0x0000;

// ---------- the public face, callable from any thread ----------

enum Cmd {
    State(State),
    Offer,
    Error(String),
    Show,
    Hide,
}

static APP: OnceLock<AppHandle> = OnceLock::new();
static HWND_RAW: AtomicIsize = AtomicIsize::new(0);
static QUEUE: Mutex<VecDeque<Cmd>> = Mutex::new(VecDeque::new());
static LEVEL: AtomicU32 = AtomicU32::new(0);

fn push(cmd: Cmd) {
    QUEUE.lock().unwrap().push_back(cmd);
    let h = HWND_RAW.load(Ordering::Acquire);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(HWND(h as _), WM_WAKE, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn set_state(state: State) {
    push(Cmd::State(state));
}
pub fn set_offer() {
    push(Cmd::Offer);
}
pub fn set_error(message: &str) {
    push(Cmd::Error(message.to_string()));
}
pub fn set_level(level: f32) {
    LEVEL.store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
}
pub fn show() {
    push(Cmd::Show);
}
pub fn hide() {
    push(Cmd::Hide);
}

/// Starts the pill on its own thread. `visible`: whether it starts shown.
pub fn start(app: AppHandle, visible: bool) {
    let _ = APP.set(app);
    // Dev: BLURT_PILL_DEMO=<state> in a debug build holds the pill in one
    // state (with a fake voice for listening), for screenshots.
    if cfg!(debug_assertions) {
        if let Ok(demo) = std::env::var("BLURT_PILL_DEMO") {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(800));
                let st = match demo.as_str() {
                    "listening" => Some(State::Listening),
                    "handsfree" => Some(State::Handsfree),
                    "processing" => Some(State::Processing),
                    "done" => Some(State::Done),
                    _ => None,
                };
                match demo.as_str() {
                    "offer" => set_offer(),
                    "error" => set_error("Microphone not available"),
                    _ => {}
                }
                if let Some(s) = st {
                    set_state(s);
                }
                let mut t = 0f32;
                loop {
                    t += 0.04;
                    set_level(0.55 + 0.35 * (t * 2.1).sin());
                    std::thread::sleep(Duration::from_millis(16));
                }
            });
        }
    }
    std::thread::Builder::new()
        .name("pill".into())
        .spawn(move || unsafe {
            // The pill must animate smoothly even while the model is using
            // every core to transcribe.
            use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL};
            let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
            if let Err(e) = run(visible) {
                log::error!("pill failed: {e}");
            }
        })
        .ok();
}

fn demo() -> bool {
    cfg!(debug_assertions) && std::env::var("BLURT_PILL_DEMO").is_ok()
}

// ---------- easing ----------

/// CSS cubic-bezier(x1, y1, x2, y2), solved for y at x.
#[derive(Clone, Copy)]
struct Bez(f32, f32, f32, f32);
impl Bez {
    fn at(self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        let Bez(x1, y1, x2, y2) = self;
        let c = |t: f32, a: f32, b: f32| 3.0 * (1.0 - t) * (1.0 - t) * t * a + 3.0 * (1.0 - t) * t * t * b + t * t * t;
        let mut t = x;
        for _ in 0..8 {
            let fx = c(t, x1, x2) - x;
            let d = 3.0 * (1.0 - t) * (1.0 - t) * x1 + 6.0 * (1.0 - t) * t * (x2 - x1) + 3.0 * t * t * (1.0 - x2);
            if d.abs() < 1e-5 {
                break;
            }
            t = (t - fx / d).clamp(0.0, 1.0);
        }
        c(t, y1, y2)
    }
}
const EASE: Bez = Bez(0.25, 0.1, 0.25, 1.0);
const EASE_OUT: Bez = Bez(0.22, 1.0, 0.36, 1.0);
const SPRING: Bez = Bez(0.34, 1.35, 0.64, 1.0);
const RISE: Bez = Bez(0.2, 0.9, 0.3, 1.05);
const SQUEEZE: Bez = Bez(0.5, 0.0, 0.75, 0.0);
const SLIDE: Bez = Bez(0.45, 0.0, 0.55, 1.0);

/// A value that eases from where it is to a target.
#[derive(Clone, Copy)]
struct Tween {
    from: f32,
    to: f32,
    t0: Instant,
    dur: f32,
    ease: Bez,
}
impl Tween {
    fn new(v: f32) -> Self {
        Self { from: v, to: v, t0: Instant::now(), dur: 0.0, ease: EASE }
    }
    fn get(&self, now: Instant) -> f32 {
        if self.dur <= 0.0 {
            return self.to;
        }
        let k = (now - self.t0).as_secs_f32() / self.dur;
        if k >= 1.0 {
            return self.to;
        }
        self.from + (self.to - self.from) * self.ease.at(k)
    }
    fn to(&mut self, v: f32, ms: f32, ease: Bez, now: Instant) {
        if (self.to - v).abs() < 1e-6 && !self.moving(now) {
            return;
        }
        self.from = self.get(now);
        self.to = v;
        self.t0 = now;
        self.dur = ms / 1000.0;
        self.ease = ease;
    }
    fn snap(&mut self, v: f32) {
        self.from = v;
        self.to = v;
        self.dur = 0.0;
    }
    fn moving(&self, now: Instant) -> bool {
        self.dur > 0.0 && (now - self.t0).as_secs_f32() < self.dur
    }
}

// ---------- the model ----------

struct Model {
    state: State,
    offer: bool,
    ready: bool,
    copied: bool,
    hover: bool,
    pressed: bool,
    /// a hold-to-talk take started by holding the pill
    holding: bool,
    hover_ctl: i8, // -1 cancel, 1 stop, 0 none
    tip_text: Vec<(String, bool)>, // runs; true = accent
    // geometry and layers
    w: Tween,
    h: Tween,
    line_a: Tween,
    line_hover: Tween,
    wave_a: Tween,
    ctl_a: Tween,
    think_a: Tween,
    done_a: Tween,
    badge_s: Tween,
    badge_a: Tween,
    copy_a: Tween,
    copy_y: Tween,
    err_a: Tween,
    tip_a: Tween,
    tip_y: Tween,
    border_err: Tween,
    press: Tween,
    ready_bg: Tween,
    // appear / vanish: 1 shown, 0 squeezed away
    vis: Tween,
    hide_after: Option<Instant>,
    shake_t0: Option<Instant>,
    // deadlines
    to_idle: Option<Instant>,
    to_ready: Option<Instant>,
    tip_off: Option<Instant>,
    // the ribbon
    amp: f32,
    vel: f32,
    phase: f32,
    grow: f32,
    think_t0: Instant,
    last: Option<Instant>,
}

impl Model {
    fn new(visible: bool) -> Self {
        let now = Instant::now();
        let mut m = Self {
            state: State::Idle,
            offer: false,
            ready: false,
            copied: false,
            hover: false,
            pressed: false,
            holding: false,
            hover_ctl: 0,
            tip_text: Vec::new(),
            w: Tween::new(84.0),
            h: Tween::new(26.0),
            line_a: Tween::new(1.0),
            line_hover: Tween::new(0.0),
            wave_a: Tween::new(0.0),
            ctl_a: Tween::new(0.0),
            think_a: Tween::new(0.0),
            done_a: Tween::new(0.0),
            badge_s: Tween::new(0.5),
            badge_a: Tween::new(0.0),
            copy_a: Tween::new(0.0),
            copy_y: Tween::new(4.0),
            err_a: Tween::new(0.0),
            tip_a: Tween::new(0.0),
            tip_y: Tween::new(6.0),
            border_err: Tween::new(0.0),
            press: Tween::new(1.0),
            ready_bg: Tween::new(0.0),
            vis: Tween::new(0.0),
            hide_after: None,
            shake_t0: None,
            to_idle: None,
            to_ready: None,
            tip_off: None,
            amp: 0.0,
            vel: 0.0,
            phase: 0.0,
            grow: 0.0,
            think_t0: now,
            last: None,
        };
        if visible {
            m.vis.to(1.0, 420.0, RISE, now);
        }
        m
    }

    fn set_state(&mut self, next: State, now: Instant) {
        if next == self.state {
            return;
        }
        let prev = self.state;
        self.state = next;
        self.to_idle = None;
        let listening = matches!(next, State::Listening | State::Handsfree);
        let was_listening = matches!(prev, State::Listening | State::Handsfree);
        if listening && !was_listening {
            self.grow = 0.0;
            self.amp = 0.0;
            self.vel = 0.0;
            LEVEL.store(0f32.to_bits(), Ordering::Relaxed);
        }
        let (w, h) = match next {
            State::Handsfree => (150.0, 30.0),
            State::Done | State::Error => (84.0, 28.0),
            _ => (84.0, 26.0),
        };
        self.w.to(w, 280.0, SPRING, now);
        self.h.to(h, 280.0, SPRING, now);
        self.line_a.to(if next == State::Idle { 1.0 } else { 0.0 }, 160.0, EASE, now);
        self.wave_a.to(if listening { 1.0 } else { 0.0 }, 160.0, EASE, now);
        self.ctl_a.to(if next == State::Handsfree { 1.0 } else { 0.0 }, 160.0, EASE, now);
        if next == State::Processing {
            self.think_t0 = now;
        }
        self.think_a.to(if next == State::Processing { 1.0 } else { 0.0 }, 160.0, EASE, now);
        if next == State::Done {
            self.done_a.to(1.0, 160.0, EASE, now);
            self.badge_a.to(1.0, 160.0, EASE, now);
            self.badge_s.snap(0.5);
            self.badge_s.to(1.0, 360.0, SPRING, now);
            self.to_idle = (!demo()).then(|| now + Duration::from_millis(1200));
        } else {
            self.done_a.to(0.0, 520.0, EASE_OUT, now);
            self.badge_a.to(0.0, 520.0, EASE_OUT, now);
            self.badge_s.to(0.5, 520.0, EASE_OUT, now);
        }
        if next == State::Error {
            self.err_a.to(1.0, 160.0, EASE, now);
            self.border_err.to(1.0, 200.0, EASE, now);
            self.shake_t0 = Some(now);
            self.to_idle = (!demo()).then(|| now + Duration::from_millis(2200));
        } else {
            self.err_a.to(0.0, 160.0, EASE, now);
            self.border_err.to(0.0, 200.0, EASE, now);
        }
        if next != State::Idle {
            self.show_tip(false, now);
        }
        if next != State::Done {
            self.offer = false;
            self.ready = false;
            self.copied = false;
            self.to_ready = None;
            self.copy_a.to(0.0, 220.0, EASE, now);
            self.copy_y.to(4.0, 260.0, EASE_OUT, now);
            self.ready_bg.to(0.0, 200.0, EASE, now);
        }
    }

    fn set_offer(&mut self, now: Instant) {
        if self.state != State::Done {
            self.set_state(State::Done, now);
        }
        self.offer = true;
        self.to_ready = Some(now + Duration::from_millis(1200));
        self.to_idle = (!demo()).then(|| now + Duration::from_millis(6000));
    }

    fn become_ready(&mut self, now: Instant) {
        self.ready = true;
        self.w.to(100.0, 280.0, SPRING, now);
        self.done_a.to(0.0, 160.0, EASE, now);
        self.copy_a.to(1.0, 220.0, EASE, now);
        self.copy_y.to(0.0, 260.0, EASE_OUT, now);
    }

    fn set_error(&mut self, message: String, now: Instant) {
        self.set_state(State::Error, now);
        if !message.is_empty() {
            self.tip_text = vec![(message, false)];
            self.show_tip(true, now);
            self.tip_off = (!demo()).then(|| now + Duration::from_millis(2100));
        }
    }

    fn show_tip(&mut self, on: bool, now: Instant) {
        self.tip_a.to(if on { 1.0 } else { 0.0 }, 160.0, EASE, now);
        self.tip_y.to(if on { 0.0 } else { 6.0 }, 220.0, EASE_OUT, now);
        if !on {
            self.tip_off = None;
        }
    }

    fn set_hover(&mut self, on: bool, now: Instant) {
        if on == self.hover {
            return;
        }
        self.hover = on;
        self.line_hover.to(if on { 1.0 } else { 0.0 }, if on { 240.0 } else { 320.0 }, EASE, now);
        if self.ready {
            self.ready_bg.to(if on { 1.0 } else { 0.0 }, 160.0, EASE, now);
        }
        if !on {
            self.hover_ctl = 0;
            // a held pill keeps its press while the cursor wanders off it
            if self.pressed && !self.holding {
                self.pressed = false;
                self.press.to(1.0, 200.0, EASE_OUT, now);
            }
        }
    }

    /// Advance the deadlines; returns the next one, if any.
    fn tick(&mut self, now: Instant) -> Option<Instant> {
        if let Some(t) = self.to_ready {
            if now >= t {
                self.to_ready = None;
                self.become_ready(now);
            }
        }
        if let Some(t) = self.tip_off {
            if now >= t {
                self.tip_off = None;
                self.show_tip(false, now);
            }
        }
        if let Some(t) = self.to_idle {
            if now >= t {
                self.to_idle = None;
                self.set_state(State::Idle, now);
            }
        }
        [self.to_ready, self.tip_off, self.to_idle, self.hide_after].into_iter().flatten().min()
    }

    /// Whether anything is moving this instant.
    fn animating(&self, now: Instant) -> bool {
        let listening = matches!(self.state, State::Listening | State::Handsfree);
        let tweens = [
            &self.w, &self.h, &self.line_a, &self.line_hover, &self.wave_a, &self.ctl_a, &self.think_a, &self.done_a,
            &self.badge_s, &self.badge_a, &self.copy_a, &self.copy_y, &self.err_a, &self.tip_a, &self.tip_y,
            &self.border_err, &self.press, &self.ready_bg, &self.vis,
        ];
        listening
            || self.state == State::Processing
            || self.think_a.get(now) > 0.001
            || self.wave_a.get(now) > 0.001
            || self.shake_t0.is_some_and(|t| (now - t).as_secs_f32() < 0.36)
            || tweens.iter().any(|t| t.moving(now))
    }
}

// ---------- drawing ----------

// The pill speaks in Inter, like the rest of the app, not the system font.
// The woff2 the window already ships is unpacked and loaded privately; if any
// step fails (older Windows), the pill falls back to Segoe UI Variable.
static INTER_WOFF2: &[u8] = include_bytes!("../../overlay/app/fonts/inter-latin.woff2");

unsafe fn inter_collection(dwrite: &IDWriteFactory) -> windows::core::Result<IDWriteFontCollection> {
    use windows::core::Interface;
    let f5: IDWriteFactory5 = dwrite.cast()?;
    let stream: IDWriteFontFileStream = f5.UnpackFontFile(DWRITE_CONTAINER_TYPE_WOFF2, INTER_WOFF2.as_ptr() as _, INTER_WOFF2.len() as u32)?;
    let size = stream.GetFileSize()?;
    let mut start = std::ptr::null_mut();
    let mut ctx = std::ptr::null_mut();
    stream.ReadFileFragment(&mut start, 0, size, &mut ctx)?;
    let bytes = std::slice::from_raw_parts(start as *const u8, size as usize).to_vec();
    stream.ReleaseFileFragment(ctx);
    let loader = f5.CreateInMemoryFontFileLoader()?;
    f5.RegisterFontFileLoader(&loader)?;
    // no owner: the loader keeps its own copy of the bytes
    let file = loader.CreateInMemoryFontFileReference(&f5, bytes.as_ptr() as _, bytes.len() as u32, None)?;
    let builder = f5.CreateFontSetBuilder()?;
    builder.AddFontFile(&file)?;
    let set = builder.CreateFontSet()?;
    Ok(f5.CreateFontCollectionFromFontSet(&set)?.cast()?)
}

struct Gfx {
    factory: ID2D1Factory,
    dwrite: IDWriteFactory,
    rt: ID2D1DCRenderTarget,
    round: ID2D1StrokeStyle,
    brush: ID2D1SolidColorBrush,
    accent: ID2D1SolidColorBrush,
    tip_fmt: IDWriteTextFormat,
    copy_fmt: IDWriteTextFormat,
    // the pixels
    memdc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    px_w: i32,
    px_h: i32,
    scale: f32,
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: a.clamp(0.0, 1.0) }
}
fn pt(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}
fn rrect(cx: f32, cy: f32, w: f32, h: f32, r: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: cx - w / 2.0, top: cy - h / 2.0, right: cx + w / 2.0, bottom: cy + h / 2.0 }, radiusX: r, radiusY: r }
}
/// Scale about (ox, oy) then translate.
fn xform(sx: f32, sy: f32, ox: f32, oy: f32, tx: f32, ty: f32) -> Matrix3x2 {
    Matrix3x2 { M11: sx, M12: 0.0, M21: 0.0, M22: sy, M31: ox - ox * sx + tx, M32: oy - oy * sy + ty }
}
/// a then b.
fn mul(a: &Matrix3x2, b: &Matrix3x2) -> Matrix3x2 {
    Matrix3x2 {
        M11: a.M11 * b.M11 + a.M12 * b.M21,
        M12: a.M11 * b.M12 + a.M12 * b.M22,
        M21: a.M21 * b.M11 + a.M22 * b.M21,
        M22: a.M21 * b.M12 + a.M22 * b.M22,
        M31: a.M31 * b.M11 + a.M32 * b.M21 + b.M31,
        M32: a.M31 * b.M12 + a.M32 * b.M22 + b.M32,
    }
}
const IDENTITY: Matrix3x2 = Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: 0.0, M32: 0.0 };

impl Gfx {
    unsafe fn new(hwnd: HWND) -> windows::core::Result<Self> {
        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0 * scale,
            dpiY: 96.0 * scale,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let rt = factory.CreateDCRenderTarget(&props)?;
        rt.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        let round = factory.CreateStrokeStyle(
            &D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                ..Default::default()
            },
            None,
        )?;
        let brush = rt.CreateSolidColorBrush(&rgba(255, 255, 255, 1.0), None)?;
        let accent = rt.CreateSolidColorBrush(&rgba(110, 168, 255, 1.0), None)?;
        let inter = inter_collection(&dwrite).ok();
        let fmt = |size: f32, weight: DWRITE_FONT_WEIGHT| match &inter {
            Some(c) => dwrite.CreateTextFormat(w!("Inter"), c, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("en-us")),
            None => dwrite.CreateTextFormat(w!("Segoe UI Variable Text"), None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("en-us")),
        };
        let tip_fmt = fmt(13.0, DWRITE_FONT_WEIGHT_MEDIUM)?;
        let copy_fmt = fmt(12.5, DWRITE_FONT_WEIGHT_MEDIUM)?;
        let mut g = Self {
            factory,
            dwrite,
            rt,
            round,
            brush,
            accent,
            tip_fmt,
            copy_fmt,
            memdc: HDC::default(),
            bmp: HBITMAP::default(),
            old: HGDIOBJ::default(),
            px_w: 0,
            px_h: 0,
            scale,
        };
        g.resize(scale)?;
        Ok(g)
    }

    unsafe fn resize(&mut self, scale: f32) -> windows::core::Result<()> {
        if !self.memdc.is_invalid() {
            SelectObject(self.memdc, self.old);
            let _ = DeleteObject(self.bmp);
            let _ = DeleteDC(self.memdc);
        }
        self.scale = scale;
        self.rt.SetDpi(96.0 * scale, 96.0 * scale);
        self.px_w = (WIDTH * scale).round() as i32;
        self.px_h = (HEIGHT * scale).round() as i32;
        self.memdc = CreateCompatibleDC(None);
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: self.px_w,
                biHeight: -self.px_h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        self.bmp = CreateDIBSection(self.memdc, &bi, DIB_RGB_COLORS, &mut bits, None, 0)?;
        self.old = SelectObject(self.memdc, self.bmp);
        let rc = RECT { left: 0, top: 0, right: self.px_w, bottom: self.px_h };
        self.rt.BindDC(self.memdc, &rc)?;
        Ok(())
    }

    unsafe fn color(&self, c: D2D1_COLOR_F) -> &ID2D1SolidColorBrush {
        self.brush.SetColor(&c);
        &self.brush
    }

    unsafe fn text_layout(&self, runs: &[(String, bool)], fmt: &IDWriteTextFormat) -> windows::core::Result<(IDWriteTextLayout, f32, f32)> {
        let full: String = runs.iter().map(|(s, _)| s.as_str()).collect();
        let wide: Vec<u16> = full.encode_utf16().collect();
        let layout = self.dwrite.CreateTextLayout(&wide, fmt, 2000.0, 100.0)?;
        let mut at = 0u32;
        for (s, accent) in runs {
            let n = s.encode_utf16().count() as u32;
            if *accent {
                let range = DWRITE_TEXT_RANGE { startPosition: at, length: n };
                layout.SetDrawingEffect(&self.accent, range)?;
                layout.SetFontWeight(DWRITE_FONT_WEIGHT_SEMI_BOLD, range)?;
            }
            at += n;
        }
        let mut m = DWRITE_TEXT_METRICS::default();
        layout.GetMetrics(&mut m)?;
        Ok((layout, m.widthIncludingTrailingWhitespace, m.height))
    }

    /// Ink body with its soft halo and hairline edge.
    unsafe fn body(&self, cx: f32, cy: f32, w: f32, h: f32, fill: D2D1_COLOR_F, edge: D2D1_COLOR_F, alpha: f32) {
        let r = h / 2.0;
        // the halo: a few widening rings, fading out
        for i in 0..6 {
            let g = 1.0 + i as f32 * 2.0;
            let a = 0.22 * alpha * (1.0 - i as f32 / 6.0) * 0.32;
            self.rt.DrawRoundedRectangle(&rrect(cx, cy, w + g, h + g, r + g / 2.0), self.color(rgba(0, 0, 0, a)), 2.0, None);
        }
        self.rt.DrawRoundedRectangle(&rrect(cx, cy, w + 1.0, h + 1.0, r + 0.5), self.color(rgba(0, 0, 0, 0.18 * alpha)), 1.0, None);
        self.rt.FillRoundedRectangle(&rrect(cx, cy, w, h, r), self.color(D2D1_COLOR_F { a: fill.a * alpha, ..fill }));
        self.rt.DrawRoundedRectangle(&rrect(cx, cy, w - 1.0, h - 1.0, r - 0.5), self.color(D2D1_COLOR_F { a: edge.a * alpha, ..edge }), 1.0, None);
    }

    unsafe fn render(&self, m: &mut Model, now: Instant) -> windows::core::Result<()> {
        // the ribbon's physics, per frame
        let dt = m.last.map(|l| (now - l).as_secs_f32().min(0.05)).unwrap_or(0.016);
        m.last = Some(now);
        let level = f32::from_bits(LEVEL.load(Ordering::Relaxed));
        if matches!(m.state, State::Listening | State::Handsfree) {
            let w = 2.0 * std::f32::consts::PI * 10.0;
            let e = (-w * dt).exp();
            let x = m.amp - level;
            let nx = (x + (m.vel + w * x) * dt) * e;
            m.vel = (m.vel - w * (m.vel + w * x) * dt) * e;
            m.amp = level + nx;
            m.grow = (m.grow + dt / 0.3).min(1.0);
            m.phase += dt * 4.5;
        }

        let rt = &self.rt;
        rt.BeginDraw();
        rt.SetTransform(&IDENTITY);
        rt.Clear(Some(&rgba(0, 0, 0, 0.0)));

        let vis = m.vis.get(now);
        let squeezing = m.vis.to < 0.5;
        let k = if squeezing { 1.0 - vis } else { 1.0 - vis };
        // appear / vanish: scale about the bottom centre of the pill
        let (sx, sy, ty) = (1.0 - 0.1 * k, 1.0 - 0.96 * k, 6.0 * k);
        let pw = m.w.get(now);
        let ph = m.h.get(now);
        let cx = WIDTH / 2.0;
        let bottom = HEIGHT - PAD_BOTTOM;
        let cy = bottom - ph / 2.0;
        let shake = m.shake_t0.map(|t| {
            let p = ((now - t).as_secs_f32() / 0.36).min(1.0);
            let keys = [(0.0, 0.0), (0.25, -3.0), (0.5, 3.0), (0.75, -2.0), (1.0, 0.0)];
            let mut v = 0.0;
            for win in keys.windows(2) {
                let ((a, va), (b, vb)) = (win[0], win[1]);
                if p >= a && p <= b {
                    let u = EASE.at((p - a) / (b - a));
                    v = va + (vb - va) * u;
                }
            }
            v
        }).unwrap_or(0.0);
        let press = m.press.get(now);
        let alpha = vis.max(0.0);

        // ---- the tip ----
        let tip_a = m.tip_a.get(now) * alpha;
        if tip_a > 0.005 {
            if let Ok((layout, tw, th)) = self.text_layout(&m.tip_text, &self.tip_fmt) {
                let tipw = tw + 36.0;
                let tiph = 36.0;
                let tcy = bottom - ph - 8.0 - tiph / 2.0 + m.tip_y.get(now);
                let s = 0.98 + 0.02 * (1.0 - m.tip_y.get(now) / 6.0);
                rt.SetTransform(&xform(s, s, cx, tcy, 0.0, 0.0));
                self.body(cx, tcy, tipw, tiph, rgba(18, 18, 22, 0.97), rgba(255, 255, 255, 0.24), tip_a);
                self.accent.SetOpacity(tip_a);
                rt.DrawTextLayout(pt(cx - tw / 2.0, tcy - th / 2.0), &layout, self.color(rgba(244, 244, 238, tip_a)), D2D1_DRAW_TEXT_OPTIONS_NONE);
                self.accent.SetOpacity(1.0);
                rt.SetTransform(&IDENTITY);
            }
        }

        // ---- the pill ----
        let t = xform(sx * press, sy * press, cx, if squeezing || k > 0.0 { bottom } else { cy }, shake, ty);
        rt.SetTransform(&t);
        let err = m.border_err.get(now);
        let edge = if err > 0.0 {
            let base = rgba(255, 255, 255, 0.24);
            let red = rgba(255, 107, 92, 0.45);
            D2D1_COLOR_F { r: base.r + (red.r - base.r) * err, g: base.g + (red.g - base.g) * err, b: base.b + (red.b - base.b) * err, a: base.a + (red.a - base.a) * err }
        } else if m.hover {
            rgba(255, 255, 255, 0.2)
        } else {
            rgba(255, 255, 255, 0.24)
        };
        let rb = m.ready_bg.get(now);
        // No backdrop blur natively, so the ink is a touch more opaque than the CSS 0.92.
        let fill = rgba((18.0 + 16.0 * rb) as u8, (18.0 + 16.0 * rb) as u8, (22.0 + 18.0 * rb) as u8, 0.97);
        self.body(cx, cy, pw, ph, fill, edge, alpha);

        // idle line
        let line_a = m.line_a.get(now) * alpha;
        if line_a > 0.005 {
            // hovered, the line turns sky blue and reaches a little further:
            // a click starts dictating
            let k = m.line_hover.get(now);
            let mix = |a: f32, b: f32| (a + (b - a) * k) as u8;
            let half = 19.0 + 5.0 * k;
            let col = rgba(mix(244.0, 125.0), mix(244.0, 178.0), mix(238.0, 255.0), (0.55 + 0.45 * k) * line_a);
            rt.DrawLine(pt(cx - half, cy), pt(cx + half, cy), self.color(col), 2.0 + 0.4 * k, &self.round);
        }

        // the ribbon
        let wave_a = m.wave_a.get(now) * alpha;
        if wave_a > 0.005 {
            let inset = if m.state == State::Handsfree || m.ctl_a.get(now) > 0.5 { 32.0 } else { 0.0 };
            let lw = pw - inset * 2.0;
            let full = lw - 12.0;
            let e = 1.0 - (1.0 - m.grow).powi(3);
            let span = 40.0 + (full - 40.0) * e;
            let x0 = cx - span / 2.0;
            let a = ph * 0.36 * m.amp;
            if e < 1.0 {
                rt.DrawLine(pt(x0, cy), pt(x0 + span, cy), self.color(rgba(244, 244, 238, 0.42 * (1.0 - e) * wave_a)), 2.0, &self.round);
            }
            let geom = self.factory.CreatePathGeometry()?;
            let sink = geom.Open()?;
            let n = span.max(2.0).ceil() as usize;
            let mut pts = Vec::with_capacity(n + 1);
            for i in 0..=n {
                let t = i as f32 / n as f32;
                let env = (std::f32::consts::PI * t).sin().powi(2);
                let y = cy + (t * std::f32::consts::PI * 2.0 * 2.0 + m.phase).sin() * a * env;
                pts.push(pt(x0 + span * t, y));
            }
            sink.BeginFigure(pts[0], D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddLines(&pts[1..]);
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            let wa = wave_a * e;
            rt.DrawGeometry(&geom, self.color(rgba(110, 168, 255, 0.16 * wa)), 6.0, &self.round);
            rt.DrawGeometry(&geom, self.color(rgba(110, 168, 255, wa)), 2.2, &self.round);
        }

        // hands-free controls
        let ctl_a = m.ctl_a.get(now) * alpha;
        if ctl_a > 0.005 {
            let cl = cx - pw / 2.0 + 7.0 + 11.0;
            let cr = cx + pw / 2.0 - 7.0 - 11.0;
            let ca = if m.hover_ctl == -1 { 0.16 } else { 0.08 };
            rt.FillEllipse(&D2D1_ELLIPSE { point: pt(cl, cy), radiusX: 11.0, radiusY: 11.0 }, self.color(rgba(255, 255, 255, ca * ctl_a)));
            let s = 11.0 / 16.0;
            let g = |x: f32, y: f32| pt(cl + (x - 8.0) * s, cy + (y - 8.0) * s);
            rt.DrawLine(g(4.0, 4.0), g(12.0, 12.0), self.color(rgba(244, 244, 238, ctl_a)), 2.0 * s, &self.round);
            rt.DrawLine(g(12.0, 4.0), g(4.0, 12.0), self.color(rgba(244, 244, 238, ctl_a)), 2.0 * s, &self.round);
            let (sr, sg, sb) = if m.hover_ctl == 1 { (141, 187, 255) } else { (110, 168, 255) };
            rt.FillEllipse(&D2D1_ELLIPSE { point: pt(cr, cy), radiusX: 11.0, radiusY: 11.0 }, self.color(rgba(sr, sg, sb, ctl_a)));
            let s = 14.0 / 16.0;
            let r = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: cr - 5.0 * s, top: cy - 5.0 * s, right: cr + 5.0 * s, bottom: cy + 5.0 * s }, radiusX: 2.0 * s, radiusY: 2.0 * s };
            rt.FillRoundedRectangle(&r, self.color(rgba(11, 26, 51, ctl_a)));
        }

        // processing: a blue slider along the line
        let think_a = m.think_a.get(now) * alpha;
        if think_a > 0.005 {
            rt.DrawLine(pt(cx - 19.0, cy), pt(cx + 19.0, cy), self.color(rgba(244, 244, 238, 0.42 * think_a)), 2.0, &self.round);
            let p = ((now - m.think_t0).as_secs_f32() / 1.3) % 2.0;
            let u = SLIDE.at(if p < 1.0 { p } else { 2.0 - p });
            let left = cx - 20.0 + 26.0 * u;
            rt.DrawLine(pt(left + 2.0, cy), pt(left + 12.0, cy), self.color(rgba(110, 168, 255, 0.25 * think_a)), 6.0, &self.round);
            rt.DrawLine(pt(left + 2.0, cy), pt(left + 12.0, cy), self.color(rgba(110, 168, 255, think_a)), 4.0, &self.round);
        }

        // done: the check
        let badge_a = m.badge_a.get(now) * m.done_a.get(now).max(0.0) * alpha;
        if badge_a > 0.005 {
            let s = m.badge_s.get(now);
            rt.SetTransform(&mul(&xform(s, s, cx, cy, 0.0, 0.0), &t));
            rt.FillEllipse(&D2D1_ELLIPSE { point: pt(cx, cy), radiusX: 9.0, radiusY: 9.0 }, self.color(rgba(110, 168, 255, badge_a)));
            let gs = 12.0 / 16.0;
            let g = |x: f32, y: f32| pt(cx + (x - 8.0) * gs, cy + (y - 8.0) * gs);
            let geom = self.factory.CreatePathGeometry()?;
            let sink = geom.Open()?;
            sink.BeginFigure(g(3.5, 8.5), D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddLines(&[g(6.5, 11.5), g(12.5, 5.0)]);
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            rt.DrawGeometry(&geom, self.color(rgba(11, 26, 51, badge_a)), 2.6 * gs, &self.round);
            rt.SetTransform(&t);
        }

        // the empty-paste offer: the pill becomes a Copy button
        let copy_a = m.copy_a.get(now) * alpha;
        if copy_a > 0.005 {
            let label = if m.copied { "Copied" } else { "Copy" };
            if let Ok((layout, tw, th)) = self.text_layout(&[(label.to_string(), false)], &self.copy_fmt) {
                let total = 12.0 + 6.0 + tw;
                let x = cx - total / 2.0;
                let y = cy + m.copy_y.get(now);
                let s = 12.0 / 16.0;
                let g = |px: f32, py: f32| pt(x + px * s, y - 6.0 + py * s);
                let blue = self.color(rgba(110, 168, 255, copy_a)).clone();
                let r = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: g(5.0, 5.0).x, top: g(5.0, 5.0).y, right: g(13.0, 13.0).x, bottom: g(13.0, 13.0).y }, radiusX: 2.0 * s, radiusY: 2.0 * s };
                rt.DrawRoundedRectangle(&r, &blue, 1.8 * s, &self.round);
                let geom = self.factory.CreatePathGeometry()?;
                let sink = geom.Open()?;
                sink.BeginFigure(g(3.0, 11.0), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddLines(&[g(3.0, 4.0), g(4.0, 3.0), g(11.0, 3.0)]);
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.Close()?;
                rt.DrawGeometry(&geom, &blue, 1.8 * s, &self.round);
                // whole device pixels, or 12px text goes soft
                let snap = |v: f32| (v * self.scale).round() / self.scale;
                rt.DrawTextLayout(pt(snap(x + 18.0), snap(y - th / 2.0)), &layout, self.color(rgba(244, 244, 238, copy_a)), D2D1_DRAW_TEXT_OPTIONS_NONE);
            }
        }

        // error: the red badge
        let err_a = m.err_a.get(now) * alpha;
        if err_a > 0.005 {
            rt.FillEllipse(&D2D1_ELLIPSE { point: pt(cx, cy), radiusX: 9.0, radiusY: 9.0 }, self.color(rgba(255, 107, 92, err_a)));
            let gs = 12.0 / 16.0;
            let g = |x: f32, y: f32| pt(cx + (x - 8.0) * gs, cy + (y - 8.0) * gs);
            let ink = self.color(rgba(42, 10, 6, err_a)).clone();
            rt.DrawLine(g(8.0, 3.5), g(8.0, 9.0), &ink, 2.6 * gs, &self.round);
            rt.DrawLine(g(8.0, 12.2), g(8.0, 12.5), &ink, 2.6 * gs, &self.round);
        }

        rt.SetTransform(&IDENTITY);
        rt.EndDraw(None, None)
    }
}

// ---------- the window ----------

struct Pill {
    hwnd: HWND,
    gfx: Gfx,
    model: Model,
    shown: bool,
    pos: POINT,
}

thread_local! {
    static PILL: RefCell<Option<Pill>> = const { RefCell::new(None) };
}

/// Where the window goes: bottom centre of the primary monitor's work area.
unsafe fn placement(px_w: i32, px_h: i32) -> POINT {
    let mon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
    let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    let _ = GetMonitorInfoW(mon, &mut info);
    let wa = info.rcWork;
    POINT { x: wa.left + ((wa.right - wa.left) - px_w) / 2, y: wa.bottom - px_h }
}

unsafe fn present(p: &mut Pill) {
    let now = Instant::now();
    if p.gfx.render(&mut p.model, now).is_err() {
        // The render target was lost (display change, driver reset): rebuild.
        if let Ok(g) = Gfx::new(p.hwnd) {
            p.gfx = g;
        }
        return;
    }
    let size = SIZE { cx: p.gfx.px_w, cy: p.gfx.px_h };
    let src = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
    let _ = UpdateLayeredWindow(p.hwnd, None, Some(&p.pos), Some(&size), p.gfx.memdc, Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA);
}

unsafe fn topmost(hwnd: HWND) {
    let _ = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
}

unsafe extern "system" fn on_foreground(_: HWINEVENTHOOK, _: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
    // Another window came to the front: make sure it did not land on top of us.
    let h = HWND_RAW.load(Ordering::Acquire);
    if h != 0 {
        topmost(HWND(h as _));
    }
}

/// Which part of the pill a point (logical, window-local) is over.
/// 0 = outside, 1 = body, 2 = cancel, 3 = stop.
fn hit(m: &Model, x: f32, y: f32) -> u8 {
    let now = Instant::now();
    let (pw, ph) = (m.w.get(now), m.h.get(now));
    let cx = WIDTH / 2.0;
    let cy = HEIGHT - PAD_BOTTOM - ph / 2.0;
    let inside = (x - cx).abs() <= pw / 2.0 + 2.0 && (y - cy).abs() <= ph / 2.0 + 2.0;
    if !inside {
        return 0;
    }
    if m.state == State::Handsfree {
        let cl = cx - pw / 2.0 + 18.0;
        let cr = cx + pw / 2.0 - 18.0;
        if ((x - cl).powi(2) + (y - cy).powi(2)).sqrt() <= 12.0 {
            return 2;
        }
        if ((x - cr).powi(2) + (y - cy).powi(2)).sqrt() <= 12.0 {
            return 3;
        }
    }
    1
}

fn send(input: Input) {
    if let Some(app) = APP.get() {
        session::send(app, input);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let local = |p: &Pill| {
        let x = (lp.0 & 0xffff) as i16 as f32 / p.gfx.scale;
        let y = ((lp.0 >> 16) & 0xffff) as i16 as f32 / p.gfx.scale;
        (x, y)
    };
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        // The pill is not a window you close: a stray WM_CLOSE (Alt+F4 sent
        // to the wrong window, a tool closing "the main window") is ignored.
        WM_CLOSE => return LRESULT(0),
        WM_SETCURSOR => {
            if (lp.0 & 0xffff) as u32 == HTCLIENT {
                if let Ok(c) = LoadCursorW(None, IDC_HAND) {
                    SetCursor(c);
                }
                return LRESULT(1);
            }
        }
        WM_MOUSEMOVE => {
            let mut track = false;
            PILL.with(|cell| {
                if let Some(p) = cell.borrow_mut().as_mut() {
                    let (x, y) = local(p);
                    let now = Instant::now();
                    let h = hit(&p.model, x, y);
                    if !p.model.hover && h > 0 {
                        track = true;
                    }
                    p.model.set_hover(h > 0, now);
                    p.model.hover_ctl = match h { 2 => -1, 3 => 1, _ => 0 };
                }
            });
            if track {
                let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut tme);
            }
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            PILL.with(|cell| {
                if let Some(p) = cell.borrow_mut().as_mut() {
                    p.model.set_hover(false, Instant::now());
                }
            });
            return LRESULT(0);
        }
        WM_LBUTTONDOWN => {
            PILL.with(|cell| {
                if let Some(p) = cell.borrow_mut().as_mut() {
                    let (x, y) = local(p);
                    if hit(&p.model, x, y) == 1 {
                        p.model.pressed = true;
                        p.model.press.to(0.97, 200.0, EASE_OUT, Instant::now());
                        SetCapture(hwnd);
                        if p.model.state == State::Idle {
                            SetTimer(hwnd, HOLD_TIMER, HOLD_MS, None);
                        }
                    }
                }
            });
            return LRESULT(0);
        }
        WM_TIMER if wp.0 == HOLD_TIMER => {
            let _ = KillTimer(hwnd, HOLD_TIMER);
            let mut start = false;
            PILL.with(|cell| {
                if let Some(p) = cell.borrow_mut().as_mut() {
                    if p.model.pressed && p.model.state == State::Idle {
                        p.model.holding = true;
                        start = true;
                    }
                }
            });
            if start {
                send(Input::PttDown);
            }
            return LRESULT(0);
        }
        WM_LBUTTONUP => {
            let _ = KillTimer(hwnd, HOLD_TIMER);
            let _ = ReleaseCapture();
            // letting go of a held pill ends that take, wherever the cursor is
            let mut ended = false;
            PILL.with(|cell| {
                if let Some(p) = cell.borrow_mut().as_mut() {
                    if p.model.holding {
                        p.model.holding = false;
                        p.model.pressed = false;
                        p.model.press.to(1.0, 200.0, EASE_OUT, Instant::now());
                        ended = true;
                    }
                }
            });
            if ended {
                send(Input::PttUp);
                return LRESULT(0);
            }
            let mut action: Option<Input> = None;
            let mut copy = false;
            PILL.with(|cell| {
                if let Some(p) = cell.borrow_mut().as_mut() {
                    let (x, y) = local(p);
                    let now = Instant::now();
                    p.model.pressed = false;
                    p.model.press.to(1.0, 200.0, EASE_OUT, now);
                    match hit(&p.model, x, y) {
                        2 => action = Some(Input::Cancel),
                        3 => action = Some(Input::PillClick),
                        1 if p.model.ready => {
                            copy = true;
                            p.model.copied = true;
                            p.model.to_idle = Some(now + Duration::from_millis(900));
                        }
                        1 => action = Some(Input::PillClick),
                        _ => {}
                    }
                }
            });
            if copy {
                if let Some(app) = APP.get() {
                    let _ = session::copy_last(app.clone());
                }
            }
            if let Some(a) = action {
                send(a);
            }
            return LRESULT(0);
        }
        WM_DISPLAYCHANGE | WM_SETTINGCHANGE | WM_DPICHANGED => {
            let _ = PostMessageW(hwnd, WM_WAKE, WPARAM(1), LPARAM(0));
        }
        WM_WTSSESSION_CHANGE if wp.0 == WTS_SESSION_UNLOCK => crate::ptt_hook::reinstall(),
        WM_POWERBROADCAST if wp.0 == PBT_APMRESUMEAUTOMATIC => crate::ptt_hook::reinstall(),
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

unsafe fn run(visible: bool) -> windows::core::Result<()> {
    let hinst = GetModuleHandleW(None)?;
    let class = w!("neptune.pill");
    let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst.into(), lpszClassName: class, ..Default::default() };
    RegisterClassW(&wc);
    let ex = WINDOW_EX_STYLE(WS_EX_LAYERED.0 | WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0);
    let hwnd = CreateWindowExW(ex, class, w!("Neptune"), WS_POPUP, 0, 0, 1, 1, None, None, hinst, None)?;
    let gfx = Gfx::new(hwnd)?;
    let pos = placement(gfx.px_w, gfx.px_h);
    let mut pill = Pill { hwnd, gfx, model: Model::new(visible), shown: false, pos };
    if visible {
        present(&mut pill);
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        topmost(hwnd);
        pill.shown = true;
    }
    PILL.with(|cell| *cell.borrow_mut() = Some(pill));
    HWND_RAW.store(hwnd.0 as isize, Ordering::Release);
    let _ = WTSRegisterSessionNotification(hwnd, 0);
    let _hook = SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_foreground), 0, 0, WINEVENT_OUTOFCONTEXT);

    let mut msg = MSG::default();
    loop {
        // Every pending message first.
        let mut replace = false;
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                return Ok(());
            }
            if msg.message == WM_WAKE && msg.wParam.0 == 1 {
                replace = true;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // Commands from other threads, and the frame.
        let mut wait: Option<Option<Instant>> = None; // None = render now; Some(d) = sleep until d (or forever)
        let mut act_show = false;
        let mut act_hide = false;
        PILL.with(|cell| {
            let mut guard = cell.borrow_mut();
            let Some(p) = guard.as_mut() else { return };
            let now = Instant::now();
            if replace {
                let scale = GetDpiForWindow(p.hwnd).max(96) as f32 / 96.0;
                if (scale - p.gfx.scale).abs() > 0.001 {
                    let _ = p.gfx.resize(scale);
                }
                p.pos = placement(p.gfx.px_w, p.gfx.px_h);
            }
            let cmds: Vec<Cmd> = QUEUE.lock().unwrap().drain(..).collect();
            for c in cmds {
                match c {
                    Cmd::State(s) => p.model.set_state(s, now),
                    Cmd::Offer => p.model.set_offer(now),
                    Cmd::Error(m) => p.model.set_error(m, now),
                    Cmd::Show => {
                        p.model.hide_after = None;
                        p.model.vis.to(1.0, 420.0, RISE, now);
                        act_show = !p.shown;
                    }
                    Cmd::Hide => {
                        p.model.vis.to(0.0, 340.0, SQUEEZE, now);
                        p.model.hide_after = Some(now + Duration::from_millis(360));
                    }
                }
            }
            if let Some(t) = p.model.hide_after {
                if now >= t {
                    p.model.hide_after = None;
                    act_hide = true;
                }
            }
            let next = p.model.tick(now);
            if !act_hide && (p.shown || act_show) && (p.model.animating(now) || act_show || replace) {
                present(p);
                wait = None;
            } else {
                wait = Some(next);
            }
            if act_show {
                p.shown = true;
            }
            if act_hide {
                p.shown = false;
            }
        });
        if act_show {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            topmost(hwnd);
        }
        if act_hide {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        match wait {
            // Something is moving: wait for the next display refresh.
            None => {
                if DwmFlush().is_err() {
                    std::thread::sleep(Duration::from_millis(16));
                }
            }
            // Nothing moving, but a state ends later: sleep until then.
            Some(Some(t)) => {
                let ms = t.saturating_duration_since(Instant::now()).as_millis().min(u32::MAX as u128) as u32;
                MsgWaitForMultipleObjects(None, BOOL(0), ms.max(1), QS_ALLINPUT);
            }
            // Fully idle: sleep until a message arrives.
            Some(None) => {
                let _ = WaitMessage();
            }
        }
    }
}
