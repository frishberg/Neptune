// Global hotkeys. Tauri's global-shortcut plugin gives us press AND release
// events, which is what makes hold-to-talk work without a low-level hook.
//
// Bindings come from settings and can be changed at runtime with `apply`.
// Defaults (same feel as Wispr Flow, different keys):
//   Ctrl+Space          hold to talk
//   Ctrl+Shift+Space    toggle hands-free
//   Esc                 cancel (only registered while listening)
//   Ctrl+Shift+V        paste last transcript
//   Ctrl+Shift+H        open history

use std::sync::Mutex;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::session::{self, Input};
use crate::settings::Settings;

#[derive(Default)]
struct Bindings {
    ptt: Option<Shortcut>,
    handsfree: Option<Shortcut>,
    paste_last: Option<Shortcut>,
    history: Option<Shortcut>,
}

struct State(Mutex<Bindings>);
/// Shortcuts currently held. A held combo auto-repeats its Pressed event;
/// only the first one counts, until Released.
struct Held(Mutex<Vec<Shortcut>>);

pub fn register(app: &AppHandle, settings: &Settings) -> anyhow::Result<()> {
    app.manage(State(Mutex::new(Bindings::default())));
    app.manage(Held(Mutex::new(Vec::new())));
    app.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(move |app, shortcut, event| {
                {
                    let held = app.state::<Held>();
                    let mut held = held.0.lock().unwrap();
                    match event.state() {
                        ShortcutState::Pressed => {
                            if held.contains(shortcut) {
                                return; // auto-repeat of a combo still held
                            }
                            held.push(*shortcut);
                        }
                        ShortcutState::Released => held.retain(|s| s != shortcut),
                    }
                }
                let b = app.state::<State>();
                let b = b.0.lock().unwrap();
                let is = |o: &Option<Shortcut>| o.as_ref().map(|s| s == shortcut).unwrap_or(false);
                let input = match event.state() {
                    ShortcutState::Pressed if is(&b.ptt) => Input::PttDown,
                    ShortcutState::Released if is(&b.ptt) => Input::PttUp,
                    ShortcutState::Pressed if is(&b.handsfree) => Input::HandsfreeToggle,
                    ShortcutState::Pressed if is(&b.paste_last) => Input::PasteLast,
                    ShortcutState::Pressed if is(&b.history) => Input::OpenHistory,
                    ShortcutState::Pressed if shortcut.key == Code::Escape && shortcut.mods.is_empty() => Input::Cancel,
                    _ => return,
                };
                drop(b);
                session::send(app, input);
            })
            .build(),
    )?;
    apply(app, settings)
}

/// (Re)registers every binding from settings. Bad combos are skipped.
///
/// On Windows hold-to-talk goes through the low-level hook in ptt_hook, which
/// can hide the held modifiers from other apps; the plugin only handles it on
/// other platforms.
pub fn apply(app: &AppHandle, s: &Settings) -> anyhow::Result<()> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    #[cfg(windows)]
    crate::ptt_hook::set_binding(crate::ptt_hook::parse(&s.ptt), crate::ptt_hook::parse(&s.handsfree));
    let mut b = Bindings {
        ptt: if cfg!(windows) { None } else { parse(&s.ptt) },
        handsfree: parse(&s.handsfree),
        paste_last: parse(&s.paste_last),
        history: parse(&s.history),
    };
    for sc in [&mut b.ptt, &mut b.handsfree, &mut b.paste_last, &mut b.history] {
        if let Some(x) = sc {
            if let Err(e) = gs.register(*x) {
                log::error!("hotkey {:?} failed: {e}", x);
                *sc = None;
            }
        }
    }
    *app.state::<State>().0.lock().unwrap() = b;
    Ok(())
}

/// Esc is only grabbed while we are listening so it never interferes otherwise.
pub fn grab_escape(app: &AppHandle, grab: bool) {
    let esc = Shortcut::new(None, Code::Escape);
    let gs = app.global_shortcut();
    let _ = if grab { gs.register(esc) } else { gs.unregister(esc) };
}

/// ["Ctrl", "Shift", "Space"] -> Shortcut. Up to three keys, at least one
/// modifier, never Esc (Esc is the cancel key and is grabbed separately).
pub fn parse(keys: &[String]) -> Option<Shortcut> {
    if keys.len() > 3 {
        return None;
    }
    let mut mods = Modifiers::empty();
    let mut code: Option<Code> = None;
    for k in keys {
        match k.as_str() {
            "Ctrl" | "Control" => mods |= Modifiers::CONTROL,
            "Shift" => mods |= Modifiers::SHIFT,
            "Alt" | "Option" => mods |= Modifiers::ALT,
            "Win" | "Cmd" | "Meta" | "Super" => mods |= Modifiers::SUPER,
            other => code = key_code(other),
        }
    }
    let code = code?;
    if mods.is_empty() || code == Code::Escape {
        return None;
    }
    Some(Shortcut::new(Some(mods), code))
}

fn key_code(k: &str) -> Option<Code> {
    let up = k.to_uppercase();
    Some(match up.as_str() {
        "SPACE" | " " => Code::Space,
        "ESC" | "ESCAPE" => Code::Escape,
        "ENTER" | "RETURN" => Code::Enter,
        "TAB" => Code::Tab,
        "BACKSPACE" => Code::Backspace,
        "DELETE" => Code::Delete,
        "HOME" => Code::Home,
        "END" => Code::End,
        "PAGEUP" => Code::PageUp,
        "PAGEDOWN" => Code::PageDown,
        "INSERT" => Code::Insert,
        "CAPSLOCK" => Code::CapsLock,
        "ARROWUP" | "UP" => Code::ArrowUp,
        "ARROWDOWN" | "DOWN" => Code::ArrowDown,
        "ARROWLEFT" | "LEFT" => Code::ArrowLeft,
        "ARROWRIGHT" | "RIGHT" => Code::ArrowRight,
        "`" | "BACKQUOTE" => Code::Backquote,
        "-" | "MINUS" => Code::Minus,
        "=" | "EQUAL" => Code::Equal,
        "[" => Code::BracketLeft,
        "]" => Code::BracketRight,
        "\\" => Code::Backslash,
        ";" => Code::Semicolon,
        "'" => Code::Quote,
        "," => Code::Comma,
        "." => Code::Period,
        "/" => Code::Slash,
        "F1" => Code::F1, "F2" => Code::F2, "F3" => Code::F3, "F4" => Code::F4,
        "F5" => Code::F5, "F6" => Code::F6, "F7" => Code::F7, "F8" => Code::F8,
        "F9" => Code::F9, "F10" => Code::F10, "F11" => Code::F11, "F12" => Code::F12,
        s if s.len() == 1 && s.as_bytes()[0].is_ascii_uppercase() => match s.as_bytes()[0] {
            b'A' => Code::KeyA, b'B' => Code::KeyB, b'C' => Code::KeyC, b'D' => Code::KeyD, b'E' => Code::KeyE,
            b'F' => Code::KeyF, b'G' => Code::KeyG, b'H' => Code::KeyH, b'I' => Code::KeyI, b'J' => Code::KeyJ,
            b'K' => Code::KeyK, b'L' => Code::KeyL, b'M' => Code::KeyM, b'N' => Code::KeyN, b'O' => Code::KeyO,
            b'P' => Code::KeyP, b'Q' => Code::KeyQ, b'R' => Code::KeyR, b'S' => Code::KeyS, b'T' => Code::KeyT,
            b'U' => Code::KeyU, b'V' => Code::KeyV, b'W' => Code::KeyW, b'X' => Code::KeyX, b'Y' => Code::KeyY,
            b'Z' => Code::KeyZ, _ => return None,
        },
        s if s.len() == 1 && s.as_bytes()[0].is_ascii_digit() => match s.as_bytes()[0] {
            b'0' => Code::Digit0, b'1' => Code::Digit1, b'2' => Code::Digit2, b'3' => Code::Digit3, b'4' => Code::Digit4,
            b'5' => Code::Digit5, b'6' => Code::Digit6, b'7' => Code::Digit7, b'8' => Code::Digit8, b'9' => Code::Digit9,
            _ => return None,
        },
        _ => return None,
    })
}
