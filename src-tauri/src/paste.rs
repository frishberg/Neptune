// Put text where the cursor is.
//
// Copy to clipboard, send Ctrl+V (Cmd+V on Mac), then put the user's old
// clipboard back. Works in nearly everything, terminals included.

use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::{thread, time::Duration};

pub fn paste(text: &str) -> anyhow::Result<()> {
    let mut clip = arboard::Clipboard::new()?;
    let previous = clip.get_text().ok();
    clip.set_text(text.to_string())?;

    let mut enigo = Enigo::new(&Settings::default())?;
    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    enigo.key(modifier, Direction::Press)?;
    enigo.key(Key::Unicode('v'), Direction::Click)?;
    enigo.key(modifier, Direction::Release)?;

    // Give the target app a beat to read the clipboard before we restore it.
    thread::sleep(Duration::from_millis(150));
    if let Some(p) = previous {
        let _ = clip.set_text(p);
    }
    Ok(())
}

