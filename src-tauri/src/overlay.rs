// The pill, and the main window.
//
// On Windows the pill is drawn natively (pill_native.rs): no browser runs
// while neptune sits in the background. Elsewhere it is the webview in
// overlay/index.html, built here on startup. Everything else in the app
// talks to the pill only through the functions in this file.

#[cfg(not(windows))]
use std::sync::Mutex;
#[cfg(not(windows))]
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
#[cfg(not(windows))]
use tauri::{LogicalPosition, WebviewWindow};

#[cfg(not(windows))]
pub const WINDOW: &str = "pill";
#[cfg(not(windows))]
pub const WIDTH: f64 = 440.0;
#[cfg(not(windows))]
pub const HEIGHT: f64 = 120.0;

#[derive(Clone, Copy, Serialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Idle,
    Listening,
    Handsfree,
    Processing,
    Done,
    Error,
}

#[cfg(not(windows))]
fn window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(WINDOW)
}

/// Starts the pill. `visible`: whether it shows right away.
#[cfg(windows)]
pub fn start_pill(app: &AppHandle, visible: bool) -> tauri::Result<()> {
    crate::pill_native::start(app.clone(), visible);
    Ok(())
}

#[cfg(not(windows))]
pub fn start_pill(app: &AppHandle, visible: bool) -> tauri::Result<()> {
    tauri::WebviewWindowBuilder::new(app, WINDOW, tauri::WebviewUrl::App("index.html".into()))
        .title("Neptune")
        .inner_size(WIDTH, HEIGHT)
        .transparent(true)
        .decorations(false)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .resizable(false)
        .visible_on_all_workspaces(true)
        .build()?;
    place(app)?;
    start_hit_test(app.clone());
    if visible {
        let h = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(700));
            let _ = h.emit_to(WINDOW, "appear", ());
        });
    } else {
        hide(app);
    }
    Ok(())
}

/// Puts the pill at the bottom centre of the work area.
#[cfg(not(windows))]
fn place(app: &AppHandle) -> tauri::Result<()> {
    let Some(w) = window(app) else { return Ok(()) };
    let monitor = w.current_monitor()?.or(w.primary_monitor()?);
    if let Some(m) = monitor {
        // work_area excludes the taskbar (Windows) and dock/menu bar (Mac),
        // so the pill sits just above them instead of on top of them.
        let scale = m.scale_factor();
        let area = m.work_area();
        let size = area.size.to_logical::<f64>(scale);
        let pos = area.position.to_logical::<f64>(scale);
        let x = pos.x + (size.width - WIDTH) / 2.0;
        let y = pos.y + size.height - HEIGHT;
        w.set_position(LogicalPosition::new(x, y))?;
    }
    // Click-through by default. The hit-test thread lets clicks in only
    // while the cursor is over the pill itself.
    w.set_ignore_cursor_events(true)?;
    w.show()?;
    Ok(())
}

#[cfg(not(windows))]
/// The state the pill is showing, for the hit test below.
static STATE: Mutex<State> = Mutex::new(State::Idle);

#[cfg(not(windows))]
/// Bottom padding of the stage and the pill's size per state. These mirror
/// pill.css and must move with it.
#[cfg(not(windows))]
const PAD_BOTTOM: f64 = 8.0;
#[cfg(not(windows))]
/// The pill is offering the text (done, with a Copy button).
static OFFER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(not(windows))]
fn pill_size(state: State) -> (f64, f64) {
    match state {
        State::Handsfree => (150.0, 30.0),
        State::Done if OFFER.load(std::sync::atomic::Ordering::Relaxed) => (100.0, 28.0),
        _ => (84.0, 28.0),
    }
}

/// The window is a 440 x 120 transparent sheet; only the pill in it is real.
/// Tauri cannot hit-test per region, so a thread polls the cursor and flips
/// the window between click-through and clickable as the cursor crosses the
/// pill's edge. Everything around the pill stays clickable for the app
/// underneath. The webview also gets a "hover" event so it can show and hide
/// the tip, because a click-through window never sees mouseleave.
#[cfg(not(windows))]
fn start_hit_test(app: AppHandle) {
    std::thread::spawn(move || {
        let mut inside = false;
        // Window geometry comes from the main thread (a blocking round trip),
        // so refresh it a few times a second; only the cursor is read every
        // tick, and that is a direct OS call.
        let mut geom: Option<(f64, f64, f64, bool)> = None; // x, y (physical), scale, visible
        let mut tick: u32 = 0;
        loop {
            std::thread::sleep(Duration::from_millis(16));
            let Some(w) = window(&app) else { continue };
            if tick % 20 == 0 || geom.is_none() {
                geom = match (w.outer_position(), w.scale_factor(), w.is_visible()) {
                    (Ok(p), Ok(s), Ok(v)) => Some((p.x as f64, p.y as f64, s, v)),
                    _ => None,
                };
            }
            tick = tick.wrapping_add(1);
            // Other windows can slip above a topmost window (another topmost
            // app, a window that just went fullscreen and back). Re-assert
            // the pill's place twice a second so it is always on top.
            if tick % 30 == 0 {
                assert_topmost(&w);
            }
            let Some((wx, wy, scale, visible)) = geom else { continue };
            if !visible {
                continue;
            }
            let now = match app.cursor_position() {
                Ok(c) => {
                    let lx = (c.x - wx) / scale;
                    let ly = (c.y - wy) / scale;
                    let (pw, ph) = pill_size(*STATE.lock().unwrap());
                    let slack = 3.0;
                    let x0 = (WIDTH - pw) / 2.0 - slack;
                    let x1 = (WIDTH + pw) / 2.0 + slack;
                    let y1 = HEIGHT - PAD_BOTTOM + slack;
                    let y0 = y1 - ph - 2.0 * slack;
                    lx >= x0 && lx <= x1 && ly >= y0 && ly <= y1
                }
                Err(_) => false,
            };
            if now != inside {
                inside = now;
                let _ = w.set_ignore_cursor_events(!inside);
                assert_topmost(&w);
                let _ = app.emit_to(WINDOW, "hover", inside);
            }
        }
    });
}

#[cfg(not(windows))]
/// Bumped on every show so a hide that is still waiting on its animation
/// knows it has been overtaken.
static SHOW_GEN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

#[cfg(windows)]
pub fn show(_app: &AppHandle) {
    crate::pill_native::show();
}

#[cfg(not(windows))]
pub fn show(app: &AppHandle) {
    SHOW_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if let Some(w) = window(app) {
        let _ = w.show();
        assert_topmost(&w);
        let _ = app.emit_to(WINDOW, "appear", ());
    }
}

#[cfg(not(windows))]
fn assert_topmost(w: &WebviewWindow) {
    let _ = w.set_always_on_top(true);
}

#[cfg(windows)]
pub fn hide(_app: &AppHandle) {
    crate::pill_native::hide();
}

/// The pill melts away first; the window goes once that is over.
#[cfg(not(windows))]
pub fn hide(app: &AppHandle) {
    let gen = SHOW_GEN.load(std::sync::atomic::Ordering::SeqCst);
    let _ = app.emit_to(WINDOW, "vanish", ());
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(460));
        if SHOW_GEN.load(std::sync::atomic::Ordering::SeqCst) != gen {
            return;
        }
        if let Some(w) = window(&app) {
            let _ = w.hide();
        }
    });
}

/// The main window: Home, History, Settings.
pub const MAIN: &str = "main";

/// Opens the main window, building it if it does not exist. It is built on
/// demand and destroyed when closed, so while you are not looking at it,
/// none of it is in memory.
pub fn show_main(app: &AppHandle, view: Option<&str>) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        if let Some(v) = view {
            let _ = app.emit_to(MAIN, "view", v);
        }
        return;
    }
    let mut url = match view {
        Some(v) => format!("app/index.html?view={v}"),
        None => "app/index.html".to_string(),
    };
    // Dev: NEPTUNE_ONBOARDING=<step> in a debug build opens on that step.
    if cfg!(debug_assertions) {
        if let Ok(step) = std::env::var("NEPTUNE_ONBOARDING") {
            url = format!("app/index.html?onboarding={step}");
        }
    }
    let built = tauri::WebviewWindowBuilder::new(app, MAIN, tauri::WebviewUrl::App(url.into()))
        .title("Neptune")
        .inner_size(960.0, 760.0)
        .min_inner_size(760.0, 560.0)
        .center()
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(true)
        .focused(true)
        .build();
    match built {
        Ok(w) => {
            if let Ok(icon) = tauri::image::Image::from_bytes(include_bytes!("../icons/128x128@2x.png")) {
                let _ = w.set_icon(icon);
            }
            let _ = w.set_focus();
        }
        Err(e) => log::error!("main window failed to open: {e}"),
    }
}

/// Closes the main window, which destroys it and frees its memory.
pub fn hide_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.close();
    }
}

pub fn set_state(app: &AppHandle, state: State) {
    #[cfg(windows)]
    crate::pill_native::set_state(state);
    #[cfg(not(windows))]
    {
        if state != State::Done {
            OFFER.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        *STATE.lock().unwrap() = state;
        let _ = app.emit_to(WINDOW, "state", state);
    }
    let _ = app.emit_to(MAIN, "state", state);
}

/// Done.
pub fn set_done(app: &AppHandle) {
    set_state(app, State::Done);
}

/// Done, but the text had nowhere to go: the pill shows a Copy button.
pub fn set_offer(app: &AppHandle) {
    set_state(app, State::Done);
    #[cfg(windows)]
    crate::pill_native::set_offer();
    #[cfg(not(windows))]
    {
        OFFER.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = app.emit_to(WINDOW, "offer", ());
    }
}

/// Error state with a reason the pill can show.
pub fn set_error(app: &AppHandle, message: &str) {
    #[cfg(windows)]
    crate::pill_native::set_error(message);
    #[cfg(not(windows))]
    {
        *STATE.lock().unwrap() = State::Error;
        let _ = app.emit_to(WINDOW, "error", message);
    }
    let _ = app.emit_to(MAIN, "state", State::Error);
}

/// 0.0 .. 1.0 microphone level, sent ~60 times a second while listening.
/// Only the pill moves with the voice. The main window stays still.
pub fn set_level(app: &AppHandle, level: f32) {
    #[cfg(windows)]
    {
        let _ = app;
        crate::pill_native::set_level(level);
    }
    #[cfg(not(windows))]
    let _ = app.emit_to(WINDOW, "level", level);
}
