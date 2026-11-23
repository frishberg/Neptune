<div align="center">

<img src="docs/icon.png" alt="Neptune" width="96" height="96">

# Neptune

<a href="../../releases/latest"><img src="https://img.shields.io/github/v/release/frishberg/Neptune?label=version&color=2f5fe0" alt="Version"></a>
<a href="LICENSE"><img src="https://img.shields.io/github/license/frishberg/Neptune?color=67ac09" alt="License: MIT"></a>
<a href="https://neptunetype.com"><img src="https://img.shields.io/badge/runs-100%25%20offline-2f5fe0" alt="Runs 100% offline"></a>

**Free voice dictation for Windows that runs entirely on your PC.**
Hold `Ctrl` + `Space`, talk, let go, and clean text lands wherever your cursor is.

<a href="https://neptunetype.com/#download"><img src="docs/btn-store.svg" alt="Recommended: get it from the Microsoft Store" height="54"></a>
&nbsp;
<a href="../../releases/latest"><img src="docs/btn-exe.svg" alt="Direct: download the .exe" height="54"></a>

<img src="docs/screenshot.png" width="640" alt="The Neptune window: words spoken, hours saved, streak, speaking speed, the last month as a calendar, and the apps you talk into">

</div>

## What it does

- **Hold to talk** in any app with a text cursor: Outlook, Slack, Chrome, VS Code, Claude Code, Windows Terminal, anything. Let go and the words are pasted at the cursor. If there's nowhere to type, the pill offers a Copy button.
- **Cleans up as it goes**: ums and uhs come out, and "new line" or "new paragraph" do what they say.
- **Hands-free** for longer dictation, **paste last** to put your last dictation in again, and a searchable **history** of everything you've said.
- **Home** shows what you've saved: words spoken, hours not spent typing, your streak, how fast you speak, and where you talk.
- A small **pill** at the bottom of the screen shows only while you talk.

A free, open-source alternative to Wispr Flow: no account, no cloud, no subscription, no word limit.

## Shortcuts

| Action | Keys |
| --- | --- |
| Hold to talk | hold `Ctrl` + `Space`, let go to paste |
| Hands-free | `Ctrl` + `Shift` + `Space` to start, again to finish |
| Cancel | `Esc` while listening |
| Paste last | `Ctrl` + `Shift` + `V` |
| Open history | `Ctrl` + `Shift` + `H` |

All of them can be changed in Settings.

## Privacy

Your voice is turned into text on your own PC. Audio, text and history never leave it. The speech model downloads once during setup; after that Neptune never goes online, not even to check for updates, unless you click **Check for updates** in Settings. No account, no analytics, no ads. The Microsoft Store version is kept up to date by the Store. Full [Privacy Policy](https://neptunetype.com/privacy/).

## How it works

```
hotkey -> mic (16 kHz) -> silence trimmed -> speech model -> cleanup -> paste at cursor
```

- **Speech model**: NVIDIA Parakeet TDT 0.6B v3 (25 European languages), int8 ONNX on the CPU. A GPU made no measurable difference, so there is no GPU option. It loads when you talk and unloads after a minute idle, so Neptune costs about 70 MB when you're not using it.
- **Chunking**: while you talk, the audio is cut at pauses and transcribed as you go, so letting go feels instant however long you spoke.
- **Cleanup**: rules only, no LLM: filler words, spoken layout commands, repeated words.
- **Insert**: a clipboard paste, with your old clipboard put back afterwards.
- **Hold-to-talk**: a low-level keyboard hook catches the trigger key so it never types. Modifiers are never faked, so Windows always sees your real keys.

## Build from source

Needs Rust (stable), Node 20 and the Visual Studio C++ build tools.

```
npm ci
npx tauri build --bundles nsis
```

The installer lands in `src-tauri/target/release/bundle/nsis/`. To try the UI without building, serve this folder (`python -m http.server 4173`) and open `/overlay/app/index.html?view=home` (or `history`, `settings`, `?onboarding=0`); it runs on sample data.

## Layout

```
overlay/         the pill (also drawn natively on Windows: src-tauri/src/pill_native.rs)
overlay/app/     the window: plain HTML, CSS and JS, no framework
src-tauri/       the Rust backend: hotkeys, audio, inference, paste, history, updates
src-tauri/windows/installer.nsi   the installer: no pages to click, opens straight into onboarding
scripts/         icon, share-card and Store art renderers; an idle memory and CPU check
model.json       the speech model the app should be using (read only by Check for updates)
```

Data lives in `%APPDATA%\app.blurt` (the folder keeps an older name so existing installs keep their history): `settings.json`, `models/`, and `history/`. Uninstalling removes the speech model; settings and history stay unless you tick the box.

## Stack

Tauri 2. Rust does everything that touches the OS. The window is plain HTML, CSS and JS in a frameless webview, built only while it's open.

## Releasing

Push a `v*` tag and `.github/workflows/release.yml` builds the installer and a draft release with the updater feed (`latest.json`). It signs with Microsoft Trusted Signing when the Azure settings listed at the top of the workflow are set.

## License

MIT. See [LICENSE](LICENSE). The speech model keeps its own licence (Parakeet: CC-BY-4.0). Made by [Aron Frishberg](https://aronfrishberg.com/).
