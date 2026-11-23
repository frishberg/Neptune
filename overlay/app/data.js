// Data layer for the main window.
//
// Inside Tauri every function calls a Rust command. In a plain browser the
// same functions return generated data so the screens can be designed and
// screenshotted without the backend. The shape of each record is the
// contract the Rust side will fill.

(function () {
  const tauri = window.__TAURI__;
  const invoke = tauri && tauri.core ? tauri.core.invoke : null;

  // ---- mock corpus ----
  // [name, key, weight]: keys match the real icon cache, so a demo run of the
  // app shows the real icons; the weights give "Where you talk" a nice spread
  const APPS = [
    ['Claude Code', 'claudecode', 30], ['Outlook', 'outlook', 19], ['Chrome', 'chrome', 16],
    ['Windows Terminal', 'windowsterminal', 12], ['VS Code', 'code', 8],
  ];
  const APP_TOTAL = APPS.reduce((n, a) => n + a[2], 0);
  const pickApp = (r) => { let x = r * APP_TOTAL; for (const a of APPS) { if ((x -= a[2]) < 0) return a; } return APPS[0]; };
  const LINES = [
    'Also, remind me to send the deck to Gatsby Robotics before the standup tomorrow.',
    'Can we move the review to Thursday? I want to get the memory numbers in first.',
    'Refactor the session loop so the recorder owns the level pump instead of the state machine.',
    'Thanks for today. New paragraph. I will follow up with the timings once the build finishes.',
    'Hey, quick one. Does the trim threshold still clip the first word on quiet takes?',
    'Order the USB mic and two of the short cables, the long ones do not fit the desk.',
    'Sounds good. Let me know when the Mac build is green and I will cut a release.',
    'The pill should never widen. Only the amplitude follows the voice.',
    'Ship it. We can tune the unload timer after people have used it for a week.',
    'Book the dentist for the twelfth, morning if they have it.',
    'I think the sidebar is right. Five things, no more.',
    'Push the fix for the Bluetooth hands-free profile, it records silence otherwise.',
    'Yes. Merging now.',
    'What is the download size again? Six hundred and fifty megabytes I think.',
    'Draft: hi Maya, thanks for the intro. Would Tuesday or Wednesday next week work for a call?',
    'Add a retry with backoff to the upload step, three attempts, and log the last error.',
    'Running five minutes late, start without me and I will catch up on the notes.',
    'Let us keep the onboarding to one screen. Hold the keys, say something, done.',
    'Write a test that feeds it ten seconds of silence and checks nothing gets pasted.',
  ];
  const RAW = {
    'Also, remind me to send the deck to Gatsby Robotics before the standup tomorrow.':
      'Also, um, remind me to send the deck to gatsby robotics before the standup tomorrow.',
    'Thanks for today. New paragraph. I will follow up with the timings once the build finishes.':
      'Thanks for today, new paragraph, I will follow up with the timings once the build finishes.',
  };

  function seeded(seed) {
    let s = seed >>> 0;
    return () => ((s = (s * 1664525 + 1013904223) >>> 0) / 4294967296);
  }

  function mockHistory() {
    const rnd = seeded(7);
    const out = [];
    const now = Date.now();
    let id = 1;
    for (let day = 0; day < 84; day++) {
      const isWeekend = new Date(now - day * 864e5).getDay() % 6 === 0;
      // a heavy daily user who is using it more and more: busier recent weeks
      const n = day === 0 ? 9 : (isWeekend ? 2 + Math.floor(rnd() * 4) : 9 + Math.floor(rnd() * 14)) + (day < 7 ? 6 : 0);
      if (day === 19 || day === 47) continue; // a couple of days off, after a long streak
      for (let k = 0; k < n; k++) {
        const t = now - day * 864e5 - Math.floor(rnd() * 10 * 3600e3) - 60e3 * k * 7;
        const [app, key] = pickApp(rnd());
        const text = LINES[Math.floor(rnd() * LINES.length)];
        const words = text.split(/\s+/).length;
        const secs = words / (2.6 + rnd() * 0.8);
        let status = 'pasted';
        if (day === 0 && k === 2) status = 'paste_failed';
        if (day === 1 && k === 1) status = 'nothing';
        const none = status === 'nothing';
        out.push({
          id: id++, at: t, app, appKey: key, text: none ? '' : text, raw: none ? '' : (RAW[text] || text),
          words: none ? 0 : words, secs, status, audio: day < 7,
        });
      }
    }
    out.sort((a, b) => b.at - a.at);
    // the two newest takes, the ones Home shows under Latest
    const pin = (e, app, appKey, text) => Object.assign(e, { app, appKey, text, raw: text, words: text.split(/\s+/).length, status: 'pasted' });
    pin(out[0], 'Claude Code', 'claudecode', 'Write a test that feeds it ten seconds of silence and checks nothing gets pasted.');
    pin(out[1], 'Outlook', 'outlook', 'Running five minutes late, start without me and I will catch up on the notes.');
    return out;
  }

  const mock = {
    // ?empty=1 previews the screens as a brand-new user sees them.
    history: new URLSearchParams(location.search).has('empty') ? [] : mockHistory(),
    settings: {
      ptt: ['Ctrl', 'Space'], handsfree: ['Ctrl', 'Shift', 'Space'], pasteLast: ['Ctrl', 'Shift', 'V'], history: ['Ctrl', 'Shift', 'H'],
      mic: 'Microphone Array (Cirrus Logic)',
      mics: ['Microphone Array (Cirrus Logic)', 'Headset (WH-1000XM5 Hands-Free)', 'USB Audio Device'],
      model: 'parakeet', modelState: 'ready',
      fillers: true, textlessCopy: true,
      keepAudio: '7d', keepHistory: 'forever',
      autostart: true, pill: true, onboarded: true, modelKeepSecs: 60,
      modelSpec: { id: 'parakeet-tdt-0.6b-v3-int8', name: 'Parakeet TDT 0.6B v3', url: '', files: [], sizes: [670000000] },
      modelUpdate: null, modelChecked: 0,
    },
    version: '1.1.8',
  };

  // Inside Tauri a failure is a failure: it is thrown to the caller so the
  // screen can say so. The generated data is only for the browser.
  async function call(cmd, args, fallback) {
    if (invoke) return invoke(cmd, args);
    return typeof fallback === 'function' ? fallback() : fallback;
  }

  // Screenshot mode for the website: the native app launched with
  // NEPTUNE_DEMO=1 shows the sample history instead of the real one.
  const demo = invoke ? invoke('demo_mode').catch(() => false) : Promise.resolve(false);

  window.blurt = window.blurt || {};
  window.blurt.data = {
    isNative: !!invoke,
    // rows from before the rename are filed under the old name
    history: () => demo.then((d) => (d ? mock.history : call('history_list', {}, () => mock.history))).then((list) => (list || []).map((e) => (e.appKey === 'blurt' ? { ...e, app: 'Neptune', appKey: 'neptune' } : e))),
    // in demo mode the sample rows are edited in memory and the real history is never touched
    deleteEntry: (id) => { const f = () => { mock.history = mock.history.filter((e) => e.id !== id); }; return demo.then((d) => (d ? f() : call('history_delete', { id }, f))); },
    updateEntry: (id, text) => { const f = () => { const e = mock.history.find((x) => x.id === id); if (e) e.text = text; }; return demo.then((d) => (d ? f() : call('history_update', { id, text }, f))); },
    copy: (text) => call('copy_text', { text }, () => navigator.clipboard && navigator.clipboard.writeText(text)),
    paste: (text) => call('paste_text', { text }, () => {}),
    retry: (id) => demo.then((d) => (d ? undefined : call('history_retry', { id }, () => {}))),
    settings: () => call('settings_get', {}, () => mock.settings),
    saveSettings: (s) => call('settings_set', { settings: s }, () => { Object.assign(mock.settings, s); }),
    version: () => call('app_version', {}, () => mock.version),
    appIcon: (key) => call('app_icon', { key }, () => null),
    modelState: () => call('model_state_get', {}, () => mock.settings.modelState),
    modelDownload: (spec) => call('model_download', { spec: spec || null }, () => {}),
    modelCheck: (force) => call('model_check', { force: !!force }, () => null),
    modelPause: (paused) => call('model_download_pause', { paused: !!paused }, () => {}),
    updateCheck: () => call('app_update_check', {}, () => null),
    updateInstall: () => call('app_update_install', {}, () => {}),
    meterStart: () => call('meter_start', {}, () => {}),
    setTryMode: (on) => call('set_try_mode', { on }, () => {}),
    keyRelay: (on) => call('key_relay', { on }, () => {}),
    windowKey: (kind) => call('window_key', { kind }, () => {}),
    meterStop: () => call('meter_stop', {}, () => {}),
    // Backend events. In the browser nothing ever fires. Resolves to an
    // unlisten function.
    on: (name, fn) => (tauri && tauri.event) ? tauri.event.listen(name, (e) => fn(e.payload)) : Promise.resolve(() => {}),
    // Window chrome. In the browser these just log.
    minimize: () => tauri ? tauri.window.getCurrentWindow().minimize() : console.log('minimize'),
    maximize: () => tauri ? tauri.window.getCurrentWindow().toggleMaximize() : console.log('maximize'),
    startDrag: () => (tauri ? tauri.window.getCurrentWindow().startDragging() : null),
    close: () => tauri ? tauri.window.getCurrentWindow().close() : console.log('close'),
  };
})();
