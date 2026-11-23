// The Store package's icons (msix/Assets): the same bare ringed planet as the
// app icon, on transparency. The 44 px icon also comes in every taskbar size,
// "unplated", so Windows shows the planet itself and no coloured square.
//   node scripts/render-msix-assets.js
const puppeteer = require('puppeteer-core');
const path = require('path');
const fs = require('fs');
const OUT = path.join(__dirname, '..', 'msix', 'Assets');
fs.mkdirSync(OUT, { recursive: true });

const ring = (rw) => `
  <clipPath id="f"><rect x="-80" y="0" width="160" height="80" transform="rotate(-16)"/></clipPath>
  <ellipse rx="46" ry="11" transform="rotate(-16)" fill="none" stroke="#7fa2ff" stroke-width="${rw}"/>
  <circle r="25" fill="#2f5fe0"/>
  <ellipse rx="46" ry="11" transform="rotate(-16)" fill="none" stroke="#7fa2ff" stroke-width="${rw}" clip-path="url(#f)"/>`;
// the planet fills `fill` of the box; the ring thickens as the icon shrinks
const planet = (w, h, fill) => {
  const s = Math.min(w, h);
  const rw = s >= 128 ? 3.6 : s >= 48 ? 4.6 : 6;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="${-50 * w / s / fill} ${-50 * h / s / fill} ${100 * w / s / fill} ${100 * h / s / fill}"><g transform="scale(1.02)">${ring(rw)}</g></svg>`;
};

const jobs = [
  ['StoreLogo.png', 50, 50, 1],
  ['Square44x44Logo.png', 44, 44, 1],
  ['Square150x150Logo.png', 150, 150, 0.66],
  ['Wide310x150Logo.png', 310, 150, 0.66],
];
for (const t of [16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256]) {
  jobs.push([`Square44x44Logo.targetsize-${t}.png`, t, t, 1]);
  jobs.push([`Square44x44Logo.targetsize-${t}_altform-unplated.png`, t, t, 1]);
}

(async () => {
  const b = await puppeteer.launch({ executablePath: 'C:/Program Files/Google/Chrome/Application/chrome.exe', headless: 'new' });
  const p = await b.newPage();
  for (const [name, w, h, fill] of jobs) {
    await p.setViewport({ width: w, height: h, deviceScaleFactor: 1 });
    await p.setContent(`<html><body style="margin:0;background:transparent">${planet(w, h, fill)}</body></html>`);
    await p.screenshot({ path: path.join(OUT, name), omitBackground: true, clip: { x: 0, y: 0, width: w, height: h } });
  }
  await b.close();
  console.log('ok', jobs.length);
})();
