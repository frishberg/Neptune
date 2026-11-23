// Which app has the keyboard right now. Captured when a dictation starts so
// history can say where the text went, and so the insert method can be
// chosen per app (terminals get typed into).

#[derive(Debug, Clone, Default)]
pub struct App {
    /// Human name, e.g. "Slack" or "Windows Terminal".
    pub name: String,
    /// Stable key for icons and matching, e.g. "slack" or "windowsterminal".
    pub key: String,
    /// Full path of the executable, for icon extraction. Empty if unknown.
    pub exe: String,
}

#[cfg(windows)]
pub fn foreground() -> App {
    use windows::Win32::Foundation::{CloseHandle, HWND};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId};

    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return App::default();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return App::default();
        };
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return App::default();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let stem = std::path::Path::new(&path).file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        // A terminal with Claude Code running inside it is, for our purposes,
        // Claude Code: that is where the words are going.
        //
        // Windows Terminal holds many tabs, and a tab opened from Explorer or
        // the Start menu is handed to it by Windows, so its processes are not
        // Windows Terminal's children at all. What does follow the tab you are
        // in is the window title: it is the active tab's title, and Claude
        // Code marks its tab's title while it runs. A single-window console
        // (cmd, conhost, ...) has one session, so its child processes are
        // reliable there.
        if is_terminal(&stem) {
            let mut t = [0u16; 512];
            let n = GetWindowTextW(hwnd, &mut t).max(0) as usize;
            let title = String::from_utf16_lossy(&t[..n]);
            let tabbed = stem.eq_ignore_ascii_case("windowsterminal");
            let claude = if tabbed { claude_title(&title).then(String::new) } else { child_named(pid, "claude.exe") };
            if let Some(exe) = claude {
                return App { name: "Claude Code".into(), key: "claudecode".into(), exe };
            }
        }
        let mut app = from_exe(&stem);
        app.exe = path;
        app
    }
}

/// Claude Code titles its terminal tab with a small star-like mark in front
/// (it cycles through these while it works), or with its own name.
fn claude_title(title: &str) -> bool {
    let first = title.trim_start().chars().next();
    matches!(first, Some('·' | '✢' | '✳' | '✶' | '✻' | '✽' | '✴')) || title.contains("Claude Code")
}

fn is_terminal(stem: &str) -> bool {
    matches!(
        stem.to_lowercase().as_str(),
        "windowsterminal" | "cmd" | "powershell" | "pwsh" | "conhost" | "alacritty" | "wezterm-gui" | "wezterm" | "hyper" | "tabby" | "mintty" | "kitty" | "ghostty"
    )
}

/// Walks the process tree under `root` and returns the path of the first
/// descendant whose executable is `name`.
#[cfg(windows)]
fn child_named(root: u32, name: &str) -> Option<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    let mut procs: Vec<(u32, u32, String)> = vec![]; // pid, parent, exe name
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                procs.push((e.th32ProcessID, e.th32ParentProcessID, String::from_utf16_lossy(&e.szExeFile[..n]).to_lowercase()));
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    let mut queue = vec![root];
    let mut seen = std::collections::HashSet::new();
    while let Some(p) = queue.pop() {
        if !seen.insert(p) {
            continue;
        }
        for (pid, parent, exe) in &procs {
            if *parent == p && *pid != p {
                if exe == name {
                    // Full path, for the icon.
                    let path = unsafe {
                        OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, *pid).ok().and_then(|h| {
                            let mut buf = [0u16; 1024];
                            let mut len = buf.len() as u32;
                            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
                            let _ = CloseHandle(h);
                            ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
                        })
                    };
                    return Some(path.unwrap_or_default());
                }
                queue.push(*pid);
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn child_named(_root: u32, _name: &str) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::claude_title;

    #[test]
    fn claude_marks_its_tab() {
        assert!(claude_title("✳ Canal St project Spotify integration"));
        assert!(claude_title("✶ Working on the pill"));
        assert!(claude_title("Claude Code"));
        assert!(!claude_title("Windows PowerShell"));
        assert!(!claude_title("C:\\WINDOWS\\system32\\cmd.exe"));
        assert!(!claude_title(""));
    }
}

#[cfg(not(windows))]
pub fn foreground() -> App {
    App::default()
}

/// Turns an executable stem into a display name and key.
fn from_exe(stem: &str) -> App {
    let key: String = stem.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect();
    let name = match key.as_str() {
        "slack" => "Slack",
        "code" => "VS Code",
        "cursor" => "Cursor",
        "chrome" => "Chrome",
        "msedge" => "Edge",
        "firefox" => "Firefox",
        "notion" => "Notion",
        "olk" | "outlook" => "Outlook",
        "windowsterminal" => "Windows Terminal",
        "cmd" => "Command Prompt",
        "powershell" | "pwsh" => "PowerShell",
        "discord" => "Discord",
        "notepad" => "Notepad",
        "winword" => "Word",
        "excel" => "Excel",
        "teams" | "msteams" => "Teams",
        "telegram" => "Telegram",
        "whatsapp" => "WhatsApp",
        "obsidian" => "Obsidian",
        "explorer" => "Explorer",
        "neptune" | "blurt" => "Neptune",
        "claudecode" => "Claude Code",
        "" => "Unknown",
        _ => "",
    };
    let name = if name.is_empty() { title_case(stem) } else { name.to_string() };
    App { name, key, exe: String::new() }
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
