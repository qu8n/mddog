#![expect(clippy::unwrap_used, reason = "a failed step should fail the test")]

use mddog::page::{self, remote, script};
use mddog::{assets, config::Mermaid, engine::Engine, render};
use std::{
    path::Path,
    time::{Duration, Instant},
};
const WAIT: Duration = Duration::from_secs(30);
fn wait(eng: &Engine, js: &str) {
    assert!(eng.wait_js(js, WAIT), "timed out after {WAIT:?} waiting for `{js}`");
}
fn loaded(eng: &Engine) {
    assert!(eng.wait_loaded(WAIT), "timed out after {WAIT:?} loading the page");
}
fn next_frame(eng: &Engine) {
    eng.eval("window.mddogFrame = false; requestAnimationFrame(() => { window.mddogFrame = true; }); ''").unwrap();
    wait(eng, "window.mddogFrame");
}
fn load(eng: &mut Engine, md: &str, width: f64) {
    load_page(eng, md, width, false, Mermaid::Pele);
}
fn load_page(eng: &mut Engine, md: &str, width: f64, wrap: bool, engine: Mermaid) {
    if load_unstarted(eng, md, width, wrap, engine) {
        eng.eval(script::START_MERMAID).unwrap();
    }
}
/// Loads a page as the viewer does before its first paint: obsidian-engine diagrams wait for `START_MERMAID`.
fn load_unstarted(eng: &mut Engine, md: &str, width: f64, wrap: bool, engine: Mermaid) -> bool {
    load_themed(eng, md, width, wrap, engine, "github-dark")
}
fn load_themed(eng: &mut Engine, md: &str, width: f64, wrap: bool, engine: Mermaid, theme: &str) -> bool {
    let doc = render(md);
    let runtime = (doc.math || doc.mermaid).then(assets::runtime_dir);
    let html = page::build(
        &doc.html,
        &page::Opts { theme: mddog::theme::builtin(theme).unwrap(), base: Path::new("/tmp"), pad_x: 0.0, pad_y: 0.0, math: doc.math, mermaid: doc.mermaid, engine, wrap, runtime: runtime.as_deref() },
    );
    // The viewer's own load, so the diagram checks also cover what it does with the page file.
    let tmp = std::env::temp_dir().join(format!("mddog-search-test-{}.html", std::process::id()));
    eng.resize(width, 600.0);
    page::load(eng, &tmp, &html, true, WAIT).unwrap();
    wait(eng, "document.fonts.status === 'loaded'");
    doc.mermaid
}
fn matches(eng: &mut Engine, md: &str, width: f64, query: &str) -> Vec<Vec<script::Rect>> {
    load(eng, md, width);
    eng.search(query, None)
}
fn center(eng: &Engine, selector: &str) -> (f64, f64) {
    let json =
        eng.eval(&format!("(() => {{ const r = document.querySelector({selector:?}).getBoundingClientRect(); return JSON.stringify([r.left + r.width / 2, r.top + r.height / 2]); }})()")).unwrap();
    let v: Vec<f64> = serde_json::from_str(&json).unwrap();
    (v[0], v[1])
}
fn link_at(eng: &Engine, (x, y): (f64, f64)) -> String {
    eng.eval(&format!("mddogMouse.link({x}, {y})")).unwrap()
}
fn press_at(eng: &Engine, (x, y): (f64, f64)) -> String {
    eng.eval(&format!("mddogMouse.press({x}, {y})")).unwrap()
}

fn hover_at(eng: &Engine, (x, y): (f64, f64)) -> (String, Vec<Vec<f64>>) {
    serde_json::from_str(&eng.eval(&format!("mddogMouse.hover({x}, {y}, 8)")).unwrap()).unwrap()
}

/// Every button is a pointer over its box, its icon and the icon's path. Only code-block copy buttons get the hover look.
fn button_checks(eng: &mut Engine, theme: &str, check: &mut impl FnMut(&str, bool)) {
    let long = "x".repeat(300);
    let md = format!("```rust\nlet a = \"{long}\";\n```\n\n```mermaid\ngraph LR\n  A --> B\n```\n\n<details>\n<summary>More</summary>\n\nbody\n\n</details>\n");
    let engine = if theme.starts_with("obsidian") { Mermaid::Obsidian } else { Mermaid::Pele };
    if load_themed(eng, &md, 600.0, false, engine, theme) {
        eng.eval(script::START_MERMAID).unwrap();
    }
    wait(eng, "mddog.state.mermaid === true");
    eng.eval("mddogCode.update()").unwrap();
    let look = |eng: &Engine, sel: &str| {
        eng.eval(&format!("(() => {{ const b = document.querySelector({sel:?}); return String(b.classList.contains('mddog-hover') || b.classList.contains('tooltipped')); }})()")).unwrap() == "true"
    };
    for part in ["", " svg", " path"] {
        let at = center(eng, &format!(".highlight clipboard-copy{part}"));
        let (shape, rects) = hover_at(eng, at);
        check(&format!("b1_{theme}_code_copy{part}_pointer_and_hover"), shape == "pointer" && rects.len() <= 1 && look(eng, ".highlight clipboard-copy"));
        hover_at(eng, (-1.0, -1.0));
    }
    let github = eng.eval("String(!!document.querySelector('section clipboard-copy')?.offsetParent)").unwrap() == "true";
    check(&format!("b2_{theme}_mermaid_buttons_shown"), github != theme.starts_with("obsidian"));
    if github {
        for sel in ["section clipboard-copy", "section clipboard-copy svg", "section clipboard-copy path", "section summary", "section summary svg", "section summary path"] {
            let (shape, rects) = hover_at(eng, center(eng, sel));
            check(
                &format!("b3_{theme}_{}_pointer_no_hover", sel.replace(' ', "_")),
                shape == "pointer" && rects.is_empty() && !look(eng, sel.split(" svg").next().unwrap().split(" path").next().unwrap()),
            );
        }
        hover_at(eng, (-1.0, -1.0));
    }
    check(&format!("b4_{theme}_details_summary_pointer"), hover_at(eng, center(eng, ".markdown-body > details > summary")).0 == "pointer");
    let bar: Vec<f64> = serde_json::from_str(
        &eng.eval("(() => { const r = document.querySelector('mddog-hbar').getBoundingClientRect(); return JSON.stringify([r.left + scrollX + r.width / 2, r.top + scrollY + r.height / 2]); })()")
            .unwrap(),
    )
    .unwrap();
    check(&format!("b5_{theme}_scroll_bar_pointer"), hover_at(eng, (bar[0], bar[1])).0 == "pointer" && hover_at(eng, (bar[0], bar[1] + 7.0)).0 == "pointer");
    hover_at(eng, (-1.0, -1.0));
}

fn hover_checks(eng: &mut Engine, check: &mut impl FnMut(&str, bool)) {
    load(eng, "```rust\nfn main() {}\n```\n\nsee [a link](https://example.com) here\n", 600.0);
    let block: Vec<f64> = serde_json::from_str(
        &eng.eval("(() => { const r = document.querySelector('.highlight').getBoundingClientRect(); return JSON.stringify([r.left + scrollX, r.top + scrollY, r.width, r.height]); })()").unwrap(),
    )
    .unwrap();
    let copy = center(eng, ".highlight clipboard-copy");
    let tip = "(() => { const b = document.querySelector('.highlight clipboard-copy'); const t = getComputedStyle(b, '::after'); return JSON.stringify([b.classList.contains('mddog-hover'), t.display, t.content, getComputedStyle(b).backgroundColor]); })()";
    let idle: (bool, String, String, String) = serde_json::from_str(&eng.eval(tip).unwrap()).unwrap();
    check("h1_hover_button_is_pointer_and_redraws_block", hover_at(eng, copy) == ("pointer".into(), vec![block.clone()]));
    let on: (bool, String, String, String) = serde_json::from_str(&eng.eval(tip).unwrap()).unwrap();
    check("h2_hover_shows_tooltip_and_background", on.0 && on.1 == "block" && on.2 == "\"Copy code to clipboard\"" && on.3 != idle.3);
    let fits = "(() => { const b = document.querySelector('.highlight clipboard-copy'); const h = document.querySelector('.highlight').getBoundingClientRect(); const r = b.getBoundingClientRect(); const t = getComputedStyle(b, '::after'); const w = parseFloat(t.width) + parseFloat(t.paddingLeft) + parseFloat(t.paddingRight); return String(r.left - 6 - w >= h.left && r.right <= h.right); })()";
    check("h3_tooltip_left_of_button_inside_block", eng.eval(fits).unwrap() == "true");
    check("h4_same_target_no_redraw", hover_at(eng, (copy.0 - 1.0, copy.1)) == ("pointer".into(), vec![]));
    let code = center(eng, ".highlight pre span, .highlight pre");
    check("h5_leaving_clears_and_redraws", hover_at(eng, code) == ("text".into(), vec![block]) && eng.eval(tip).unwrap() == serde_json::to_string(&idle).unwrap());
    check("h6_link_is_pointer", hover_at(eng, center(eng, "p a")).0 == "pointer");
    let p: Vec<f64> =
        serde_json::from_str(&eng.eval("(() => { const r = document.querySelector('p').getBoundingClientRect(); return JSON.stringify([r.right - 2, r.top + r.height / 2]); })()").unwrap()).unwrap();
    check("h7_blank_is_default", hover_at(eng, (p[0], p[1])) == ("default".into(), vec![]));
    check("h7_off_page_is_default", hover_at(eng, (-1.0, -1.0)) == ("default".into(), vec![]));
    hover_at(eng, copy);
    press_at(eng, copy);
    hover_at(eng, code);
    check("h8_copied_wins_after_leaving", eng.eval(tip).unwrap() == serde_json::to_string(&(false, "block", "\"Copied!\"", &idle.3)).unwrap());
    hover_at(eng, copy);
    eng.eval("mddogMouse.resetCopy()").unwrap();
    let after: (bool, String, String, String) = serde_json::from_str(&eng.eval(tip).unwrap()).unwrap();
    check("h9_reset_while_hovered_keeps_hover_tooltip", after.0 && after.1 == "block" && after.2 == "\"Copy code to clipboard\"");
    hover_at(eng, (-1.0, -1.0));
}

fn main() {
    // SAFETY: the first line of a single-threaded main, so no other thread reads the environment.
    unsafe { std::env::set_var("MDDOG_CACHE_DIR", std::env::temp_dir().join("mddog-test-cache")) };
    let mut eng = Engine::new(600.0);
    let mut failed = 0;
    let mut check = |name: &str, ok: bool| {
        println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
        failed += usize::from(!ok);
    };

    let m = matches(&mut eng, "say hello **world** now\n", 600.0, "hello world");
    check("s15_spans_formatting", m.len() == 1);

    let m = matches(&mut eng, "Apple apple APPLE ap.ple\n", 600.0, "apple");
    check("s14_case_insensitive", m.len() == 3);
    let m = matches(&mut eng, "a.b axb\n", 600.0, "a.b");
    check("s14_literal_not_regex", m.len() == 1);
    let m = matches(&mut eng, "one two\n", 600.0, " two");
    check("s14_untrimmed_query", m.len() == 1);
    let m = matches(&mut eng, "It contains the compiler,\nstandard library\n", 600.0, "compiler, standard");
    check("d2_across_soft_break", m.len() == 1);

    let words = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore et dolore magna aliqua";
    let md = format!("{words} {words} {words}\n");
    let narrow = matches(&mut eng, &md, 200.0, "magna aliqua lorem ipsum");
    let wrapped = narrow.iter().any(|r| r.len() >= 2 && r[0][1] != r[1][1]);
    check("d2_across_wrap", narrow.len() == 2 && wrapped);

    let m = matches(&mut eng, "# Title\n\nTitle again in text.\n\n| a | b |\n|---|---|\n| title | x |\n", 600.0, "title");
    check("d1_every_occurrence_in_dom", m.len() == 3);

    let m = matches(&mut eng, "first para ends\n\nwith next para\n", 600.0, "ends with");
    check("no_match_across_blocks", m.is_empty());

    // MathJax gets the math as text, so `<b` is a less-than sign, not a tag that swallows the rest.
    load(&mut eng, "$a<b$ and $a < b > c$ and $x &lt; y$.\n", 600.0);
    wait(&eng, script::MATH_DONE);
    let math = eng.eval("JSON.stringify([...document.querySelectorAll('math-renderer')].map((m) => m.querySelector('math')?.textContent.replace(/\\s/g, '')))").unwrap();
    check("math_keeps_html_like_text", math == r#"["a<b","a<b>c","x<y"]"#);

    // Remote images go through the app's scheme handler, which refuses private hosts and says so in the placeholder.
    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    server.set_nonblocking(true).unwrap();
    let port = server.local_addr().unwrap().port();
    load(&mut eng, &format!("![loop](http://127.0.0.1:{port}/a.png) ![](http://localhost:{port}/b.png) ![lan](https://10.1.2.3/c.png)\n"), 600.0);
    check("r1_remote_load_started", remote::load(&eng, Duration::from_secs(10)));
    wait(&eng, script::REMOTE_DONE);
    let why = eng.eval("JSON.stringify([...document.querySelectorAll('.mddog-image-missing')].map((b) => b.textContent + ':' + b.dataset.why))").unwrap();
    check("r1_private_hosts_blocked", why == r#"["loop:blocked","image:blocked","lan:blocked"]"#);

    // A remembered size opens each loading box at the size the image lays out at, scaled down to the page width.
    // data: URLs stand in for remote ones, since they load without the network.
    load(&mut eng, "![wide](https://x.test/w.svg) ![small](https://x.test/s.svg) <img src=\"https://x.test/h.svg\" width=\"1600\" height=\"400\">\n", 600.0);
    let svg = |w, h| format!("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='{w}' height='{h}'%3E%3C/svg%3E");
    let (wide, small, sized) = (svg(1200, 300), svg(100, 40), svg(1600, 400));
    eng.eval(&format!("const [w, s, h] = document.querySelectorAll('img[data-mddog-src]'); w.dataset.mddogSrc = {wide:?}; s.dataset.mddogSrc = {small:?}; h.dataset.mddogSrc = {sized:?}; ''"))
        .unwrap();
    let sizes = serde_json::json!({ &wide: [1200, 300], &small: [100, 40] });
    eng.eval(&format!("mddog.guessImages({sizes}); ''")).unwrap();
    let boxes = "JSON.stringify([...document.querySelectorAll('article img')].map((i) => { const r = i.getBoundingClientRect(); return [r.width, r.height]; }))";
    let before: Vec<[f64; 2]> = serde_json::from_str(&eng.eval(boxes).unwrap()).unwrap();
    remote::load(&eng, Duration::from_secs(10));
    wait(&eng, script::REMOTE_DONE);
    let boxes: Vec<[f64; 2]> = serde_json::from_str(&eng.eval(boxes).unwrap()).unwrap();
    check("r3_guessed_image_box_matches_image", before[..2] == boxes[..2] && boxes[0][0] < 1200.0 && boxes[0][0] == boxes[0][1] * 4.0 && boxes[1] == [100.0, 40.0]);
    // An image with its own size keeps the renderer's scaling, which the guess never touches.
    check("r3_sized_image_keeps_its_scaling", boxes[2][0] < 1600.0 && boxes[2][0] == boxes[2][1] * 4.0);
    check("r3_image_sizes_recorded", serde_json::from_str::<serde_json::Value>(&eng.eval("JSON.stringify(mddog.state.images)").unwrap()).unwrap() == sizes);

    // An image URL in the page itself skips the scheme handler, so the content rule stops it.
    check("r2_content_rules_added", eng.content_rules());
    let tmp = std::env::temp_dir().join(format!("mddog-search-test-rules-{}.html", std::process::id()));
    let html = format!("<!DOCTYPE html><img src=\"http://127.0.0.1:{port}/x.png\"><img src=\"http://localhost:{port}/x.png\"><video src=\"http://127.0.0.1:{port}/x.mp4\"></video>");
    page::load(&eng, &tmp, &html, true, WAIT).unwrap();
    wait(&eng, "[...document.images].every((i) => i.complete) && document.querySelector('video').networkState === HTMLMediaElement.NETWORK_NO_SOURCE");
    check("r2_private_loads_blocked", server.accept().is_err_and(|e| e.kind() == std::io::ErrorKind::WouldBlock));

    load(&mut eng, "[site](https://example.com/a) and https://auto.dev/x and [jump](#target) and [doc](docs/a.md) plain\n\n## Target\n", 600.0);
    check("m1_link_href", link_at(&eng, center(&eng, "a[href^='https://example']")) == r#"["https://example.com/a","https://example.com/a"]"#);
    check("m1_autolink", link_at(&eng, center(&eng, "a[href^='https://auto']")).contains("https://auto.dev/x"));
    check("m1_anchor_link", link_at(&eng, center(&eng, "a[href='#target']")).starts_with(r##"["#target","##));
    check("m1_relative_link", link_at(&eng, center(&eng, "a[href='docs/a.md']")).contains("file://"));
    check("m1_no_link_on_text", link_at(&eng, (590.0, 5.0)).is_empty());
    let top: f64 = eng.eval("mddogMouse.anchorY('target')").unwrap().parse().unwrap();
    let want: f64 = eng.eval("document.querySelector('h2').getBoundingClientRect().top + scrollY").unwrap().parse().unwrap();
    check("m2_anchor_y", top > 0.0 && (top - want).abs() < 0.5);
    check("m2_anchor_missing", eng.eval("mddogMouse.anchorY('nope%20x')").unwrap() == "-1");

    check("m2_anchor_empty_is_top", eng.eval("mddogMouse.anchorY('')").unwrap() == "0");
    check("m2_anchor_malformed_percent", eng.eval("mddogMouse.anchorY('50%')").unwrap() == "-1");
    load(&mut eng, "intro\n\n<h2 id=\"a%20b\">Raw</h2>\n\n<h2 id=\"c d\">Decoded</h2>\n", 600.0);
    check("m2_anchor_raw_percent_id", eng.eval("mddogMouse.anchorY('a%20b')").unwrap().parse::<f64>().is_ok_and(|y| y > 0.0));
    check("m2_anchor_decoded_fallback", eng.eval("mddogMouse.anchorY('c%20d')").unwrap().parse::<f64>().is_ok_and(|y| y > 0.0));

    // With an odd cell height at scale 2, every other tile starts on a half CSS pixel. Its pixels must be the
    // same rows as a snapshot from the whole pixel above, not a shifted or re-rasterized copy.
    load(&mut eng, "# Mermaid\n\nalpha beta gamma\n", 600.0);
    let whole = eng.snapshot(10.0, 41.0, 2.0).unwrap();
    let half = eng.snapshot(10.5, 40.5, 2.0).unwrap();
    let row = whole.width * 4;
    check("s0_half_pixel_tile_matches_whole", half.height == 81 && half.rgba.to_vec() == whole.rgba[row..].to_vec());
    let ends_half = eng.snapshot(10.0, 40.5, 2.0).unwrap();
    check("s0_half_pixel_end_matches_whole", ends_half.height == 81 && ends_half.rgba.to_vec() == whole.rgba[..81 * row].to_vec());

    load(&mut eng, "alpha beta gamma\n\nsecond para\n", 600.0);
    let rect = eng.eval("(() => { const r = document.createRange(); r.selectNodeContents(document.querySelector('p')); const b = r.getBoundingClientRect(); return JSON.stringify([b.left, b.top, b.right, b.bottom]); })()").unwrap();
    let r: Vec<f64> = serde_json::from_str(&rect).unwrap();
    let y = f64::midpoint(r[1], r[3]);
    let before = eng.snapshot(0.0, 100.0, 1.0).unwrap().rgba.to_vec();
    eng.eval(&format!("mddogMouse.select({}, {y}, {}, {y})", r[0], r[2])).unwrap();
    check("m3_select_text", eng.eval("getSelection().toString()").unwrap() == "alpha beta gamma");
    next_frame(&eng);
    check("m3_selection_painted", eng.snapshot(0.0, 100.0, 1.0).unwrap().rgba.to_vec() != before);
    eng.eval(&format!("mddogMouse.select({}, {y}, {}, {y})", r[2], r[0])).unwrap();
    check("m3_select_backwards", eng.eval("getSelection().toString()").unwrap() == "alpha beta gamma");
    let rects = eng.eval("mddogMouse.clear()").unwrap();
    check("m3_clear_reports_rects", !serde_json::from_str::<Vec<script::Rect>>(&rects).unwrap_or_default().is_empty() && eng.eval("getSelection().toString()").unwrap().is_empty());
    next_frame(&eng);
    check("m3_clear_restores_pixels", eng.snapshot(0.0, 100.0, 1.0).unwrap().rgba.to_vec() == before);
    eng.eval(&format!("mddogMouse.word({}, {y})", r[0] + (r[2] - r[0]) / 2.0)).unwrap();
    check("m3_double_click_word", eng.eval("getSelection().toString()").unwrap().trim() == "beta");
    eng.eval(&format!("mddogMouse.select({}, {y}, 5000, {y})", r[0])).unwrap();
    check("m3_select_past_edge_clamped", eng.eval("getSelection().toString()").unwrap() == "alpha beta gamma");

    load(&mut eng, "<details>\n<summary>More</summary>\n\nHidden body\n\n</details>\n\nplain\n", 600.0);
    check("p1_no_press_on_text", press_at(&eng, center(&eng, "p")).is_empty());
    check("p1_summary_needs_layout", press_at(&eng, center(&eng, "summary")) == r#"["layout"]"#);
    check("p1_summary_opens", eng.eval("String(document.querySelector('details').open)").unwrap() == "true");

    load(&mut eng, "```mermaid\ngraph LR\n  A --> B\n```\n", 600.0);
    wait(&eng, "mddog.state.mermaid === true");
    check("p2_expand_button", eng.eval("String(!!document.querySelector('section summary[aria-label=\"Open dialog\"]'))").unwrap() == "true");
    let copy = center(&eng, "clipboard-copy");
    let section =
        eng.eval("(() => { const r = document.querySelector('section').getBoundingClientRect(); return JSON.stringify([r.left + scrollX, r.top + scrollY, r.width, r.height]); })()").unwrap();
    let (shape, rects) = hover_at(&eng, copy);
    let inside = "(() => { const b = document.querySelector('clipboard-copy'); const s = document.querySelector('section').getBoundingClientRect(); const r = b.getBoundingClientRect(); const t = getComputedStyle(b, '::after'); const w = parseFloat(t.width) + parseFloat(t.paddingLeft) + parseFloat(t.paddingRight); const h = parseFloat(t.height) + parseFloat(t.paddingTop) + parseFloat(t.paddingBottom); const mid = (r.top + r.bottom) / 2; return JSON.stringify([t.content, r.left - 6 - w >= s.left && mid - h / 2 >= s.top && mid + h / 2 <= s.bottom]); })()";
    let tip = eng.eval(inside).unwrap();
    check("h10_mermaid_hover_no_tooltip", shape == "pointer" && rects.is_empty() && tip.starts_with(r#"["none","#));
    hover_at(&eng, (-1.0, -1.0));
    check("p3_copy_mermaid_source", press_at(&eng, copy) == format!(r#"["copy","graph LR\n  A --> B",[{section}],"the diagram source"]"#));
    let feedback = "(() => { const b = document.querySelector('clipboard-copy'); const tip = getComputedStyle(b, '::after'); return JSON.stringify([b.getAttribute('aria-label'), b.querySelector('.js-clipboard-copy-icon').classList.contains('d-none'), b.querySelector('.js-clipboard-check-icon').classList.contains('d-none'), tip.display, tip.opacity, tip.content]); })()";
    check("p3_copy_shows_check_and_tooltip", eng.eval(feedback).unwrap() == r#"["Copied!",true,false,"block","1","\"Copied!\""]"#);
    check("h10_copied_tooltip_inside_section", eng.eval(inside).unwrap().starts_with(r#"["\"Copied!\"",true"#));
    press_at(&eng, copy);
    check("p3_copy_twice_keeps_check", eng.eval(feedback).unwrap() == r#"["Copied!",true,false,"block","1","\"Copied!\""]"#);
    check("p3_reset_reports_section", eng.eval("mddogMouse.resetCopy()").unwrap() == format!("[{section}]"));
    check("p3_reset_restores_button", eng.eval(feedback).unwrap() == r#"["Copy mermaid code",false,true,"inline","1","none"]"#);
    check("p3_reset_when_idle", eng.eval("mddogMouse.resetCopy()").unwrap() == "[]");
    load(&mut eng, "```rust\nfn main() {}\n```\n\n```\nplain\n```\n", 600.0);
    let boxes = "JSON.stringify([...document.querySelectorAll('.highlight, .snippet-clipboard-content')].map((e) => { const r = e.getBoundingClientRect(); return [r.left + scrollX, r.top + scrollY, r.width, r.height]; }))";
    let boxes: Vec<Vec<f64>> = serde_json::from_str(&eng.eval(boxes).unwrap()).unwrap();
    let copy = center(&eng, ".highlight clipboard-copy");
    check(
        "p7_copy_code_shown",
        eng.eval("String(getComputedStyle(document.querySelector('.highlight clipboard-copy')).visibility === 'visible' && document.querySelector('.highlight clipboard-copy').offsetWidth > 0)")
            .unwrap()
            == "true",
    );
    hover_checks(&mut eng, &mut check);
    for theme in ["github-light", "github-dark", "obsidian-light", "obsidian-dark"] {
        button_checks(&mut eng, theme, &mut check);
    }
    load(&mut eng, "```rust\nfn main() {}\n```\n\n```\nplain\n```\n", 600.0);
    let hit: serde_json::Value = serde_json::from_str(&press_at(&eng, copy)).unwrap();
    let rects: Vec<Vec<f64>> = serde_json::from_value(hit[2].clone()).unwrap_or_default();
    check("p7_copy_code", boxes.len() == 2 && hit[0] == "copy" && hit[1] == "fn main() {}" && rects == [boxes[0].clone()] && hit[3] == "the code");
    check("p7_copy_plain_code", press_at(&eng, center(&eng, ".snippet-clipboard-content clipboard-copy")).starts_with(r#"["copy","plain","#));
    load(&mut eng, "```mermaid\ngraph LR\n  A --> B\n```\n", 600.0);
    wait(&eng, "mddog.state.mermaid === true");
    check("p4_diagram_drawn", eng.eval("String(!!document.querySelector('section svg.pele'))").unwrap() == "true");
    check("p4_no_zoom_when_it_fits", eng.eval("String(!document.querySelector('.pele-zoom-in')?.offsetParent)").unwrap() == "true");

    let wide = (b'A'..=b'J').map(|c| format!("  participant {0} as Participant {0}\n", c as char)).collect::<String>();
    load(&mut eng, &format!("```mermaid\nsequenceDiagram\n{wide}  A->>J: hi\n```\n"), 600.0);
    wait(&eng, "mddog.state.mermaid === true");
    wait(&eng, "!!document.querySelector('.pele-zoom-in')?.offsetParent");
    next_frame(&eng);
    check("p4_zoom_in_pointer", hover_at(&eng, center(&eng, ".pele-zoom-in")) == ("pointer".into(), vec![]));
    let viewbox = "document.querySelector('section svg.pele').getAttribute('viewBox')";
    let before = eng.eval(viewbox).unwrap();
    let pressed = press_at(&eng, center(&eng, ".pele-zoom-in"));
    let zoomed_section =
        eng.eval("(() => { const r = document.querySelector('section').getBoundingClientRect(); return JSON.stringify([r.left + scrollX, r.top + scrollY, r.width, r.height]); })()").unwrap();
    check("p4_zoom_button_repaints_section", pressed == format!(r#"["rect",{zoomed_section}]"#));
    check("p4_zoom_in_zooms", eng.eval(viewbox).unwrap() != before);
    // A zoom puts its own layout on the diagram, which the expanded copy must leave behind.
    eng.eval("mddogMouse.expand(0, 0, 400)").unwrap();
    let zoom_style = "(() => { const s = document.querySelector('mddog-expanded svg').style; return JSON.stringify(['margin', 'display', 'cursor', 'user-select', 'touch-action', 'outline'].filter((p) => s.getPropertyValue(p))); })()";
    check("p4_expanded_drops_zoom_layout", eng.eval(zoom_style).unwrap() == "[]");
    eng.eval("mddogMouse.collapse()").unwrap();
    check("p4_zoom_reset_pointer", hover_at(&eng, center(&eng, ".pele-zoom-reset")) == ("pointer".into(), vec![]));
    press_at(&eng, center(&eng, ".pele-zoom-reset"));
    check("p4_reset_restores_view", eng.eval(viewbox).unwrap() == before);

    load(&mut eng, "```mermaid\nflowchart LR\n  A --> B\n```\n", 600.0);
    wait(&eng, "mddog.state.mermaid === true");
    check("p6_pele_expands", eng.eval("mddogMouse.expand(0, 0, 200)").unwrap() != "[]" && eng.eval("String(!!document.querySelector('mddog-expanded svg'))").unwrap() == "true");
    // Pele keeps its colors in the SVG's style attribute, so the copy must keep them too.
    let paint = |root: &str| {
        eng.eval(&format!("JSON.stringify([...document.querySelectorAll('{root} svg.pele :is(rect, path, text)')].map((n) => {{ const s = getComputedStyle(n); return [s.fill, s.stroke]; }}))"))
            .unwrap()
    };
    let (page_paint, expanded_paint) = (paint("section"), paint("mddog-expanded"));
    check("p6_expanded_keeps_colors", expanded_paint.len() > 2 && expanded_paint == page_paint);
    eng.eval("mddogMouse.collapse()").unwrap();
    load(&mut eng, "```mermaid\nflowchart LR\n  A --> B -->\n```\n", 600.0);
    wait(&eng, "mddog.state.mermaid === true");
    check("p5_parse_error_shown", eng.eval("String(document.querySelector('section .pele-error')?.textContent.startsWith('Parse error'))").unwrap() == "true");

    // A real local image, so only the frame's CSP can stop the image shape from loading it.
    let dot = std::env::temp_dir().join(format!("mddog-search-dot-{}.png", std::process::id()));
    std::fs::write(
        &dot,
        [
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4, 0, 0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 100, 96, 0, 0, 0, 6, 0, 2,
            48, 129, 208, 47, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ],
    )
    .unwrap();
    let md = format!(
        "```mermaid\n---\ntitle: Front\n---\nflowchart LR\n  A --> B\n```\n\n```mermaid\npie\n  \"a\" : 1\n```\n\n```mermaid\nflowchart LR\n  A --> B -->\n```\n\n```mermaid\nflowchart LR\n  I@{{ img: \"file://{}\", h: 40 }}\n```\n",
        dot.display()
    );
    let md = md.as_str();
    // Pele draws in the page, which has no CSP, so it must leave a diagram's image URLs out. Pele drops file: URLs itself, so the check uses http.
    load(&mut eng, "```mermaid\nflowchart LR\n  I@{ img: \"http://127.0.0.1:9/a.png\", h: 40 }\n```\n", 600.0);
    wait(&eng, "mddog.state.mermaid === true");
    check("p5_diagram_images_not_loaded", eng.eval("String(!!document.querySelector('svg:not(.octicon)') && !document.querySelector('svg image'))").unwrap() == "true");
    load_unstarted(&mut eng, md, 600.0, false, Mermaid::Obsidian);
    let skeleton = "JSON.stringify([document.querySelectorAll('.mddog-loading .mddog-skeleton').length, document.querySelectorAll('.mddog-obsidian').length, mddog.state.mermaid])";
    check("o0_skeleton_before_start", eng.eval(skeleton).unwrap() == "[4,0,false]");
    eng.eval(script::START_MERMAID).unwrap();
    wait(&eng, "mddog.state.mermaid === true");
    check("o0_skeleton_gone_when_drawn", eng.eval(skeleton).unwrap() == "[0,4,true]");
    // Obsidian's settings: a flowchart keeps its own width, a pie fills the page width up to its own, and a parse error is shown.
    let drawn = "JSON.stringify([...document.querySelectorAll('.mddog-obsidian')].map((b) => { const i = b.querySelector(':scope > img'); return i ? [i.style.width, decodeURIComponent(i.src).includes('Front')] : b.querySelector('.flash-error')?.textContent.startsWith('Parse error'); }))";
    let drawn: serde_json::Value = serde_json::from_str(&eng.eval(drawn).unwrap()).unwrap();
    check("o1_drawn_with_frontmatter_title", drawn[0][0].as_str().is_some_and(|w| w.ends_with("px")) && drawn[0][1] == true);
    check("o1_pie_fills_the_width", drawn[1][0] == "100%");
    check("o1_parse_error_shown", drawn[2] == true);
    // Mermaid draws in a frame that can load only data: images, so the image shape can't load the file and shows
    // mermaid's loading error. Without that CSP it would draw.
    check("o2_diagram_images_not_loaded", eng.eval("document.querySelectorAll('.mddog-obsidian')[3].textContent").unwrap() == "Loading error.");
    let _ = std::fs::remove_file(&dot);
    check("o2_no_frame_or_buttons", eng.eval("String(!document.querySelector('iframe') && !document.querySelector('section .js-render-block-actions').offsetParent)").unwrap() == "true");
    check("o3_dark_theme_inverts", eng.eval("getComputedStyle(document.querySelector('.mddog-obsidian > img')).filter").unwrap().starts_with("invert(1)"));
    check("o4_size_kept", eng.eval("String(Object.keys(mddog.state.sizes).length === 2)").unwrap() == "true");
    // A flowchart keeps its own size, so its width is stored as 0 and a guess doesn't shrink it. The pie fills the width.
    check("o4_fixed_size_stored_unscaled", eng.eval("JSON.stringify(Object.values(mddog.state.sizes).map((s) => s[0] === 0))").unwrap() == "[true,false]");

    let md = "```mermaid\ngraph LR\n  A[A long first node] --> B[A long second node] --> C[A long third node] --> D[A long fourth node]\n```\n\n```mermaid\ngraph TD\n  A --> B --> C\n```\n";
    let boxes = "JSON.stringify([...document.querySelectorAll('.render-container')].map((b) => b.getBoundingClientRect().height))";
    load_page(&mut eng, md, 900.0, false, Mermaid::Obsidian);
    wait(&eng, "mddog.state.mermaid === true");
    let sizes = eng.eval("JSON.stringify(mddog.state.sizes)").unwrap();
    let mut guessed = Vec::new();
    for width in [400.0, 900.0] {
        load_unstarted(&mut eng, md, width, false, Mermaid::Obsidian);
        eng.eval(&format!("mddog.guess({sizes}); ''")).unwrap();
        let guess = eng.eval(boxes).unwrap();
        eng.eval(script::START_MERMAID).unwrap();
        wait(&eng, "mddog.state.mermaid === true");
        guessed.push(guess == eng.eval(boxes).unwrap());
    }
    check("o5_skeleton_takes_the_drawn_height", guessed == [true, true]);

    for (name, dark, want) in [("th1_dark_background", true, [0x0d, 0x11, 0x17]), ("th1_light_background", false, [0xff, 0xff, 0xff])] {
        let doc = render("# Title\n\ntext\n");
        let html = page::build(
            &doc.html,
            &page::Opts {
                theme: mddog::theme::builtin(if dark { "github-dark" } else { "github-light" }).unwrap(),
                base: Path::new("/tmp"),
                pad_x: 0.0,
                pad_y: 0.0,
                math: false,
                mermaid: false,
                engine: Default::default(),
                wrap: false,
                runtime: None,
            },
        );
        let tmp = std::env::temp_dir().join(format!("mddog-theme-test-{}.html", std::process::id()));
        std::fs::write(&tmp, html).unwrap();
        eng.load_file(&tmp, dark);
        loaded(&eng);
        let _ = std::fs::remove_file(&tmp);
        let shot = eng.snapshot(0.0, 10.0, 1.0).unwrap();
        check(name, shot.rgba[..3] == want);
        let text = eng.eval("getComputedStyle(document.querySelector('h1')).color").unwrap();
        check(&format!("{name}_text"), text == if dark { "rgb(240, 246, 252)" } else { "rgb(31, 35, 40)" });
    }

    let s = mddog::engine::backing_scale();
    let row = |b: &mddog::engine::Bitmap, r: f64| b.rgba[b.width * 4 * (r * s) as usize..].to_vec();
    let page = |n: usize| (0..n).map(|i| format!("Row {i} of a long page.\n\n")).collect::<String>();
    for (n, cases) in [(30, [("t1_tile_short_page", 0.5), ("t1_tile_short_page_end", 1.0)]), (300, [("t1_tile_mid_page", 0.5), ("t1_tile_page_end", 1.0)])] {
        load(&mut eng, &(page(n) + "[last](https://example.com/last)\n"), 600.0);
        let doc: f64 = eng.eval("String(document.documentElement.scrollHeight)").unwrap().parse().unwrap();
        eng.resize(600.0, doc);
        for (name, at) in cases {
            let y = (doc * at).min(doc - 120.0);
            let (a, b) = (eng.snapshot(y - 100.0, 200.0, s).unwrap(), eng.snapshot(y, 100.0, s).unwrap());
            check(name, a.height == (200.0 * s) as usize && row(&a, 100.0) == *b.rgba && *b.rgba != *eng.snapshot(0.0, 100.0, s).unwrap().rgba);
        }
    }
    let last = eng.eval("(() => { const r = document.querySelector('a').getBoundingClientRect(); return JSON.stringify([r.left + scrollX + 5, r.top + scrollY + r.height / 2]); })()").unwrap();
    let last: Vec<f64> = serde_json::from_str(&last).unwrap();
    eng.snapshot(0.0, 100.0, s);
    check("t1_link_below_view", link_at(&eng, (last[0], last[1])).contains("example.com/last"));

    let long = (0..40).map(|i| format!("word{i}")).collect::<Vec<_>>().join("_");
    load(&mut eng, &format!("```rust\nlet x = \"{long}\";\n```\n\n```\nshort\n```\n"), 600.0);
    eng.eval("mddogLayout()").unwrap();
    let bars = "(() => JSON.stringify([...document.querySelectorAll('mddog-hbar')].map((t) => { const r = t.getBoundingClientRect(); const b = t.firstElementChild.getBoundingClientRect(); return [r.left, r.width, b.left, b.width]; })))()";
    let before: Vec<[f64; 4]> = serde_json::from_str(&eng.eval(bars).unwrap()).unwrap();
    check("c1_wide_pre_gets_bar", before.len() == 1 && before[0][3] > 0.0 && before[0][3] < before[0][1] && before[0][2] == before[0][0]);
    let thumb_top: f64 = eng.eval("String(document.querySelector('mddog-hbar > span').getBoundingClientRect().top + 2)").unwrap().parse().unwrap();
    let strip = eng.snapshot(thumb_top, 2.0, s).unwrap().rgba.to_vec();
    check("c1_bar_drawn", strip.chunks(4).any(|p| p[0].abs_diff(0x65) < 3 && p[1].abs_diff(0x6c) < 3 && p[2].abs_diff(0x76) < 3));
    let bottom: f64 = eng.eval("String(document.querySelector('pre').getBoundingClientRect().bottom - 12)").unwrap().parse().unwrap();
    eng.eval("document.querySelector('mddog-hbars').style.display = 'none'").unwrap();
    next_frame(&eng);
    let strip = eng.snapshot(bottom, 10.0, s).unwrap();
    let inside = |p: &[u8]| p[0].abs_diff(0x15) < 3 && p[1].abs_diff(0x1b) < 3 && p[2].abs_diff(0x23) < 3;
    let row = strip.width * 4;
    let room = eng.eval("(() => { const p = document.querySelector('pre'); return String(p.offsetHeight - p.clientHeight); })()").unwrap();
    check("c1_no_native_bar", room == "0" && strip.rgba.chunks(row).all(|r| r[(8.0 * s) as usize * 4..row - (8.0 * s) as usize * 4].chunks(4).all(inside)));
    eng.eval("document.querySelector('mddog-hbars').style.display = ''").unwrap();
    let wide = center(&eng, "pre");
    let short = center(&eng, "pre:not(.highlight pre)");
    check("c2_short_pre_does_not_scroll", eng.eval(&format!("mddogCode.scroll({}, {}, 50)", short.0, short.1)).unwrap() == "[]");
    let pixels = eng.snapshot(0.0, 200.0, s).unwrap().rgba.to_vec();
    let rects = eng.eval(&format!("mddogCode.scroll({}, {}, 50)", wide.0, wide.1)).unwrap();
    let pre_box = eng.eval("(() => { const r = document.querySelector('pre').getBoundingClientRect(); return JSON.stringify([[r.left + scrollX, r.top + scrollY, r.width, r.height]]); })()").unwrap();
    check("c2_scroll_reports_pre", rects == pre_box);
    check("c2_scroll_moves_code", eng.eval("String(document.querySelector('pre').scrollLeft)").unwrap() == "50");
    let after: Vec<[f64; 4]> = serde_json::from_str(&eng.eval(bars).unwrap()).unwrap();
    check("c2_scroll_moves_bar", after.len() == 1 && after[0][2] > before[0][2] && after[0][3] == before[0][3]);
    next_frame(&eng);
    check("c2_scroll_repaints", eng.snapshot(0.0, 200.0, s).unwrap().rgba.to_vec() != pixels);
    eng.eval(&format!("mddogCode.scroll({}, {}, 100000)", wide.0, wide.1)).unwrap();
    let end: Vec<[f64; 4]> = serde_json::from_str(&eng.eval(bars).unwrap()).unwrap();
    check("c2_bar_ends_at_track_end", (end[0][2] + end[0][3] - (end[0][0] + end[0][1])).abs() < 0.5);
    check("c2_scroll_stops_at_end", eng.eval(&format!("mddogCode.scroll({}, {}, 50)", wide.0, wide.1)).unwrap() == "[]");
    let left = "String(document.querySelector('pre').scrollLeft)";
    let (_, by) = center(&eng, "mddog-hbar");
    let (tx, _) = center(&eng, "mddog-hbar > span");
    check("c4_grab_misses_code", eng.eval(&format!("mddogCode.grab({}, {}, 8)", wide.0, wide.1)).unwrap().is_empty());
    check("c4_grab_thumb_stays", eng.eval(&format!("mddogCode.grab({tx}, {}, 8)", by - 7.0)).unwrap() == "[]");
    check("c4_drag_to_start", eng.eval(&format!("mddogCode.drag({})", tx - before[0][1])).unwrap() == pre_box && eng.eval(left).unwrap() == "0");
    let end_x = before[0][0] + before[0][1] - 1.0;
    check("c4_grab_track_jumps", eng.eval(&format!("mddogCode.grab({end_x}, {by}, 8)")).unwrap() == pre_box && eng.eval(left).unwrap() != "0");
    let max: f64 = eng.eval("(() => { const p = document.querySelector('pre'); return String(p.scrollWidth - p.clientWidth); })()").unwrap().parse().unwrap();
    check("c4_track_end_is_end", (eng.eval(left).unwrap().parse::<f64>().unwrap() - max).abs() < 1.0);
    eng.eval(&format!("mddogCode.drag({})", end_x - 10.0)).unwrap();
    let mid: f64 = eng.eval(left).unwrap().parse().unwrap();
    check("c4_drag_follows_pointer", (mid - (max - 10.0 * max / (before[0][1] - before[0][3]))).abs() < 1.0);
    eng.eval("mddogCode.release()").unwrap();
    check("c4_release_lets_go", eng.eval("mddogCode.drag(0)").unwrap() == "[]" && eng.eval(left).unwrap() == mid.to_string());
    eng.eval("mddogCode.wrap(true); mddogLayout()").unwrap();
    let wrapped = "(() => { const p = document.querySelector('pre'); return String(p.scrollWidth <= p.clientWidth && !document.querySelector('mddog-hbar')); })()";
    check("c3_wrap_removes_bar", eng.eval(wrapped).unwrap() == "true");
    eng.eval("mddogCode.wrap(false); mddogLayout()").unwrap();
    check("c3_unwrap_restores_bar", eng.eval("String(document.querySelectorAll('mddog-hbar').length)").unwrap() == "1");
    load_page(&mut eng, &format!("```rust\nlet x = \"{long}\";\n```\n"), 600.0, true, Mermaid::Pele);
    eng.eval("mddogLayout()").unwrap();
    check("c3_wrap_option_wraps", eng.eval(wrapped).unwrap() == "true");
    load(&mut eng, &format!("{}```\n{long}\n```\n", page(300)), 600.0);
    let doc: f64 = eng.eval("String(document.documentElement.scrollHeight)").unwrap().parse().unwrap();
    eng.resize(600.0, doc);
    eng.eval("mddogLayout()").unwrap();
    let far =
        eng.eval("(() => { const r = document.querySelector('pre').getBoundingClientRect(); return JSON.stringify([r.left + scrollX + r.width / 2, r.top + scrollY + r.height / 2]); })()").unwrap();
    let far: Vec<f64> = serde_json::from_str(&far).unwrap();
    eng.snapshot(0.0, 100.0, s);
    check("c2_scroll_below_view", eng.eval(&format!("mddogCode.scroll({}, {}, 50)", far[0], far[1])).unwrap() != "[]");

    let table = "| a | b | c | d | e | f | g | h |\n|---|---|---|---|---|---|---|---|\n| aaaaaaaaaaaaaaaaaaaa | bbbbbbbbbbbbbbbbbbbb | cccccccccccccccccccc | dddddddddddddddddddd | eeeeeeeeeeeeeeeeeeee | ffffffffffffffffffff | gggggggggggggggggggg | needle |\n";
    load(&mut eng, &format!("{table}\n```\n{long}\n```\n"), 400.0);
    eng.eval("mddogLayout()").unwrap();
    check("w1_wide_table_gets_bar", eng.eval("String(document.querySelectorAll('mddog-hbar').length)").unwrap() == "2");
    let moved: Vec<script::Rect> = serde_json::from_str(&eng.eval("mddogCode.scrollView(0, 100000, 40)").unwrap()).unwrap();
    check("w2_scroll_view_moves_both", moved.len() == 2 && eng.eval("String(document.querySelector('table').scrollLeft)").unwrap() == "40");
    check("w2_scroll_view_misses_range", eng.eval("mddogCode.scrollView(100000, 200000, 40)").unwrap() == "[]");
    check("w3_positions", eng.eval("mddogCode.positions()").unwrap() == "[40,40]");
    eng.eval("mddogCode.scrollView(0, 100000, -1000)").unwrap();
    check("w3_restore", eng.eval("mddogCode.restore('[25,30]').length > 2 && String(document.querySelector('table').scrollLeft + document.querySelector('pre').scrollLeft)").unwrap() == "55");
    eng.eval("mddogCode.scrollView(0, 100000, -1000)").unwrap();
    let rects: Vec<script::Rect> = serde_json::from_str(&eng.eval("mddogSearch.run('needle', 0); mddogSearch.setCurrent(0)").unwrap()).unwrap();
    check("v2_search_scrolls_table", rects.len() >= 2 && eng.eval("String(document.querySelector('table').scrollLeft > 0)").unwrap() == "true");
    let m = matches(&mut eng, "Apple apple APPLE\n", 600.0, "Apple");
    check("v6_smart_case", m.len() == 1);

    // A reload of a simple page swaps its article in place, which must give what a fresh load of it gives.
    let next = format!("# Two\n\n{}| a | b |\n|---|---|\n| [x](https://example.com) | y |\n\n```\nplain\n```\n", page(40));
    load(&mut eng, &next, 600.0);
    let fresh = eng.eval("mddogLayout()").unwrap();
    load(&mut eng, "# One\n\n```rust\nfn main() {}\n```\n\nsome text\n", 600.0);
    let copy = center(&eng, "clipboard-copy");
    hover_at(&eng, copy);
    press_at(&eng, copy);
    eng.search("text", None);
    eng.eval("getSelection().selectAllChildren(document.body); mddog.state.remote = true; ''").unwrap();
    page::swap(&eng, &render(&next).html).unwrap();
    check("l1_swap_lays_out_as_a_load", eng.eval("mddogLayout()").unwrap() == fresh);
    let reset = "String(getSelection().isCollapsed && mddogMouse.resetCopy() === '[]' && CSS.highlights.get('mddog-match').size === 0 && mddog.state.remote === false)";
    check("l1_swap_drops_page_state", eng.eval(reset).unwrap() == "true");
    // The old article's images keep loading after a swap, and must not mark the new page's remote images done.
    load(&mut eng, "![x](http://127.0.0.1/x.png)\n", 600.0);
    eng.eval("mddog.loadRemote(5000); ''").unwrap();
    page::swap(&eng, &render("plain\n").html).unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    check("l1_swap_ignores_old_images", eng.eval("String(mddog.state.remote)").unwrap() == "false");

    let md = "# Spare\n\nSome *text* and `code`.\n";
    load(&mut eng, md, 600.0);
    let local = eng.snapshot(0.0, 100.0, s).unwrap().rgba.to_vec();
    let (a, b) = std::os::unix::net::UnixStream::pair().unwrap();
    let viewer = std::thread::spawn(move || {
        let c = mddog::engine::spare::Client::over(a)?;
        let doc = c.render(md, std::path::Path::new("."))?;
        let html = page::build(
            &doc.html,
            &page::Opts {
                theme: mddog::theme::builtin("github-dark").unwrap(),
                base: Path::new("/tmp"),
                pad_x: 0.0,
                pad_y: 0.0,
                math: false,
                mermaid: false,
                engine: Default::default(),
                wrap: false,
                runtime: None,
            },
        );
        let tmp = std::env::temp_dir().join(format!("mddog-spare-test-{}.html", std::process::id()));
        std::fs::write(&tmp, html).ok()?;
        let mut eng = Engine::Remote(c);
        eng.resize(600.0, 600.0);
        eng.load_file(&tmp, true);
        loaded(&eng);
        wait(&eng, "document.fonts.status === 'loaded'");
        let _ = std::fs::remove_file(&tmp);
        Some((eng.eval("String(1 + 1)").ok()?, eng.snapshot(0.0, 100.0, s)?.rgba.to_vec()))
    });
    mddog::engine::spare::serve_on(&mut eng, b, |src, _| Some(render(src)));
    check("spare_serves_a_viewer", viewer.join().unwrap().is_some_and(|(two, px)| two == "2" && px == local));

    // A quit gives up on a busy page after its short limit, not the full call limit. It leaves the engine stalled, so it runs last.
    eng.eval("setTimeout(() => { const t = Date.now() + 2000; while (Date.now() < t); }); ''").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let t = Instant::now();
    let r = eng.eval_within("'x'", Duration::from_millis(20));
    check("eval_within_gives_up_on_a_busy_page", r.is_err() && t.elapsed() < Duration::from_millis(500) && eng.stalled());

    let _ = std::fs::remove_file(std::env::temp_dir().join(format!("mddog-search-test-{}.html", std::process::id())));
    if failed > 0 {
        eprintln!("{failed} search test(s) failed");
        std::process::exit(1);
    }
}
