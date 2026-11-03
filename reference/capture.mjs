import { chromium, webkit } from 'playwright';
import fs from 'node:fs';
import path from 'node:path';

const PAGES = {
  'gfm.md': { url: 'https://github.com/qu8n/gfm-test/blob/main/README.md', body: 'article.markdown-body', origin: 'https://github.com' },
};
const args = Object.fromEntries(process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith('--') ? [...acc, [a.slice(2), all[i + 1]]] : acc), []));
const engineName = args.engine || 'chromium';
const file = args.file || 'gfm.md';
const width = Number(args.width || 898);
const scheme = args.scheme || 'dark';
const bg = args.bg || (scheme === 'dark' ? '#0d1117' : '#ffffff');
const out = args.out || 'renders';
const freeze = args.freeze !== 'false';
fs.mkdirSync(out, { recursive: true });

const source = args.url ? { ...PAGES[file], url: args.url } : PAGES[file];
if (!PAGES[file]) throw new Error(`no reference page for ${file}; known: ${Object.keys(PAGES).join(', ')}`);
const sel = source.body;
const engine = engineName === 'webkit' ? webkit : chromium;
const browser = await engine.launch();

async function open(viewportWidth) {
  const ctx = await browser.newContext({ colorScheme: scheme, deviceScaleFactor: 2, viewport: { width: viewportWidth, height: 1000 } });
  const page = await ctx.newPage();
  await page.goto(source.url, { waitUntil: 'load', timeout: 60000 });
  await page.waitForSelector(sel, { timeout: 30000 });
  const width = () => page.$eval(sel, (el) => el.getBoundingClientRect().width);
  let w = await width();
  for (let i = 0; i < 20; i++) {
    await page.waitForTimeout(500);
    const next = await width();
    if (next === w && i >= 3) break;
    w = next;
  }
  return { ctx, page, w };
}

let viewport = Number(args.viewport || 1280), session = await open(viewport);
for (let i = 0; i < 6 && session.w !== width; i++) {
  await session.ctx.close();
  viewport += Math.round(width - session.w);
  session = await open(viewport);
}
if (session.w !== width) throw new Error(`no viewport gives .markdown-body width ${width}; closest ${session.w} at ${viewport}`);
const { page } = session;
const body = page.locator(sel);

await page.evaluate(() => document.fonts.ready);
const deadline = Date.now() + 30000;
for (;;) {
  const state = await body.evaluate((el) => {
    const maths = [...el.querySelectorAll('math-renderer')];
    const mathDone = maths.every((m) => m.querySelector('math, mjx-container, .flash-error'));
    const imgsDone = [...el.querySelectorAll('img')].every((i) => i.complete);
    const sections = el.querySelectorAll('section[data-type="mermaid"]').length;
    const iframes = el.querySelectorAll('section[data-type="mermaid"] iframe').length;
    return { mathDone, imgsDone, sections, iframes };
  });
  const frames = page.frames().filter((f) => f.url().includes('viewscreen'));
  let svgs = 0;
  for (const f of frames) svgs += await f.$$eval('svg#diagram, .mermaid svg', (s) => s.length).catch(() => 0);
  if (state.mathDone && state.imgsDone && state.iframes === state.sections && svgs >= state.sections) break;
  if (Date.now() > deadline) throw new Error(`render not ready: ${JSON.stringify({ ...state, svgs })}`);
  await page.waitForTimeout(250);
}
await page.waitForTimeout(1500);

const skip = scheme === 'dark' ? /light|high_contrast|primer-react-brand|marketing-/ : /dark|high_contrast|primer-react-brand|marketing-/;
const css = await page.evaluate(async (skip) => {
  const parts = [];
  for (const node of document.querySelectorAll('link[rel="stylesheet"], style')) {
    if (node.tagName === 'STYLE') { parts.push(node.textContent); continue; }
    if (!node.href || !node.sheet || new RegExp(skip).test(node.href) || node.media === 'print') continue;
    parts.push(await (await fetch(node.href)).text());
  }
  return parts.join('\n');
}, skip.source);
await page.addStyleTag({ content: `*:has(${sel}) { background: ${bg} !important; background-color: ${bg} !important; }` });
const top = await body.evaluate((el) => el.getBoundingClientRect().top + window.scrollY);
const nudge = Math.ceil(top) - top;
if (nudge > 0) {
  const mt = await body.evaluate((el) => parseFloat(getComputedStyle(el).marginTop) || 0);
  await page.addStyleTag({ content: `${sel} { margin-top: ${mt + nudge}px !important; }` });
  await page.waitForTimeout(500);
}
const finalTop = await body.evaluate((el) => el.getBoundingClientRect().top + window.scrollY);
await page.mouse.move(0, 0);
await page.waitForTimeout(300);

const stem = path.join(out, `${file}.${engineName}.w${width}${scheme === 'dark' ? '' : '.light'}`);
await body.screenshot({ path: `${stem}.png`, animations: 'disabled' });

const geometry = await body.evaluate((el) => {
  const top = el.getBoundingClientRect().top;
  return [...el.children].map((c) => {
    const r = c.getBoundingClientRect();
    return { tag: c.tagName.toLowerCase(), cls: c.className, y: r.top - top, h: r.height, text: (c.textContent || '').trim().slice(0, 60) };
  });
});
fs.writeFileSync(`${stem}.geometry.json`, JSON.stringify({ viewport, width, geometry }, null, 1));

if (freeze) {
  const dom = await body.evaluate((el) => el.outerHTML);
  fs.writeFileSync(`${stem}.dom.html`, dom);
  const iframeShots = [];
  const iframes = await body.locator('iframe').all();
  for (const [i, f] of iframes.entries()) {
    const box = await f.boundingBox();
    const buf = await f.screenshot({ animations: 'disabled' });
    iframeShots.push({ w: box.width, h: box.height, png: buf.toString('base64') });
    const frame = await (await f.elementHandle()).contentFrame();
    if (frame) fs.writeFileSync(`${stem}.iframe${i}.html`, await frame.content());
  }
  const chain = await body.evaluate((el) => {
    const out = [];
    for (let p = el.parentElement; p && p !== document.body; p = p.parentElement) {
      out.unshift({ tag: p.tagName.toLowerCase(), attrs: [...p.attributes].filter((a) => a.name !== 'style').map((a) => [a.name, a.value]) });
    }
    return {
      html: [...document.documentElement.attributes].map((a) => [a.name, a.value]),
      body: [...document.body.attributes].filter((a) => a.name !== 'style').map((a) => [a.name, a.value]),
      chain: out,
    };
  });
  const esc = (v) => v.replace(/&/g, '&amp;').replace(/"/g, '&quot;');
  const attrs = (list) => list.map(([k, v]) => ` ${k}="${esc(v)}"`).join('');
  let inner = dom.replace(/ (src|href)="\/(?!\/)/g, ` $1="${source.origin}/`);
  let n = 0;
  inner = inner.replace(/<iframe[\s\S]*?<\/iframe>/g, () => {
    const s = iframeShots[n++];
    return s ? `<img alt="" src="data:image/png;base64,${s.png}" style="display:block;width:${s.w}px;height:${s.h}px">` : '';
  });
  for (let i = chain.chain.length - 1; i >= 0; i--) {
    const c = chain.chain[i];
    inner = `<${c.tag}${attrs(c.attrs)} style="margin:0!important;padding:0!important;border:0!important;width:${width}px!important;max-width:none!important;background:${bg}!important">${inner}</${c.tag}>`;
  }
  const html = `<!DOCTYPE html><html${attrs(chain.html)}><head><meta charset="utf-8"><style>${css}</style><style>html,body{margin:0!important;padding:0!important;background:${bg}!important}body{width:${width}px!important;min-width:0!important}</style></head><body${attrs(chain.body)}>${inner}</body></html>`;
  fs.writeFileSync(`${stem}.frozen.html`, html);
}
console.log(JSON.stringify({ file, scheme, engine: engineName, viewport, width, top, nudge, finalTop, png: `${stem}.png` }));
await browser.close();
