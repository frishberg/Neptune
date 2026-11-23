// Hold-to-talk on Windows, done with a low-level keyboard hook instead of a
// registered hotkey.
//
// Kept simple on purpose. The hook swallows the trigger key (Space) while the
// combo is held, so it never types, and that is all it changes. Modifiers are
// never faked: Windows always sees the real Ctrl, Shift, Alt and Win. (An
// earlier version faked a Ctrl release while dictating; one missed key-up
// and Windows believed Ctrl was held forever, so typing turned into
// shortcuts in some apps.) The one injected key is a no-op tap when Win or
// Alt is in the combo, so letting go of them does not open Start or a menu.
//
// Everything that decides is in `Machine`, a pure state machine with no
// Win32 in it, so it can be tested. The hook callback feeds it events and
// carries out what it says. There is deliberately no timer and no guessing:
// if an event is ever lost to a secure desktop, the next tap of that key
// puts things right, which is the simplest recovery there is.

#![cfg(windows)]

use std::sync::{Mutex, OnceLock};

use tauri::AppHandle;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    VK_BACK, VK_CAPITAL, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_F10, VK_F11, VK_F12, VK_F2, VK_F3,
    VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_HOME, VK_INSERT, VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN,
    VK_MENU, VK_NEXT, VK_OEM_1, VK_OEM_2, VK_OEM_3, VK_OEM_4, VK_OEM_5, VK_OEM_6, VK_OEM_7, VK_OEM_COMMA, VK_OEM_MINUS,
    VK_OEM_PERIOD, VK_OEM_PLUS, VK_PRIOR, VK_RCONTROL, VK_RETURN, VK_RIGHT, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
    VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, SetWindowsHookExW, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::session::{self, Input};

/// Marks the events we inject so the hook ignores them.
const TAG: usize = 0xB1A7;

// Modifier bits for a binding.
pub const CTRL: u32 = 1;
pub const SHIFT: u32 = 2;
pub const ALT: u32 = 4;
pub const WIN: u32 = 8;

/// A virtual key that no keyboard has. Tapped before releasing Win so
/// Windows does not read "Win pressed and released alone" as "open Start".
const VK_NOTHING: u16 = 0xFF;

// ---------- the machine ----------

/// What the machine wants done, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    Send(Input),
    /// Tap the key that does not exist.
    Nothing,
}

#[derive(Debug, Default)]
pub struct Out {
    /// True: the event must not reach any app.
    pub swallow: bool,
    pub acts: Vec<Act>,
}

#[derive(Debug, Default)]
pub struct Machine {
    /// The hold-to-talk binding. key == 0 means disabled.
    mods: u32,
    key: u16,
    /// The hands-free binding, used to lock a held take on.
    hf_mods: u32,
    hf_key: u16,
    /// A held take is in progress and we are watching for the release.
    active: bool,
    /// The take was locked on to hands-free; the trigger key is still down.
    locked: bool,
    /// Modifiers physically down, as seen through the hook.
    phys_mods: u32,
    /// Trigger key physically down, as seen through the hook.
    key_down: bool,
    /// A take ended while the trigger was still held: its repeats and its
    /// release are eaten, so they never type.
    eat: bool,
    /// Repeats of the trigger seen since it went down (no take running).
    /// Zero means it was pressed a moment ago: still "pressed together".
    repeats: u32,
    /// The take began trigger-first, so the app saw the trigger go down;
    /// it gets the release too.
    pass_up: bool,
}

fn bit_of(vk: u16) -> u32 {
    match VIRTUAL_KEY(vk) {
        VK_LCONTROL | VK_RCONTROL | VK_CONTROL => CTRL,
        VK_LSHIFT | VK_RSHIFT | VK_SHIFT => SHIFT,
        VK_LMENU | VK_RMENU | VK_MENU => ALT,
        VK_LWIN | VK_RWIN => WIN,
        _ => 0,
    }
}

impl Machine {
    pub fn set_binding(&mut self, ptt: Option<(u32, u16)>, handsfree: Option<(u32, u16)>) {
        let (m, k) = ptt.unwrap_or((0, 0));
        self.mods = m;
        self.key = k;
        let (hm, hk) = handsfree.unwrap_or((0, 0));
        self.hf_mods = hm;
        self.hf_key = hk;
    }

    /// After the hook was off for a while: whatever it thought was held is
    /// unknown now. Keeps the bindings.
    pub fn forget_keys(&mut self) {
        self.active = false;
        self.locked = false;
        self.phys_mods = 0;
        self.key_down = false;
        self.eat = false;
        self.repeats = 0;
        self.pass_up = false;
    }

    /// The take is over.
    fn finish(&mut self, acts: &mut Vec<Act>) {
        self.active = false;
        self.pass_up = false;
        acts.push(Act::Send(Input::PttUp));
    }

    /// The combo is complete: start a take.
    fn start(&mut self, acts: &mut Vec<Act>) {
        // Win or Alt let go "alone" would open Start or a menu, so a no-op
        // key goes in between.
        self.active = true;
        if self.mods & (WIN | ALT) != 0 {
            acts.push(Act::Nothing);
        }
        acts.push(Act::Send(Input::PttDown));
    }

    /// A physical key event. Injected events never reach this.
    pub fn event(&mut self, vk: u16, down: bool) -> Out {
        let mut out = Out::default();
        if self.key == 0 {
            return out;
        }
        let bit = bit_of(vk);
        let was_down = bit != 0 && self.phys_mods & bit != 0;
        if bit != 0 {
            if down {
                self.phys_mods |= bit;
            } else {
                self.phys_mods &= !bit;
            }
        }

        // Trigger first, then the modifiers, pressed together: the combo is
        // complete while the trigger is fresh (not repeating yet). The
        // modifier itself passes; the trigger already reached the app.
        if down && bit != 0 && !was_down && !self.active && !self.locked && !self.eat
            && self.key_down && self.repeats == 0 && self.phys_mods == self.mods
        {
            self.pass_up = true;
            self.start(&mut out.acts);
            return out;
        }

        // A held modifier auto-repeats its key-down; that is just the real
        // key, held, and passes like any other.
        if self.active && down && was_down && self.mods & bit != 0 {
            return out;
        }

        // Locking on: while held, the hands-free combo is completed on top
        // of the hold-to-talk combo (extra modifiers, or its own key). The
        // take keeps going and stops answering to the release.
        if self.active && down && self.hf_key != 0 {
            let same_key = self.hf_key == self.key;
            let key_ok = same_key || vk == self.hf_key;
            if key_ok && self.phys_mods == self.hf_mods && (bit != 0 || !same_key) {
                self.active = false;
                self.locked = true;
                out.acts.push(Act::Send(Input::HandsfreeToggle));
                if !same_key {
                    out.swallow = true;
                }
                return out;
            }
        }

        // The key that started a locked take: its repeats and its release
        // never reach apps.
        if self.locked && vk == self.key {
            if !down {
                self.locked = false;
                self.key_down = false;
            }
            out.swallow = true;
            return out;
        }

        if vk == self.key {
            if down {
                if self.active || self.eat {
                    out.swallow = true; // a repeat while dictating, or after
                    return out;
                }
                // Only a fresh press starts a take; a repeat never does.
                let fresh = !self.key_down;
                self.key_down = true;
                self.repeats = if fresh { 0 } else { self.repeats + 1 };
                if fresh && self.phys_mods == self.mods {
                    self.start(&mut out.acts);
                    out.swallow = true;
                }
                return out;
            }
            self.key_down = false;
            if self.eat {
                self.eat = false;
                out.swallow = true;
                return out;
            }
            if self.active {
                // swallowed, unless the app saw this key go down
                out.swallow = !self.pass_up;
                self.finish(&mut out.acts);
            }
            return out;
        }

        // A modifier physically released while dictating ends it too. The
        // system already thinks it is up, so the event passes harmlessly.
        if self.active && !down && bit != 0 {
            self.finish(&mut out.acts);
            // the trigger is still held: keep it from typing until it is up
            self.eat = self.key_down;
        }
        out
    }

}

// ---------- binding names ----------

/// ["Ctrl", "Space"] -> (modifier bits, virtual key). None if unusable.
pub fn parse(keys: &[String]) -> Option<(u32, u16)> {
    let mut mods = 0;
    let mut vk: Option<u16> = None;
    for k in keys {
        match k.as_str() {
            "Ctrl" | "Control" => mods |= CTRL,
            "Shift" => mods |= SHIFT,
            "Alt" | "Option" => mods |= ALT,
            "Win" | "Cmd" | "Meta" | "Super" => mods |= WIN,
            other => vk = vk_of(other),
        }
    }
    let vk = vk?;
    if mods == 0 || vk == VK_ESCAPE.0 {
        return None;
    }
    Some((mods, vk))
}

fn vk_of(name: &str) -> Option<u16> {
    let up = name.to_uppercase();
    let v = match up.as_str() {
        "SPACE" => VK_SPACE,
        "ENTER" | "RETURN" => VK_RETURN,
        "TAB" => VK_TAB,
        "BACKSPACE" => VK_BACK,
        "DELETE" => VK_DELETE,
        "HOME" => VK_HOME,
        "END" => VK_END,
        "PAGEUP" => VK_PRIOR,
        "PAGEDOWN" => VK_NEXT,
        "INSERT" => VK_INSERT,
        "CAPSLOCK" => VK_CAPITAL,
        "ARROWUP" | "UP" => VK_UP,
        "ARROWDOWN" | "DOWN" => VK_DOWN,
        "ARROWLEFT" | "LEFT" => VK_LEFT,
        "ARROWRIGHT" | "RIGHT" => VK_RIGHT,
        "`" => VK_OEM_3, "-" => VK_OEM_MINUS, "=" => VK_OEM_PLUS, "[" => VK_OEM_4, "]" => VK_OEM_6,
        "\\" => VK_OEM_5, ";" => VK_OEM_1, "'" => VK_OEM_7, "," => VK_OEM_COMMA, "." => VK_OEM_PERIOD, "/" => VK_OEM_2,
        "F1" => VK_F1, "F2" => VK_F2, "F3" => VK_F3, "F4" => VK_F4, "F5" => VK_F5, "F6" => VK_F6,
        "F7" => VK_F7, "F8" => VK_F8, "F9" => VK_F9, "F10" => VK_F10, "F11" => VK_F11, "F12" => VK_F12,
        s if s.len() == 1 && (s.as_bytes()[0].is_ascii_uppercase() || s.as_bytes()[0].is_ascii_digit()) => {
            return Some(s.as_bytes()[0] as u16); // letters and digits are their own VK codes
        }
        _ => return None,
    };
    Some(v.0)
}

// ---------- Win32 glue ----------

static APP: OnceLock<AppHandle> = OnceLock::new();
static MACHINE: Mutex<Machine> = Mutex::new(Machine {
    mods: 0,
    key: 0,
    hf_mods: 0,
    hf_key: 0,
    active: false,
    locked: false,
    phys_mods: 0,
    key_down: false,
    eat: false,
    repeats: 0,
    pass_up: false,
});

/// Sets (or clears, with None) the hold-to-talk binding the hook watches,
/// and the hands-free binding used to lock a held dictation on.
pub fn set_binding(ptt: Option<(u32, u16)>, handsfree: Option<(u32, u16)>) {
    MACHINE.lock().unwrap().set_binding(ptt, handsfree);
}

/// The modifiers Windows says are physically down right now. Asked at the
/// moment the trigger key goes down, so a key-up the hook never saw (lock
/// screen, UAC prompt, Ctrl+Alt+Del) cannot leave a modifier stuck "held".
fn os_mods() -> u32 {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    let held = |vk: i32| unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 };
    let mut m = 0;
    if held(0xA2) || held(0xA3) { m |= CTRL; }
    if held(0xA0) || held(0xA1) { m |= SHIFT; }
    if held(0xA4) || held(0xA5) { m |= ALT; }
    if held(0x5B) || held(0x5C) { m |= WIN; }
    m
}

/// True if Windows has this key down already. Inside the hook the event
/// being looked at is not counted yet, so for a fresh press this is false.
fn os_key_down(vk: u16) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

/// The key relay: while on, every physical key is handed (through a channel,
/// so the hook never waits) to the main window as a "key" event [vk, down].
/// On only while a page that draws keycaps is showing.
static RELAY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static RELAY_TX: OnceLock<std::sync::mpsc::Sender<(u16, bool, bool)>> = OnceLock::new();

pub fn set_relay(on: bool) {
    RELAY.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The hook thread, so it can be asked to reinstall.
static HOOK_THREAD: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
const WM_REINSTALL: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 7;

/// Installs the hook on its own thread. Call once at startup.
pub fn install(app: AppHandle) {
    let _ = APP.set(app);
    // Everything slow (telling the window, the test log) happens here, off
    // the hook thread: Windows gives the hook callback only a moment.
    let (tx, rx) = std::sync::mpsc::channel::<(u16, bool, bool)>();
    let _ = RELAY_TX.set(tx);
    std::thread::spawn(move || {
        use tauri::Emitter;
        for (vk, down, swallow) in rx {
            if hook_test() {
                crate::debug::log(&format!("hook vk={vk:#x} down={down} swallow={swallow}"));
            }
            if RELAY.load(std::sync::atomic::Ordering::Relaxed) {
                if let Some(app) = APP.get() {
                    let _ = app.emit_to("main", "key", (vk, down));
                }
            }
        }
    });
    std::thread::spawn(|| unsafe {
        use windows::Win32::System::Threading::GetCurrentThreadId;
        use windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx;
        use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL};
        use windows::Win32::UI::WindowsAndMessaging::{SetTimer, WM_TIMER};
        HOOK_THREAD.store(GetCurrentThreadId(), std::sync::atomic::Ordering::Release);
        // Windows drops a low-level hook whose callback misses its timeout,
        // silently, and that is easy while the model loads on every core.
        // So this thread answers first, and the hook is re-armed every few
        // seconds while no key is held, in case it was dropped anyway.
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
        let mut hook: Option<HHOOK> = SetWindowsHookExW(WH_KEYBOARD_LL, Some(callback), None, 0).ok();
        if hook.is_none() {
            log::error!("keyboard hook failed; hold-to-talk falls back to nothing");
        }
        SetTimer(None, 0, 1000, None);
        let mut tick: u32 = 0;
        // Low-level hooks need a message loop on the installing thread.
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            if msg.message == WM_TIMER {
                tick = tick.wrapping_add(1);
                let mut m = MACHINE.lock().unwrap();
                // A held take whose modifiers Windows says are up: the
                // release was lost (a secure desktop, a dropped hook). End it.
                if m.active && os_mods() & m.mods != m.mods {
                    m.active = false;
                    m.eat = m.key_down;
                    drop(m);
                    apply(vec![Act::Send(Input::PttUp)]);
                    continue;
                }
                if !m.active && !m.locked && tick % 5 == 0 {
                    if let Some(h) = hook.take() {
                        let _ = UnhookWindowsHookEx(h);
                    }
                    hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(callback), None, 0).ok();
                    if hook_test() {
                        crate::debug::log(&format!("hook re-armed: {}", hook.is_some()));
                    }
                    // Anything remembered as held may be stale; held
                    // modifiers are read back from Windows at the next press.
                    m.key_down = false;
                    m.phys_mods = 0;
                }
                continue;
            }
            if msg.message == WM_REINSTALL {
                // Windows can drop a low-level hook silently (after the lock
                // screen, sleep, or a stall). Putting it back is harmless.
                if let Some(h) = hook.take() {
                    let _ = UnhookWindowsHookEx(h);
                }
                // Keys held across the gap were never seen going up; forget
                // them, and end a take that was held through it.
                let was_held = { let mut m = MACHINE.lock().unwrap(); let a = m.active; m.forget_keys(); a };
                if was_held {
                    apply(vec![Act::Send(Input::PttUp)]);
                }
                hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(callback), None, 0).ok();
            }
        }
    });
}

/// Puts the hook back. Called on unlock and on resume from sleep.
pub fn reinstall() {
    let t = HOOK_THREAD.load(std::sync::atomic::Ordering::Acquire);
    if t != 0 {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(t, WM_REINSTALL, WPARAM(0), LPARAM(0));
        }
    }
}

fn hook_test() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| cfg!(debug_assertions) && std::env::var_os("NEPTUNE_HOOK_TEST").is_some())
}

fn inject(vk: u16, up: bool) {
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: TAG,
            },
        },
    };
    unsafe {
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

fn apply(acts: Vec<Act>) {
    for a in acts {
        match a {
            Act::Send(i) => {
                if let Some(app) = APP.get() {
                    session::send(app, i);
                }
            }
            Act::Nothing => {
                inject(VK_NOTHING, false);
                inject(VK_NOTHING, true);
            }
        }
    }
}

unsafe extern "system" fn callback(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let k = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
    // Test switch (debug builds, NEPTUNE_HOOK_TEST=1): take injected keys
    // too, except our own, so a script can drive the real hook.
    let ours = k.dwExtraInfo == TAG;
    let injected = ours || ((k.flags.0 & LLKHF_INJECTED.0) != 0 && !hook_test());
    let msg = wparam.0 as u32;
    let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
    if injected || !(down || up) {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let vk = k.vkCode as u16;
    let out = {
        let mut m = MACHINE.lock().unwrap();
        if down && vk == m.key && !m.active && !m.locked {
            // Windows already has the key down: this is an auto-repeat of a
            // press the hook never saw, not a new press. Leave it alone.
            if !m.key_down && os_key_down(vk) {
                drop(m);
                return CallNextHookEx(None, code, wparam, lparam);
            }
            m.phys_mods = os_mods();
        }
        m.event(vk, down)
    };
    if hook_test() || RELAY.load(std::sync::atomic::Ordering::Relaxed) {
        if let Some(tx) = RELAY_TX.get() {
            let _ = tx.send((vk, down, out.swallow));
        }
    }
    apply(out.acts);
    if out.swallow {
        LRESULT(1)
    } else {
        CallNextHookEx(None, code, wparam, lparam)
    }
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    const SPACE: u16 = 0x20;
    const LCTRL: u16 = 0xA2;
    const LSHIFT: u16 = 0xA0;
    const LWIN: u16 = 0x5B;
    const KEY_H: u16 = 0x48;

    fn machine() -> Machine {
        let mut m = Machine::default();
        m.set_binding(Some((CTRL, SPACE)), Some((CTRL | SHIFT, SPACE)));
        m
    }

    fn sends(out: &[Act]) -> Vec<String> {
        out.iter().filter_map(|a| if let Act::Send(i) = a { Some(format!("{i:?}")) } else { None }).collect()
    }

    #[test]
    fn hold_and_release_key_first() {
        let mut m = machine();
        let o = m.event(LCTRL, true);
        assert!(!o.swallow && o.acts.is_empty());
        let o = m.event(SPACE, true);
        assert!(o.swallow);
        assert_eq!(o.acts, vec![Act::Send(Input::PttDown)]);
        // Space repeats are swallowed; Ctrl's are the real key and pass
        assert!(m.event(SPACE, true).swallow);
        assert!(!m.event(LCTRL, true).swallow);
        // release Space with Ctrl still held: the take ends, nothing faked
        let o = m.event(SPACE, false);
        assert!(o.swallow);
        assert_eq!(o.acts, vec![Act::Send(Input::PttUp)]);
        // the real Ctrl release passes
        let o = m.event(LCTRL, false);
        assert!(!o.swallow && o.acts.is_empty());
        assert!(!m.active && !m.locked && m.phys_mods == 0);
    }

    #[test]
    fn release_modifier_first() {
        let mut m = machine();
        m.event(LCTRL, true);
        m.event(SPACE, true);
        let o = m.event(LCTRL, false);
        assert!(!o.swallow, "the real release passes, the system already thinks it is up");
        assert_eq!(o.acts, vec![Act::Send(Input::PttUp)], "nothing to press back: the hand is off Ctrl");
        // Space is still held: its repeats and release are eaten, never typed
        assert!(m.event(SPACE, true).swallow);
        let o = m.event(SPACE, false);
        assert!(o.swallow && o.acts.is_empty());
        assert!(!m.eat && !m.key_down);
    }

    #[test]
    fn space_then_ctrl_together_is_the_shortcut() {
        let mut m = machine();
        let o = m.event(SPACE, true);
        assert!(!o.swallow && o.acts.is_empty(), "Space alone reaches the app");
        let o = m.event(LCTRL, true);
        assert!(!o.swallow, "Ctrl is never swallowed");
        assert_eq!(sends(&o.acts), vec!["PttDown"], "pressed together: the shortcut");
        assert!(m.event(SPACE, true).swallow, "repeats while dictating never type");
        let o = m.event(SPACE, false);
        assert!(!o.swallow, "the app saw Space go down, so it sees it come up");
        assert_eq!(sends(&o.acts), vec!["PttUp"]);
        assert!(!m.event(LCTRL, false).swallow);
    }

    #[test]
    fn space_held_a_while_then_ctrl_is_typing() {
        let mut m = machine();
        m.event(SPACE, true);
        m.event(SPACE, true); // repeating: this is someone typing spaces
        let o = m.event(LCTRL, true);
        assert!(!o.swallow && o.acts.is_empty());
        let o = m.event(SPACE, true);
        assert!(!o.swallow && o.acts.is_empty(), "a repeat never starts a take");
        m.event(SPACE, false);
        // a fresh press with Ctrl held is the shortcut
        assert_eq!(sends(&m.event(SPACE, true).acts), vec!["PttDown"]);
    }

    #[test]
    fn two_takes_in_a_row_without_lifting_ctrl() {
        let mut m = machine();
        m.event(LCTRL, true);
        m.event(SPACE, true);
        m.event(SPACE, false);
        assert_eq!(m.phys_mods, CTRL);
        let o = m.event(SPACE, true);
        assert!(o.swallow, "Space must start the take, not type");
        assert_eq!(sends(&o.acts), vec!["PttDown"]);
        assert_eq!(sends(&m.event(SPACE, false).acts), vec!["PttUp"]);
        assert!(!m.event(LCTRL, false).swallow);
    }

    #[test]
    fn a_long_hold_is_never_cut_short() {
        let mut m = machine();
        m.event(LCTRL, true);
        m.event(SPACE, true);
        for _ in 0..2000 {
            assert!(m.event(SPACE, true).swallow);
        }
        assert!(m.active);
        assert_eq!(sends(&m.event(SPACE, false).acts), vec!["PttUp"]);
    }

    #[test]
    fn a_lost_release_is_put_right_by_the_next_tap() {
        let mut m = machine();
        m.event(LCTRL, true);
        m.event(SPACE, true);
        m.event(LCTRL, false); // hand off Ctrl: take ends
        assert!(!m.active);
        // the Space release went to a secure desktop and never arrived.
        // Space is up as far as the hook can tell after the next press+release:
        // the hook still thinks Space is held, so the next tap is eaten once
        assert!(m.event(SPACE, true).swallow);
        assert!(m.event(SPACE, false).swallow);
        assert!(!m.key_down && !m.eat);
        // and after that Space types normally
        assert!(!m.event(SPACE, true).swallow, "not active, no modifier: Space types");
        assert!(!m.event(SPACE, false).swallow);
    }

    #[test]
    fn wrong_combo_passes_through() {
        let mut m = machine();
        m.event(LSHIFT, true);
        let o = m.event(SPACE, true);
        assert!(!o.swallow && o.acts.is_empty());
        assert!(!m.active);
        assert!(!m.event(SPACE, false).swallow);
        assert!(!m.event(KEY_H, true).swallow);
    }

    #[test]
    fn lock_on_with_shift_then_end_with_hold_combo() {
        let mut m = machine();
        m.event(LCTRL, true);
        m.event(SPACE, true);
        let o = m.event(LSHIFT, true);
        assert!(!o.swallow, "Shift itself goes through");
        assert_eq!(sends(&o.acts), vec!["HandsfreeToggle"]);
        assert!(m.locked && !m.active);
        // letting everything go: Space never reaches apps, no PttUp is sent
        assert!(!m.event(LSHIFT, false).swallow);
        let o = m.event(SPACE, false);
        assert!(o.swallow && o.acts.is_empty());
        assert!(!m.locked);
        assert!(!m.event(LCTRL, false).swallow);
        // pressing the hold combo again while hands-free: PttDown ends the take on the session side
        m.event(LCTRL, true);
        let o = m.event(SPACE, true);
        assert_eq!(sends(&o.acts), vec!["PttDown"]);
        assert_eq!(sends(&m.event(SPACE, false).acts), vec!["PttUp"]);
    }

    #[test]
    fn lock_on_with_its_own_key() {
        let mut m = Machine::default();
        m.set_binding(Some((CTRL, SPACE)), Some((CTRL, KEY_H)));
        m.event(LCTRL, true);
        m.event(SPACE, true);
        let o = m.event(KEY_H, true);
        assert!(o.swallow, "the lock key must not type");
        assert_eq!(sends(&o.acts), vec!["HandsfreeToggle"]);
        assert!(m.locked);
    }

    #[test]
    fn another_key_pressed_while_holding_does_not_end_the_take() {
        let mut m = machine();
        m.event(LCTRL, true);
        m.event(SPACE, true);
        assert!(!m.event(KEY_H, true).swallow);
        assert!(m.active);
        assert_eq!(sends(&m.event(SPACE, false).acts), vec!["PttUp", ]);
    }

    #[test]
    fn win_key_gets_a_nothing_tap_before_release() {
        let mut m = Machine::default();
        m.set_binding(Some((WIN, SPACE)), None);
        m.event(LWIN, true);
        let o = m.event(SPACE, true);
        assert_eq!(o.acts, vec![Act::Nothing, Act::Send(Input::PttDown)]);
    }

    #[test]
    fn disabled_binding_touches_nothing() {
        let mut m = Machine::default();
        m.event(LCTRL, true);
        let o = m.event(SPACE, true);
        assert!(!o.swallow && o.acts.is_empty());
    }
}
