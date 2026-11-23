// neptune pill controller.
// States: idle | listening | handsfree | processing | done | error
// The native side drives this through Tauri events (state, level, error).
// Standalone in a browser it also responds to ?state= for screenshots and to
// the space bar for a quick demo.

(function () {
  const pill = document.getElementById('pill');
  const stage = document.getElementById('stage');
  const tipText = document.querySelector('.tip-text');
  const errText = document.querySelector('.err-text');
  const canvas = document.getElementById('wave');
  const ctx = canvas.getContext('2d');
  const DOT_COUNT = 5;


  function fill(el, n) {
    for (let k = 0; k < n; k++) el.appendChild(document.createElement('i'));
  }

  // ---- wave ----
  // One sine, shaped by a bell envelope so the wave tapers to nothing at both
  // ends of the pill. Amplitude follows the mic level almost directly: a fast
  // attack so each syllable lands, a short release so the gaps between them
  // do not flicker. A dead mic is a near-flat line, which is the honest signal.
  const LAYERS = [
    { freq: 2.0, speed: 1.0, alpha: 1.0, width: 2.2 },
  ];
  let level = 0;
  let amp = 0;
  let vel = 0; // the spring's velocity
  let phase = 0;
  let raf = 0;
  let demoTimer = 0;
  let last = 0;

  function setLevel(v) {
    level = Math.max(0, Math.min(1, Number(v) || 0));
  }

  function sizeCanvas() {
    const dpr = window.devicePixelRatio || 1;
    const r = canvas.parentElement.getBoundingClientRect();
    const w = Math.max(1, Math.round(r.width));
    const h = Math.max(1, Math.round(r.height));
    if (canvas.width !== w * dpr || canvas.height !== h * dpr) {
      canvas.width = w * dpr;
      canvas.height = h * dpr;
    }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    return { w, h };
  }

  // The ribbon starts as the 40 px idle line and, when listening begins,
  // grows once to full length. After that the span never changes: only the
  // amplitude follows the voice.
  const IDLE_SPAN = 40;
  const GROW_SECS = 0.3;
  let grow = 0; // 0 = idle line, 1 = full ribbon

  function paint() {
    const { w, h } = sizeCanvas();
    ctx.clearRect(0, 0, w, h);
    const mid = h / 2;
    const full = w - 12;
    const e = 1 - Math.pow(1 - grow, 3); // ease-out
    const span = IDLE_SPAN + (full - IDLE_SPAN) * e;
    const x0 = (w - span) / 2;
    const A = h * 0.36 * amp;

    // The idle line, only while the ribbon is still growing out of it.
    ctx.lineCap = 'round';
    if (e < 1) {
      ctx.strokeStyle = `rgba(244, 244, 238, ${0.42 * (1 - e)})`;
      ctx.lineWidth = 2;
      ctx.beginPath(); ctx.moveTo(x0, mid); ctx.lineTo(x0 + span, mid); ctx.stroke();
    }

    // The ribbon: two sines on the same span, one bright, one faint echo.
    // Flat blue, no gradient. The bell envelope already brings both ends to
    // rest on the line, so the ribbon needs no fade to sit inside the pill.
    ctx.strokeStyle = '#6ea8ff';
    ctx.shadowColor = 'rgba(110, 168, 255, 0.45)';
    ctx.shadowBlur = 5;
    for (const L of LAYERS) {
      ctx.beginPath();
      // A Hann envelope with two crests: that pair has an average of exactly
      // zero at every phase, so the ribbon balances on the midline by
      // construction, with nothing to correct and nothing to bob.
      for (let x = 0; x <= span; x += 1) {
        const t = x / span;
        const env = Math.pow(Math.sin(Math.PI * t), 2);
        const y = mid + Math.sin(t * Math.PI * 2 * L.freq + phase * L.speed) * A * env;
        if (x === 0) ctx.moveTo(x0 + x, y); else ctx.lineTo(x0 + x, y);
      }
      ctx.globalAlpha = L.alpha * e;
      ctx.lineWidth = L.width;
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
    ctx.shadowBlur = 0;
  }

  function frame(now) {
    const dt = last ? Math.min(0.05, (now - last) / 1000) : 0.016;
    last = now;
    // A critically damped spring toward the level, solved exactly per
    // frame: the amplitude has continuous velocity, so it never hops.
    {
      const w = 2 * Math.PI * 10, e = Math.exp(-w * dt);
      const x = amp - level;
      const nx = (x + (vel + w * x) * dt) * e;
      vel = (vel - w * (vel + w * x) * dt) * e;
      amp = level + nx;
    }
    grow = Math.min(1, grow + dt / GROW_SECS);
    phase += dt * 4.5;
    paint();
    raf = requestAnimationFrame(frame);
  }

  function startWave() {
    if (!raf) { last = 0; grow = 0; raf = requestAnimationFrame(frame); }
  }
  function stopWave() {
    cancelAnimationFrame(raf);
    raf = 0;
    amp = 0;
    vel = 0;
    level = 0;
    grow = 0;
  }

  // ---- state machine ----
  let doneTimer = 0;
  let hold = false; // screenshot mode: transient states stay put

  function setState(next) {
    const prev = pill.dataset.state;
    if (prev === next) return;
    clearTimeout(doneTimer);
    pill.dataset.state = next;

    const listening = next === 'listening' || next === 'handsfree';
    if (listening) startWave();
    else stopWave();

    if (next !== 'idle') stage.classList.remove('show-tip');

    if (!hold && next === 'done') doneTimer = setTimeout(() => setState('idle'), pill.classList.contains('offer') ? 5000 : 1200);
    if (!hold && next === 'error') doneTimer = setTimeout(() => setState('idle'), 2200);
    if (next === 'idle') { clearTimeout(readyTimer); pill.classList.remove('offer', 'ready'); copyLabel.textContent = 'Copy'; }
  }
  // Done, but the text had nowhere to go: show Copy for a while.
  let readyTimer = 0;
  function setOffer() {
    pill.classList.add('offer');
    clearTimeout(doneTimer); clearTimeout(readyTimer);
    readyTimer = setTimeout(() => pill.classList.add('ready'), 1200);
    if (!hold) doneTimer = setTimeout(() => setState('idle'), 6000);
  }
  function setError(message) {
    errText.textContent = message || 'Something went wrong';
    setState('error');
    if (message) {
      if (!tipRestore) tipRestore = tipText.innerHTML;
      tipText.textContent = message;
      stage.classList.add('show-tip');
      setTimeout(() => { if (pill.dataset.state === 'error' && hold) return; stage.classList.remove('show-tip'); if (tipRestore) { tipText.innerHTML = tipRestore; tipRestore = null; } }, 2100);
    }
  }

  function setTip(html) { tipText.innerHTML = html; }
  function showTip(show) { stage.classList.toggle('show-tip', !!show); }
  function hide(hidden) { stage.classList.toggle('hidden', !!hidden); }

  pill.addEventListener('mouseenter', () => {
    if (pill.dataset.state === 'idle') showTip(true);
  });
  pill.addEventListener('mouseleave', () => showTip(false));

  // ---- native side (Tauri) ----
  const tauri = window.__TAURI__;
  function invoke(cmd) {
    if (tauri && tauri.core) tauri.core.invoke(cmd);
  }

  pill.addEventListener('click', (e) => {
    if (e.target.closest('.ctl')) return;
    if (pill.classList.contains('ready')) {
      // the pill is the Copy button right now
      if (tauri) invoke('copy_last');
      copyLabel.textContent = 'Copied';
      clearTimeout(doneTimer);
      doneTimer = setTimeout(() => setState('idle'), 900);
      return;
    }
    if (tauri) invoke('pill_clicked'); else toggleDemo();
  });
  document.getElementById('cancel').addEventListener('click', (e) => {
    e.stopPropagation();
    if (tauri) invoke('cancel'); else { fakeLevels(false); setState('idle'); }
  });
  const copyLabel = document.querySelector('.copy-label span');
  document.getElementById('stop').addEventListener('click', (e) => {
    e.stopPropagation();
    if (tauri) invoke('pill_clicked'); else toggleDemo();
  });

  if (tauri && tauri.event) {
    tauri.event.listen('state', (e) => setState(e.payload));
    tauri.event.listen('level', (e) => setLevel(e.payload));
    tauri.event.listen('error', (e) => setError(e.payload));
    tauri.event.listen('offer', () => setOffer());

    // Hidden from settings or the tray: melt away first, the window follows.
    tauri.event.listen('vanish', () => { stage.classList.remove('appear'); stage.classList.add('vanish'); });
    tauri.event.listen('appear', () => { stage.classList.remove('vanish'); stage.classList.add('appear'); setTimeout(() => stage.classList.remove('appear'), 600); });
    // The window is click-through around the pill, so the browser never
    // sees mouseleave. The backend tells us when the cursor crosses the edge.
    tauri.event.listen('hover', (e) => showTip(!!e.payload && pill.dataset.state === 'idle'));
  }

  // ---- demo mode (browser only) ----
  function fakeLevels(on) {
    clearInterval(demoTimer);
    if (!on) return;
    let t = 0;
    demoTimer = setInterval(() => {
      t += 0.09;
      const burst = Math.max(0, Math.sin(t * 1.3)) * Math.max(0, Math.sin(t * 0.37) + 0.4);
      setLevel(burst * (0.55 + 0.45 * Math.random()));
    }, 40);
  }

  function toggleDemo() {
    const s = pill.dataset.state;
    if (s === 'idle') { setState('handsfree'); fakeLevels(true); }
    else if (s === 'handsfree' || s === 'listening') {
      fakeLevels(false);
      setState('processing');
      setTimeout(() => setState('done'), 1400);
    }
  }

  if (!tauri) {
    let held = false;
    window.addEventListener('keydown', (e) => {
      if (e.code === 'Space' && !held && pill.dataset.state === 'idle') {
        e.preventDefault();
        held = true; setState('listening'); fakeLevels(true);
      }
      if (e.code === 'Escape') { fakeLevels(false); setState('idle'); }
    });
    window.addEventListener('keyup', (e) => {
      if (e.code === 'Space' && held) {
        held = false; fakeLevels(false); setState('processing');
        setTimeout(() => setState('done'), 1400);
      }
    });
  }

  // ---- screenshot / debug params ----
  const q = new URLSearchParams(location.search);
  hold = !!q.get('hold');
  if (q.get('slow')) document.documentElement.style.setProperty('--slow', q.get('slow'));
  if (q.get('state')) {
    setState(q.get('state'));
    if (q.get('state') === 'listening' || q.get('state') === 'handsfree') {
      if (q.get('freeze')) {
        // Paint one good frame with a fixed amplitude and stop.
        setTimeout(() => {
          cancelAnimationFrame(raf); raf = 0;
          amp = 0.85; phase = 2.1; grow = 1;
          paint();
        }, 700);
      } else {
        fakeLevels(true);
      }
    }
  }
  // ?to=listening&at=700 switches state after a delay, for motion screenshots.
  if (q.get('to')) {
    setTimeout(() => {
      setState(q.get('to'));
      if (q.get('to') === 'listening' || q.get('to') === 'handsfree') { level = 0.8; }
    }, Number(q.get('at') || 0));
  }
  if (q.get('tip')) showTip(true);
  if (q.get('error')) setError(q.get('error'));
  if (q.get('offer')) { setState('done'); setOffer(); }


  window.blurt = { setState, setLevel, setError, setTip, showTip, hide };
})();
