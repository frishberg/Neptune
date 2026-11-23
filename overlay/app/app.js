// neptune main window controller. Three views on the rail, plus onboarding.
// Everything reads through window.blurt.data so it works in a browser with
// generated data and in Tauri with the real backend.

(function () {
  const D = window.blurt.data;
  const main = document.getElementById('main');
  const q = new URLSearchParams(location.search);
  if (!D.isNative) document.body.classList.add('preview');
  // Deep: the app lives at night. ?theme=light still previews the day look.
  document.documentElement.dataset.theme = q.get('theme') || 'dark';

  document.getElementById('min').addEventListener('click', D.minimize);
  document.getElementById('max').addEventListener('click', D.maximize);
  document.getElementById('close').addEventListener('click', D.close);
  // The title bar is everything above the page's first card: from the top
  // of the window down to 8 px short of that card (on Home, the Words spoken
  // card), right of the rail. A press anywhere in it drags the window, unless
  // it lands on something you can click or type in. A double-click there
  // maximizes and restores, like any title bar.
  const TITLE_MIN = 56, TITLE_GAP = 8;
  const titleBottom = () => {
    const head = document.querySelector('#main .page:last-child .head');
    const below = head && head.nextElementSibling;
    return below ? Math.max(TITLE_MIN, below.getBoundingClientRect().top - TITLE_GAP) : TITLE_MIN;
  };
  const inTitleBand = (ev) => ev.clientX > document.getElementById('rail').getBoundingClientRect().right
    && ev.clientY < titleBottom()
    && !ev.target.closest('button, a, input, textarea, select, label, .search, .tt');
  // (the drag swallows the mouse-up, so a double-click is read off the press)
  document.addEventListener('mousedown', (ev) => {
    if (ev.button !== 0 || !inTitleBand(ev)) return;
    ev.preventDefault();
    if (ev.detail === 2) D.maximize();
    else D.startDrag();
  });
  let version = '';
  D.version().then((v) => (version = v));
  // The foot of the rail says nothing unless something needs you: a dot
  // with a tip, one per notice, each a click away from Settings.
  const foot = document.getElementById('side-foot');
  let modelState = 'ready';
  let appUpdate = null; // { version, notes } once a newer release is known
  function renderFoot() {
    foot.replaceChildren();
    const dot = (cls, text, id) => foot.append(h('button', { class: 'fdot ' + cls, onclick: () => show('settings') }, h('span', { class: 'tip', id }, text)));
    if (modelState === 'missing') dot('missing', 'No speech model yet. Click to get it');
    if (modelState === 'downloading') dot('loading', 'Downloading model', 'dl-text');
    if (appUpdate) dot('update', `Neptune ${appUpdate.version} is ready to install`);
  }
  function setModelState(st) { modelState = st; renderFoot(); }
  let downloading = false;
  // Whoever is showing progress right now (onboarding step, Settings) hooks in here.
  let onDownload = null;
  D.on('download', (d) => {
    downloading = !d.finished;
    if (modelState !== 'downloading' && !d.finished) setModelState('downloading');
    const txt = document.getElementById('dl-text');
    if (txt) txt.textContent = d.error ? 'Download failed' : d.finished ? 'Model ready' : `Downloading model, ${Math.round((d.done / d.total) * 100)}%` + dlRate(d);
    if (d.finished) setTimeout(() => setModelState(d.error ? 'missing' : 'ready'), d.error ? 2500 : 1200);
    if (onDownload) onDownload(d);
  });
  // Speed and time left, from the progress so far. Paused reads as paused.
  let dlPaused = false, dlT0 = 0, dlD0 = 0;
  function dlRate(d) {
    if (d.finished) return '';
    if (dlPaused) return ' · paused';
    if (!dlT0) { dlT0 = Date.now(); dlD0 = d.done; return ''; }
    const dt = (Date.now() - dlT0) / 1000;
    if (dt < 1) return '';
    const bps = (d.done - dlD0) / dt;
    if (bps <= 0) return '';
    const left = (d.total - d.done) / bps;
    const eta = left >= 90 ? `${Math.round(left / 60)} min left` : `${Math.round(left)} s left`;
    return ` · ${(bps / 1048576).toFixed(bps >= 10485760 ? 0 : 1)} MB/s · ${eta}`;
  }
  async function pauseDownload(paused) {
    dlPaused = paused; dlT0 = 0;
    try { await D.modelPause(paused); } catch (e) { toast('Could not pause: ' + e); }
  }
  // Starts the download unless one is running. Resolves false if it could not start.
  async function startDownload(spec) {
    dlPaused = false; dlT0 = 0;
    if (downloading) return true;
    downloading = true;
    try { await D.modelDownload(spec); return true; } catch (e) { downloading = false; console.error(e); toast('Could not start the download: ' + e); return false; }
  }
  // A newer release, if there is one. Only when you ask (Settings, Check for
  // updates): after the one model download, Neptune never goes online by
  // itself. The Store keeps its own copy up to date.
  let updateProgress = null;
  D.on('update', (p) => { if (updateProgress) updateProgress(p); });
  async function checkUpdate() {
    try { appUpdate = await D.updateCheck(); } catch (e) { appUpdate = null; }
    renderFoot();
    return appUpdate;
  }
  D.modelState().then(setModelState).catch(() => setModelState('missing'));
  D.on('model_state', setModelState);
  D.on('view', (v) => { if (views[v]) show(v); });
  // A dictation landed, or an entry changed. Redraw quietly (no count-up
  // replay), and not while the user is mid-edit on the History page.
  let quietUntil = 0;
  let historyApi = null;
  D.on('history_changed', async () => {
    if (Date.now() < quietUntil) return;
    if (current === 'history' && historyApi && await historyApi.refresh()) return;
    if (current === 'history' || current === 'home') show(current, true);
  });

  // A small notice at the bottom of the window, gone in a moment.
  let toastEl = null, toastTimer = 0;
  function toast(msg) {
    if (!toastEl) { toastEl = h('div', { class: 'toast' }); document.body.append(toastEl); }
    toastEl.textContent = msg; toastEl.classList.add('show');
    clearTimeout(toastTimer); toastTimer = setTimeout(() => toastEl.classList.remove('show'), 2600);
  }
  // Copy says so where you clicked: the icon becomes a check for a moment.
  const copyText = async (text, btn) => {
    try { await D.copy(text); } catch (e) { toast('Could not copy: ' + e); return; }
    if (!btn || btn._did) return;
    btn._did = true;
    const keep = [...btn.childNodes];
    btn.replaceChildren(...[ICON.check(), keep.some((n) => n.nodeType === 3) ? 'Copied' : null].filter(Boolean));
    btn.classList.add('did');
    setTimeout(() => { btn.replaceChildren(...keep); btn.classList.remove('did'); btn._did = false; }, 1400);
  };
  const pasteText = async (text) => { try { await D.paste(text); } catch (e) { toast('Could not paste: ' + e); } };

  // One tooltip for the whole window, drawn by us: anything with data-tip
  // gets a small ink label above it (below, if there is no room).
  const tt = document.createElement('div'); tt.className = 'tt';
  document.body.append(tt);
  let ttTimer = 0, ttFor = null;
  const hideTip = () => { clearTimeout(ttTimer); ttFor = null; tt.classList.remove('show'); };
  document.addEventListener('mouseover', (ev) => {
    const el = ev.target.closest('[data-tip]');
    if (!el || el === ttFor) return;
    clearTimeout(ttTimer); ttFor = el;
    ttTimer = setTimeout(() => {
      if (!el.isConnected) return;
      tt.textContent = el.dataset.tip;
      const r = el.getBoundingClientRect();
      tt.style.left = (r.left + r.width / 2) + 'px';
      const above = r.top - 8 - tt.offsetHeight;
      tt.classList.toggle('below', above < 6);
      tt.style.top = (above < 6 ? r.bottom + 8 : above) + 'px';
      tt.classList.add('show');
    }, 70);
  });
  document.addEventListener('mouseout', (ev) => {
    const el = ev.target.closest('[data-tip]');
    if (el && !(ev.relatedTarget && el.contains(ev.relatedTarget))) hideTip();
  });
  document.addEventListener('mousedown', hideTip);
  document.addEventListener('scroll', hideTip, true);

  // The mark in the corner is still. It is a logo, not a toy.
  const stillMotion = matchMedia('(prefers-reduced-motion: reduce)').matches;

  // ---- the rail ----
  // A ribbon down the rail, threaded through the three section beads and
  // pinned at each bead's centre. Its shape is fixed. The blue runs from the
  // logo to the active bead, and the only thing that ever moves is that blue
  // sliding to a new bead when you pick one.
  const railApi = (function rail() {
    const rail = document.getElementById('rail'), track = document.getElementById('track'), ink = document.getElementById('ink'), ring = document.getElementById('ring');
    const wave = rail.querySelector('.wave');
    const nodes = [...rail.querySelectorAll('.node')];
    // the one moment of the old drift the shape is frozen at; the app icon
    // is drawn from the same formula and moment
    const PH = 2.0;
    let inkY = 0, inkFrom = 0, inkTarget = 0, inkT0 = 0, H = 720, beads = [0, 0, 0], raf = 0;
    const active = () => Math.max(0, nodes.findIndex((n) => n.classList.contains('on')));
    const nodeY = (i) => { const r = nodes[i].getBoundingClientRect(), rr = rail.getBoundingClientRect(); return r.top + r.height / 2 - rr.top; };
    function measure() {
      H = rail.getBoundingClientRect().height || 720;
      wave.setAttribute('viewBox', `0 0 84 ${H}`);
      beads = nodes.map((_, i) => nodeY(i));
    }
    // On the last section the blue runs all the way to the ribbon's end, so
    // the whole line can be coloured in.
    const target = () => (active() === nodes.length - 1 ? H - 44 : beads[active()]);
    function moveInk() {
      measure();
      inkFrom = inkY; inkTarget = target(); inkT0 = performance.now();
      if (!raf) raf = requestAnimationFrame(slide);
    }
    new ResizeObserver(() => { measure(); inkY = inkFrom = inkTarget = target(); draw(); }).observe(rail);
    // Sample the ribbon every few pixels and join the samples with a
    // Catmull-Rom spline, so the stroke is one continuous curve with no
    // corners anywhere, however it bends.
    function spline(pts) {
      if (pts.length < 2) return '';
      let d = `M${pts[0][0].toFixed(1)},${pts[0][1].toFixed(1)}`;
      for (let i = 0; i < pts.length - 1; i++) {
        const p0 = pts[Math.max(0, i - 1)], p1 = pts[i], p2 = pts[i + 1], p3 = pts[Math.min(pts.length - 1, i + 2)];
        const c1 = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6];
        const c2 = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
        d += ` C${c1[0].toFixed(1)},${c1[1].toFixed(1)} ${c2[0].toFixed(1)},${c2[1].toFixed(1)} ${p2[0].toFixed(1)},${p2[1].toFixed(1)}`;
      }
      return d;
    }
    // The same spline, closed, for the loop around the mark.
    function loop(pts) {
      const n = pts.length, at = (i) => pts[(i + n) % n];
      let d = `M${pts[0][0].toFixed(2)},${pts[0][1].toFixed(2)}`;
      for (let i = 0; i < n; i++) {
        const p0 = at(i - 1), p1 = at(i), p2 = at(i + 1), p3 = at(i + 2);
        d += ` C${(p1[0] + (p2[0] - p0[0]) / 6).toFixed(2)},${(p1[1] + (p2[1] - p0[1]) / 6).toFixed(2)} ${(p2[0] - (p3[0] - p1[0]) / 6).toFixed(2)},${(p2[1] - (p3[1] - p1[1]) / 6).toFixed(2)} ${p2[0].toFixed(2)},${p2[1].toFixed(2)}`;
      }
      return d + 'Z';
    }
    // each gap's hand: how far its swell leans (px, sign is the side), how
    // many little pen wavers ride on it, and where they start
    const HAND = [[7.5, 2.2, 0.4], [-6.2, 1.6, 2.1], [5.2, 2.6, 4.0], [-5, 1.9, 1.2]];
    // The logo is a filled pebble the ribbon hangs from: never quite round.
    const RX = 42, RY = 34;
    const ringR = (a) => 21.6 + Math.sin(3 * a + PH * 0.9) * 0.5 + Math.sin(2 * a - PH * 0.6 + 1.3) * 0.4;
    // the blue slides to the picked bead, easing out over 640 ms
    function slide(now) {
      const k = stillMotion ? 1 : Math.min(1, (now - inkT0) / 640), e = 1 - Math.pow(1 - k, 3);
      inkY = inkFrom + (inkTarget - inkFrom) * e;
      draw();
      raf = k < 1 ? requestAnimationFrame(slide) : 0;
    }
    function draw() {
      // the ribbon unspools from the bottom of the logo
      const ringPts = [];
      for (let i = 0; i < 36; i++) { const a = (i / 36) * Math.PI * 2, r = ringR(a); ringPts.push([RX + Math.cos(a) * r, RY + Math.sin(a) * r]); }
      ring.setAttribute('d', loop(ringPts));
      const top = RY + ringR(Math.PI / 2), bottom = H - 44;
      // Drawn by hand: the ribbon is pinned at the centre of every bead, and
      // each gap between them is its own single slow swell, with a faint pen
      // waver on top. The bend at a bead is hidden behind the bead itself.
      const stops = [top, ...beads, bottom];
      const all = [], blue = [];
      for (let y = top; y <= bottom + 0.5; y += 6) {
        let g = 0;
        while (g < stops.length - 2 && y > stops[g + 1]) g++;
        const u = Math.min(1, Math.max(0, (y - stops[g]) / (stops[g + 1] - stops[g])));
        const [swell, waves, phase] = HAND[g % HAND.length];
        const x = 42 + Math.sin(Math.PI * u) * swell + Math.sin(u * Math.PI * 2 * waves + phase) * 1.1 * Math.sin(Math.PI * u);
        all.push([x, y]);
        if (y <= inkY) blue.push([x, y]);
      }
      // the blue ends exactly at the active bead, on the curve
      if (blue.length && blue.length < all.length) {
        const a = all[blue.length - 1], b = all[blue.length];
        const t = (inkY - a[1]) / (b[1] - a[1]);
        blue.push([a[0] + (b[0] - a[0]) * t, inkY]);
      }
      track.setAttribute('d', spline(all)); ink.setAttribute('d', spline(blue));
    }
    setTimeout(() => { measure(); inkY = inkFrom = inkTarget = target(); draw(); }, 30);
    // where a bead sits, relative to the main area, for the page sweep
    const origin = (view) => {
      const n = nodes.find((x) => x.dataset.view === view) || nodes[0];
      const r = n.getBoundingClientRect(), m = main.getBoundingClientRect();
      return { x: r.left + r.width / 2 - m.left, y: r.top + r.height / 2 - m.top };
    };
    return { moveInk, origin };
  })();

  // ---- helpers ----
  const h = (tag, attrs, ...kids) => {
    const el = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs || {})) {
      if (k === 'class') el.className = v;
      else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
      else if (k === 'html') el.innerHTML = v;
      else el.setAttribute(k, v);
    }
    for (const kid of kids.flat()) if (kid != null) el.append(kid.nodeType ? kid : document.createTextNode(kid));
    return el;
  };
  // ---- hand-drawn shapes ----
  // A smooth curve through points (Catmull-Rom), open or closed.
  const curve = (pts, closed) => {
    const n = pts.length, f = (v) => v.toFixed(2);
    const at = (i) => (closed ? pts[(i + n) % n] : pts[Math.max(0, Math.min(n - 1, i))]);
    let d = `M${f(pts[0][0])},${f(pts[0][1])}`;
    for (let i = 0; i < (closed ? n : n - 1); i++) {
      const p0 = at(i - 1), p1 = at(i), p2 = at(i + 1), p3 = at(i + 2);
      d += `C${f(p1[0] + (p2[0] - p0[0]) / 6)},${f(p1[1] + (p2[1] - p0[1]) / 6)} ${f(p2[0] - (p3[0] - p1[0]) / 6)},${f(p2[1] - (p3[1] - p1[1]) / 6)} ${f(p2[0])},${f(p2[1])}`;
    }
    return closed ? d + 'Z' : d;
  };
  // Each bar in Where you talk ends the way a pen lifts off the page: the
  // line carries on a moment, then flicks up and away.
  const penLift = () => h('span', { class: 'end', html: '<svg viewBox="0 0 9 8" aria-hidden="true"><path d="M0 6.5C3.2 6.5 5 6 7.6 1.6"/></svg>' });
  // The centre squiggle: a hairline with one short blue wave in the middle.
  // It sits above Latest on Home and between days in History.
  const DAYSEP_WAVE = (() => {
    const pts = [];
    for (let i = 0; i <= 24; i++) { const u = i / 24; pts.push([u * 52, 6 + Math.sin(u * Math.PI * 4) * 3 * Math.sin(Math.PI * u)]); }
    return curve(pts, false);
  })();
  const daySep = () => h('div', { class: 'daysep', 'aria-hidden': 'true' }, h('i'),
    h('span', { html: `<svg viewBox="0 0 52 12"><path d="${DAYSEP_WAVE}"/></svg>` }).firstChild, h('i'));
  const svg = (paths, vb = '0 0 16 16') => h('span', { html: `<svg viewBox="${vb}" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${paths}</svg>` }).firstChild;
  const ICON = {
    copy: () => svg('<rect x="5" y="5" width="8" height="8" rx="2"/><path d="M3 11V4a1 1 0 0 1 1-1h7"/>'),
    paste: () => svg('<path d="M8 2v9M4.5 7.5 8 11l3.5-3.5M3 13.5h10"/>'),
    retry: () => svg('<path d="M13 8a5 5 0 1 1-1.5-3.6"/><path d="M13 3v3h-3"/>'),
    trash: () => svg('<path d="M3 4.5h10M6.5 4.5v-1h3v1M4.5 4.5l.6 8h5.8l.6-8"/>'),
    search: () => svg('<circle cx="7" cy="7" r="4.2"/><path d="M10.2 10.2 13.5 13.5"/>'),
    x: () => svg('<path d="M4 4l8 8M12 4l-8 8"/>'),
    check: () => svg('<path d="M3.5 8.5l3 3L12.5 5"/>'),
  };
  // A letter tile first; the app's real icon replaces it once the backend
  // has it (pulled from the executable the first time we saw the app).
  // How much of its square each icon's drawing really fills, measured once
  // per icon from its non-transparent pixels. Icons are then scaled so their
  // drawings all come out the same size: a full-bleed square like Windows
  // Terminal's no longer looks bigger than a round one with a margin.
  const ICON_FILL = 0.84;
  const iconFill = new Map();
  const measureFill = (url) => new Promise((done) => {
    const im = new Image();
    im.onload = () => {
      const n = 48, c = document.createElement('canvas'); c.width = c.height = n;
      const g = c.getContext('2d', { willReadFrequently: true }); g.drawImage(im, 0, 0, n, n);
      const px = g.getImageData(0, 0, n, n).data;
      let x0 = n, y0 = n, x1 = -1, y1 = -1;
      for (let y = 0; y < n; y++) for (let x = 0; x < n; x++) if (px[(y * n + x) * 4 + 3] > 24) { x0 = Math.min(x0, x); x1 = Math.max(x1, x); y0 = Math.min(y0, y); y1 = Math.max(y1, y); }
      done(x1 < 0 ? 1 : Math.max(x1 - x0 + 1, y1 - y0 + 1) / n);
    };
    im.onerror = () => done(1);
    im.src = url;
  });
  const fitIcon = (img, url) => {
    if (!iconFill.has(url)) iconFill.set(url, measureFill(url));
    iconFill.get(url).then((fill) => { img.style.transform = `scale(${Math.min(1.25, ICON_FILL / fill).toFixed(3)})`; });
  };
  const iconCache = new Map();
  const appIcon = (name, key) => {
    const el = h('span', { class: 'appicon', title: name }, name[0]);
    if (!key) return el;
    if (key === 'neptune' || key === 'blurt') { el.replaceChildren(h('img', { src: 'icon.png', alt: name })); el.style.background = 'transparent'; return el; }
    if (key === 'claudecode') {
      // The Claude mark itself, floating, no tile.
      el.replaceChildren(h('span', { html: '<svg viewBox="0 0 24 24" fill="#d97757"><path d="m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z"/></svg>' }).firstChild);
      el.style.background = 'transparent'; el.classList.add('bare');
      return el;
    }
    const use = (url) => {
      if (!url) return;
      const img = h('img', { src: url, alt: name });
      fitIcon(img, url);
      el.replaceChildren(img); el.style.background = 'transparent';
    };
    if (iconCache.has(key)) use(iconCache.get(key));
    else D.appIcon(key).then((url) => { iconCache.set(key, url || null); use(url); });
    return el;
  };
  const fmtTime = (t) => new Date(t).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  const fmtN = (n) => n.toLocaleString();
  const dayKey = (t) => new Date(t).toDateString();
  const dayLabel = (t) => {
    const d = new Date(t), now = new Date();
    const diff = Math.round((new Date(now.toDateString()) - new Date(d.toDateString())) / 864e5);
    if (diff === 0) return 'Today';
    if (diff === 1) return 'Yesterday';
    return d.toLocaleDateString([], { weekday: 'long', month: 'short', day: 'numeric' });
  };
  // The average typist. Dhakal, Feit, Kristensson and Oulasvirta (CHI 2018)
  // measured 168,000 people online and found a mean of 52 words a minute.
  const AVG_WPM = 52;

  // ---- views ----
  const views = { home, history, settings };
  let current = null;

  // Each render gets a ticket; a view that finishes loading after a newer
  // render started throws its work away instead of stacking two pages.
  let seq = 0, quiet = false;
  function show(name, silent) {
    const switching = current !== name;
    current = name;
    quiet = !!silent;
    // Settings draws keycaps that follow the real keys (onboarding sets its own)
    if (!document.body.classList.contains('onboarding')) D.keyRelay(name === 'settings');
    const my = ++seq;
    historyApi = null;
    for (const b of document.querySelectorAll('.item')) b.classList.toggle('on', b.dataset.view === name);
    railApi.moveInk();
    if (main._ro) { main._ro.disconnect(); main._ro = null; }
    if (main._ro2) { main._ro2.disconnect(); main._ro2 = null; }
    const page = h('div', { class: 'page' });
    const sweep = switching && !quiet && !stillMotion && main.querySelector('.page');
    views[name](my, page)
      .catch((e) => { page.replaceChildren(h('div', { class: 'card empty' }, h('b', {}, 'Something went wrong'), String(e))); })
      .then(() => { if (my !== seq) return; mount(page, name, sweep); });
  }
  const stale = (my) => my !== seq;
  // The new page grows out of the bead you clicked as a circle over the old
  // one; the old one leaves once it is covered.
  function mount(page, name, sweep) {
    const old = [...main.children];
    if (!sweep) { main.replaceChildren(page); return; }
    const { x, y } = railApi.origin(name);
    const m = main.getBoundingClientRect();
    const R = Math.hypot(Math.max(x, m.width - x), Math.max(y, m.height - y)) + 24;
    page.classList.add('swept');
    page.style.clipPath = `circle(0px at ${x}px ${y}px)`;
    old.forEach((c) => { c.style.pointerEvents = 'none'; }); // the old page is scenery now
    main.append(page);
    const t0 = performance.now();
    (function step(now) {
      const k = Math.min(1, (now - t0) / 820), e = 1 - Math.pow(1 - k, 2.2);
      page.style.clipPath = `circle(${(e * R).toFixed(0)}px at ${x}px ${y}px)`;
      if (k < 1) requestAnimationFrame(step);
      else { old.forEach((c) => c.remove()); page.style.clipPath = ''; }
    })(t0);
  }
  for (const b of document.querySelectorAll('.item')) b.addEventListener('click', () => show(b.dataset.view));

  // ---------- pebbles ----------
  // Home is made of the logo's shape: near-round, a little uneven, flat and
  // still. Each pebble is seeded (by its date, usually), so it is the same
  // shape every time you open the window.
  // `lo` is the smallest corner radius. Two corners on one edge that add up
  // to less than 100% leave that edge flat; lo = 50 can never do that.
  const pebble = (seed, lo = 40, hi = 60) => {
    let t = (seed * 2654435761) >>> 0;
    const r = () => { t = (t + 0x6d2b79f5) >>> 0; let x = Math.imul(t ^ (t >>> 15), 1 | t); x ^= x + Math.imul(x ^ (x >>> 7), 61 | x); return ((x ^ (x >>> 14)) >>> 0) / 4294967296; };
    const v = () => Math.round(lo + r() * (hi - lo));
    return `${v()}% ${v()}% ${v()}% ${v()}% / ${v()}% ${v()}% ${v()}% ${v()}%`;
  };
  const daySeed = (d) => Math.floor(new Date(d).setHours(12, 0, 0, 0) / 864e5);
  // the Home card's corner of night: a few stars and the planet with its ring
  // passing behind it and back across the front
  // three stars scattered unevenly round the planet, all inside the part of
  // the sky the card shows, soft rather than glowing
  const SKY = `<svg viewBox="0 0 280 200"><g fill="#fff"><circle class="star" cx="38" cy="66" r="1.2" opacity=".7"/><circle class="star" cx="211" cy="34" r="1" opacity=".55" style="animation-delay:1.4s"/><circle class="star" cx="236" cy="150" r="1.1" opacity=".6" style="animation-delay:2.6s"/></g><use href="#ringed" x="70" y="10" width="180" height="180" style="--rw:1.7"/></svg>`;

  // ---------- Home ----------
  async function home(my, page) {
    const [hist, st] = await Promise.all([D.history(), D.settings()]);
    if (stale(my)) return;
    const wpmTyping = AVG_WPM;
    const ok = hist.filter((e) => e.status === 'pasted');
    const totalWords = ok.reduce((s, e) => s + e.words, 0);
    const totalSecs = ok.reduce((s, e) => s + e.secs, 0);
    const speakWpm = totalSecs ? Math.round(totalWords / (totalSecs / 60)) : 0;
    const weekAgo = Date.now() - 7 * 864e5;
    const weekWords = ok.filter((e) => e.at > weekAgo).reduce((s, e) => s + e.words, 0);
    const savedMin = Math.max(0, totalWords / wpmTyping - totalSecs / 60);
    const savedH = savedMin / 60;

    // streak: consecutive days ending today (or yesterday) with a dictation
    const days = new Set(ok.map((e) => dayKey(e.at)));
    let streak = 0;
    for (let d = 0; d < 400; d++) {
      const key = dayKey(Date.now() - d * 864e5);
      if (days.has(key)) streak++;
      else if (d === 0) continue; // today may still be empty
      else break;
    }
    const firstAt = ok.length ? ok[ok.length - 1].at : Date.now();

    // the greeting follows the clock, like the sky behind it
    const hr = new Date().getHours();
    const hello = hr < 5 ? 'Good night' : hr < 12 ? 'Good morning' : hr < 18 ? 'Good afternoon' : 'Good evening';
    const headEl = h('div', { class: 'head', 'data-tauri-drag-region': '' }, h('h1', {}, hello), h('span', { class: 'sub' }, `${fmtN(ok.length)} dictations since ${new Date(firstAt).toLocaleDateString([], { month: 'short', day: 'numeric' })}`));

    const none = ok.length === 0;
    const keys = (st.ptt || ['Ctrl', 'Space']).join('+');
    const hero = h('div', { class: 'card stat hero' },
      h('div', { class: 'sky', 'aria-hidden': 'true', html: SKY }),
      h('div', { class: 'k' }, 'Words spoken into your computer'), countUp(h('div', { class: 'v' }, fmtN(totalWords)), totalWords),
      none
        ? h('div', { class: 'side-text' }, 'Hold ', h('b', {}, keys), ' in any app and say something. Your numbers start here.')
        : h('div', { class: 'side-text' }, 'That is ', h('b', {}, `${savedH >= 1 ? savedH.toFixed(1) + ' hours' : Math.round(savedMin) + ' minutes'}`), ' you did not spend typing.'),
    );
    const stats = h('div', { class: 'stats' },
      h('div', { class: 'card stat' }, h('div', { class: 'k' }, 'This week'), h('div', { class: 'v' }, countUp(h('span', {}, fmtN(weekWords)), weekWords), h('small', {}, 'words')), h('div', { class: 'd' }, weekTrend(ok))),
      h('div', { class: 'card stat' }, h('div', { class: 'k' }, 'Streak'), h('div', { class: 'v' }, countUp(h('span', {}, String(streak)), streak), h('small', {}, streak === 1 ? 'day' : 'days')), h('div', { class: 'd week' }, [6, 5, 4, 3, 2, 1, 0].map((n) => {
        const d = new Date(Date.now() - n * 864e5), on = days.has(dayKey(d));
        return h('i', { class: (on ? 'on' : '') + (n === 0 ? ' today' : ''), style: `border-radius:${pebble(daySeed(d), 50)}`, 'data-tip': `${d.toLocaleDateString([], { weekday: 'long' })} · ${on ? 'dictated' : 'nothing'}` });
      }))),
      none
        ? h('div', { class: 'card stat' }, h('div', { class: 'k' }, 'You speak at'), h('div', { class: 'v' }, '—'), h('div', { class: 'd' }, 'Say a few sentences first.'))
        : h('div', { class: 'card stat' }, h('div', { class: 'k' }, 'You speak at'), h('div', { class: 'v' }, countUp(h('span', {}, String(speakWpm)), speakWpm), h('small', {}, 'wpm')), h('div', { class: 'd' }, h('b', {}, `${(speakWpm / wpmTyping).toFixed(1)}×`), ` faster than you type`)),
    );

    // The last five weeks as a calendar: Monday to Sunday across, weeks down.
    const byDay = {};
    for (const e of ok) byDay[dayKey(e.at)] = (byDay[dayKey(e.at)] || 0) + e.words;
    const today = new Date(); today.setHours(0, 0, 0, 0);
    const dow = (today.getDay() + 6) % 7; // Mon=0
    const start = new Date(today); start.setDate(today.getDate() - dow - 7 * 4);
    const cellsEl = h('div', { class: 'cells' });
    let activeDays = 0, max = 1;
    const month = [];
    for (let i = 0; i < 35; i++) { const d = new Date(start); d.setDate(start.getDate() + i); const w = d <= today ? (byDay[d.toDateString()] || 0) : 0; month.push([d, w]); if (w) { activeDays++; max = Math.max(max, w); } }
    const tune = !quiet && !stillMotion && ok.length > 0;
    for (const [d, w] of month) {
      const future = d > today;
      const lvl = future || !w ? 0 : w < max * 0.25 ? 1 : w < max * 0.5 ? 2 : w < max * 0.75 ? 3 : 4;
      const final = (lvl ? 'l' + lvl : '') + (future ? ' future' : '') + (d.getTime() === today.getTime() ? ' today' : '');
      const cell = h('i', { class: tune ? (future ? 'future' : '') : final, style: `--d:${quiet ? 0 : Math.round(Math.random() * 520)}ms;border-radius:${pebble(daySeed(d), 38, 62)}`, 'data-tip': `${d.toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' })} · ${future ? 'not yet' : w ? fmtN(w) + ' words' : 'nothing'}` });
      cell.dataset.final = final;
      cellsEl.append(cell);
    }
    // The month tunes in: cells flicker at random like a meter finding its
    // level, and one by one they lock onto their real value, so the noise
    // resolves into the picture instead of being replaced by it.
    if (tune) {
      const cells = [...cellsEl.children].filter((c) => !c.classList.contains('future'));
      const t0 = performance.now();
      const lockAt = new Map(cells.map((c) => [c, 400 + Math.random() * 1100]));
      const tick = setInterval(() => {
        const t = performance.now() - t0;
        let open = 0;
        for (const c of cells) {
          if (lockAt.get(c) === null) continue;
          if (t >= lockAt.get(c) || !cellsEl.isConnected) { c.className = c.dataset.final; lockAt.set(c, null); continue; }
          open++;
          const r = Math.random();
          c.className = r < 0.22 ? 'pop l' + (2 + Math.floor(Math.random() * 3)) : r < 0.4 ? 'l1' : '';
        }
        if (!open) clearInterval(tick);
      }, 130);
    }
    const cal = h('div', { class: 'cal' }, h('div', { class: 'wk' }, ['M', 'T', 'W', 'T', 'F', 'S', 'S'].map((x) => h('span', {}, x))), cellsEl);
    // Cells take whatever room the card has, staying square.
    const fit = () => {
      const r = cal.getBoundingClientRect();
      if (!r.width) return;
      const gap = 11;
      const s = Math.floor(Math.min((r.width - gap * 6) / 7, (r.height - 22 - gap * 4) / 5, 56));
      cal.style.setProperty('--cs', Math.max(14, s) + 'px');
    };
    main._ro = new ResizeObserver(fit); main._ro.observe(cal);
    // the bottom of the card: the busiest day, and what the shades mean
    const [bestDay, bestWords] = month.reduce((m, x) => (x[1] > m[1] ? x : m), [null, 0]);
    const legend = h('span', { class: 'cal-legend' }, 'Less', ...['', 'l1', 'l2', 'l3', 'l4'].map((l, k) => h('i', { class: l, style: `border-radius:${pebble(k * 7919 + 13, 38, 62)}` })), 'More');
    const calFoot = h('div', { class: 'cal-foot' },
      h('span', {}, bestWords ? ['Busiest: ', h('b', {}, bestDay.toLocaleDateString([], { weekday: 'long', month: 'short', day: 'numeric' })), ` · ${fmtN(bestWords)} words`] : 'Your busiest day shows up here.'),
      legend);
    const activity = h('div', { class: 'card panel activity' }, h('h3', {}, 'Last month'), cal, calFoot);

    // top apps. Dictating into neptune's own window is always a slip, not
    // somewhere you talk, so it never counts here.
    const byApp = {};
    for (const e of ok) { if (e.appKey === 'neptune' || e.appKey === 'blurt') continue; byApp[e.app] = byApp[e.app] || { key: e.appKey, words: 0 }; byApp[e.app].words += e.words; }
    const top = Object.entries(byApp).sort((a, b) => b[1].words - a[1].words).slice(0, 6);
    const maxApp = top.length ? top[0][1].words : 1;
    const appsList = h('div', { class: 'apps' });
    const apps = h('div', { class: 'card panel talk' }, h('h3', {}, 'Where you talk'),
      top.length ? null : h('div', { style: 'color:var(--faint);font-size:12.5px' }, 'Every app you dictate into shows up here.'),
      appsList);
    appsList.append(...top.map(([name, a]) => h('div', { class: 'app-row' }, appIcon(name, a.key),
        h('div', { class: 'name' },
          h('div', { class: 'line' }, h('span', {}, name), h('span', { class: 'n' }, fmtN(a.words))),
          h('div', { class: 'bar' }, grow(h('i', { style: 'width:0%' }, penLift()), (a.words / maxApp) * 100))))));

    // The last few takes, so the common case (grab what I just said) never
    // needs a second click.
    const recent = hist.filter((e) => e.text).slice(0, 8);
    const rowsEl = h('div', { class: 'rows' }, recent.map((e) => {
      const row = h('div', { class: 'row', onclick: () => { openId = e.id; show('history'); } },
        appIcon(e.app, e.appKey), h('span', { class: 'x' }, e.text), h('span', { class: 't' }, fmtTime(e.at)),
        h('span', { class: 'rowacts' }, h('button', { class: 'ib', 'data-tip': 'Copy', onclick: (ev) => { ev.stopPropagation(); copyText(e.text, ev.currentTarget); } }, ICON.copy())));
      return row;
    }));
    const latest = h('div', { class: 'latest' }, daySep(), h('h3', {}, 'Latest', h('a', { onclick: () => show('history') }, 'All history')),
      recent.length ? rowsEl : h('div', { style: 'color:var(--faint);font-size:12.5px;padding-bottom:6px' }, 'Your dictations will show up here.'));
    apps.append(latest);
    // Home never scrolls. The two lower cards take whatever height is left,
    // and each list shows only the rows that fit whole: the app list keeps
    // at least two, Latest keeps at least one, the calendar scales its cells.
    const fitList = (listEl, keep, gap) => {
      const rows = [...listEl.children];
      rows.forEach((r) => r.classList.remove('hid'));
      const H = listEl.clientHeight;
      let y = 0;
      rows.forEach((r, i) => { const rh = r.offsetHeight + gap; if (i >= keep && y + rh > H + 1) r.classList.add('hid'); else y += rh; });
    };
    // The apps are the point of this card; History has its own page. Every
    // app row shows if Latest still gets its title and one take; otherwise
    // app rows go from the bottom, down to three, and then Latest goes.
    // Latest fills whatever is left with whole rows only.
    const settle = () => {
      latest.classList.remove('hid');
      const rows = [...appsList.children];
      rows.forEach((r) => r.classList.remove('hid'));
      if (!recent.length) return;
      [...rowsEl.children].forEach((r) => r.classList.remove('hid'));
      const first = rowsEl.firstElementChild;
      const need = latest.querySelector('h3').offsetHeight + 12 + (first ? first.offsetHeight : 30) + 11;
      let shown = rows.length;
      while (latest.clientHeight < need && shown > 3) rows[--shown].classList.add('hid');
      if (latest.clientHeight < need) { latest.classList.add('hid'); return; }
      fitList(rowsEl, 1, 1);
    };
    main._ro2 = new ResizeObserver(settle);
    main._ro2.observe(apps);
    page.classList.add('home');
    page.append(headEl, hero, stats, h('div', { class: 'grid2' }, activity, apps));
  }

  // A bar that grows to its width on the next frame.
  function grow(el, pct) {
    if (quiet) { el.style.transition = 'none'; el.style.width = pct + '%'; return el; }
    requestAnimationFrame(() => requestAnimationFrame(() => { el.style.width = pct + '%'; }));
    return el;
  }

  // Runs a number up to its value over ~700 ms. Static when motion is reduced.
  function countUp(el, target) {
    if (!target || quiet || matchMedia('(prefers-reduced-motion: reduce)').matches) return el;
    const t0 = performance.now();
    const tick = (now) => {
      const p = Math.min(1, (now - t0) / 700);
      const e = 1 - Math.pow(1 - p, 3);
      el.textContent = fmtN(Math.round(target * e));
      if (p < 1) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    return el;
  }

  function weekTrend(ok) {
    const now = Date.now();
    const thisW = ok.filter((e) => e.at > now - 7 * 864e5).reduce((s, e) => s + e.words, 0);
    const lastW = ok.filter((e) => e.at <= now - 7 * 864e5 && e.at > now - 14 * 864e5).reduce((s, e) => s + e.words, 0);
    if (!lastW) return 'First week.';
    const pct = Math.round(((thisW - lastW) / lastW) * 100);
    return h('span', {}, h('b', { style: pct < 0 ? 'color:var(--muted)' : '' }, `${pct >= 0 ? '+' : ''}${pct}%`), ' vs last week');
  }

  // ---------- History ----------
  let openId = null;
  async function history(my, page) {
    const all = [...await D.history()];
    if (stale(my)) return;
    page.classList.add('history');
    let filter = q.get('q') || '';
    let filterTimer = 0;
    const input = h('input', { placeholder: 'Search dictations', value: filter, oninput: (e) => { filter = e.target.value; clearTimeout(filterTimer); filterTimer = setTimeout(() => applyFilter(true), 110); } });
    const sub = h('span', { class: 'sub' }, `${fmtN(all.length)} dictations`);
    const head = h('div', { class: 'head', 'data-tauri-drag-region': '' }, h('h1', {}, 'History'), sub, h('span', { class: 'grow' }),
      h('label', { class: 'search' }, ICON.search(), input, h('kbd', {}, 'Ctrl+Shift+H')));
    const list = h('div', {});
    const empty = h('div', { class: 'card empty gone' }, h('b', {}, 'Nothing matches'), 'Try a different word.');
    page.append(head, list, empty);

    // Every row is built once. Searching never redraws the list: rows that
    // stop matching fold shut, rows that start matching open up, day labels
    // follow their rows, and each hit is underlined as a reader reaches it.
    //
    // The rule: a term matches a word that starts with it, and the whole
    // word is the hit. "he" finds "hey" and "hello", never "the".
    const byId = new Map(all.map((e) => [e.id, e]));
    const esc = (w) => w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
    const terms = () => filter.trim().toLowerCase().split(/\s+/).filter(Boolean);
    const hitRe = (ts) => new RegExp('(?<![\\p{L}\\p{N}])(?:' + ts.map(esc).join('|') + ')[\\p{L}\\p{N}]*', 'giu');
    const matches = (e, ts) => ts.every((t) => { const re = hitRe([t]); return re.test(e.text) || re.test(e.app); });
    function renderList() {
      list.replaceChildren();
      if (!all.length) {
        list.append(h('div', { class: 'card empty' }, h('b', {}, 'Nothing yet'), 'Hold Ctrl+Space and say something. It will show up here.'));
        return;
      }
      const f = filter.trim().toLowerCase();
      let lastDay = null, rows = null;
      for (const e of all) {
        const k = dayKey(e.at);
        if (k !== lastDay) {
          if (lastDay) list.append(daySep());
          lastDay = k;
          list.append(h('div', { class: 'day' }, dayLabel(e.at)));
          rows = h('div', { class: 'rows' });
          list.append(rows);
        }
        rows.append(e.id === openId ? openRow(e, f) : closedRow(e, f));
      }
      applyFilter(false);
    }
    // One pass of reads, then one pass of writes, so the browser lays the
    // page out once instead of once per row. Only rows near the viewport
    // animate; the rest just switch, nobody can see them anyway.
    function applyFilter(animate) {
      const ts = terms();
      const f = ts.join(' ');
      const near = (r) => r.bottom > -500 && r.top < window.innerHeight + 500;
      const outs = [], ins = [], lit_ = [];
      let shown = 0;
      for (const rowsEl of list.querySelectorAll('.rows')) {
        let any = false;
        for (const row of rowsEl.children) {
          const e = byId.get(Number(row.dataset.id));
          if (!e) continue;
          const on = matches(e, ts);
          const isOn = !row.classList.contains('gone');
          if (on) { any = true; shown++; if (!row.classList.contains('open') && e.text) lit_.push([row, e]); }
          if (on && !isOn) ins.push(row);
          if (!on && isOn) outs.push(row);
        }
        const day = rowsEl.previousElementSibling;
        const dayOn = !day.classList.contains('gone');
        if (any && !dayOn) ins.push(day);
        if (!any && dayOn) outs.push(day);
      }
      const emptyOn = !empty.classList.contains('gone');
      if (shown === 0 && all.length && !emptyOn) ins.push(empty);
      if ((shown > 0 || !all.length) && emptyOn) outs.push(empty);
      sub.textContent = ts.length ? `${fmtN(shown)} of ${fmtN(all.length)}` : `${fmtN(all.length)} dictations`;

      // the hits: rebuild the text of every matching row (writes), then
      // draw strokes only for rows near the viewport; the rest are drawn
      // the moment they scroll into view.
      for (const [row] of lit_) { row.dataset.q = f; row.classList.add('unstroked'); }

      // folding: reads first
      const quick = !animate || stillMotion;
      const outRects = outs.map((el) => [el, quick ? null : el.getBoundingClientRect()]);
      for (const [el, r] of outRects) {
        if (!r || !near(r)) { el.classList.add('gone'); el.style.cssText = ''; continue; }
        el.style.height = r.height + 'px'; el.style.overflow = 'hidden';
      }
      for (const el of ins) { el.classList.remove('gone'); el.style.cssText = ''; }
      strokeVisible(); // now that every matching row is in the flow
      const inRects = ins.map((el) => [el, quick ? null : el.getBoundingClientRect()]);
      const animIn = [];
      for (const [el, r] of inRects) {
        if (!r || !near(r)) continue;
        animIn.push([el, r.height]);
        el.style.height = '0px'; el.style.overflow = 'hidden'; el.style.opacity = '0'; el.style.marginTop = '0px'; el.style.marginBottom = '-6px'; el.style.paddingTop = '0px'; el.style.paddingBottom = '0px';
      }
      const animOut = outRects.filter(([, r]) => r && near(r)).map(([el]) => el);
      // once the folds have settled, rows that moved into view get their strokes
      clearTimeout(strokeTimer); strokeTimer = setTimeout(strokeVisible, 360);
      if (!animIn.length && !animOut.length) return;
      requestAnimationFrame(() => {
        const ease = 'cubic-bezier(0.2, 0.8, 0.2, 1)';
        for (const el of animOut) {
          el.style.transition = `height 260ms ${ease}, opacity 160ms ease, margin 260ms ${ease}, padding 260ms ${ease}`;
          el.style.height = '0px'; el.style.opacity = '0'; el.style.marginTop = '0px'; el.style.marginBottom = '-6px'; el.style.paddingTop = '0px'; el.style.paddingBottom = '0px';
        }
        for (const [el, hgt] of animIn) {
          el.style.transition = `height 300ms ${ease}, opacity 220ms ease 60ms, margin 300ms ${ease}, padding 300ms ${ease}`;
          el.style.height = hgt + 'px'; el.style.opacity = '1'; el.style.marginTop = ''; el.style.marginBottom = ''; el.style.paddingTop = ''; el.style.paddingBottom = '';
        }
        setTimeout(() => {
          for (const el of animOut) { if (el.style.height === '0px') { el.classList.add('gone'); el.style.cssText = ''; } }
          for (const [el] of animIn) { if (!el.classList.contains('gone')) el.style.cssText = ''; }
        }, 320);
      });
    }
    // Strokes are drawn for rows near the viewport only, in one read pass
    // and one write pass. Rows further down get theirs on scroll.
    function strokeVisible() {
      const rows = [...list.querySelectorAll('.row.unstroked:not(.gone)')];
      if (!rows.length) return;
      const nearRows = rows.filter((r) => { const b = r.getBoundingClientRect(); return b.bottom > -300 && b.top < window.innerHeight + 300; });
      // the text with its hits marked (writes), then every mark measured (one layout)
      for (const r of nearRows) { const e = byId.get(Number(r.dataset.id)); r.classList.remove('unstroked'); if (e) r.querySelector('.x').replaceChildren(lit(e.text, r.dataset.q || '')); }
      const marks = nearRows.flatMap((r) => [...r.querySelectorAll('mark')]).map((m) => [m, Math.max(8, m.getBoundingClientRect().width)]);
      for (const [m, W] of marks) {
        m.insertAdjacentHTML('beforeend', `<svg viewBox="0 0 ${W.toFixed(1)} 6" preserveAspectRatio="none"><path d="${stroke(W)}"/></svg>`);
        m.style.setProperty('--d', Math.min(Number(m.dataset.wi || 0), 40) * 55 + 'ms');
      }
      const lens = marks.map(([m]) => { const p = m.querySelector('path'); return [p, p.getTotalLength()]; });
      for (const [p, len] of lens) p.style.setProperty('--len', len.toFixed(1));
      requestAnimationFrame(() => { for (const [m] of marks) m.classList.add('on'); });
    }
    let strokeTimer = 0;
    page.addEventListener('scroll', () => { clearTimeout(strokeTimer); strokeTimer = setTimeout(strokeVisible, 60); }, { passive: true });

    // The pen stroke under a hit: a few points with a little wobble joined
    // by a smooth spline, so it reads as a steady hand, not a ruler.
    function stroke(W) {
      const step = 18, amp = 0.9, drift = Math.random() * 0.6 - 0.3;
      const pts = [];
      for (let px = 0; px <= W; px += step) pts.push([px, 3.2 + drift + (Math.random() * 2 - 1) * amp]);
      if (pts[pts.length - 1][0] < W) pts.push([W, 3.2 + drift + (Math.random() * 2 - 1) * amp * 0.6]);
      let d = `M${pts[0][0]} ${pts[0][1].toFixed(2)}`;
      for (let i = 0; i < pts.length - 1; i++) {
        const p0 = pts[Math.max(0, i - 1)], p1 = pts[i], p2 = pts[i + 1], p3 = pts[Math.min(pts.length - 1, i + 2)];
        const c1 = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6], c2 = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
        d += ` C${c1[0].toFixed(1)} ${c1[1].toFixed(2)} ${c2[0].toFixed(1)} ${c2[1].toFixed(2)} ${p2[0].toFixed(1)} ${p2[1].toFixed(2)}`;
      }
      return d;
    }
    // A take that lands while this page is open slides in at the top and
    // everything below it moves down to make room, instead of the page
    // redrawing. Anything more complicated than "new rows on top" falls
    // back to a quiet redraw.
    historyApi = {
      async refresh() {
        const fresh = await D.history();
        if (stale(my)) return true;
        const known = new Set(all.map((e) => e.id));
        const added = fresh.filter((e) => !known.has(e.id));
        const same = fresh.length === all.length + added.length && all.every((e) => { const g = fresh.find((x) => x.id === e.id); return g && g.text === e.text && g.status === e.status && g.audio === e.audio; });
        if (!added.length || !same || filter.trim() || openId) return false;
        added.sort((a, b) => a.at - b.at).forEach((e) => {
          all.unshift(e);
          let day = list.firstElementChild, rows = day && day.nextElementSibling;
          if (!day || day.textContent !== dayLabel(e.at)) {
            const older = list.firstElementChild;
            day = h('div', { class: 'day' }, dayLabel(e.at)); rows = h('div', { class: 'rows' });
            list.prepend(day, rows);
            if (older) list.insertBefore(daySep(), older);
          }
          // The new row opens up from nothing: the rows below ride down on
          // its growing height, then its contents fade in.
          const row = closedRow(e, ''); rows.prepend(row);
          if (!stillMotion) {
            const target = row.offsetHeight;
            row.classList.add('arrive'); row.style.height = '0px'; row.style.overflow = 'hidden'; row.style.opacity = '0';
            row.style.marginBottom = '-6px';
            requestAnimationFrame(() => {
              row.style.transition = 'height 560ms cubic-bezier(0.2, 0.8, 0.2, 1), margin-bottom 560ms cubic-bezier(0.2, 0.8, 0.2, 1), opacity 420ms ease 220ms';
              row.style.height = target + 'px'; row.style.marginBottom = ''; row.style.opacity = '1';
              setTimeout(() => { row.style.height = ''; row.style.overflow = ''; row.style.transition = ''; row.style.opacity = ''; row.classList.remove('arrive'); }, 620);
            });
          }
        });
        head.querySelector('.sub').textContent = `${fmtN(all.length)} dictations`;
        return true;
      },
    };
    // The searched words, lit up inside the text.
    function lit(text, f) {
      if (!f || !text) return text;
      const ts = f.split(/\s+/).filter(Boolean);
      if (!ts.length) return text;
      const re = hitRe(ts);
      const frag = document.createDocumentFragment();
      let last = 0;
      for (const m of text.matchAll(re)) {
        if (m.index > last) frag.append(text.slice(last, m.index));
        const mark = h('mark', {}, m[0]);
        mark.dataset.wi = (text.slice(0, m.index).match(/[\p{L}\p{N}]+/gu) || []).length;
        frag.append(mark);
        last = m.index + m[0].length;
      }
      if (last < text.length) frag.append(text.slice(last));
      return frag;
    }

    const secs = (n) => (n >= 60 ? `${Math.floor(n / 60)}m ${Math.round(n % 60)}s` : `${Math.round(n)}s`);
    function metaLine(e) {
      const parts = [fmtTime(e.at), e.app, secs(e.secs)];
      if (e.words) parts.push(`${fmtN(e.words)} word${e.words === 1 ? '' : 's'}`);
      const m = h('div', { class: 'm' });
      parts.forEach((p, i) => { if (i) m.append(h('i', {}, '·')); m.append(p); });
      if (e.status === 'paste_failed') { m.append(h('i', {}, '·'), h('span', { class: 'warn' }, 'Not pasted')); }
      return m;
    }
    function closedRow(e, f) {
      const acts = h('span', { class: 'rowacts' },
        e.text ? h('button', { class: 'ib', 'data-tip': 'Copy', onclick: (ev) => { ev.stopPropagation(); copyText(e.text, ev.currentTarget); } }, ICON.copy()) : null,
        e.audio ? h('button', { class: 'ib', 'data-tip': 'Listen again', onclick: (ev) => { ev.stopPropagation(); retry(e, ev.currentTarget); } }, ICON.retry()) : null,
        h('button', { class: 'ib', 'data-tip': 'Delete', onclick: (ev) => { ev.stopPropagation(); remove(e); } }, ICON.trash()));
      const row = h('div', { class: 'row', tabindex: '0', 'data-id': String(e.id), onclick: () => toggle(e.id) },
        appIcon(e.app, e.appKey),
        h('div', { class: 'body' }, h('div', { class: 'x' }, e.text ? lit(e.text, f) : h('i', { style: 'color:var(--faint)' }, 'No speech detected')), metaLine(e)),
        acts);
      row.addEventListener('keydown', (ev) => {
        if (ev.key === 'Enter') toggle(e.id);
        if (ev.key === 'Delete') remove(e);
      });
      return row;
    }

    // Opening a card grows it out of its row; closing shrinks it back. If
    // another card was open it closes at the same time. Nothing else on the
    // page is touched, so the rows around them simply slide.
    function toggle(id) {
      const f = filter.trim().toLowerCase();
      const prev = openId;
      openId = id === openId ? null : id;
      for (const i of new Set([prev, openId].filter(Boolean))) {
        const el = list.querySelector(`.row[data-id="${i}"]`);
        const e = all.find((x) => x.id === i);
        if (!el || !e) continue;
        morph(el, i === openId ? openRow(e, f) : closedRow(e, f));
      }
    }
    function morph(oldEl, newEl) {
      const h0 = oldEl.offsetHeight;
      // the old face rides along as a ghost and fades while the new one arrives
      const ghost = document.createElement('div');
      ghost.className = oldEl.className + ' ghost';
      ghost.append(...oldEl.childNodes);
      oldEl.replaceWith(newEl);
      if (stillMotion) return;
      const h1 = newEl.offsetHeight;
      newEl.style.height = h0 + 'px';
      newEl.style.overflow = 'hidden';
      newEl.classList.add('morph');
      newEl.append(ghost);
      requestAnimationFrame(() => {
        newEl.style.transition = 'height 380ms cubic-bezier(0.2, 0.8, 0.2, 1)';
        newEl.style.height = h1 + 'px';
        setTimeout(() => {
          ghost.remove();
          newEl.style.height = ''; newEl.style.overflow = ''; newEl.style.transition = '';
          newEl.classList.remove('morph');
          if (newEl.classList.contains('open')) newEl.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
        }, 400);
      });
    }

    async function retry(e, btn) {
      btn.disabled = true; btn.classList.add('busy');
      try { await D.retry(e.id); show('history', true); }
      catch (err) { btn.disabled = false; btn.classList.remove('busy'); toast('Retry failed: ' + err); }
    }
    async function remove(e) {
      try { await D.deleteEntry(e.id); } catch (err) { toast('Could not delete: ' + err); return; }
      if (openId === e.id) openId = null;
      const at = all.indexOf(e); if (at >= 0) all.splice(at, 1);
      quietUntil = Date.now() + 800;
      renderList();
    }

    function openRow(e, f) {
      const ta = h('textarea', {}, e.text);
      // Saved as you type, a beat after the last keystroke. The backend's
      // change event is ignored meanwhile so the page does not redraw under
      // the cursor.
      let saveTimer = 0;
      ta.addEventListener('input', () => {
        quietUntil = Date.now() + 2000;
        clearTimeout(saveTimer);
        saveTimer = setTimeout(() => { e.text = ta.value; quietUntil = Date.now() + 1500; D.updateEntry(e.id, ta.value).catch((err) => toast('Could not save: ' + err)); }, 600);
      });
      const when = new Date(e.at).toLocaleString([], { weekday: 'short', month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' });
      // Click the card to close it; the text and the buttons keep their clicks.
      return h('div', { class: 'row open', 'data-id': String(e.id), onclick: (ev) => { if (ev.target.closest('textarea, button')) return; toggle(e.id); } },
        h('div', { class: 'meta' }, appIcon(e.app, e.appKey), h('b', {}, e.app), h('span', {}, '·'), h('span', {}, when), h('span', {}, '·'), h('span', {}, secs(e.secs)),
          e.words ? [h('span', {}, '·'), h('span', {}, `${fmtN(e.words)} words`)] : null,
          e.status === 'paste_failed' ? h('span', { class: 'warn', style: 'color:var(--red)' }, '· Not pasted') : null),
        ta,
        h('div', { class: 'acts' },
          h('button', { class: 'btn sm primary', onclick: (ev) => copyText(ta.value, ev.currentTarget) }, ICON.copy(), 'Copy'),
          e.audio ? h('button', { class: 'btn sm', onclick: (ev) => { const b = ev.currentTarget; b.textContent = 'Listening again…'; retry(e, b); } }, ICON.retry(), 'Retry') : null,
          h('span', { class: 'sp' }),
          h('button', { class: 'btn sm quiet danger', onclick: () => remove(e) }, ICON.trash(), 'Delete')));
    }
    renderList();
  }

  // Our own dropdown: a button and a small menu that rises in below it.
  // options: [{ value, label }]; onpick(value)
  function dropdown(options, value, onpick) {
    const label = h('span', { class: 'dd-label' });
    const btn = h('button', { class: 'dd-btn', type: 'button' }, label, svg('<path d="M4 6.5l4 4 4-4"/>'));
    const menu = h('div', { class: 'dd-menu' });
    const wrap = h('div', { class: 'dd' }, btn, menu);
    const setValue = (v) => {
      value = v;
      const o = options.find((x) => x.value === v) || options[0];
      label.textContent = o ? o.label : '';
      for (const it of menu.children) it.classList.toggle('on', it.dataset.value === v);
    };
    for (const o of options) {
      menu.append(h('button', { class: 'dd-item', type: 'button', 'data-value': o.value, onclick: () => { close(); if (o.value !== value) { setValue(o.value); onpick(o.value); } } }, h('span', {}, o.label), ICON.check()));
    }
    const onDoc = (e) => { if (!wrap.contains(e.target)) close(); };
    const onKey = (e) => { if (e.key === 'Escape') close(); };
    function open() { wrap.classList.add('open'); document.addEventListener('mousedown', onDoc); document.addEventListener('keydown', onKey); }
    function close() { wrap.classList.remove('open'); document.removeEventListener('mousedown', onDoc); document.removeEventListener('keydown', onKey); }
    btn.addEventListener('click', () => (wrap.classList.contains('open') ? close() : open()));
    setValue(value);
    wrap.setValue = setValue;
    return wrap;
  }

  // ---------- Settings ----------
  async function settings(my, page) {
    const s = await D.settings();
    if (stale(my)) return;
    const save = () => D.saveSettings(s).catch((e) => toast('Could not save: ' + e));
    const keys = (arr) => h('span', { class: 'keys' }, arr.map((k) => h('kbd', {}, k)));
    const toggle = (key) => h('button', { class: 'toggle' + (s[key] ? ' on' : ''), role: 'switch', onclick: (e) => { s[key] = !s[key]; e.currentTarget.classList.toggle('on', s[key]); save(); } });
    // A segmented control whose highlight slides to the option you pick.
    const seg = (key, opts) => {
      const ind = h('i', { class: 'ind' });
      const el = h('span', { class: 'seg' }, ind);
      const place = () => { const on = el.querySelector('button.on'); if (!on) return; ind.style.width = on.offsetWidth + 'px'; ind.style.transform = `translateX(${on.offsetLeft}px)`; };
      for (const [v, label] of opts) el.append(h('button', { class: s[key] === v ? 'on' : '', type: 'button', onclick: (ev) => { s[key] = v; for (const b of el.querySelectorAll('button')) b.classList.toggle('on', b === ev.currentTarget); place(); save(); } }, label));
      requestAnimationFrame(() => { ind.style.transition = 'none'; place(); requestAnimationFrame(() => { ind.style.transition = ''; }); });
      return el;
    };
    const row = (b, sub, ctl) => h('div', { class: 'srow' }, h('div', { class: 'l' }, h('b', {}, b), sub ? h('span', {}, sub) : null), ctl);
    const section = (title, ...rows) => h('div', { class: 'section' }, h('h2', {}, title), h('div', { class: 'card' }, rows));

    const shortcut = (key, label, sub) => {
      const k = keys(s[key]);
      k.dataset.tip = 'Click, then press the keys you want';
      k.style.cursor = 'pointer';
      k.addEventListener('click', () => recordKeys(k, (combo) => { s[key] = combo; k.replaceChildren(...combo.map((x) => h('kbd', {}, x))); save(); }));
      return row(label, sub, k);
    };

    const micOpts = [{ value: '', label: 'System default' }, ...(s.mics || []).map((m) => ({ value: m, label: m }))];
    const mic = dropdown(micOpts, s.mic || '', (v) => { s.mic = v; save(); if (D.isNative) D.meterStart(); });
    const meter = h('div', { class: 'meter' }, h('i'));
    fakeMeter(meter.firstChild);

    // A newer model, if the manifest names one. Quiet unless it does.
    const modelRow = h('div', { class: 'srow' });
    const renderModel = () => {
      const spec = s.modelSpec || {}, upd = s.modelUpdate;
      const mb = (n) => `${Math.round((n || 0) / 1048576)} MB`;
      const size = mb((spec.sizes || []).reduce((a, b) => a + b, 0));
      const control = s.modelState !== 'ready'
        ? (downloading
          ? h('button', { class: 'btn sm', onclick: async (ev) => { const b = ev.currentTarget; await pauseDownload(!dlPaused); b.textContent = dlPaused ? 'Resume' : 'Pause'; } }, dlPaused ? 'Resume' : 'Pause')
          : h('button', { class: 'btn sm primary', onclick: async (ev) => { const b = ev.currentTarget; b.disabled = true; b.textContent = 'Downloading…'; if (!await startDownload()) { b.disabled = false; b.textContent = 'Download'; } else renderModel(); } }, `Download ${size}`))
        : upd
          ? h('button', { class: 'btn sm primary', disabled: downloading ? '' : null, onclick: async (ev) => { const b = ev.currentTarget; b.disabled = true; b.textContent = 'Updating…'; if (!await startDownload(upd)) { b.disabled = false; b.textContent = `Update to ${upd.name}`; } } }, downloading ? 'Updating…' : `Update to ${upd.name} · ${mb(upd.sizes.reduce((a, b) => a + b, 0))}`)
          : null;
      modelRow.replaceChildren(...[
        h('div', { class: 'l' }, h('b', {}, 'Model'), h('span', {}, `${spec.name || 'Parakeet TDT 0.6B v3'} · ${size} · 25 languages · runs on your machine`)),
        control].filter(Boolean));
    };
    renderModel();
    // a newer model is looked for only with Check for updates, below
    const checkModel = () => (D.isNative ? D.modelCheck(true).then((upd) => { if (upd && !s.modelUpdate) { s.modelUpdate = upd; renderModel(); } }).catch(() => {}) : Promise.resolve());

    // Version line, with the update check tucked in.
    const about = h('div', { class: 'about' }, h('b', {}, 'Neptune ' + version), h('span', {}, s.packaged ? 'Free and open source. The Microsoft Store keeps it up to date.' : 'Free and open source. Offline unless you check for updates.'));
    const renderAbout = (state) => {
      [...about.querySelectorAll('.btn, .prog, .note')].forEach((n) => n.remove());
      if (state === 'checking') about.append(h('span', { class: 'note', style: 'margin-left:auto' }, 'Checking…'));
      else if (state === 'none') about.append(h('span', { class: 'note', style: 'margin-left:auto' }, 'Up to date'));
      else if (state === 'installing') { const bar = h('i'); about.append(h('span', { class: 'prog' }, bar)); updateProgress = (p) => { bar.style.width = (p.total ? (p.done / p.total) * 100 : 30) + '%'; if (p.error) { toast('Update failed: ' + p.error); renderAbout(appUpdate ? 'ready' : 'idle'); } }; }
      else if (appUpdate) about.append(h('button', { class: 'btn sm primary', onclick: async () => { renderAbout('installing'); try { await D.updateInstall(); } catch (e) { toast('Update failed: ' + e); renderAbout('ready'); } } }, `Update to ${appUpdate.version}`));
      else about.append(h('button', { class: 'btn sm', onclick: async () => { renderAbout('checking'); const [u] = await Promise.all([checkUpdate(), checkModel()]); renderAbout(u ? 'ready' : 'none'); } }, 'Check for updates'));
    };
    renderAbout(appUpdate ? 'ready' : 'idle');

    page.append(
      h('div', { class: 'head', 'data-tauri-drag-region': '' }, h('h1', {}, 'Settings')),
      section('Shortcuts',
        shortcut('ptt', 'Hold to talk', 'Hold, speak, release. Text lands at the cursor.'),
        shortcut('handsfree', 'Hands-free', 'Press once to start, again to finish. Esc cancels.'),
        shortcut('pasteLast', 'Paste last dictation'),
        shortcut('history', 'Open history')),
      section('Storage',
        row('Keep audio', 'Needed for Retry. Never leaves this machine.', retention('keepAudio')),
        row('Keep history', null, retention('keepHistory'))),
      section('Microphone',
        row('Input', 'Bluetooth hands-free profiles are skipped; they record silence.', mic),
        row('Level', 'Say something. The bar should move.', meter)),
      section('General',
        row('Remove filler words', '"um", "uh", "er" and friends.', toggle('fillers')),
        row('Show Copy button on empty paste', 'When the text lands somewhere that cannot take it, like the desktop or this window, the pill offers a Copy button instead.', toggle('textlessCopy')),
        row('Start at login', null, toggle('autostart')),
        row('Show the pill', 'The little indicator at the bottom of the screen.', toggle('pill')),
        row('Keep the model loaded', 'After a dictation, how long the speech model stays in memory. Shorter frees about 700 MB sooner; the next dictation then loads it again while you talk.',
          dropdown([{ value: '0', label: 'Unload right away' }, { value: '60', label: '1 minute' }, { value: '300', label: '5 minutes' }, { value: '900', label: '15 minutes' }, { value: '-1', label: 'Always' }],
            String(s.modelKeepSecs ?? 60), (v) => { s.modelKeepSecs = Number(v); save(); })),
        modelRow),
      about,
    );

    // "Keep for": a value you click, then a number and a unit. Or forever.
    function retention(key) {
      const UNITS = [['h', 'hours'], ['d', 'days'], ['mo', 'months'], ['y', 'years'], ['dec', 'decades'], ['c', 'centuries']];
      const parse = (v) => { const m = /^(\d+)(h|d|w|mo|y|dec|c)$/.exec(v || ''); return m ? { n: Number(m[1]), u: m[2] === 'w' ? 'd' : m[2], nRaw: m[2] === 'w' ? Number(m[1]) * 7 : Number(m[1]) } : null; };
      const label = (v) => { if (v === 'off') return 'Off'; const p = parse(v); if (!p) return 'Forever'; const u = UNITS.find((x) => x[0] === p.u); const n = p.nRaw; return `${n} ${n === 1 ? u[1].replace(/ies$/, 'y').replace(/s$/, '') : u[1]}`; };
      const cur = parse(s[key]) || { n: 30, u: 'd', nRaw: 30 };
      let forever = !parse(s[key]);
      let n = cur.nRaw, u = cur.u;
      const txt = h('span', {}, label(s[key]));
      const btn = h('button', { class: 'ret-btn', type: 'button' }, txt, svg('<path d="M4 6.5l4 4 4-4"/>'));
      const num = h('input', { class: 'num', type: 'number', min: '1', max: '999', value: n });
      const unit = dropdown(UNITS.map(([value, l]) => ({ value, label: l })), u, (v) => { u = v; forever = false; apply(); });
      // Forever is a switch like every other switch in Settings. Touching the
      // number or the unit turns it off; turning it on dims them.
      const fbtn = h('button', { class: 'toggle' + (forever ? ' on' : ''), role: 'switch', type: 'button' });
      const line = h('div', { class: 'line' + (forever ? ' dim' : '') }, num, unit);
      const pop = h('div', { class: 'ret-pop' }, line, h('div', { class: 'or' }, h('span', {}, 'or'), h('b', {}, 'Forever'), fbtn),
        h('div', { class: 'hint' }, key === 'keepAudio' ? 'Audio is about 2 MB a minute of speech.' : 'Older dictations are removed as they age out.'));
      const wrap = h('div', { class: 'ret' }, btn, pop);
      const apply = () => {
        n = Math.min(999, Math.max(1, Math.round(Number(num.value)) || 1));
        if (document.activeElement !== num) num.value = n;
        s[key] = forever ? 'forever' : `${n}${u}`;
        const next = label(s[key]);
        if (next !== txt.textContent) { txt.classList.remove('swap'); void txt.offsetWidth; txt.textContent = next; txt.classList.add('swap'); }
        fbtn.classList.toggle('on', forever);
        line.classList.toggle('dim', forever);
        save();
      };
      let numTimer = 0;
      num.addEventListener('input', () => { forever = false; clearTimeout(numTimer); numTimer = setTimeout(apply, 350); });
      num.addEventListener('change', () => { forever = false; clearTimeout(numTimer); apply(); });
      fbtn.addEventListener('click', () => { forever = !forever; apply(); });
      const onDoc = (e) => { if (!wrap.contains(e.target)) close(); };
      const onKey = (e) => { if (e.key === 'Escape') close(); };
      function open() { wrap.classList.add('open'); document.addEventListener('mousedown', onDoc); document.addEventListener('keydown', onKey); }
      function close() { wrap.classList.remove('open'); document.removeEventListener('mousedown', onDoc); document.removeEventListener('keydown', onKey); }
      btn.addEventListener('click', () => (wrap.classList.contains('open') ? close() : open()));
      return wrap;
    }
  }

  // Click a shortcut, press the keys, release. Esc cancels. A binding needs
  // at least one modifier and at most three keys, same rules as the backend.
  function recordKeys(el, done) {
    const before = [...el.children].map((k) => k.textContent);
    const restore = (msg) => {
      el.classList.remove('rec');
      el.replaceChildren(...before.map((k) => h('kbd', {}, k)));
      if (msg) { el.dataset.tip = msg; setTimeout(() => (el.dataset.tip = 'Click, then press the keys you want'), 2500); }
    };
    el.classList.add('rec');
    el.replaceChildren(h('kbd', {}, 'Press keys…'));
    const held = new Set();
    const mods = ['Ctrl', 'Alt', 'Shift', 'Win'];
    // Physical key names, so Shift+1 records as "1" and not "!", and the
    // result is always something the backend can bind.
    const CODES = { Space: 'Space', Enter: 'Enter', Tab: 'Tab', Backspace: 'Backspace', Delete: 'Delete', Home: 'Home', End: 'End', PageUp: 'PageUp', PageDown: 'PageDown', Insert: 'Insert', CapsLock: 'CapsLock',
      ArrowUp: 'ArrowUp', ArrowDown: 'ArrowDown', ArrowLeft: 'ArrowLeft', ArrowRight: 'ArrowRight', Backquote: '`', Minus: '-', Equal: '=', BracketLeft: '[', BracketRight: ']', Backslash: '\\', Semicolon: ';', Quote: "'", Comma: ',', Period: '.', Slash: '/',
      ControlLeft: 'Ctrl', ControlRight: 'Ctrl', ShiftLeft: 'Shift', ShiftRight: 'Shift', AltLeft: 'Alt', AltRight: 'Alt', MetaLeft: 'Win', MetaRight: 'Win' };
    const name = (e) => {
      const c = e.code || '';
      if (CODES[c]) return CODES[c];
      if (/^Key[A-Z]$/.test(c)) return c[3];
      if (/^Digit[0-9]$/.test(c)) return c[5];
      if (/^F([1-9]|1[0-2])$/.test(c)) return c;
      return null;
    };
    const stop = () => { window.removeEventListener('keydown', down, true); window.removeEventListener('keyup', up, true); window.removeEventListener('blur', onBlur); };
    const onBlur = () => { stop(); restore(); };
    // Keys you are holding look pressed; they lift when you let go.
    const pressed = new Set();
    const render = () => el.replaceChildren(...[...held].map((k) => h('kbd', { class: pressed.has(k) ? 'down' : '' }, k)));
    const down = (e) => {
      e.preventDefault();
      if (e.key === 'Escape') { stop(); restore(); return; }
      const n = name(e);
      if (n) { held.add(n); pressed.add(n); }
      render();
    };
    const up = (e) => {
      e.preventDefault();
      if (e.key === 'Escape') return;
      const n = name(e);
      if (n) pressed.delete(n);
      if (pressed.size) { render(); return; }
      const combo = [...held].sort((a, b) => mods.indexOf(b) - mods.indexOf(a));
      const hasMod = combo.some((k) => mods.includes(k));
      const hasKey = combo.some((k) => !mods.includes(k));
      stop();
      if (combo.length <= 3 && hasMod && hasKey) { el.classList.remove('rec'); done(combo); }
      else restore('Needs a modifier plus one key');
    };
    window.addEventListener('keydown', down, true);
    window.addEventListener('keyup', up, true);
    window.addEventListener('blur', onBlur);
  }

  // On Settings, pressing a real key presses every cap that shows it, and
  // letting go lifts it. It should feel like the keyboard under your hands.
  const KEYCAP = { Space: 'Space', Enter: 'Enter', Tab: 'Tab', Backspace: 'Backspace', Delete: 'Delete', Home: 'Home', End: 'End', PageUp: 'PageUp', PageDown: 'PageDown', Insert: 'Insert', CapsLock: 'CapsLock',
    ArrowUp: 'ArrowUp', ArrowDown: 'ArrowDown', ArrowLeft: 'ArrowLeft', ArrowRight: 'ArrowRight', ControlLeft: 'Ctrl', ControlRight: 'Ctrl', ShiftLeft: 'Shift', ShiftRight: 'Shift', AltLeft: 'Alt', AltRight: 'Alt', MetaLeft: 'Win', MetaRight: 'Win' };
  const capName = (e) => { const c = e.code || ''; if (KEYCAP[c]) return KEYCAP[c]; if (/^Key[A-Z]$/.test(c)) return c[3]; if (/^Digit[0-9]$/.test(c)) return c[5]; if (/^F([1-9]|1[0-2])$/.test(c)) return c; return null; };
  // Caps follow the keyboard in Settings and on onboarding's last step; the
  // Try step drives its own.
  const capsLive = () => !document.querySelector('.keys.rec') && !document.querySelector('.onb .trystep')
    && (document.body.classList.contains('onboarding') ? !!document.querySelector('.onb .shortcuts') : current === 'settings');
  const pressCaps = (e, down) => {
    if (!capsLive()) return;
    const n = capName(e); if (!n) return;
    for (const k of document.querySelectorAll('.keys kbd')) if (k.textContent === n) k.classList.toggle('down', down);
  };
  window.addEventListener('keydown', (e) => pressCaps(e, true));
  window.addEventListener('keyup', (e) => pressCaps(e, false));

  // Hold-to-talk while this window has the keyboard. The bindings come from
  // settings and follow every save.
  let binds = null;
  D.settings().then((st) => { binds = st; }).catch(() => {});
  { const save = D.saveSettings; D.saveSettings = (st) => { binds = st; return save(st); }; }
  const MODS = ['Ctrl', 'Shift', 'Alt', 'Win'];
  const isCombo = (e, keys) => {
    if (!keys || !keys.length) return false;
    const main = keys.find((k) => !MODS.includes(k));
    return !!main && capName(e) === main && e.ctrlKey === keys.includes('Ctrl') && e.shiftKey === keys.includes('Shift')
      && e.altKey === keys.includes('Alt') && e.metaKey === keys.includes('Win');
  };
  let winTake = false;
  // a trigger key still held after its take ended: eaten until it is up
  let eatKey = null;
  const trigger = () => binds && binds.ptt.find((k) => !MODS.includes(k));
  // the trigger key, held and not repeating yet: still "pressed together"
  let mainHeld = false, mainRepeated = false;
  window.addEventListener('keydown', (e) => {
    if (!D.isNative || !binds || document.querySelector('.keys.rec')) return;
    if (eatKey && capName(e) === eatKey) { e.preventDefault(); return; }
    const n = capName(e);
    if (n === trigger()) { if (!e.repeat) { mainHeld = true; mainRepeated = false; } else mainRepeated = true; }
    // Space first, then Ctrl, pressed together: the combo completes on the
    // modifier, while the trigger is still fresh
    if (!winTake && MODS.includes(n) && mainHeld && !mainRepeated && !e.repeat
      && e.ctrlKey === binds.ptt.includes('Ctrl') && e.shiftKey === binds.ptt.includes('Shift')
      && e.altKey === binds.ptt.includes('Alt') && e.metaKey === binds.ptt.includes('Win')) {
      winTake = true; D.windowKey('down'); return;
    }
    if (isCombo(e, binds.handsfree)) { e.preventDefault(); if (!e.repeat) D.windowKey('handsfree'); return; }
    if (isCombo(e, binds.ptt) || (winTake && capName(e) === binds.ptt.find((k) => !MODS.includes(k)))) {
      e.preventDefault(); // never let Space press the focused button
      if (!winTake && !e.repeat) { winTake = true; D.windowKey('down'); }
    }
  }, true);
  window.addEventListener('keyup', (e) => {
    if (!binds) return;
    const n = capName(e);
    if (n === trigger()) mainHeld = false;
    if (eatKey && n === eatKey) { e.preventDefault(); eatKey = null; return; }
    if (winTake && binds.ptt.includes(n)) {
      e.preventDefault(); winTake = false; D.windowKey('up');
      if (n !== trigger()) eatKey = trigger(); // a modifier went first; the trigger is still down
    }
  }, true);
  // A button clicked with the mouse lets go of focus, so a later Space (the
  // shortcut pressed in the wrong order, say) cannot press it again. Keyboard
  // users who Tab to a button keep Space as usual.
  document.addEventListener('click', (e) => { const b = e.target.closest && e.target.closest('button'); if (b && e.detail > 0) b.blur(); });
  window.addEventListener('blur', () => { mainHeld = false; if (winTake) { winTake = false; D.windowKey('up'); } });
  window.addEventListener('blur', () => { for (const k of document.querySelectorAll('.keys kbd.down')) k.classList.remove('down'); });
  // The trigger key itself never reaches this window: the hook swallows it
  // to start the take. So the take starting is what presses its cap: held
  // for as long as the hold lasts, tapped for a hands-free toggle.
  let lastPillState = 'idle';
  let tryHook = null; // set while the onboarding's Try step is on screen
  // Physical keys from the hook (Space included, which never reaches the
  // window) while a page with keycaps shows. Windows virtual-key codes.
  const VK = { 0x10: 'Shift', 0xA0: 'Shift', 0xA1: 'Shift', 0x11: 'Ctrl', 0xA2: 'Ctrl', 0xA3: 'Ctrl', 0x12: 'Alt', 0xA4: 'Alt', 0xA5: 'Alt', 0x5B: 'Win', 0x5C: 'Win',
    0x20: 'Space', 0x0D: 'Enter', 0x09: 'Tab', 0x08: 'Backspace', 0x2E: 'Delete', 0x24: 'Home', 0x23: 'End', 0x21: 'PageUp', 0x22: 'PageDown', 0x2D: 'Insert', 0x14: 'CapsLock',
    0x26: 'ArrowUp', 0x28: 'ArrowDown', 0x25: 'ArrowLeft', 0x27: 'ArrowRight' };
  const vkName = (vk) => VK[vk] || (vk >= 0x41 && vk <= 0x5A ? String.fromCharCode(vk) : vk >= 0x30 && vk <= 0x39 ? String(vk - 0x30) : vk >= 0x70 && vk <= 0x7B ? 'F' + (vk - 0x6F) : null);
  D.on('key', (ev) => {
    const n = vkName(ev[0]); if (!n) return;
    if (tryHook) { tryHook.key(n, ev[1]); return; }
    if (typeof capsLive === 'function' && capsLive()) for (const k of document.querySelectorAll('.keys kbd')) if (k.textContent === n) k.classList.toggle('down', ev[1]);
  });
  D.on('try_text', (t) => { if (tryHook) tryHook.text(t); });
  D.on('state', (st) => {
    if (tryHook) tryHook.state(st);
    if (capsLive()) {
      const rows = document.querySelectorAll('.keys');
      const cap = (row, on, ms) => { const k = row && row.lastElementChild; if (!k) return; k.classList.toggle('down', on); if (ms) setTimeout(() => k.classList.remove('down'), ms); };
      if (st === 'listening' && lastPillState !== 'listening') cap(rows[0], true);
      if (lastPillState === 'listening' && st !== 'listening') cap(rows[0], false);
      if ((st === 'handsfree') !== (lastPillState === 'handsfree')) cap(rows[1], true, 240);
    }
    lastPillState = st;
  });

  // Mic meter. Native: the backend streams "level" events while the bar is
  // on screen. Browser: a fake voice.
  let meterBar = null;
  let meterLevel = 0;
  D.on('level', (v) => { meterLevel = v; });
  function fakeMeter(bar) {
    meterBar = bar;
    if (D.isNative) D.meterStart();
    let t = 0;
    const iv = setInterval(() => {
      if (!bar.isConnected) { clearInterval(iv); if (meterBar === bar) { meterBar = null; if (D.isNative) D.meterStop(); } return; }
      let v;
      if (D.isNative) { v = Math.max(0, meterLevel); bar.parentElement.classList.toggle('off', meterLevel < 0); }
      else { t += 0.12; v = Math.max(0, Math.sin(t * 1.3)) * Math.max(0, Math.sin(t * 0.37) + 0.4); }
      bar.style.width = (v * 96 + 4) + '%';
    }, 60);
  }

  // ---------- Onboarding ----------
  // A soft shape, seeded once per install so every copy of neptune has its
  // own, drifting slowly the whole time it is on screen.
  function shape(seed, round) {
    const svg = h('span', { html: '<svg class="shape" viewBox="0 0 680 470"><path/></svg>' }).firstChild;
    const path = svg.querySelector('path');
    // round: the welcome's planet, still a little hand-made at the edge
    const n = 6, cx = 340, cy = 235, rx = round ? 222 : 320, ry = round ? 216 : 210;
    let q = seed >>> 0; const r = () => ((q = (q * 1664525 + 1013904223) >>> 0) / 4294967296);
    const base = Array.from({ length: n }, () => (round ? 0.97 + r() * 0.05 : 0.9 + r() * 0.14)), sp = Array.from({ length: n }, () => 0.4 + r() * 0.8), ph = Array.from({ length: n }, () => r() * 6.283);
    const still = matchMedia('(prefers-reduced-motion: reduce)').matches;
    function draw(t) {
      const pts = base.map((b, i) => { const a = (i / n) * 6.283; const k = b + (still ? 0 : Math.sin(t * 0.0004 * sp[i] + ph[i]) * 0.025); return [cx + Math.cos(a) * rx * k, cy + Math.sin(a) * ry * k]; });
      let d = '';
      for (let i = 0; i < n; i++) { const p0 = pts[(i - 1 + n) % n], p1 = pts[i], p2 = pts[(i + 1) % n], p3 = pts[(i + 2) % n];
        const c1 = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6], c2 = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
        d += (i === 0 ? `M${p1[0]},${p1[1]}` : '') + ` C${c1[0]},${c1[1]} ${c2[0]},${c2[1]} ${p2[0]},${p2[1]}`; }
      path.setAttribute('d', d + ' Z');
      if (svg.isConnected || !t) requestAnimationFrame(draw);
    }
    requestAnimationFrame(draw);
    return svg;
  }
  // The onboarding sky: seeded stars over the whole window, a few brighter
  // ones with a faint halo. Same seed, same sky, every step. It behaves like a
  // real sky, barely: the field turns about the pole once a sidereal day (CSS),
  // and stars twinkle more the nearer they sit to the horizon, as starlight
  // crossing more air does; overhead they hardly flicker at all.
  function sky(seed) {
    let q = (seed ^ 0x5bd1e995) >>> 0; const r = () => ((q = (q * 1664525 + 1013904223) >>> 0) / 4294967296);
    let dots = '';
    // the field is wider than the window, so the turn never empties a corner;
    // same density as 110 stars over the window itself
    for (let k = 0; k < 360; k++) {
      const x = (r() * 180 - 40).toFixed(2), y = (r() * 180 - 40).toFixed(2), big = r() < 0.12;
      const rad = big ? 1.1 + r() * 0.7 : 0.4 + r() * 0.6, op = big ? 0.9 : 0.25 + r() * 0.5;
      const low = Math.min(1, Math.max(0, Number(y) / 100)); // 0 overhead, 1 at the horizon
      const tw = r() < 0.08 + low * 0.5 ? ` class="tw" style="--o:${op.toFixed(2)};--a:${(0.12 + low * 0.3).toFixed(2)};animation-delay:-${(r() * 9).toFixed(2)}s;animation-duration:${(5 + r() * 6).toFixed(1)}s"` : '';
      if (big) dots += `<circle cx="${x}%" cy="${y}%" r="${(rad * 3.2).toFixed(2)}" fill="#fff" opacity=".06"/>`;
      dots += `<circle cx="${x}%" cy="${y}%" r="${rad.toFixed(2)}" fill="#fff" opacity="${op.toFixed(2)}"${tw}/>`;
    }
    return h('span', { html: `<svg class="sky" aria-hidden="true"><g class="field">${dots}</g><line class="shoot" x1="0" y1="0" x2="130" y2="0"/></svg>` }).firstChild;
  }
  // Neptune's ring around the welcome's cream planet: the far half is drawn
  // under the planet, the near half over it.
  function ring(front) {
    const half = front ? 'M -360 0 A 360 70 0 0 0 360 0' : 'M -360 0 A 360 70 0 0 1 360 0';
    return h('span', { html: `<svg class="ring ${front ? 'front' : 'back'}" viewBox="-490 -130 980 260" aria-hidden="true"><g transform="rotate(-9)"><path d="${half}"/></g></svg>` }).firstChild;
  }

  function installSeed() {
    try { let s = localStorage.getItem('blurt.seed'); if (!s) { s = String(Math.floor(Math.random() * 1e9)); localStorage.setItem('blurt.seed', s); } return Number(s); } catch (e) { return 7; }
  }

  function onboarding(startAt) {
    const onb = h('div', { class: 'onb' });
    const stage = h('div', { class: 'stage' });
    const dots = h('span', { class: 'dots' });
    const back = h('button', { class: 'btn quiet', onclick: () => go(i - 1) }, 'Back');
    // What Continue does can change (Retry after a failed download), so it
    // goes through one slot instead of stacking click handlers.
    let nextAction = () => go(i + 1);
    const next = h('button', { class: 'btn primary', onclick: () => nextAction() }, 'Continue');
    const foot = h('div', { class: 'foot' }, dots, h('span', { class: 'sp' }), back, next);
    onb.append(sky(installSeed()), stage, foot);
    stage.setAttribute('data-tauri-drag-region', '');
    document.getElementById('win').append(onb);
    document.body.classList.add('onboarding');
    let i = startAt || 0;

    const mark = () => h('span', { html: '<svg class="mark-big" viewBox="0 0 64 64"><use href="#ringed" width="64" height="64" style="--rw:2.6"/><circle class="star" cx="58" cy="8" r="1.3" fill="#fff"/><circle class="star" cx="6" cy="54" r="1" fill="#fff" style="animation-delay:1.4s"/></svg>' }).firstChild;
    const steps = [
      () => h('div', { class: 'step' }, h('h1', { class: 'line' }, 'Say it ', h('i', {}, 'out loud.')), h('p', {}, 'Hold a key, talk, let go. Clean text lands wherever your cursor is.'),
        h('div', { class: 'three' }, h('span', {}, 'Free'), h('span', {}, 'No account'), h('span', {}, 'Nothing leaves this PC'))),
      () => {
        // The one download. The person starts it (nothing fetches on its own)
        // and it cannot be skipped: without the model there is no app.
        const bar = h('i');
        const pct = h('span', {}, '');
        const rate = h('span', {}, '650 MB, once');
        const pauseBtn = h('a', { class: 'skip', style: 'display:none', onclick: async () => { await pauseDownload(!dlPaused); pauseBtn.textContent = dlPaused ? 'Resume' : 'Pause'; } }, dlPaused ? 'Resume' : 'Pause');
        const check = h('span', { class: 'dlcheck', html: '<svg viewBox="0 0 16 16"><path d="M3.5 8.5l3 3 6-6.5"/></svg>' });
        const get = h('button', { class: 'btn primary get', onclick: () => begin() }, 'Download');
        const card = h('div', { class: 'card dl' },
          h('div', { class: 'dlhead' }, h('div', {}, h('b', {}, 'Parakeet TDT 0.6B v3'), h('span', { class: 'by' }, 'Speech model by NVIDIA')), get, check),
          h('div', { class: 'progress' }, bar), h('div', { class: 'r' }, rate, pct));
        next.textContent = 'Continue'; next.disabled = true;
        // Arriving: the bar runs to the end, then the card settles into ready
        // (bar turns green, a check pops in, Continue wakes up). Already on
        // disk: the same, without the wait.
        let arrived = false;
        const finish = (instant) => {
          if (arrived) return; arrived = true; onDownload = null;
          bar.style.width = '100%'; pct.textContent = '100%'; pauseBtn.remove();
          get.style.display = 'none';
          setTimeout(() => {
            card.classList.add('done'); rate.textContent = 'Ready'; pct.textContent = '';
            next.disabled = false; nextAction = () => go(i + 1); next.classList.add('nudge');
          }, instant ? 0 : 420);
        };
        // what went wrong, in words a person can act on; progress is kept
        const human = (m) => /os error 112|space/i.test(m) ? 'Not enough disk space. Neptune needs about 700 MB free.'
          : /dns|resolve|connect|timed? ?out|network|unreachable|reset|dropped/i.test(m) ? 'The connection dropped. Try again to pick up where it stopped.'
          : 'Something went wrong (' + m + '). Try again to pick up where it stopped.';
        const failed = (msg) => { rate.textContent = human(String(msg)); rate.classList.add('err'); get.textContent = 'Try again'; get.style.display = ''; pauseBtn.style.display = 'none'; card.classList.remove('going'); onDownload = null; };
        function begin() {
          get.style.display = 'none'; pauseBtn.style.display = ''; card.classList.add('going'); rate.classList.remove('err'); rate.textContent = 'Starting…'; if (!pct.textContent) pct.textContent = '0%';
          if (D.isNative) {
            onDownload = (d) => {
              if (d.error) { failed(d.error); return; }
              const p = Math.min(100, (d.done / d.total) * 100);
              bar.style.width = p + '%'; pct.textContent = Math.round(p) + '%';
              rate.textContent = d.finished ? 'Ready' : (d.done / 1048576).toFixed(0) + ' of ' + (d.total / 1048576).toFixed(0) + ' MB' + dlRate(d);
              if (d.finished) finish();
            };
            startDownload().then((ok) => { if (!ok) failed('could not start'); });
          } else {
            let p = 0;
            const iv = setInterval(() => { p = Math.min(100, p + 3 + Math.random() * 4); bar.style.width = p + '%'; pct.textContent = Math.round(p) + '%'; rate.textContent = p < 100 ? (650 * p / 100).toFixed(0) + ' of 650 MB · 41 MB/s' : 'Ready'; if (p >= 100) { clearInterval(iv); finish(); } }, 120);
          }
        }
        // already on disk (a reinstall): nothing to do
        // already on disk (a reinstall): ready at once. Still running (the
        // window was closed and reopened, or Back then Continue): follow it.
        if (D.isNative) D.modelState().then((st) => { if (st === 'ready') finish(true); else if (st === 'downloading') begin(); }).catch(() => {});
        return h('div', { class: 'step' }, h('h1', { class: 'line' }, 'One ', h('i', {}, 'download.')),
          h('p', {}, "Neptune hears you with Parakeet, NVIDIA's open speech model. It runs on your PC, and it is the only thing Neptune ever fetches."),
          h('div', { class: 'body' }, card, h('div', { class: 'links' }, pauseBtn)));
      },
      () => {
        const meter = h('div', { class: 'meter' }, h('i')); fakeMeter(meter.firstChild);
        const sel = h('div', {});
        D.settings().then((st) => {
          const opts = [{ value: '', label: 'System default' }, ...(st.mics || []).map((m) => ({ value: m, label: m }))];
          sel.replaceChildren(dropdown(opts, st.mic || '', (v) => { st.mic = v; D.saveSettings(st).then(() => { if (D.isNative) D.meterStart(); }).catch((e) => toast('Could not save: ' + e)); }));
        }).catch(() => {});
        return h('div', { class: 'step' }, h('h1', { class: 'line' }, 'Which ', h('i', {}, 'microphone?')), h('p', {}, 'Say something. The bar should move.'),
          h('div', { class: 'body' }, h('div', { class: 'card miccard' }, sel,
            h('div', { class: 'meterrow' }, 'Level', meter))));
      },
      () => tryStep(),
      () => h('div', { class: 'step' }, mark(), h('h1', { class: 'line' }, "That's the ", h('i', {}, 'whole app.')), h('p', {}, 'Neptune lives in the tray. Open it for your history, or to change a key.'),
        h('div', { class: 'body shortcuts' }, h('div', { class: 'card' },
          h('div', { class: 'srow' }, h('div', { class: 'l' }, h('b', {}, 'Hold to talk')), h('span', { class: 'keys' }, h('kbd', {}, 'Ctrl'), h('kbd', {}, 'Space'))),
          h('div', { class: 'srow' }, h('div', { class: 'l' }, h('b', {}, 'Hands-free')), h('span', { class: 'keys' }, h('kbd', {}, 'Ctrl'), h('kbd', {}, 'Shift'), h('kbd', {}, 'Space'))),
          h('div', { class: 'srow' }, h('div', { class: 'l' }, h('b', {}, 'Open history')), h('span', { class: 'keys' }, h('kbd', {}, 'Ctrl'), h('kbd', {}, 'Shift'), h('kbd', {}, 'H')))))),
    ];
    // Try it: hold the real keys, talk, let go, and watch it type.
    function tryStep() {
      const ctrl = h('kbd', {}, 'Ctrl'), space = h('kbd', {}, 'Space');
      const keys = h('span', { class: 'keys trykeys', role: 'img', 'aria-label': 'Ctrl and Space' }, ctrl, space);
      const chip = h('span', { class: 'chip' }, '');
      const out = h('span', { class: 'out' });
      const pad = h('div', { class: 'card trypad empty' }, chip, h('div', { class: 'txt' }, out, h('i', { class: 'caret' })));
      const hint = h('div', { class: 'tryhint' }, 'Hold both keys, say anything, let go.');
      const press = (on) => { ctrl.classList.toggle('down', on); space.classList.toggle('down', on); };
      // the chip fades out whole: its word stays until it has faded, so
      // the dot is never left alone for a moment
      let chipOff = 0;
      const setChip = (txt, cls) => {
        clearTimeout(chipOff);
        if (txt) { chip.textContent = txt; chip.className = 'chip show' + (cls ? ' ' + cls : ''); return; }
        chip.classList.remove('show');
        chipOff = setTimeout(() => { chip.textContent = ''; chip.className = 'chip'; }, 220);
      };
      // until the first real take, the keys show how it is done
      let demo = true, t1 = 0, t2 = 0;
      const loop = () => {
        if (!demo || !keys.isConnected) return;
        ctrl.classList.add('down');
        t1 = setTimeout(() => space.classList.add('down'), 140);
        t2 = setTimeout(() => { press(false); setTimeout(loop, 1900); }, 1300);
      };
      setTimeout(loop, 700);
      const stopDemo = () => { if (!demo) return; demo = false; clearTimeout(t1); clearTimeout(t2); press(false); keys.classList.add('live'); };
      // Ctrl reaches the window (only the trigger key is swallowed), so the
      // first touch of it ends the demo and presses the real cap
      const onKey = (e) => {
        if (!keys.isConnected) { window.removeEventListener('keydown', onKey); window.removeEventListener('keyup', onKey); return; }
        if (e.key !== 'Control' && e.code !== 'Space') return;
        stopDemo(); (e.code === 'Space' ? space : ctrl).classList.toggle('down', e.type === 'keydown');
        if (e.key === 'Control') watchTaken(e.type === 'keydown');
      };
      window.addEventListener('keydown', onKey); window.addEventListener('keyup', onKey);
      let typing = null, tries = 0;
      const type = (text) => {
        clearInterval(typing);
        pad.classList.remove('empty'); pad.classList.add('typing');
        const start = out.textContent ? out.textContent + ' ' : '';
        let k = 0;
        const step = Math.max(1, Math.round(text.length / 90));
        typing = setInterval(() => {
          k = Math.min(text.length, k + step);
          out.textContent = start + text.slice(0, k);
          if (k >= text.length) {
            clearInterval(typing); pad.classList.remove('typing');
            setChip('', '');
            tries++;
            hint.replaceChildren(h('b', {}, tries > 1 ? 'Again, perfect.' : "That's it."), ' It works the same in any app, wherever your cursor is.');
            hint.classList.add('ok'); next.classList.add('nudge');
          }
        }, 16);
      };
      // Ctrl held a while and no take: probably another app has the combo
      let taken = 0;
      const watchTaken = (on) => {
        clearTimeout(taken);
        if (on) taken = setTimeout(() => { if (!chip.classList.contains('on')) { hint.replaceChildren('Nothing yet? Another app may be using Ctrl + Space (Handy, for one). Close it, or pick a new key in Settings.'); hint.classList.add('bad'); } }, 2600);
      };
      tryHook = {
        key(n, down) {
          if (n !== 'Ctrl' && n !== 'Space') return;
          stopDemo();
          (n === 'Ctrl' ? ctrl : space).classList.toggle('down', down);
          if (n === 'Ctrl') watchTaken(down);
        },
        state(st) {
          if (st === 'listening' || st === 'handsfree') watchTaken(false);
          if (st === 'listening' || st === 'handsfree') { stopDemo(); press(st === 'listening'); setChip('Listening', 'on'); hint.classList.remove('ok', 'bad'); if (!pad.classList.contains('typing')) hint.textContent = 'Listening. Let go when you are done.'; }
          // one indicator only: Listening, which fades when you let go
          else if (st === 'processing') { press(false); setChip('', ''); }
          else if (st === 'error') { press(false); setChip('', ''); hint.textContent = 'Did not catch that. Hold the keys a little longer and try again.'; hint.classList.add('bad'); }
          else if (st === 'idle') { press(false); if (chip.classList.contains('on')) setChip('', ''); }
        },
        text: type,
      };
      // browser preview: clicking the keys plays a take
      if (!D.isNative) keys.addEventListener('click', () => {
        tryHook.state('listening');
        setTimeout(() => tryHook.state('processing'), 1600);
        setTimeout(() => { tryHook.state('done'); type('Testing, one, two. This is my first time using Neptune, and it already works.'); }, 2300);
      });
      return h('div', { class: 'step trystep' }, h('h1', { class: 'line' }, 'Try ', h('i', {}, 'it.')),
        h('p', {}, 'Hold Ctrl and Space, say a sentence, then let go.'),
        h('div', { class: 'body' }, keys, pad, hint));
    }

    let busy = false;
    function go(n) {
      if (busy) return;
      const old = stage.firstElementChild;
      if (old && n < steps.length) {
        busy = true;
        old.classList.add('leave');
        setTimeout(() => { busy = false; place(n); }, 150);
        return;
      }
      place(n);
    }
    const seed = installSeed();
    function place(n) {
      if (n >= steps.length) {
        onb.remove(); document.body.classList.remove('onboarding', 'onb-0'); onDownload = null; tryHook = null; D.setTryMode(false); D.keyRelay(false);
        D.settings().then((st) => { st.onboarded = true; return D.saveSettings(st); }).catch((e) => toast('Could not save: ' + e));
        show('home'); return;
      }
      i = Math.max(0, n);
      try { localStorage.setItem('neptune.onbStep', String(i)); } catch (e) {}
      nextAction = () => go(i + 1);
      if (i !== 1) onDownload = null;
      stage.classList.toggle('ink', i === 0);
      document.body.classList.toggle('onb-0', i === 0);
      if (i !== 3) tryHook = null;
      D.setTryMode(i === 3);
      D.keyRelay(i === 3 || i === 4);
      stage.replaceChildren(...(i === 0 ? [ring(false), shape(seed, true), ring(true)] : [shape(seed + i * 101)]), steps[i]());
      dots.replaceChildren(...steps.map((_, k) => h('i', { class: k === i ? 'on' : '' })));
      back.style.visibility = i === 0 ? 'hidden' : 'visible';
      if (i !== 1) { next.disabled = false; next.textContent = i === steps.length - 1 ? 'Done' : i === 0 ? 'Get started' : 'Continue'; }
    }
    go(i);
  }

  // ---- boot ----
  if (q.has('onboarding')) onboarding(Number(q.get('onboarding')) || 0);
  // A first run that was interrupted (app closed, PC restarted) comes back
  // to the step it was on, but never past the download if the model is not
  // on disk yet.
  else if (D.isNative) D.settings().then((st) => {
    if (st.onboarded) return;
    let at = 0; try { at = Math.min(4, Math.max(0, Number(localStorage.getItem('neptune.onbStep')) || 0)); } catch (e) {}
    if (at > 1 && st.modelState !== 'ready') at = 1;
    onboarding(at);
  }).catch((e) => toast('Could not read settings: ' + e));
  show(q.get('view') || 'home');
  window.blurt.show = show;
  window.blurt.refreshHistory = () => (historyApi ? historyApi.refresh() : Promise.resolve(false));
  window.blurt.onboarding = onboarding;
})();
