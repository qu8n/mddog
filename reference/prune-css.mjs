import { webkit } from 'playwright';
import postcss from 'postcss';
import selectorParser from 'postcss-selector-parser';
import fs from 'node:fs';

const args = process.argv.slice(2);
const keepArg = args.find((a) => a.startsWith('--keep='));
const dropArg = args.find((a) => a.startsWith('--drop='));
const always = keepArg ? new RegExp(keepArg.slice('--keep='.length)) : null;
const never = dropArg ? new RegExp(dropArg.slice('--drop='.length)) : null;
const [out, ...pages] = args.filter((a) => a !== keepArg && a !== dropArg);
const html = fs.readFileSync(pages[0], 'utf8');
const css = [...html.matchAll(/<style>([\s\S]*?)<\/style>/g)].map((m) => m[1]).filter((c) => !c.startsWith("html,body{margin:0!important;padding:0!important")).join('\n');
const root = postcss.parse(css);

const KEEP_PSEUDO = new Set([':not', ':is', ':where', ':has', ':first-child', ':last-child', ':nth-child', ':nth-of-type', ':first-of-type', ':last-of-type', ':only-child', ':empty', ':root', ':checked', ':disabled', ':nth-last-child', ':only-of-type']);
function testable(sel) {
  try {
    return selectorParser((s) => {
      s.walkPseudos((p) => {
        if (p.value.startsWith('::') || !KEEP_PSEUDO.has(p.value)) p.remove();
      });
    }).processSync(sel).trim() || '*';
  } catch {
    return null;
  }
}

const selectors = new Set();
root.walkRules((r) => {
  if (r.parent && r.parent.type === 'atrule' && /keyframes/i.test(r.parent.name)) return;
  for (const s of r.selectors) { const t = testable(s); if (t) selectors.add(t); }
});
const list = [...selectors];
const browser = await webkit.launch();
const matched = new Set();
for (const p of pages) {
  const page = await browser.newPage({ colorScheme: 'dark' });
  await page.goto('file://' + fs.realpathSync(p), { waitUntil: 'load' });
  const hits = await page.evaluate((list) => list.filter((s) => { try { return !!document.querySelector(s); } catch { return false; } }), list);
  hits.forEach((h) => matched.add(h));
  await page.close();
}
await browser.close();

const usedVars = new Set();
root.walkRules((r) => {
  if (r.parent && r.parent.type === 'atrule' && /keyframes/i.test(r.parent.name)) return;
  const keep = r.selectors.filter((s) => { const t = testable(s); return !(never && never.test(s)) && ((always && always.test(s)) || (t && matched.has(t))); });
  if (!keep.length) { r.remove(); return; }
  r.selectors = keep;
});
root.walkAtRules((a) => {
  if (/^(media|supports|layer|container)$/i.test(a.name) && (!a.nodes || !a.nodes.length)) a.remove();
});
root.walkAtRules((a) => {
  if (/^(media|supports|layer|container)$/i.test(a.name) && (!a.nodes || !a.nodes.length)) a.remove();
});
const seen = new Set();
root.each((n) => {
  const t = n.toString();
  if (seen.has(t)) n.remove();
  else seen.add(t);
});
const text = root.toString();
fs.writeFileSync(out, text);
console.log(JSON.stringify({ inBytes: css.length, outBytes: text.length, selectors: list.length, matched: matched.size }));
