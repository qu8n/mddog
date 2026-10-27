// Builds assets/github.css and the GitHub themes' colors from MIT packages, so mddog ships none of github.com's own files:
// github-markdown-css for the Markdown rules, @primer/css for the buttons, tooltips and utilities around them, and @primer/primitives for the colors and sizes.
// github-overrides.css, mddog's own rules for what those leave out, goes last. Run it from reference/ after `cargo build --release`: it prunes the rules against the pages mdd builds for the fixtures, plus the kitchen sink below.
import fs from 'node:fs';
import postcss from 'postcss';
import { execFileSync, spawn } from 'node:child_process';

const sink = `
<h1>h1 <code>code</code></h1><h2>h2</h2><h3>h3</h3><h4>h4</h4><h5>h5</h5><h6>h6</h6>
<div class="markdown-heading" dir="auto"><h1 class="heading-element" dir="auto">x</h1><a class="anchor" href="#x"><svg class="octicon octicon-link"></svg></a></div>
<div class="markdown-heading" dir="auto"><h6 class="heading-element" dir="auto">x</h6><a class="anchor" href="#x"><svg class="octicon octicon-link"></svg></a></div>
<p dir="auto"><b>b</b><i>i</i><strong>s</strong><em>e</em><tt>t</tt><ins>i</ins><del>d</del><s>s</s><strike>s</strike><sup>1</sup><sub>2</sub><kbd>k</kbd><samp>s</samp><var>v</var><q>q</q><mark>m</mark><span>s</span><br><code class="notranslate">c</code><a href="#x">a</a><ruby>r<rp>(</rp><rt>t</rt><rp>)</rp></ruby></p>
<p align="center"><img src="x.png" align="left" width="10"><img src="x.png" align="right"><img class="emoji" src="x.png" align="absmiddle"></p>
<themed-picture data-catalyst-inline="true"><picture><source media="(prefers-color-scheme: dark)" srcset="x.png"><img src="x.png"></picture></themed-picture>
<video src="x.mp4"></video>
<blockquote><p>q</p><blockquote><p>q</p></blockquote></blockquote>
<ul dir="auto"><li>a<ul><li>b<ul><li>c</li></ul></li></ul></li></ul>
<ol dir="auto" start="3" type="a"><li>a<ol><li>b<ol><li>c</li></ol></li></ol></li></ol>
<ul class="contains-task-list"><li class="task-list-item"><input type="checkbox" class="task-list-item-checkbox" disabled checked> t</li></ul>
<dl><dt>t</dt><dd>d</dd></dl>
<hr>
<details open><summary>s</summary><p>d</p></details>
<markdown-accessiblity-table data-catalyst=""><table><caption>c</caption><thead><tr><th align="left">h</th></tr></thead><tbody><tr><td align="center"><code>c</code></td></tr></tbody><tfoot><tr><td>f</td></tr></tfoot></table></markdown-accessiblity-table>
<table><tbody><tr><th>title</th><td>v</td></tr></tbody></table>
<div class="highlight highlight-source-js notranslate position-relative overflow-auto" dir="auto"><pre><span class="pl-c">c</span></pre><div class="zeroclipboard-container"><clipboard-copy aria-label="Copy" class="ClipboardButton btn btn-invisible js-clipboard-copy m-2 p-0 d-flex flex-justify-center flex-items-center" data-copy-feedback="Copied!" data-tooltip-direction="w" value="c" tabindex="0" role="button"><svg class="octicon octicon-copy js-clipboard-copy-icon"></svg><svg class="octicon octicon-check js-clipboard-check-icon color-fg-success d-none"></svg></clipboard-copy></div></div>
<div class="snippet-clipboard-content notranslate position-relative overflow-auto"><pre class="notranslate"><code>plain</code></pre><div class="zeroclipboard-container"><clipboard-copy class="ClipboardButton btn btn-invisible js-clipboard-copy m-2 p-0 d-flex flex-justify-center flex-items-center tooltipped-no-delay tooltipped tooltipped-w"><svg class="octicon octicon-copy js-clipboard-copy-icon d-none"></svg><svg class="octicon octicon-check js-clipboard-check-icon color-fg-success"></svg></clipboard-copy></div></div>
<pre><code>plain</code></pre>
<div class="markdown-alert markdown-alert-note" dir="auto"><p class="markdown-alert-title" dir="auto"><svg class="octicon octicon-info mr-2"></svg>Note</p><p>x</p></div>
<div class="markdown-alert markdown-alert-tip" dir="auto"><p class="markdown-alert-title" dir="auto"><svg class="octicon octicon-light-bulb mr-2"></svg>Tip</p></div>
<div class="markdown-alert markdown-alert-important" dir="auto"><p class="markdown-alert-title" dir="auto"><svg class="octicon octicon-report mr-2"></svg>Important</p></div>
<div class="markdown-alert markdown-alert-warning" dir="auto"><p class="markdown-alert-title" dir="auto"><svg class="octicon octicon-alert mr-2"></svg>Warning</p></div>
<div class="markdown-alert markdown-alert-caution" dir="auto"><p class="markdown-alert-title" dir="auto"><svg class="octicon octicon-stop mr-2"></svg>Caution</p></div>
<p><math-renderer class="js-inline-math" style="display: inline-block"><math><mi>x</mi></math></math-renderer><math-renderer class="js-display-math" style="display: block"><div class="flash flash-error">e</div><pre>raw</pre></math-renderer></p>
<section class="js-render-needs-enrichment render-needs-enrichment position-relative" data-type="mermaid"><div class="js-render-enrichment-target" dir="auto"><div class="render-plaintext-hidden" dir="auto"><pre lang="mermaid">x</pre></div><div class="js-render-block-actions position-absolute top-0 pr-2 right-0 d-flex flex-justify-end flex-items-center"><details class="details-reset details-overlay details-overlay-dark" style="display: contents"><summary role="button" class="btn my-2 mr-2 p-0 d-inline-flex"><svg class="octicon m-2"></svg></summary></details><clipboard-copy class="btn my-2 js-clipboard-copy p-0 d-inline-flex tooltipped-no-delay tooltipped tooltipped-s" aria-label="Copied!"><svg class="octicon octicon-copy js-clipboard-copy-icon m-2"></svg><svg class="octicon octicon-check js-clipboard-check-icon color-fg-success d-none m-2"></svg></clipboard-copy></div><div class="render-container color-bg-transparent js-render-target p-0 is-render-automatic is-render-requested is-render-ready" data-type="mermaid"><iframe class="render-viewer"></iframe></div></div></section>
<div class="render-container is-render-automatic is-render-requested is-render-failed"><iframe class="render-viewer"></iframe></div>
<section data-footnotes="" class="footnotes"><h2 id="footnote-label" class="sr-only" dir="auto">Footnotes</h2><ol dir="auto"><li id="user-content-fn-1"><p dir="auto">f <a href="#x" data-footnote-backref="" class="data-footnote-backref">↩</a></p></li></ol></section>
<p><sup><a href="#user-content-fn-1" id="user-content-fnref-1" data-footnote-ref="" aria-describedby="footnote-label">1</a></sup></p>
<table class="mddog-jsx"><thead><tr><th><code>&lt;X&gt;</code></th></tr></thead><tbody><tr><td><p>x</p></td></tr></tbody></table>
<div class="mddog-image-missing"><span>alt</span></div>
<p><a href="x.png#gh-light-mode-only"><img src="x.png"></a><a href="x.png#gh-dark-mode-only"><img src="x.png"></a><a name="n">t</a></p>
`;


const mod = (p) => fs.readFileSync(`node_modules/${p}`, 'utf8');
const license = (pkg) => `/*! ${pkg} | MIT License | ${mod(`${pkg}/package.json`).match(/"homepage":\s*"([^"]+)"/)?.[1] ?? pkg} */\n`;

// The page mdd builds for a file, with its scripts and CSS taken out, so pruning sees mddog's markup.
async function page(md) {
  const mdd = spawn('../target/release/mdd', ['--headless', '120', md], { env: { ...process.env, MDDOG_NO_SPARE: '1' } });
  const line = await new Promise((ok) => mdd.stdout.once('data', (d) => ok(d.toString().split('\n')[0])));
  const dir = JSON.parse(line).tiles[0].file.replace(/\/[^/]+$/, '');
  const html = fs.readFileSync(`${dir}/page.html`, 'utf8');
  mdd.kill();
  return html.replace(/<script[\s\S]*?<\/script>/g, '').replace(/<style>[\s\S]*?<\/style>/g, '');
}

// Custom properties come from the themes and the size sheet below, so the rule sheets keep none.
function rules(css) {
  const root = postcss.parse(css);
  root.walkComments((c) => c.remove());
  root.walkDecls((d) => d.prop.startsWith('--') && d.remove());
  root.walkAtRules('media', (a) => /prefers-color-scheme/.test(a.params) && a.remove());
  root.walkRules((r) => !r.nodes.length && r.remove());
  root.walkAtRules((a) => a.nodes && !a.nodes.length && a.remove());
  return root.toString();
}

// One :root block of every custom property the files declare, later files winning.
function vars(...files) {
  const out = new Map();
  for (const f of files) postcss.parse(mod(f)).walkDecls((d) => d.prop.startsWith('--') && d.parent.type === 'rule' && d.parent.parent.type === 'root' && out.set(d.prop, d.value));
  return out;
}
const block = (m) => `:root{${[...m].map(([k, v]) => `${k}:${v}`).join(';')}}`;

// Each file parses on its own: joined, a sheet's end can swallow the next one's start.
// github.com scopes its alert and syntax rules without .markdown-body, so a theme's own rule for them wins by order. These keep that.
const unscoped = (css) => {
  const root = postcss.parse(css);
  root.walkRules((r) => { r.selectors = r.selectors.map((x) => x.replace(/^\.markdown-body (?=\.(pl-|markdown-alert))/, '')); });
  return root.toString();
};
const src = [rules(mod('@primer/css/dist/core.css')), rules(mod('@primer/css/dist/tooltips.css')), unscoped(rules(mod('github-markdown-css/github-markdown.css')))].join('\n');
const marker = '<article class="markdown-body entry-content container-lg" itemprop="text">';
fs.mkdirSync('scratch', { recursive: true });
const pages = [];
for (const md of ['gfm.md']) {
  let html = (await page(`../fixtures/${md}`)).replace('<head>', () => `<head><style>${src}</style>`);
  if (md === 'gfm.md') html = html.replace(marker, () => marker + sink);
  fs.writeFileSync(`scratch/${md}.html`, html);
  pages.push(`scratch/${md}.html`);
}
execFileSync('node', ['prune-css.mjs', '--keep=^\\.pl-[a-z]+[0-9]?(?![\\w-])', 'scratch/pruned.css', ...pages], { stdio: 'inherit' });

// Sizes, spacing and fonts are the same in both modes, so they sit in github.css. Colors go to each theme, under its mddog block.
const sizes = vars(...['base/size/size', 'functional/size/border', 'functional/size/radius', 'functional/size/size', 'functional/spacing/space', 'base/typography/typography', 'functional/typography/typography'].map((f) => `@primer/primitives/dist/css/${f}.css`));
const notice = license('github-markdown-css') + license('@primer/css') + license('@primer/primitives');
fs.writeFileSync('../assets/github.css', `${notice}${block(sizes)}\n${fs.readFileSync('scratch/pruned.css', 'utf8')}\n${fs.readFileSync('github-overrides.css', 'utf8')}`);
const primer = '/* Primer: rebuilt by reference/build-css.mjs */\n';
for (const mode of ['dark', 'light']) {
  const theme = `../assets/themes/github-${mode}.css`;
  const head = fs.readFileSync(theme, 'utf8').split(primer)[0];
  fs.writeFileSync(theme, `${head}${primer}${license('@primer/primitives')}${block(vars(`@primer/primitives/dist/css/functional/themes/${mode}.css`))}\n`);
}
