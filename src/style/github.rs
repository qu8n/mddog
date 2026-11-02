//! The GitHub style: the markup github.com's repo view puts around rendered Markdown, and the page and base CSS that every theme builds on. It also holds the CSS and script the obsidian Mermaid engine needs. The github.com captures in `reference/` test this style.

use std::collections::HashMap;

use kuchikiki::NodeRef;

use crate::config;
use crate::page::{self, Opts, file_url, percent_path};
use crate::render::dom::*;
use crate::render::markdown::escape;
use crate::render::octicons;
use crate::theme::{self, Theme};

const CSS: &str = include_str!("../../assets/github.css");
const EXTRA_CSS: &str = concat!(
    "html{overflow:hidden!important}::highlight(mddog-match){background-color:rgba(210,153,34,0.4)}::highlight(mddog-current){background-color:#f2cc60;color:#0d1117}::selection{background-color:rgba(56,139,253,0.4)}",
    "img[data-mddog-src],img.mddog-loading{border-radius:6px;background:var(--bgColor-neutral-muted)}img[data-mddog-src]:not([width]):not([height]),img.mddog-loading:not([width]):not([height]){width:88px;height:20px}.mddog-image-missing[data-why]::after{content:attr(data-why);opacity:.7}",
    ".mddog-image-missing{display:inline-flex;align-items:center;gap:6px;max-width:100%;padding:4px 8px;border:1px dashed var(--borderColor-default);border-radius:6px;color:var(--fgColor-muted);font-size:12px;line-height:18px;vertical-align:middle}",
    ".js-clipboard-copy.tooltipped:after{display:inline-block;opacity:1}.js-clipboard-copy{transition:none!important}.btn.js-clipboard-copy.mddog-hover{background-color:var(--button-default-bgColor-hover,var(--color-btn-hover-bg));border-color:var(--button-default-borderColor-hover,var(--color-btn-hover-border))}",
    ".markdown-body pre,.markdown-body table{scrollbar-width:none}.markdown-body pre::-webkit-scrollbar,.markdown-body table::-webkit-scrollbar{display:none}.markdown-body table.mddog-scrolls{padding-bottom:12px}mddog-hbars{position:absolute;top:0;left:0;pointer-events:none}mddog-hbar{position:absolute;border-radius:3px;background:var(--bgColor-neutral-muted)}mddog-hbar>span{position:absolute;top:0;bottom:0;border-radius:3px;background:var(--borderColor-emphasis)}",
    ".mddog-wrap .markdown-body pre,.mddog-wrap .markdown-body pre code{white-space:pre-wrap!important;overflow-wrap:anywhere!important}",
    // GitHub makes inline math an inline-block, which WebKit sizes from a wrong width estimate for stretchy symbols.
    ".js-inline-math{display:inline!important}.js-inline-math,.js-inline-math>math{vertical-align:baseline;overflow:visible}",
    "math-renderer merror{background:var(--bgColor-danger-muted)!important;color:var(--fgColor-danger)!important;border:1px solid var(--borderColor-danger-muted)!important;border-radius:var(--borderRadius-small);padding:0 .3em;font-family:var(--fontStack-monospace);font-size:.85em}",
    "mddog-expanded{position:absolute;left:0;right:0;z-index:20;display:flex;box-sizing:border-box;padding:16px;background:inherit}mddog-expanded>div{flex:1;display:flex;align-items:center;justify-content:center;min-width:0;min-height:0}mddog-expanded svg{width:100%;max-width:none;max-height:100%}",
    ".markdown-body .data-footnote-backref{font-family:\"Apple Color Emoji\";line-height:1}",
    ".markdown-body input.task-list-item-checkbox{-webkit-appearance:none;appearance:none;width:13px;height:13px;box-sizing:border-box;border:1px solid #d1d1d1;border-radius:2px;background:#f8f8f8 no-repeat center/13px 13px}",
    ".markdown-body input.task-list-item-checkbox:checked{border-color:#d1d1d1;background-color:#d1d1d1;background-image:url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 13 13'%3E%3Cpath d='M3 6.6 5.4 9 10 4.2' fill='none' stroke='%23ededed' stroke-width='1.9'/%3E%3C/svg%3E\")}",
    "@media (prefers-color-scheme:dark){.markdown-body input.task-list-item-checkbox{border-color:#626262;background-color:#3b3b3b}.markdown-body input.task-list-item-checkbox:checked{border-color:#757575;background-color:#757575;background-image:url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 13 13'%3E%3Cpath d='M3 6.6 5.4 9 10 4.2' fill='none' stroke='%233b3b3b' stroke-width='1.9'/%3E%3C/svg%3E\")}}"
);

const MERMAID_CSS: &str = concat!(
    ".pele{--pele-bg:var(--mddog-bg);--pele-fg:var(--fgColor-default);--pele-muted:var(--fgColor-muted);--pele-line:var(--fgColor-muted);--pele-surface:var(--bgColor-muted);--pele-surface-alt:var(--bgColor-neutral-muted);--pele-border:var(--borderColor-default);--pele-accent:var(--fgColor-accent);--pele-error:var(--fgColor-danger);--pele-font:var(--fontStack-sansSerif);--pele-font-mono:var(--fontStack-monospace);--pele-radius:var(--borderRadius-medium)}",
    ".pele-zoom{--pele-bg:var(--bgColor-muted);--pele-zoom-top:auto;--pele-zoom-right:0;--pele-zoom-bottom:0;flex-direction:row!important}",
    ".render-container[data-type=mermaid]:has(.pele-zoom){padding-right:48px}.render-container[data-type=mermaid] div:has(>.pele-zoom){padding-bottom:32px}",
    ".pele-error{text-align:left;margin-right:48px!important;white-space:pre-wrap!important;padding:var(--base-size-12) var(--base-size-16);border:1px solid var(--borderColor-danger-muted);border-radius:var(--borderRadius-medium);background:var(--bgColor-danger-muted)}",
    ".pele-error:before{content:'Unable to render Mermaid diagram';display:block;margin-bottom:var(--base-size-8);font:600 14px var(--fontStack-sansSerif)}section[data-type=mermaid] .js-render-block-actions{z-index:1}",
    ".render-container[data-type=mermaid]{padding:var(--base-size-16) 0;background:transparent;line-height:normal}"
);

const SKELETON_CSS: &str = concat!(
    ".mddog-loading{position:relative}.mddog-loading .render-viewer{visibility:hidden}.js-render-block-actions:has(~.mddog-loading){z-index:1}",
    ".mddog-skeleton{position:absolute;inset:0;display:flex;align-items:center;justify-content:center;gap:8%;border:1px solid var(--borderColor-default);border-radius:var(--borderRadius-medium);background:var(--bgColor-muted)}",
    ".mddog-skeleton span{width:16%;height:48px;border-radius:var(--borderRadius-medium);background:var(--bgColor-neutral-muted)}"
);

/// Obsidian's look: the SVG left-aligned at its own size, with no frame or buttons, and in a dark theme the same filter Obsidian puts on it.
const OBSIDIAN_CSS: &str = concat!(
    ".render-container[data-type=mermaid]{padding:0;text-align:left;background:transparent;line-height:normal}section[data-type=mermaid] .js-render-block-actions{display:none!important}",
    ".mddog-obsidian>img{display:block}.mddog-obsidian .flash{text-align:left}"
);
const OBSIDIAN_DARK_CSS: &str = ".mddog-obsidian>img{filter:invert(100%) hue-rotate(180deg) saturate(1.25)}";

const CHAIN: [&str; 5] = [
    r#"<div data-turbo-body="" class="logged-out env-production page-responsive">"#,
    r#"<div class="application-main ">"#,
    r#"<main id="js-repo-pjax-container">"#,
    r#"<div id="repo-content-pjax-container" class="repository-content ">"#,
    r#"<div class="js-snippet-clipboard-copy-unpositioned">"#,
];

pub fn dress(body: &NodeRef, front: Option<&str>) {
    let mut slugs = HashMap::new();
    if let Some(table) = front.and_then(front_matter_html) {
        for n in fragment(&table).into_iter().rev() {
            match body.first_child() {
                Some(first) => first.insert_before(n),
                None => body.append(n),
            }
        }
    }
    for node in elements(body) {
        if node.parent().is_none() {
            continue;
        }
        let tag = name(&node).unwrap_or_default();
        match tag.as_str() {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" if !has_class(&node, "sr-only") => heading(&node, &tag, &mut slugs),
            "code" | "pre" if has_class(&node, "notranslate") => remove_attr(&node, "class"),
            "table" => remove_attr(&node, "role"),
            "markdown-accessiblity-table" | "math-renderer" => set_attr(&node, "data-catalyst", ""),
            "input" if has_class(&node, "task-list-item-checkbox") => set_attr(&node, "style", ""),
            "div" if has_class(&node, "highlight-source-mermaid") => {
                let section = fragment(&mermaid_html(&node.text_contents())).remove(0);
                if let Some(pos) = attr(&node, "data-sourcepos") {
                    set_attr(&section, "data-sourcepos", &pos);
                }
                replace(&node, &section);
            }
            _ => {}
        }
    }
    for node in elements(body) {
        let tag = name(&node).unwrap_or_default();
        let auto = match tag.as_str() {
            "p" | "ol" => true,
            "ul" => !has_class(&node, "contains-task-list"),
            "div" => has_attr(&node, "align") || has_class(&node, "markdown-alert") || has_class(&node, "highlight"),
            "h2" => has_class(&node, "sr-only"),
            _ => false,
        };
        if auto {
            set_attr(&node, "dir", "auto");
        }
    }
}

/// The top-level `key: value` lines of YAML front matter, without its `---` lines, as GitHub's table shows them.
pub fn front_matter_rows(yaml: &str) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    for line in yaml.lines().take_while(|l| l.trim_end() != "---") {
        let Some((k, v)) = line.split_once(':') else { continue };
        if k.starts_with([' ', '\t', '-', '#']) || k.trim().is_empty() {
            continue;
        }
        let v = v.trim();
        let v = v.strip_prefix('"').and_then(|x| x.strip_suffix('"')).or_else(|| v.strip_prefix('\'').and_then(|x| x.strip_suffix('\''))).unwrap_or(v);
        rows.push((k.trim().to_string(), v.to_string()));
    }
    rows
}

fn front_matter_html(raw: &str) -> Option<String> {
    let yaml = raw.split_once('\n').map_or("", |(_, rest)| rest);
    let rows: String = front_matter_rows(yaml).iter().map(|(k, v)| format!("<tr>\n<th>{}</th>\n<td>{}</td>\n</tr>\n", escape(k), escape(v))).collect();
    (!rows.is_empty()).then(|| format!("<markdown-accessiblity-table data-catalyst=\"\"><table>\n<tbody>\n{rows}</tbody>\n</table></markdown-accessiblity-table>\n"))
}

fn slug(text: &str, slugs: &mut HashMap<String, usize>) -> String {
    static PUNCTUATION: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| regex::Regex::new(r"[^\p{L}\p{M}\p{Nd}\p{Pc}\- ]").expect("valid regex"));
    let base = PUNCTUATION.replace_all(&text.to_lowercase(), "").replace(' ', "-");
    let n = slugs.entry(base.clone()).or_insert(0);
    let out = if *n == 0 { base } else { format!("{base}-{n}") };
    *n += 1;
    out
}

fn heading(node: &NodeRef, tag: &str, slugs: &mut HashMap<String, usize>) {
    let text = node.text_contents();
    let text = text.trim();
    let id = slug(text, slugs);
    let wrapper = el("div", &[("class", "markdown-heading"), ("dir", "auto")]);
    if let Some(pos) = attr(node, "data-sourcepos") {
        set_attr(&wrapper, "data-sourcepos", &pos);
    }
    let h = el(tag, &[]);
    if let (Some(from), Some(to)) = (node.as_element(), h.as_element()) {
        let mut to = to.attributes.borrow_mut();
        for (k, v) in from.attributes.borrow().map.iter().filter(|(k, _)| &*k.local != "data-sourcepos") {
            to.map.insert(k.clone(), v.clone());
        }
    }
    set_attr(&h, "class", "heading-element");
    set_attr(&h, "dir", "auto");
    move_children(node, &h);
    wrapper.append(h);
    for f in fragment(&format!("<a id=\"user-content-{id}\" class=\"anchor\" aria-label=\"Permalink: {}\" href=\"#{id}\">{}</a>", escape(text), octicons::svg("link", ""))) {
        wrapper.append(f);
    }
    replace(node, &wrapper);
}

fn mermaid_html(code: &str) -> String {
    format!(
        "<section class=\"js-render-needs-enrichment render-needs-enrichment position-relative\" data-type=\"mermaid\" aria-label=\"mermaid rendered output container\"><div class=\"js-render-enrichment-target\" dir=\"auto\"><div class=\"js-render-block-actions position-absolute top-0 pr-2 right-0 d-flex flex-justify-end flex-items-center\"><details class=\"details-reset details-overlay details-overlay-dark\" style=\"display: contents\"><summary role=\"button\" aria-label=\"Open dialog\" class=\"btn my-2 mr-2 p-0 d-inline-flex\" aria-haspopup=\"dialog\">{}</summary></details><clipboard-copy class=\"btn my-2 js-clipboard-copy p-0 d-inline-flex tooltipped-no-delay\" role=\"button\" data-copy-feedback=\"Copied!\" data-tooltip-direction=\"w\" aria-label=\"Copy mermaid code\" value=\"{code}\" tabindex=\"0\">{}</clipboard-copy></div><div class=\"render-container js-render-target\" data-type=\"mermaid\"><mddog-mermaid data-code=\"{code}\"></mddog-mermaid></div></div></section>",
        octicons::MERMAID_EXPAND,
        octicons::MERMAID_COPY,
        code = escape(code),
    )
}

fn scripts(o: &Opts) -> String {
    let mut out = page::scripts(o);
    if let (true, Some(rt), config::Mermaid::Obsidian) = (o.mermaid, o.runtime, o.engine) {
        let dark = if o.theme.dark { OBSIDIAN_DARK_CSS } else { "" };
        let src = serde_json::to_string(&file_url(&rt.join("mermaid/mermaid.min.js"))).expect("json").replace("</", "<\\/");
        out.push_str(&format!("<style>{SKELETON_CSS}{OBSIDIAN_CSS}{dark}</style><script>document.addEventListener('DOMContentLoaded', () => mddog.renderObsidian({src}))</script>"));
    } else if let (true, Some(rt)) = (o.mermaid, o.runtime) {
        out.push_str(&format!(
            "<style>{MERMAID_CSS}</style><script src=\"{}\"></script><script>document.addEventListener('DOMContentLoaded', () => mddog.renderMermaid())</script>",
            file_url(&rt.join("pele/pele.global.js"))
        ));
    }
    out
}

const PRIMER_LIGHT: &str = include_str!("../../assets/primer-light.css");
const PRIMER_DARK: &str = include_str!("../../assets/primer-dark.css");

/// The theme as WebKit's page CSS: Primer's colors of its mode, then its tokens in place of the Primer variables and type that github.css reads.
fn theme_css(t: &Theme) -> String {
    let c = |name: &str| theme::hex(t.color(name));
    let fonts = |name: &str| t.fonts(name).iter().map(|f| if f.contains(' ') { format!("\"{f}\"") } else { f.clone() }).collect::<Vec<_>>().join(",");
    let mut out = String::from(if t.dark { PRIMER_DARK } else { PRIMER_LIGHT });
    out.push_str(&format!(
        ":root{{color-scheme:{};--fgColor-default:{text};--fgColor-muted:{muted};--fgColor-accent:{link};--bgColor-default:{bg};--bgColor-muted:{pre};--bgColor-neutral-muted:{code};--borderColor-default:{rule};--borderColor-muted:{rule};--color-prettylights-syntax-comment:{};--color-prettylights-syntax-keyword:{};--color-prettylights-syntax-string:{};--color-prettylights-syntax-constant:{};--color-prettylights-syntax-entity:{};--color-prettylights-syntax-markup-inserted-text:{};--color-prettylights-syntax-markup-deleted-text:{};--fontStack-monospace:{mono}}}",
        if t.dark { "dark" } else { "light" },
        c("pl-comment"),
        c("pl-keyword"),
        c("pl-string"),
        c("pl-constant"),
        c("pl-entity"),
        c("pl-inserted"),
        c("pl-deleted"),
        text = c("text"),
        muted = c("text-muted"),
        link = c("link"),
        bg = c("bg"),
        pre = c("pre-bg"),
        code = c("code-bg"),
        rule = c("rule"),
        mono = fonts("font-code"),
    ));
    let scale = t.heading_scale();
    out.push_str(&format!(".markdown-body{{font-family:{};line-height:{}}}", fonts("font-body"), t.number("line-height")));
    // Headings 1 to 5 take the heading color, and 6 keeps GitHub's muted one.
    for (i, em) in scale.iter().enumerate() {
        let color = if i < 5 { c("heading") } else { c("text-muted") };
        out.push_str(&format!(".markdown-body h{}{{font-size:{em}em;color:{color}}}", i + 1));
    }
    out.push_str(&format!(
        ".markdown-body code,.markdown-body tt,.markdown-body pre,.markdown-body .highlight pre{{font-size:{}em}}.markdown-body pre code{{font-size:100%}}.markdown-body table tr:nth-child(2n){{background-color:{}}}",
        t.number("code-size"),
        c("alt-bg")
    ));
    out
}

pub fn page(article: &str, o: &Opts) -> String {
    let mut base = o.base.to_string_lossy().into_owned();
    if !base.ends_with('/') {
        base.push('/');
    }
    let open: String = CHAIN.concat();
    let close: String = CHAIN.iter().map(|t| if t.starts_with("<main") { "</main>" } else { "</div>" }).collect();
    format!(
        r#"<!DOCTYPE html><html lang="en" data-color-mode="auto" data-light-theme="light" data-dark-theme="dark" data-a11y-animated-images="system" data-a11y-link-underlines="true"{wrap}><head><meta charset="utf-8"><base href="file://{}"><style>{CSS}</style><style>{extra}</style>{scripts}<style>{theme}</style><style>:root{{--mddog-bg:{bg}}}html,body{{margin:0!important;padding:0!important;background:{bg}!important;min-width:0!important}}body>div,body>div div:not(.markdown-body *),body main{{margin:0!important;padding:0!important;border:0!important;max-width:none!important;width:auto!important;background:{bg}!important}}article.markdown-body{{margin:0!important;padding:{py}px {px}px!important;max-width:none!important}}</style></head><body class="logged-out env-production page-responsive">{open}<article class="markdown-body entry-content container-lg" itemprop="text">{article}</article>{close}</body></html>"#,
        percent_path(&base),
        scripts = scripts(o),
        extra = EXTRA_CSS,
        theme = theme_css(o.theme),
        wrap = if o.wrap { r#" class="mddog-wrap""# } else { "" },
        bg = theme::hex(o.theme.palette.bg),
        px = o.pad_x,
        py = o.pad_y,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn built(t: &Theme) -> String {
        let o = Opts { theme: t, base: Path::new("/"), pad_x: 0.0, pad_y: 0.0, math: false, mermaid: false, engine: config::Mermaid::Obsidian, wrap: false, runtime: None };
        page("<p>x</p>", &o)
    }

    #[test]
    fn themes_sit_on_primer_colors_of_their_mode() {
        let light = theme::builtin("github-light").unwrap();
        let html = built(light);
        assert!(html.contains(PRIMER_LIGHT) && !html.contains(PRIMER_DARK));
        let mine = theme::parse("mine.css", ":root{color-scheme:dark;--mddog-bg:#101010;--mddog-link:#abcdef;--mddog-font-body:\"My Font\", system-ui}").unwrap();
        let html = built(&mine);
        let (under, own) = (html.find(PRIMER_DARK).unwrap(), html.find("--fgColor-accent:#abcdef").unwrap());
        assert!(under < own && !html.contains(PRIMER_LIGHT));
        assert!(html.contains("--bgColor-default:#101010") && html.contains("font-family:\"My Font\",system-ui"));
    }

    #[test]
    fn obsidian_engine_inverts_only_in_dark_themes() {
        let scripts_for = |name: &str| {
            let t = theme::builtin(name).unwrap();
            scripts(&Opts { theme: t, base: Path::new("/"), pad_x: 0.0, pad_y: 0.0, math: false, mermaid: true, engine: config::Mermaid::Obsidian, wrap: false, runtime: Some(Path::new("/rt")) })
        };
        for (name, dark) in [("obsidian-light", false), ("obsidian-dark", true), ("github-dark", true)] {
            let s = scripts_for(name);
            assert!(s.contains("mddog.renderObsidian(\"file:///rt/mermaid/mermaid.min.js\")") && !s.contains("<iframe"), "{name}");
            assert_eq!(s.contains(OBSIDIAN_DARK_CSS), dark, "{name}");
        }
    }
}
