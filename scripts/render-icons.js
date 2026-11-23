// Renders the app icon and the tray icon, both the bare ringed planet on
// transparency, to PNG at every size the app needs. (The night tile is kept
// below as `tile` for Store art.)
const puppeteer = require('puppeteer-core');
const fs = require('fs');
const OUT = process.argv[2];
const ring = (rw) => `
  <clipPath id="f"><rect x="-80" y="0" width="160" height="80" transform="rotate(-16)"/></clipPath>
  <ellipse rx="46" ry="11" transform="rotate(-16)" fill="none" stroke="#7fa2ff" stroke-width="${rw}"/>
  <circle r="25" fill="#2f5fe0"/>
  <ellipse rx="46" ry="11" transform="rotate(-16)" fill="none" stroke="#7fa2ff" stroke-width="${rw}" clip-path="url(#f)"/>`;
// the tile: small sizes drop the stars and thicken the ring so it still reads
const tile = (s) => `<svg xmlns="http://www.w3.org/2000/svg" width="${s}" height="${s}" viewBox="0 0 100 100">
  <rect x="2" y="2" width="96" height="96" rx="23" fill="#111115"/>
  ${s >= 48 ? '<g fill="#fff"><circle cx="22" cy="22" r="1.3"/><circle cx="80" cy="80" r="1.1"/><circle cx="78" cy="19" r=".9"/><circle cx="20" cy="76" r=".8"/></g>' : ''}
  <g transform="translate(50 50) scale(${s >= 48 ? .86 : .96})">${ring(s >= 48 ? 2.6 : s >= 32 ? 4 : 5)}</g></svg>`;
// the app icon is the bare planet too (Aron, 2026-11-22: the taskbar matches the
// tray, no tile); the ring thickens as the icon gets smaller so it still reads
const bare = (s) => `<svg xmlns="http://www.w3.org/2000/svg" width="${s}" height="${s}" viewBox="-50 -50 100 100"><g transform="scale(1.02)">${ring(s >= 128 ? 3.6 : s >= 48 ? 4.6 : 6)}</g></svg>`;
const tray = (s) => `<svg xmlns="http://www.w3.org/2000/svg" width="${s}" height="${s}" viewBox="-50 -50 100 100"><g transform="scale(1.02)">${ring(6)}</g></svg>`;
// the Mac menu bar wants a black-on-clear template; the ring is cut out of
// the planet with a hairline gap so the two still read as separate shapes
const mac = (s) => `<svg xmlns="http://www.w3.org/2000/svg" width="${s}" height="${s}" viewBox="-50 -50 100 100">
  <mask id="m"><rect x="-50" y="-50" width="100" height="100" fill="#fff"/><ellipse rx="46" ry="11" transform="rotate(-16)" fill="none" stroke="#000" stroke-width="16" clip-path="url(#f)"/></mask>
  <clipPath id="f"><rect x="-80" y="0" width="160" height="80" transform="rotate(-16)"/></clipPath>
  <ellipse rx="46" ry="11" transform="rotate(-16)" fill="none" stroke="#000" stroke-width="7"/>
  <circle r="25" fill="#000" mask="url(#m)"/>
  <ellipse rx="46" ry="11" transform="rotate(-16)" fill="none" stroke="#000" stroke-width="7" clip-path="url(#f)"/></svg>`;
const jobs = [
  ['icon.png', bare, 1024], ['128x128@2x.png', bare, 256], ['128x128.png', bare, 128], ['32x32.png', bare, 32],
  ['tray-win.png', tray, 64], ['tray.png', tray, 64], ['tray-mac-template.png', mac, 22], ['tray-mac-template@2x.png', mac, 44], ['app-icon.png', bare, 256],
  ...[16, 24, 32, 48, 64, 256].map((s) => [`ico-${s}.png`, bare, s]),
];
(async () => {
  const b = await puppeteer.launch({ executablePath: 'C:/Program Files/Google/Chrome/Application/chrome.exe', headless: 'new' });
  const p = await b.newPage();
  for (const [name, fn, s] of jobs) {
    await p.setViewport({ width: s, height: s, deviceScaleFactor: 1 });
    await p.setContent(`<html><body style="margin:0;background:transparent">${fn(s)}</body></html>`);
    await p.screenshot({ path: `${OUT}/${name}`, omitBackground: true, clip: { x: 0, y: 0, width: s, height: s } });
  }
  await b.close();
  console.log('ok');
})();
