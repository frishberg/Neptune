// Store (MSIX) or .exe download? Neptune ships both, like Watt's Left: the
// Microsoft Store gets an MSIX package, which the Store signs, and the
// website and GitHub offer the NSIS installer. Two things differ when the
// app runs from the package:
//  - start at login goes through the manifest's StartupTask (the registry Run
//    key a normal app uses is virtualised inside a package and does nothing)
//  - the app updates through the Store, so the in-app updater stays out of it

/// True when running from the MSIX package (installed from the Store).
pub fn is_packaged() -> bool {
    #[cfg(windows)]
    {
        use std::sync::OnceLock;
        static PACKAGED: OnceLock<bool> = OnceLock::new();
        *PACKAGED.get_or_init(|| unsafe {
            use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
            // With no buffer, a packaged process gets "buffer too small"
            // (122); an ordinary one gets "no package" (15700).
            let mut len = 0u32;
            GetCurrentPackageFullName(&mut len, windows::core::PWSTR::null()).0 == 122
        })
    }
    #[cfg(not(windows))]
    false
}

/// The StartupTask declared in msix/AppxManifest.xml.
#[cfg(windows)]
const STARTUP_TASK: &str = "NeptuneStartup";

/// Runs a WinRT call on its own thread: the startup task API is async and
/// blocking on it from the UI thread is asking for trouble.
#[cfg(windows)]
fn on_mta<T: Send + 'static>(f: impl FnOnce() -> windows::core::Result<T> + Send + 'static) -> Result<T, String> {
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        f()
    })
    .join()
    .map_err(|_| "startup task thread failed".to_string())?
    .map_err(|e| e.message().to_string())
}

/// Packaged only: is start at login on? (The user can also change it in
/// Windows Settings > Apps > Startup, so this is the truth, not our setting.)
pub fn startup_enabled() -> Option<bool> {
    #[cfg(windows)]
    {
        use windows::ApplicationModel::{StartupTask, StartupTaskState};
        if !is_packaged() {
            return None;
        }
        on_mta(|| {
            let task = StartupTask::GetAsync(&windows::core::HSTRING::from(STARTUP_TASK))?.get()?;
            let st = task.State()?;
            Ok(st == StartupTaskState::Enabled || st == StartupTaskState::EnabledByPolicy)
        })
        .ok()
    }
    #[cfg(not(windows))]
    None
}

/// Packaged only: turn start at login on or off. Err says why it could not
/// be turned on (the user switched it off in Windows Settings, or a policy).
pub fn set_startup(on: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::ApplicationModel::{StartupTask, StartupTaskState};
        on_mta(move || {
            let task = StartupTask::GetAsync(&windows::core::HSTRING::from(STARTUP_TASK))?.get()?;
            if on {
                let st = task.RequestEnableAsync()?.get()?;
                Ok(st == StartupTaskState::Enabled || st == StartupTaskState::EnabledByPolicy)
            } else {
                task.Disable()?;
                Ok(true)
            }
        })
        .and_then(|ok| if ok { Ok(()) } else { Err("Windows has start at login turned off for Neptune. Turn it on in Settings > Apps > Startup.".into()) })
    }
    #[cfg(not(windows))]
    {
        let _ = on;
        Ok(())
    }
}
