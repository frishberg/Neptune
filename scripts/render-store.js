// Microsoft Store art: 1:1 box art, 2:3 poster art, and five 1920x1080
// screenshots. Each screenshot is a designed image in the spirit of Watt's
// Left's listing: a headline and one line of plain copy on the left, the real
// app window (captured from the browser preview, on its sample data) floating
// on the night on the right. Flat colours only, no gradients.
//
//   node scripts/render-store.js        (from neptune/; writes ../store/)
//
// It serves neptune/ and website/ itself, so no preview server is needed.
const puppeteer = require('puppeteer-core');
const http = require('http');
const fs = require('fs');
const path = require('path');

const APP = path.resolve(__dirname, '..');
const SITE = path.resolve(APP, '../website');
const OUT = path.resolve(APP, '../store');
const CHROME = 'C:/Program Files/Google/Chrome/Application/chrome.exe';

// ---------- a tiny static server for one folder ----------
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.png': 'image/png', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.json': 'application/json' };
function serve(root) {
  return new Promise((done) => {
    const server = http.createServer((req, res) => {
      const file = path.join(root, decodeURIComponent(req.url.split('?')[0]));
      fs.readFile(file, (err, data) => {
        if (err) { res.writeHead(404); res.end(); return; }
        res.writeHead(200, { 'Content-Type': TYPES[path.extname(file)] || 'application/octet-stream' });
        res.end(data);
      });
    });
    server.listen(0, '127.0.0.1', () => done({ url: `http://127.0.0.1:${server.address().port}`, server }));
  });
}

// ---------- the night: stars and the ringed planet ----------
const planet = (rw) => `<clipPath id="f"><rect x="-80" y="0" width="160" height="80" transform="rotate(-16)"/></clipPath><ellipse rx="46" ry="11" transform="rotate(-16)" fill="none" stroke="#7fa2ff" stroke-width="${rw}"/><circle r="25" fill="#2f5fe0"/><ellipse rx="46" ry="11" transform="rotate(-16)" fill="none" stroke="#7fa2ff" stroke-width="${rw}" clip-path="url(#f)"/>`;
const stars = (n, w, h, seed = 1) => Array.from({ length: n }, (_, i) => {
  const k = i + seed * 13;
  return `<circle cx="${(k * 197 + 61) % w}" cy="${(k * 131 + 37) % h}" r="${1 + (k % 3) * 0.6}" fill="#fff" opacity="${0.25 + (k % 4) * 0.12}"/>`;
}).join('');
const fonts = (site) => `@font-face{font-family:"Inter";font-weight:100 900;src:url("${site}/fonts/inter-latin.woff2")}@font-face{font-family:"Space Grotesk";font-weight:700;src:url("${site}/fonts/space-grotesk-700.woff2")}`;

// ---------- box art and poster ----------
const box = (site) => `<style>${fonts(site)}body{margin:0;background:#0e0e13}</style><body style="width:2160px;height:2160px;position:relative;overflow:hidden">
<svg width="2160" height="2160" style="position:absolute">${stars(26, 2160, 2160)}</svg>
<svg viewBox="-50 -50 100 100" style="position:absolute;left:330px;top:330px;width:1500px;height:1500px;overflow:visible">${planet(2.4)}</svg></body>`;
const poster = (site) => `<style>${fonts(site)}body{margin:0;background:#0e0e13;color:#f3f0e8}</style><body style="width:1440px;height:2160px;position:relative;overflow:hidden">
<svg width="1440" height="2160" style="position:absolute">${stars(30, 1440, 2160)}</svg>
<svg viewBox="-50 -50 100 100" style="position:absolute;left:170px;top:300px;width:1100px;height:1100px;overflow:visible">${planet(2.4)}</svg>
<div style="position:absolute;left:0;right:0;top:1500px;text-align:center;font:700 170px 'Space Grotesk';letter-spacing:-.02em">Neptune</div>
<div style="position:absolute;left:0;right:0;top:1730px;text-align:center;font:500 54px Inter;color:rgba(243,240,232,.66)">Talk. Let go. It's typed.</div></body>`;

// ---------- one designed screenshot ----------
// `visual` is the HTML for the right-hand side, positioned inside a
// 1920x1080 stage; the headline and copy sit on the left.
const shot = (site, title, copy, visual, seed) => `<style>${fonts(site)}
body{margin:0;width:1920px;height:1080px;background:#0e0e13;color:#f3f0e8;font-family:Inter;position:relative;overflow:hidden}
.words{position:absolute;left:140px;top:0;bottom:0;width:640px;display:flex;flex-direction:column;justify-content:center}
h1{font:700 78px/1.06 'Space Grotesk';letter-spacing:-.025em;margin:0 0 30px;white-space:nowrap}
h1 em{font-style:normal;color:#7fa2ff}
p{font:400 30px/1.48 Inter;color:rgba(243,240,232,.66);margin:0}
.win{position:absolute;border-radius:18px;box-shadow:0 0 0 1px rgba(255,255,255,.08),0 40px 90px rgba(0,0,0,.55)}
.keys{display:flex;gap:12px;margin-top:40px}
.key{font:600 26px Inter;color:#f3f0e8;padding:12px 22px;border-radius:12px;background:#1e1e25;box-shadow:inset 0 -3px 0 rgba(0,0,0,.45),0 0 0 1px rgba(255,255,255,.1)}
</style><body>
<svg width="1920" height="1080" style="position:absolute">${stars(34, 1920, 1080, seed)}</svg>
<svg viewBox="-50 -50 100 100" style="position:absolute;right:-150px;bottom:-190px;width:620px;height:620px;overflow:visible;opacity:.9">${planet(2.4)}</svg>
<div class="words"><h1>${title}</h1><p>${copy}</p>${visual.keys || ''}</div>
${visual.html}</body>`;

// a window image placed on the right, vertically centred, at a given display width
const windowAt = (src, w, h, width, x = 1860 - width) => {
  const height = Math.round(h * width / w);
  return `<img class="win" src="${src}" style="left:${x}px;top:${Math.round((1080 - height) / 2)}px;width:${width}px;height:${height}px">`;
};

(async () => {
  const app = await serve(APP), site = await serve(SITE);
  const b = await puppeteer.launch({ executablePath: CHROME, headless: 'new' });
  const p = await b.newPage();
  fs.mkdirSync(path.join(OUT, 'screenshots'), { recursive: true });

  // the real app windows, captured at 2x without their surroundings
  const capture = async (query) => {
    await p.setViewport({ width: 1008, height: 808, deviceScaleFactor: 2 });
    await p.goto(`${app.url}/overlay/app/index.html?${query}`, { waitUntil: 'networkidle0' });
    await p.evaluate(() => document.fonts.ready);
    await new Promise((r) => setTimeout(r, 2200));
    const el = await p.$('.win');
    const png = await el.screenshot({ omitBackground: true, encoding: 'base64' });
    const box = await el.boundingBox();
    return { src: `data:image/png;base64,${png}`, w: box.width, h: box.height };
  };
  const home = await capture('view=home');
  const history = await capture('view=history');
  const settings = await capture('view=settings');
  // the pill, captured at 3x in each state
  const pillShot = async (query) => {
    await p.setViewport({ width: 520, height: 140, deviceScaleFactor: 3 });
    await p.goto(`${app.url}/overlay/index.html?${query}`, { waitUntil: 'networkidle0' });
    await new Promise((r) => setTimeout(r, 1300));
    const el = await p.$('#pill');
    const box = await el.boundingBox();
    return { src: `data:image/png;base64,${await el.screenshot({ omitBackground: true, encoding: 'base64' })}`, w: box.width, h: box.height };
  };
  const idle = await pillShot('state=idle&hold=1');
  const listening = await pillShot('state=listening&hold=1&freeze=1');
  const thinking = await pillShot('state=processing&hold=1');
  const pill = listening.src, pillBox = listening;

  // shot 1: text landing in a chat box while the pill listens
  const chat = `<div class="win" style="left:960px;top:250px;width:860px;height:470px;background:#1a1a21;padding:34px 38px;box-sizing:border-box;font-family:Inter">
    <div style="display:flex;align-items:center;gap:12px;color:rgba(243,240,232,.5);font-size:20px;margin-bottom:28px"><span style="width:12px;height:12px;border-radius:50%;background:#3a3a44"></span><span style="width:12px;height:12px;border-radius:50%;background:#3a3a44"></span><span style="width:12px;height:12px;border-radius:50%;background:#3a3a44"></span><span style="margin-left:14px">Team chat · #release</span></div>
    <div style="font-size:24px;line-height:1.5;color:rgba(243,240,232,.62);margin-bottom:18px"><b style="color:#f3f0e8;font-weight:600">Maya</b>&nbsp; Is the build green? I want to cut the release before standup.</div>
    <div style="font-size:24px;line-height:1.5;color:rgba(243,240,232,.62)"><b style="color:#f3f0e8;font-weight:600">Sam</b>&nbsp; Tests passed on my side too.</div>
    <div style="position:absolute;left:38px;right:38px;bottom:38px;border-radius:14px;background:#22222b;box-shadow:0 0 0 2px rgba(127,162,255,.55);padding:22px 24px;font-size:26px;line-height:1.5;color:#f3f0e8">Yes, it's green. Go ahead and cut it, and I'll update the changelog after standup.<span style="display:inline-block;width:3px;height:30px;background:#7fa2ff;vertical-align:-6px;margin-left:3px"></span></div></div>
    <img src="${pill}" style="position:absolute;left:${Math.round(1390 - pillBox.w * 2.6 / 2)}px;top:755px;width:${Math.round(pillBox.w * 2.6)}px">`;

  // shot 5: the pill's three moments, large, each with a label
  const row = (img, label, top) => `<div style="position:absolute;left:1000px;width:780px;top:${top}px;display:flex;align-items:center;gap:48px">
      <div style="width:420px;display:flex;justify-content:center"><img src="${img.src}" style="width:${Math.round(img.w * 3.4)}px"></div>
      <div style="font:600 30px Inter;color:#f3f0e8;white-space:nowrap">${label[0]}<div style="font:400 24px Inter;color:rgba(243,240,232,.55);margin-top:6px">${label[1]}</div></div></div>`;
  const pills = row(idle, ['Waiting', 'A quiet line. Nothing records.'], 300) + row(listening, ['Listening', 'Moves with your voice'], 470) + row(thinking, ['Typing', 'Clean text lands at your cursor'], 640);

  const shots = [
    ['1-talk.png', 'Talk. Let go.<br><em>It\'s typed.</em>', 'Hold Ctrl+Space in any app, say it, and let go. Clean text lands wherever your cursor is.', { html: chat, keys: '<div class="keys"><span class="key">Ctrl</span><span class="key">Space</span></div>' }],
    ['2-home.png', 'Your voice stays<br><em>on your PC.</em>', 'Speech is turned into text on your own machine. No account, no cloud, no word limit.', { html: windowAt(home.src, home.w, home.h, 1000) }],
    ['3-history.png', 'Everything<br>you\'ve <em>said.</em>', 'Every dictation is kept and searchable. Copy one again, or paste the last with Ctrl+Shift+V.', { html: windowAt(history.src, history.w, history.h, 1000) }],
    ['4-shortcuts.png', 'Four shortcuts.<br><em>That\'s the manual.</em>', 'Hold to talk, hands-free, paste last, open history. Change any of them in Settings.', { html: windowAt(settings.src, settings.w, settings.h, 1000) }],
    ['5-pill.png', 'Quiet until<br><em>you talk.</em>', 'A small pill at the bottom of your screen. It listens only while you hold the keys, then gets out of the way.', { html: pills }],
  ];

  const page = async (html, w, h, file) => {
    fs.writeFileSync(path.join(SITE, '_store.html'), `<!doctype html><html><head><meta charset="utf-8"></head>${html}</html>`);
    await p.setViewport({ width: w, height: h, deviceScaleFactor: 1 });
    await p.goto(`${site.url}/_store.html`, { waitUntil: 'networkidle0' });
    await p.evaluate(() => document.fonts.ready);
    await p.screenshot({ path: file });
  };
  try {
    await page(box(site.url), 2160, 2160, path.join(OUT, 'box-art-1x1.png'));
    await page(poster(site.url), 1440, 2160, path.join(OUT, 'poster-art-2x3.png'));
    for (const [i, [name, title, copy, visual]] of shots.entries()) {
      await page(shot(site.url, title, copy, visual, i + 1), 1920, 1080, path.join(OUT, 'screenshots', name));
      console.log('saved', name);
    }
  } finally {
    fs.rmSync(path.join(SITE, '_store.html'), { force: true });
    await b.close(); app.server.close(); site.server.close();
  }
})();
