window.mddog = (() => {
  const unsupported = ['DeclareMathOperator', 'DeclarePairedDelimiters', 'renewtagform', 'newtagform', 'colorbox', 'fcolorbox', 'hphantom', 'vphantom', 'phantom', 'operatorname', 'Newextarrow', 'definecolor', 'mathchoice', 'unicode', 'mmlToken'];
  const limits = { em: 2, pt: 20, pc: 0.75, ex: 2.5 };
  const units = Object.keys(limits).join('|');
  const num = '-?\\d+.?\\d*?';
  const lengthMacro = RegExp(`(hskip|hspace|raise|mspace|mskip|kern|lower|above|mkern|moveleft|moveright|rule|Rule|space|Space)({${num}(${units})})+`, 'g');
  const state = { math: false, mermaid: false, sizes: {}, images: {} };

  function decode(text) {
    const d = document.implementation.createHTMLDocument('');
    d.open();
    d.write(`<html><body>${text}</body></html>`);
    d.close();
    return d.body.textContent || '';
  }

  function clampLengths(text) {
    return text.replace(lengthMacro, (whole) => whole.replace(RegExp(`(${num})(${units})`, 'g'), (m, n, u) => `${Math.min(parseFloat(n), limits[u])}${u}`));
  }

  function flash(el, msg) {
    const raw = el.textContent;
    el.innerHTML = `<div class="flash flash-error">${msg}</div>`;
    const pre = document.createElement('pre');
    pre.textContent = raw;
    el.appendChild(pre);
  }

  function alignColumns(el) {
    for (const table of el.querySelectorAll('mtable[columnalign]')) {
      const aligns = table.getAttribute('columnalign').trim().split(/\s+/);
      if (aligns.length < 2) continue;
      for (const row of table.querySelectorAll(':scope > mtr')) {
        row.querySelectorAll('mtd').forEach((cell, i) => {
          const a = aligns[i] ?? aligns[aligns.length - 1];
          if (['left', 'center', 'right'].includes(a)) cell.style.textAlign = a;
        });
      }
    }
  }

  function renderMath() {
    const MJ = window.MathJax;
    let total = 0;
    for (const el of document.querySelectorAll('math-renderer')) {
      const braces = el.textContent.split('{').length;
      total += braces;
      if (total > 2000 || braces > 1000) { flash(el, 'Unable to render expression.'); continue; }
      const bad = unsupported.filter((m) => el.textContent.includes(`\\${m}`) || el.textContent.includes(`\\$${m}`) || el.textContent.includes(`\\$\{${m}`));
      if (bad.length) { flash(el, `The following macros are not allowed: ${bad.join(', ')}`); continue; }
      el.textContent = clampLengths(el.textContent);
      // Strip only the delimiters, so math that ends in `\$` keeps its dollar.
      const src = decode(el.textContent).trim();
      const n = src.length >= 4 && src.startsWith('$$') && src.endsWith('$$') ? 2 : 1;
      const tex = src.slice(n, src.length - n);
      try {
        const mml = MJ.tex2mml(tex, { display: !el.classList.contains('js-inline-math') });
        const holder = document.createElement('div');
        holder.innerHTML = mml;
        const math = holder.firstElementChild;
        const err = math && math.querySelector('merror');
        if (!math || (err && /Misplaced &/.test(err.textContent))) { flash(el, 'Unable to render expression.'); continue; }
        el.replaceChildren(math);
        alignColumns(el);
      } catch (e) {
        flash(el, 'Unable to render expression.');
      }
    }
  }

  function mathConfig() {
    window.MathJax = {
      loader: { load: ['ui/safe'] },
      tex: { inlineMath: [['$', '$']], displayMath: [['$$', '$$']], packages: { '[-]': ['noerrors', 'bbox', 'html', 'require', 'newcommand', 'action', 'colortbl'] }, maxMacros: 1000 },
      startup: {
        typeset: false,
        ready() {
          const MJ = window.MathJax;
          MJ.startup.defaultReady();
          for (const jax of MJ.startup.document?.inputJax ?? []) {
            jax.preFilters.add(async ({ math }) => {
              for (const m of unsupported) if (math.math.includes(`\\${m}`)) math.math = `\\text{${m} macro is not supported}`;
              return true;
            }, -1000);
          }
          const safe = MJ.startup.document?.safe;
          if (safe) {
            for (const a of ['fontfamily', 'fontstyle', 'fontweight']) safe.filterAttributes.set(a, 'filterSemiColon');
            safe.filterMethods.filterSemiColon = (_s, v) => v.split(/;/)[0];
          }
          MJ.startup.promise.then(() => { renderMath(); state.math = true; });
        },
      },
      options: { enableMenu: false, safeOptions: { allow: { URLs: 'none', classes: 'none', cssIDs: 'none', styles: 'safe' }, lengthMax: 2, safeStyles: {}, styleLengths: {} } },
    };
  }

  function key(code) {
    let h = 0x811c9dc5;
    for (let i = 0; i < code.length; i++) h = Math.imul(h ^ code.charCodeAt(i), 0x01000193);
    return (h >>> 0).toString(16);
  }

  // Sizes each skeleton as the engine will: the viewBox scaled down to the page width. A wrong guess only moves the
  // page when the diagram draws, as it would without one.
  function guess(sizes) {
    for (const el of document.querySelectorAll('mddog-mermaid')) {
      const size = sizes[key(el.dataset.code)];
      const box = el.parentElement;
      if (size) box.style.height = `${size[1] * Math.min(1, box.getBoundingClientRect().width / size[0])}px`;
    }
  }

  // Obsidian's reading view: plain mermaid with Obsidian's settings. Skeletons hold each box until startMermaid, so
  // loading mermaid (3 MB) and the first layout stay out of the first paint.
  // Mermaid lays out in a hidden frame whose CSP allows only data: images and fonts, because a diagram can name
  // images that the page's media checks don't see. Each SVG then shows as an <img>, which loads nothing either.
  // All diagrams replace their skeletons together, so the layout changes once.
  function renderObsidian(src) {
    const views = [...document.querySelectorAll('mddog-mermaid')].map((el) => {
      const container = el.parentElement;
      const skeleton = document.createElement('div');
      skeleton.className = 'mddog-skeleton';
      skeleton.append(...[0, 1, 2].map(() => document.createElement('span')));
      container.classList.add('mddog-loading');
      container.style.height = '200px';
      container.append(skeleton);
      return { el, container, skeleton, code: el.dataset.code };
    });
    const within = (p, ms) => Promise.race([p, new Promise((_, no) => setTimeout(() => no(new Error('Timed out drawing the diagram')), ms))]);
    const draw = async (mermaid) => {
      mermaid.initialize({
        startOnLoad: false,
        securityLevel: 'strict',
        themeVariables: { fontFamily: getComputedStyle(document.querySelector('.markdown-body') ?? document.body).fontFamily },
        flowchart: { useMaxWidth: false }, sequence: { useMaxWidth: false }, gitGraph: { useMaxWidth: false }, er: { useMaxWidth: false },
        gantt: { useMaxWidth: true }, journey: { useMaxWidth: true }, class: { useMaxWidth: true }, state: { useMaxWidth: true }, pie: { useMaxWidth: true },
      });
      for (const [i, v] of views.entries()) {
        try {
          v.svg = (await within(mermaid.render(`mddog-mermaid-${i}`, v.code), 4000)).svg;
        } catch (e) {
          v.error = String(e?.message ?? e);
        }
      }
    };
    // Shows every diagram, or its error, at once (also for one that never drew: mermaid failed to load, or the 45 s
    // cutoff came first). MERMAID_DONE waits for the images to decode.
    let revealed = false;
    const reveal = async () => {
      if (revealed) return;
      revealed = true;
      const imgs = [];
      for (const v of views) {
        const box = document.createElement('div');
        box.className = 'mermaid mddog-obsidian';
        const svg = v.svg && new DOMParser().parseFromString(v.svg, 'image/svg+xml').querySelector('svg');
        const vb = svg?.viewBox.baseVal;
        if (vb?.width) {
          // useMaxWidth true draws at the page width up to the diagram's own, false at its own size, as Obsidian shows it.
          const fits = svg.getAttribute('width') === '100%';
          svg.setAttribute('width', vb.width);
          svg.setAttribute('height', vb.height);
          const img = document.createElement('img');
          img.alt = 'Mermaid diagram';
          img.src = `data:image/svg+xml,${encodeURIComponent(new XMLSerializer().serializeToString(svg))}`;
          img.style.cssText = fits ? `width:100%;max-width:${vb.width}px;height:auto` : `width:${vb.width}px;height:${vb.height}px;max-width:none`;
          box.append(img);
          imgs.push(img);
          // A diagram at its own size doesn't shrink, so its width is stored as 0 and guess keeps its height.
          state.sizes[key(v.code)] = [fits ? vb.width : 0, vb.height];
        } else {
          const msg = document.createElement('div');
          msg.className = 'flash flash-error';
          msg.textContent = v.error || 'Unable to render rich display';
          box.append(msg);
        }
        v.el.replaceWith(box);
        v.skeleton.remove();
        v.container.classList.remove('mddog-loading');
        v.container.style.height = '';
      }
      await Promise.all(imgs.map((i) => i.decode().catch(() => {})));
      state.mermaid = true;
    };
    let started = false;
    const early = startMermaid.early;
    startMermaid = () => {
      if (started) return;
      started = true;
      const frame = document.createElement('iframe');
      frame.setAttribute('sandbox', 'allow-scripts allow-same-origin');
      frame.style.cssText = `position:absolute;left:0;top:0;border:0;visibility:hidden;width:${views[0]?.container.clientWidth ?? 800}px;height:10px`;
      const csp = "default-src 'none'; script-src file: 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; font-src data:";
      frame.srcdoc = `<!DOCTYPE html><html><head><meta http-equiv="Content-Security-Policy" content="${csp}"><script src="${src}"><\/script></head><body></body></html>`;
      const done = () => { frame.remove(); reveal(); };
      frame.addEventListener('load', () => { draw(frame.contentWindow.mermaid).catch(() => {}).finally(done); }, { once: true });
      document.body.append(frame);
      // Stops waiting before the viewer's 60 s LATE_LIMIT, however many diagrams are slow (each gets 4 s).
      setTimeout(done, 45000);
    };
    if (early) startMermaid();
  }

  // A start sent before DOMContentLoaded is kept and run once renderObsidian sets up.
  let startMermaid = () => { startMermaid.early = true; };

  function renderMermaid() {
    if (window.pele) {
      for (const el of document.querySelectorAll('mddog-mermaid')) {
        const box = document.createElement('div');
        el.replaceWith(box);
        // The page has no CSP, so an image URL in a diagram would skip the media and private-host checks.
        pele.mount(box, el.dataset.code, { images: false });
      }
    }
    state.mermaid = true;
  }

  return { state, mathConfig, renderMermaid, renderObsidian, guess, startMermaid: () => startMermaid() };
})();

window.mddogSearch = (() => {
  const BLOCK = 'p,li,td,th,h1,h2,h3,h4,h5,h6,pre,blockquote,div,section,table,dt,dd,summary,details,ul,ol,article,figure,math-renderer';
  const SKIP = '.render-plaintext-hidden,.sr-only,script,style,.markdown-heading > a.anchor';
  let ranges = [];
  let current = -1;

  function escapeRe(s) { return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'); }

  function rectsOf(r) {
    return [...r.getClientRects()].filter((b) => b.width > 0 && b.height > 0).map((b) => [b.left + scrollX, b.top + scrollY, b.width, b.height]);
  }

  function paint() {
    if (!window.CSS || !CSS.highlights) return;
    CSS.highlights.set('mddog-match', new Highlight(...ranges.filter((_, i) => i !== current)));
    if (current >= 0 && ranges[current]) CSS.highlights.set('mddog-current', new Highlight(ranges[current]));
    else CSS.highlights.delete('mddog-current');
  }

  function run(query, keep) {
    const root = document.querySelector('article.markdown-body');
    ranges = [];
    if (!query || !root) { current = -1; paint(); return JSON.stringify([]); }
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
      acceptNode: (n) => (n.parentElement && n.parentElement.closest(SKIP)) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT,
    });
    let text = '';
    const map = [];
    let lastBlock = null;
    for (let n = walker.nextNode(); n; n = walker.nextNode()) {
      const block = n.parentElement.closest(BLOCK);
      if (block !== lastBlock) { text += '\u0000'; lastBlock = block; }
      map.push([text.length, n]);
      text += n.data;
    }
    const locate = (off, end) => {
      let lo = 0, hi = map.length - 1;
      while (lo < hi) { const mid = (lo + hi + 1) >> 1; if (map[mid][0] <= off - (end ? 1 : 0)) lo = mid; else hi = mid - 1; }
      const [start, node] = map[lo];
      return [node, off - start];
    };
    const rects = [];
    const re = new RegExp(escapeRe(query).replace(/\s+/g, '\\s+'), /\p{Lu}/u.test(query) ? 'gu' : 'giu');
    for (let m = re.exec(text); m; m = re.exec(text)) {
      if (m[0].length === 0) { re.lastIndex++; continue; }
      if (m[0].includes('\u0000')) continue;
      const [sn, so] = locate(m.index, false);
      const [en, eo] = locate(m.index + m[0].length, true);
      const r = document.createRange();
      r.setStart(sn, so);
      r.setEnd(en, eo);
      const rs = rectsOf(r);
      if (rs.length) { ranges.push(r); rects.push(rs); }
    }
    current = ranges.length ? (keep >= 0 && keep < ranges.length ? keep : 0) : -1;
    // reveal can scroll a wide block sideways, which moves the rects inside it.
    const inWide = current >= 0 && mddogCode.reveal(ranges[current]);
    paint();
    return JSON.stringify(inWide ? ranges.map(rectsOf) : rects);
  }

  function setCurrent(i) {
    const before = current >= 0 && ranges[current] ? rectsOf(ranges[current]) : [];
    current = i;
    const held = current >= 0 && ranges[current] ? mddogCode.reveal(ranges[current]) : null;
    paint();
    const after = current >= 0 && ranges[current] ? rectsOf(ranges[current]) : [];
    return JSON.stringify(before.concat(after, held ? [held] : []));
  }

  function clear() {
    ranges = [];
    current = -1;
    paint();
  }

  return { run, setCurrent, clear };
})();

window.mddogMouse = (() => {
  const at = (x, y) => [Math.min(x - scrollX, innerWidth - 1), Math.min(y - scrollY, innerHeight - 1)];
  const caret = (x, y) => document.caretRangeFromPoint(...at(x, y));
  const rects = () => {
    const s = getSelection();
    return s.rangeCount ? [...s.getRangeAt(0).getClientRects()].map((b) => [b.left + scrollX, b.top + scrollY, b.width, b.height]) : [];
  };
  const change = (fn) => {
    const before = rects();
    fn();
    return JSON.stringify(before.concat(rects()));
  };

  const pick = (x, y) => document.elementFromPoint(...at(x, y));

  function link(x, y) {
    const a = pick(x, y)?.closest('a[href]');
    return a ? JSON.stringify([a.getAttribute('href'), a.href]) : '';
  }

  const box = (el) => {
    const r = el.getBoundingClientRect();
    return [r.left + scrollX, r.top + scrollY, r.width, r.height];
  };
  const area = (el) => box(el.closest('section, .highlight, .snippet-clipboard-content') || el);
  let copied = null;
  let hovered = null;

  // The tooltip shows while the button is hovered or just copied. Its text is the aria-label, so "Copied!" wins.
  function tip(b) {
    const on = copied?.el === b || hovered === b;
    b.classList.toggle('mddog-hover', hovered === b);
    b.classList.toggle('tooltipped', on);
    b.classList.toggle(`tooltipped-${b.dataset.tooltipDirection || 's'}`, on);
  }

  function copyFeedback(b, on) {
    b.querySelector('.js-clipboard-copy-icon')?.classList.toggle('d-none', on);
    b.querySelector('.js-clipboard-check-icon')?.classList.toggle('d-none', !on);
    tip(b);
  }

  function onText(x, y) {
    const n = caret(x, y)?.startContainer;
    if (n?.nodeType !== Node.TEXT_NODE || !n.data.trim()) return false;
    const r = document.createRange();
    r.selectNodeContents(n);
    const [vx, vy] = at(x, y);
    return [...r.getClientRects()].some((b) => vx >= b.left && vx <= b.right && vy >= b.top && vy <= b.bottom);
  }

  // Off-screen WebKit gets no real pointer, so the viewer reports it here. Returns the pointer shape and the areas to
  // redraw. Only code-block copy buttons get the hover look. `slack` widens the scroll bars as for a press.
  function hover(x, y, slack) {
    const el = x < 0 ? null : pick(x, y);
    const button = el?.closest('.zeroclipboard-container clipboard-copy') || null;
    const rects = [];
    if (button !== hovered) {
      const old = hovered;
      hovered = button;
      for (const b of [old, button]) {
        if (b?.isConnected) {
          tip(b);
          rects.push(area(b));
        }
      }
    }
    const shape = !el ? 'default' : el.closest('a[href], summary, button, [role=button]') || mddogCode.bar(x, y, slack) ? 'pointer' : onText(x, y) ? 'text' : 'default';
    return JSON.stringify([shape, rects]);
  }

  function resetCopy() {
    if (!copied) return '[]';
    const { el, label } = copied;
    copied = null;
    copyFeedback(el, false);
    el.setAttribute('aria-label', label);
    return JSON.stringify([area(el)]);
  }

  function press(x, y) {
    const hit = pick(x, y)?.closest('summary, button, [role=button]');
    if (!hit) return '';
    if (hit.matches('clipboard-copy')) {
      const rects = copied?.el === hit ? [] : JSON.parse(resetCopy());
      if (!copied) copied = { el: hit, label: hit.getAttribute('aria-label') };
      hit.setAttribute('aria-label', hit.dataset.copyFeedback || 'Copied!');
      copyFeedback(hit, true);
      const what = hit.closest('section[data-type="mermaid"]') ? 'the diagram source' : 'the code';
      return JSON.stringify(['copy', hit.getAttribute('value') || '', [area(hit), ...rects], what]);
    }
    const section = hit.matches('summary') && hit.closest('.js-render-block-actions') && hit.closest('section[data-type="mermaid"]');
    if (section) return JSON.stringify(['expand', [...document.querySelectorAll('section[data-type="mermaid"]')].indexOf(section)]);
    hit.click();
    const drawn = hit.closest('.pele-zoom')?.closest('section');
    return JSON.stringify(drawn ? ['rect', box(drawn)] : ['layout']);
  }

  let expanded = null;
  function expand(i, top, height) {
    const section = document.querySelectorAll('section[data-type="mermaid"]')[i];
    const svg = [...(section?.querySelectorAll('.render-container svg') ?? [])].find((s) => !s.closest('.pele-zoom') && !s.classList.contains('octicon'));
    if (!svg) return '[]';
    const old = JSON.parse(collapse());
    const holder = document.createElement('mddog-expanded');
    const wrap = document.createElement('div');
    wrap.className = svg.parentElement.className;
    const copy = document.importNode(svg, true);
    copy.removeAttribute('width');
    copy.removeAttribute('height');
    copy.removeAttribute('style');
    wrap.appendChild(copy);
    holder.appendChild(wrap);
    holder.style.cssText = `top:${top}px;height:${Math.max(0, Math.min(height, document.documentElement.scrollHeight - top))}px`;
    document.body.appendChild(holder);
    expanded = holder;
    return JSON.stringify([...old, box(holder)]);
  }

  function collapse() {
    if (!expanded) return '[]';
    const r = box(expanded);
    expanded.remove();
    expanded = null;
    return JSON.stringify([r]);
  }

  function anchorY(id) {
    if (!id) return 0;
    const find = (name) => document.getElementById(`user-content-${name}`) || document.getElementById(name) || document.getElementsByName(name)[0];
    let el = find(id);
    if (!el) {
      try { el = find(decodeURIComponent(id)); } catch {}
    }
    return el ? el.getBoundingClientRect().top + scrollY : -1;
  }

  function select(ax, ay, fx, fy) {
    const a = caret(ax, ay);
    const f = caret(fx, fy);
    return change(() => { if (a && f) getSelection().setBaseAndExtent(a.startContainer, a.startOffset, f.startContainer, f.startOffset); });
  }

  function word(x, y) {
    const c = caret(x, y);
    return change(() => {
      if (!c) return;
      const s = getSelection();
      s.setPosition(c.startContainer, c.startOffset);
      s.modify('move', 'backward', 'word');
      s.modify('extend', 'forward', 'word');
    });
  }

  const clear = () => change(() => getSelection().removeAllRanges());

  return { at, link, press, hover, resetCopy, expand, collapse, anchorY, select, word, clear };
})();

window.mddogCode = (() => {
  const HEIGHT = 6;
  const INSET = 3;
  const SEL = 'article.markdown-body pre, article.markdown-body table';
  let layer = null;
  const wide = (el) => el.scrollWidth > el.clientWidth;
  const all = () => [...document.querySelectorAll(SEL)];
  const box = (el) => {
    const r = el.getBoundingClientRect();
    return [r.left + scrollX, r.top + scrollY, r.width, r.height];
  };

  function scroller(el) {
    for (let e = el?.closest(SEL); e; e = e.parentElement?.closest(SEL)) if (wide(e)) return e;
    return null;
  }

  function place(track) {
    const el = track.el;
    const r = el.getBoundingClientRect();
    const width = r.width - 2 * INSET;
    track.style.cssText = `left:${r.left + scrollX + INSET}px;top:${r.bottom + scrollY - INSET - HEIGHT}px;width:${width}px;height:${HEIGHT}px`;
    const thumb = Math.max(HEIGHT * 4, (width * el.clientWidth) / el.scrollWidth);
    const left = ((width - thumb) * el.scrollLeft) / (el.scrollWidth - el.clientWidth);
    track.firstElementChild.style.cssText = `left:${left}px;width:${thumb}px`;
  }

  function update() {
    if (!layer) {
      layer = document.createElement('mddog-hbars');
      document.body.appendChild(layer);
    }
    layer.replaceChildren();
    for (const el of all()) {
      const on = !!el.getClientRects().length && wide(el);
      if (el.matches('table')) el.classList.toggle('mddog-scrolls', on);
      if (!on) continue;
      const track = document.createElement('mddog-hbar');
      track.appendChild(document.createElement('span'));
      track.el = el;
      layer.appendChild(track);
      place(track);
    }
  }

  function move(el, to) {
    const before = el.scrollLeft;
    el.scrollLeft = to;
    if (el.scrollLeft === before) return false;
    const track = [...(layer?.children ?? [])].find((t) => t.el === el);
    if (track) place(track);
    return true;
  }

  function scroll(x, y, dx) {
    const el = scroller(document.elementFromPoint(...mddogMouse.at(x, y)));
    if (!el || !move(el, el.scrollLeft + dx)) return '[]';
    return JSON.stringify([box(el)]);
  }

  let held = null;
  const ratio = (track) => {
    const el = track.el;
    const room = box(track)[2] - box(track.firstElementChild)[2];
    return room > 0 ? (el.scrollWidth - el.clientWidth) / room : 0;
  };

  // The scroll bar under a point, `slack` px taller each way so a terminal cell can reach it.
  const bar = (x, y, slack) =>
    [...(layer?.children ?? [])].find((track) => {
      const [left, top, width, height] = box(track);
      return x >= left && x <= left + width && y + slack >= top && y - slack <= top + height;
    });

  function grab(x, y, slack) {
    held = null;
    const track = bar(x, y, slack);
    if (track) {
      const el = track.el;
      const [left] = box(track);
      const [thumb, , size] = box(track.firstElementChild);
      const moved = (x < thumb || x > thumb + size) && move(el, (x - left - size / 2) * ratio(track));
      held = { el, x, start: el.scrollLeft };
      return JSON.stringify(moved ? [box(el)] : []);
    }
    return '';
  }

  function drag(x) {
    const track = held && [...layer.children].find((t) => t.el === held.el);
    if (!track || !move(track.el, held.start + (x - held.x) * ratio(track))) return '[]';
    return JSON.stringify([box(track.el)]);
  }

  const release = () => { held = null; };

  function scrollView(top, bottom, dx) {
    const out = [];
    for (const el of all()) {
      const [, y, , h] = box(el);
      if (!el.getClientRects().length || !wide(el) || y > bottom || y + h < top) continue;
      if (move(el, el.scrollLeft + dx)) out.push(box(el));
    }
    return JSON.stringify(out);
  }

  function positions() {
    return JSON.stringify(all().map((el) => (wide(el) ? el.scrollLeft : 0)));
  }

  function restore(json) {
    let saved = [];
    try { const v = JSON.parse(json); if (Array.isArray(v)) saved = v; } catch {}
    update();
    const out = [];
    all().forEach((el, i) => {
      if (saved[i] > 0 && wide(el) && move(el, saved[i])) out.push(box(el));
    });
    return JSON.stringify(out);
  }

  function reveal(range) {
    const el = scroller(range.startContainer.parentElement);
    const r = range.getClientRects()[0];
    if (!el || !r) return null;
    const c = el.getBoundingClientRect();
    const pad = 16;
    if (r.left < c.left) move(el, el.scrollLeft - (c.left - r.left) - pad);
    else if (r.right > c.right) move(el, el.scrollLeft + Math.min(r.right - c.right + pad, r.left - c.left - pad));
    return box(el);
  }

  const wrap = (on) => { document.documentElement.classList.toggle('mddog-wrap', on); };

  return { update, scroll, bar, grab, drag, release, scrollView, positions, restore, reveal, wrap };
})();

window.mddogLayout = () => {
  mddogCode.update();
  const root = document.querySelector('article.markdown-body');
  const top = root.getBoundingClientRect().top + scrollY;
  const blocks = [...root.children].map((c) => {
    const r = c.getBoundingClientRect();
    const pos = c.getAttribute('data-sourcepos') || '';
    const m = pos.match(/^(\d+):\d+-(\d+):\d+/);
    return [m ? +m[1] : 0, m ? +m[2] : 0, r.top + scrollY, r.height, (c.textContent || '').trim().replace(/\s+/g, ' ').slice(0, 48)];
  });
  const headings = [...root.querySelectorAll('h1,h2,h3,h4,h5,h6')].filter((h) => !h.classList.contains('sr-only') && h.getClientRects().length).map((h) => [+h.tagName[1], h.textContent.trim(), h.getBoundingClientRect().top + scrollY]);
  const bottom = root.getBoundingClientRect().bottom + scrollY;
  return JSON.stringify({ top, bottom, blocks, headings });
};

// Remote URLs go to the app's mddog-http(s) scheme, which fetches them in Rust (see remote.rs), so WebKit never
// reaches the network for them.
function viaApp(u) {
  try {
    const url = new URL(u, document.baseURI);
    return /^https?:$/.test(url.protocol) ? 'mddog-' + url.href : u;
  } catch {
    return u;
  }
}

// Splits a srcset as browsers do: a URL runs to whitespace, so a comma inside one (`w_100,h_100`) stays in it.
function viaAppSet(set) {
  const out = [];
  for (let rest = set.replace(/^[\s,]+/, ''); rest; rest = rest.replace(/^[\s,]+/, '')) {
    let [url] = rest.match(/^\S+/);
    rest = rest.slice(url.length);
    let desc = '';
    if (url.endsWith(',')) {
      url = url.replace(/,+$/, '');
    } else {
      const end = rest.indexOf(',');
      desc = (end < 0 ? rest : rest.slice(0, end)).trim();
      rest = end < 0 ? '' : rest.slice(end + 1);
    }
    out.push(desc ? `${viaApp(url)} ${desc}` : viaApp(url));
  }
  return out.join(', ');
}

// After a failed load, the scheme handler tells by status whether that URL was blocked (403) or just failed.
async function blocked(u) {
  const url = viaApp(u);
  if (!url.startsWith('mddog-')) return false;
  try {
    return (await fetch(url, { method: 'HEAD' })).status === 403;
  } catch {
    return false;
  }
}

// Sizes each remote image with no height from its last natural size, as the image itself will lay out: its width,
// scaled down to fit by max-width, so its box doesn't jump when it arrives. loadRemote records the sizes.
window.mddog.guessImages = (sizes) => {
  for (const img of document.querySelectorAll('img[data-mddog-src]:not([height])')) {
    const size = sizes[img.dataset.mddogSrc];
    if (!size) continue;
    if (!img.hasAttribute('width')) img.style.width = `${size[0]}px`;
    img.style.height = 'auto';
    img.style.aspectRatio = `${size[0]} / ${size[1]}`;
    img.dataset.mddogGuess = '';
  }
};

window.mddog.loadRemote = (timeout) => {
  const imgs = [...document.querySelectorAll('img[data-mddog-src]')];
  const posters = [...document.querySelectorAll('video[data-mddog-poster]')];
  const videos = [...document.querySelectorAll('video')].filter((v) => v.matches('[data-mddog-media]') || v.querySelector('source[data-mddog-media]'));
  let left = imgs.length + posters.length + videos.length;
  const settle = () => { if (--left <= 0) window.mddog.state.remote = true; };
  if (!left) { window.mddog.state.remote = true; return; }
  for (const video of posters) {
    const url = viaApp(video.dataset.mddogPoster);
    video.removeAttribute('data-mddog-poster');
    let done = false;
    const finish = (ok) => { if (done) return; done = true; if (ok) video.poster = url; settle(); };
    const probe = new Image();
    probe.onload = () => finish(true);
    probe.onerror = () => finish(false);
    setTimeout(() => finish(false), timeout);
    probe.src = url;
  }
  for (const video of videos) {
    const media = [video, ...video.querySelectorAll('source[data-mddog-media]')].filter((el) => el.dataset.mddogMedia);
    let done = false;
    const finish = () => { if (done) return; done = true; settle(); };
    video.addEventListener('loadedmetadata', finish, { once: true });
    video.addEventListener('error', finish, { once: true });
    setTimeout(finish, timeout);
    for (const el of media) {
      el.src = viaApp(el.dataset.mddogMedia);
      el.removeAttribute('data-mddog-media');
    }
    video.preload = 'metadata';
    video.load();
  }
  for (const img of imgs) {
    const src = img.dataset.mddogSrc;
    let done = false;
    const fail = async () => {
      if (done) return;
      done = true;
      const box = document.createElement('span');
      box.className = 'mddog-image-missing';
      box.textContent = img.alt || 'image';
      box.dataset.why = (await blocked(img.currentSrc || src)) ? 'blocked' : 'failed';
      img.replaceWith(box);
      settle();
    };
    const picture = img.parentElement && img.parentElement.matches('picture') ? img.parentElement : null;
    const deferred = [...(picture ? picture.querySelectorAll('source[data-mddog-srcset]') : []), img];
    img.classList.add('mddog-loading');
    const loaded = () => {
      if (done) return;
      done = true;
      if (src && !img.hasAttribute('height') && img.naturalWidth && img.naturalHeight && src.length <= 2048) window.mddog.state.images[src] = [img.naturalWidth, img.naturalHeight];
      // Only the guess's own styles go: the renderer sets its own height on an image with a height attribute.
      if (img.hasAttribute('data-mddog-guess')) {
        for (const p of ['width', 'height', 'aspect-ratio']) img.style.removeProperty(p);
        img.removeAttribute('data-mddog-guess');
      }
      img.classList.remove('mddog-loading');
      img.decode().catch(() => 0).then(settle);
    };
    img.addEventListener('load', loaded, { once: true });
    img.addEventListener('error', fail, { once: true });
    setTimeout(fail, timeout);
    for (const el of deferred) {
      if (!el.dataset.mddogSrcset) continue;
      el.srcset = viaAppSet(el.dataset.mddogSrcset);
      el.removeAttribute('data-mddog-srcset');
    }
    img.src = viaApp(src);
    img.removeAttribute('data-mddog-src');
  }
};
