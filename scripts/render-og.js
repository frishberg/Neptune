// The share card (og:image), 1200x630, drawn in the site's own fonts.
const puppeteer = require('puppeteer-core');
// the website folder, found from this script (scripts/ -> neptune/ -> repo)
const W = require('path').join(__dirname, '..', '..', 'website').split('\\').join('/');
const html = `<!doctype html><html><head><style>
@font-face { font-family: "Inter"; font-weight: 100 900; src: url("/fonts/inter-latin.woff2"); }
@font-face { font-family: "Space Grotesk"; font-weight: 700; src: url("/fonts/space-grotesk-700.woff2"); }
body { margin: 0; width: 1200px; height: 630px; background: #0e0e13; color: #f3f0e8; font-family: Inter; overflow: hidden; position: relative; }
.l { position: absolute; left: 84px; top: 84px; display: flex; align-items: center; gap: 14px; font: 700 30px "Space Grotesk"; }
h1 { position: absolute; left: 84px; top: 196px; margin: 0; font: 700 86px/1 "Space Grotesk"; letter-spacing: -.03em; }
h1 span { color: #7fa2ff; display: block; margin-top: 8px; }
p { position: absolute; left: 86px; top: 398px; margin: 0; font-size: 27px; color: rgba(243,240,232,.66); max-width: 640px; line-height: 1.4; }
.chips { position: absolute; left: 86px; bottom: 66px; display: flex; gap: 12px; }
.chips i { font-style: normal; padding: 10px 18px; border-radius: 999px; background: #1e1e25; box-shadow: inset 0 0 0 1px rgba(255,255,255,.09); font-size: 20px; font-weight: 600; }
svg.p { position: absolute; right: -90px; top: 40px; width: 560px; height: 560px; overflow: visible; }
</style></head><body>
<svg class="p" viewBox="-50 -50 100 100"><clipPath id="f"><rect x="-80" y="0" width="160" height="80" transform="rotate(-16)"/></clipPath>
<g fill="#fff"><circle cx="-44" cy="-36" r=".45"/><circle cx="30" cy="-46" r=".35"/><circle cx="-30" cy="44" r=".3"/><circle cx="46" cy="30" r=".4"/></g>
<ellipse rx="46" ry="11" transform="rotate(-16)" fill="none" stroke="#7fa2ff" stroke-width=".9"/><circle r="25" fill="#2f5fe0"/>
<ellipse rx="46" ry="11" transform="rotate(-16)" fill="none" stroke="#7fa2ff" stroke-width=".9" clip-path="url(#f)"/></svg>
<div class="l"><svg width="44" height="44" viewBox="-50 -50 100 100" style="overflow:visible"><ellipse rx="46" ry="11" transform="rotate(-16)" fill="none" stroke="#7fa2ff" stroke-width="5"/><circle r="25" fill="#2f5fe0"/><ellipse rx="46" ry="11" transform="rotate(-16)" fill="none" stroke="#7fa2ff" stroke-width="5" clip-path="url(#f)"/></svg>Neptune</div>
<h1>Talk. Let go.<span>It's typed.</span></h1>
<p>Free, open-source voice typing for Windows. Runs locally. Nothing leaves your laptop.</p>
<div class="chips"><i>Free</i><i>Open source</i><i>Runs locally</i></div>
</body></html>`;
(async () => {
  const b = await puppeteer.launch({ executablePath: 'C:/Program Files/Google/Chrome/Application/chrome.exe', headless: 'new', args: ['--allow-file-access-from-files'] });
  const p = await b.newPage();
  await p.setViewport({ width: 1200, height: 630 });
  require('fs').writeFileSync(`${W}/_og.html`, html);
  await p.goto('http://127.0.0.1:4173/_og.html', { waitUntil: 'networkidle0' });
  await p.evaluate(() => document.fonts.ready);
  await p.screenshot({ path: `${W}/img/og.png` });
  await b.close(); require('fs').unlinkSync(`${W}/_og.html`); console.log('ok');
})();
