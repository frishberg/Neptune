// neptune entry point.
//
// The app is a state machine driven by two hotkeys. Every OS-facing piece
// (hotkeys, audio, inference, paste) lives in its own small module. The pill
// window only ever receives "set state" and "level" events; it never decides
// anything.
//
//   idle --(Ctrl+Space down)--> listening --(up)--> processing --> done --> idle
//   idle --(Ctrl+Shift+Space)--> handsfree --(Ctrl+Shift+Space)--> processing
//   listening/handsfree --(Esc)--> idle

// Never show a console window, debug builds included. Logs go to files.
#![windows_subsystem = "windows"]

mod audio;
mod cleanup;
mod commands;
mod debug;
mod focus;
mod history;
mod hotkeys;
mod icons;
mod overlay;
mod package;
mod paste;
#[cfg(windows)]
mod pill_native;
mod power;
#[cfg(windows)]
mod ptt_hook;
mod session;
mod settings;
mod transcribe;
mod tray;
mod trim;

use tauri::Manager;

fn main() {
    env_logger::init();
    if debug::enabled() {
        debug::log("neptune started with debug capture ON");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // Second launch opens the window of the running one, on
            // Settings if asked (neptune.exe --settings).
            let settings = args.iter().any(|a| a == "--settings");
            overlay::show_main(app, settings.then_some("settings"));
        }))
        .setup(|app| {
            let handle = app.handle().clone();
            let s = settings::load(&handle);
            handle.manage(settings::Store(std::sync::Mutex::new(s.clone())));
            handle.manage(history::Lock(std::sync::Mutex::new(())));
            handle.manage(commands::Meter(std::sync::Mutex::new(None)));
            handle.manage(commands::UpdateSlot(std::sync::Mutex::new(None)));
            history::prune(&handle, &s.keep_audio, &s.keep_history);


            power::init();
            overlay::start_pill(&handle, s.pill)?;
            #[cfg(windows)]
            ptt_hook::install(handle.clone());
            hotkeys::register(&handle, &s)?;
            tray::build(&handle)?;
            session::start(handle.clone());

            // Dev: BLURT_DICTATE=<file.wav> runs one dictation from a file
            // shortly after launch. Debug builds only.
            if cfg!(debug_assertions) {
                if let Ok(p) = std::env::var("BLURT_DICTATE") {
                    let h = handle.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(1500));
                        session::send(&h, session::Input::DictateFile(p.into()));
                    });
                }
            }

            // First run: the window opens on the setup steps.
            if !s.onboarded || (cfg!(debug_assertions) && std::env::var_os("NEPTUNE_ONBOARDING").is_some()) {
                overlay::show_main(&handle, None);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            session::pill_clicked,
            session::set_try_mode,
            session::window_key,
            session::paste_last,
            session::cancel,
            session::copy_last,
            commands::app_version,
            commands::history_list,
            commands::history_delete,
            commands::history_update,
            commands::history_retry,
            commands::copy_text,
            commands::paste_text,
            commands::settings_get,
            commands::settings_set,
            commands::meter_start,
            commands::key_relay,
            commands::meter_stop,
            commands::model_download,
            commands::model_state_get,
            commands::model_download_pause,
            commands::model_check,
            commands::app_update_check,
            commands::app_update_install,
            commands::open_main,
            commands::app_icon,
            commands::demo_mode,
        ])
        .build(tauri::generate_context!())
        .expect("neptune failed to start")
        .run(|_app, event| {
            // Closing the last window is not quitting: neptune lives in the
            // tray. Only an explicit exit (Quit in the tray) ends it.
            if let tauri::RunEvent::ExitRequested { code: None, api, .. } = event {
                api.prevent_exit();
            }
        });
}
