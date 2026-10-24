import { webkit } from 'playwright';
import fs from 'node:fs';
import path from 'node:path';

const [refPath, mdPath, scheme = 'dark', outDir = 'scratch/compare'] = process.argv.slice(2);
fs.mkdirSync(outDir, { recursive: true });

const PROBES = [
  'article.markdown-body', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', '.markdown-heading', '.anchor', '.anchor .octicon',
  'p', 'a[href^="http"]', 'p code', 'strong', 'del', 'kbd', 'sup', 'sub', 'mark', 'ins',
  'blockquote', 'ul', 'ol', 'li', 'ul ul', '.task-list-item', 'input.task-list-item-checkbox', 'input.task-list-item-checkbox:checked',
  'hr', 'table', 'th', 'td', 'tr:nth-child(2n)', 'p img', 'details:not(.details-overlay)', 'details:not(.details-overlay) > summary',
  '.highlight', '.highlight pre', 'pre', 'pre code', '.snippet-clipboard-content', '.zeroclipboard-container', 'clipboard-copy', 'clipboard-copy svg',
  '.markdown-alert', '.markdown-alert-note', '.markdown-alert-title', '.markdown-alert-warning',
  'math-renderer.js-inline-math', 'math-renderer.js-display-math', 'math',
  'section[data-type="mermaid"]', '.render-container', '.js-render-block-actions', 'iframe, section[data-type="mermaid"] svg',
  '.footnotes', '.footnotes ol', '.footnotes li', '[data-footnote-ref]', '.data-footnote-backref', 'dl', 'dt', 'dd',
];
const PROPS = ['display', 'position', 'width', 'height', 'margin', 'padding', 'border', 'border-radius', 'color', 'background-color', 'background-image',
  'font-family', 'font-size', 'font-weight', 'line-height', 'letter-spacing', 'text-decoration', 'vertical-align', 'opacity', 'visibility', 'box-shadow', 'appearance', 'list-style-type', 'white-space', 'top', 'right'];

const browser = await webkit.launch();
async function measure(file, contentWidth) {
  const page = await browser.newPage({ colorScheme: scheme, deviceScaleFactor: 2, viewport: { width: 1400, height: 1000 } });
  await page.goto('file://' + path.resolve(file), { waitUntil: 'load' });
  const fit = async () => {
    const { w, pad, vw } = await page.$eval('article.markdown-body', (a) => {
      const cs = getComputedStyle(a);
      return { w: a.getBoundingClientRect().width - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight), pad: 0, vw: innerWidth };
    });
    return { w, vw };
  };
  if (contentWidth) {
    for (let i = 0; i < 6; i++) {
      const { w, vw } = await fit();
      if (Math.abs(w - contentWidth) < 0.5) break;
      await page.setViewportSize({ width: Math.round(vw + contentWidth - w), height: 1000 });
    }
    await page.reload({ waitUntil: 'load' });
  }
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(3000);
  const data = await page.evaluate(([probes, props]) => {
    const art = document.querySelector('article.markdown-body');
    const ar = art.getBoundingClientRect();
    const cs = getComputedStyle(art);
    const ox = ar.left + parseFloat(cs.paddingLeft), oy = ar.top + parseFloat(cs.paddingTop);
    const out = {};
    for (const p of probes) {
      let el;
      try { el = p === 'article.markdown-body' ? art : art.querySelector(p); } catch { el = null; }
      if (!el) { out[p] = null; continue; }
      const s = getComputedStyle(el);
      const r = el.getBoundingClientRect();
      const o = { box: [r.left - ox, r.top - oy, r.width, r.height].map((v) => Math.round(v * 10) / 10) };
      for (const k of props) o[k] = s.getPropertyValue(k);
      out[p] = o;
    }
    out.__blocks = [...art.children].map((c) => { const r = c.getBoundingClientRect(); return [c.tagName.toLowerCase() + (c.className ? '.' + String(c.className).split(' ')[0] : ''), Math.round((r.top - oy) * 10) / 10, Math.round(r.height * 10) / 10, (c.textContent || '').trim().slice(0, 30)]; });
    out.__height = art.getBoundingClientRect().height - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
    out.__contentWidth = ar.width - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
    return out;
  }, [PROBES, PROPS]);
  const name = path.basename(file).replace(/\.html$/, '');
  await page.locator('article.markdown-body').screenshot({ path: path.join(outDir, `${name}.${scheme}.png`), animations: 'disabled' });
  await page.close();
  return data;
}

const ref = await measure(refPath);
const md = await measure(mdPath, ref.__contentWidth);
await browser.close();

const blocks = [];
const n = Math.max(ref.__blocks.length, md.__blocks.length);
for (let i = 0; i < n; i++) {
  const a = ref.__blocks[i], b = md.__blocks[i];
  if (!a || !b || a[0] !== b[0] || a[1] !== b[1] || a[2] !== b[2]) blocks.push({ i, ref: a, mddog: b });
}
fs.writeFileSync(path.join(outDir, `blocks.${scheme}.json`), JSON.stringify(blocks, null, 1));
for (const d of blocks.slice(0, 12)) console.log(`block ${d.i}: ref=${JSON.stringify(d.ref)} mddog=${JSON.stringify(d.mddog)}`);
const diffs = [];
for (const p of ['__contentWidth', '__height', ...PROBES]) {
  const a = ref[p], b = md[p];
  if (typeof a !== 'object' || a === null || b === null) {
    if (JSON.stringify(a) !== JSON.stringify(b)) diffs.push({ probe: p, ref: a, mddog: b });
    continue;
  }
  const d = {};
  for (const k of Object.keys(a)) if (JSON.stringify(a[k]) !== JSON.stringify(b[k])) d[k] = { ref: a[k], mddog: b[k] };
  if (Object.keys(d).length) diffs.push({ probe: p, ...d });
}
fs.writeFileSync(path.join(outDir, `diff.${scheme}.json`), JSON.stringify(diffs, null, 1));
for (const d of diffs) {
  const { probe, ...rest } = d;
  console.log(`## ${probe}`);
  for (const [k, v] of Object.entries(rest)) console.log(`  ${k}: ref=${JSON.stringify(v?.ref ?? v)} mddog=${JSON.stringify(v?.mddog ?? '')}`);
}
