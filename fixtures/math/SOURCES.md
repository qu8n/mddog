# Math fixture sources

Each upstream file keeps its upstream license and copyright. Files are verbatim copies (or noted excerpts) pinned to the commit SHA shown, and are here only as test inputs. Files marked "written for mddog" are our own and CC0.

| File | Source (pinned) | License | Features | Full / excerpt |
|---|---|---|---|---|
| `meltcalc-theory.md` | https://github.com/robertsamples/meltcalc/blob/5f4e1ff3b643bec873a38a599e46bbd7f76e58a3/theory.md | MIT | display on own lines, inline, ```math fences, `\$` money, list, table, block quote | full (25082 bytes) |
| `kamui-readme.md` | https://github.com/yoyolicoris/kamui/blob/15162627c114a5bef000aa749b45eb40db080dd7/README.md | MIT | `$\`...\`$` code-span math, ```math fences, `\{ \}`, list, table | full (15369 bytes) |
| `ibl-atlas-readme.md` | https://github.com/int-brain-lab/atlas/blob/2369c06296b972409a2979c26f4ecc424d7bc80b/README.md | MIT | display, `\{ \}`, list, block quote, aligned/cases environments | full (15940 bytes) |
| `ai-whys-dl.md` | https://github.com/ZHYsfl/ai-whys/blob/534e0066be12659be6cc90861d81928690809c5c/dl.md | Apache-2.0 | display, inline, `_` in math, list, table, `\` line breaks | full (32332 bytes) |
| `ai-whys-rl.md` | https://github.com/ZHYsfl/ai-whys/blob/22db01deed06a0a3e1a1cef7359c67035805116c/rl.md | Apache-2.0 | display, inline, `_`/`*` in math, list, block quote | full (16631 bytes) |
| `adv-comp-methods-ic.md` | https://github.com/ph7klw76/Advanced_Computational_Methods/blob/5cc583f47d8240a0716cf0ad77eef8ef3bf09e37/IC.md | MIT | display, inline, display `$$..$$` on one line, list, table, `$` money | full (26900 bytes) |
| `ros-ch18.md` | https://github.com/ASKurz/Working-through-Regression-and-other-stories/blob/ac87539c2e901598a425875333d5af7126d818ab/18.md | CC0-1.0 | display, inline, block quote with math, `\` | full (38412 bytes) |
| `vae-readme.md` | https://github.com/tonyduan/variational-autoencoders/blob/2f6b2593880525f52097bffcca0a74c7dd2de38a/README.md | MIT | inline, ```math fences, list | full (10561 bytes) |
| `transformer-blocks-readme.md` | https://github.com/tonyduan/transformer-blocks/blob/42c0fca7b21ca0a872ede0fe714ead2bb960e141/README.md | MIT | inline, ```math fences, environments with `\` | full (10351 bytes) |
| `case-studies-07.md` | https://github.com/Mark-Kramer/Case-Studies-Python/blob/e4ffc49b09d3c1b24e061e63b1fe5774ab31e70f/07.md | MIT | display, inline, `\` line breaks, `\{ \}`, mixed HTML | full (34886 bytes) |
| `cmsc426-sfm.md` | https://github.com/cmsc426/cmsc426.github.io/blob/7aa019a24fbcd7605d3a7f638bf9d85f709bd92a/sfm.md | MIT | inline `$$..$$` on one line (many), list, `\`, mixed HTML | full (20299 bytes) |
| `quantum-logic-mre.md` | https://github.com/rhovision-org/quantum-logical-framework/blob/370f5797c34f835cca62970b6f14852682568b0d/MRE.md | MIT | display, inline, table, list, `_`/`*` in math | full (16184 bytes) |
| `silverbullet-math.md` | https://github.com/MrMugame/silverbullet-math/blob/fa8b915af1d09e43156f053ae17163e21a68c66c/Math.md | MIT | display, inline, code-span `$...$` docs, `\$`, list | full (3539 bytes) |
| `factorion-math.md` | https://github.com/tolik518/factorion-bot/blob/e5a27e856d43482a1a0dcb781772d1de7dc09a5c/MATH.md | MIT | ```math fences, `\` line breaks | full (6260 bytes) |
| `reduced-footnotes.md` | written for mddog | CC0-1.0 | inline and display math inside footnotes, repeated footnote ref | full |
| `reduced-money.md` | written for mddog | CC0-1.0 | plain `$` as money, `$5 and $10`, `\$`, money beside real math | full |
| `reduced-escapes-and-breaks.md` | written for mddog | CC0-1.0 | `\$`, `\{ \}`, `_`/`*` in math, `\` breaks, `$$..$$` inline, code-span math, math fence | full |
| `reduced-containers.md` | written for mddog | CC0-1.0 | math in lists, nested quotes, tables (incl. escaped pipe), fences in containers | full |
| `edge-cases.md` | written for mddog | CC0-1.0 | opener and closer edge cases, emphasis beside math, escapes, one-line display, containers, footnotes, `<summary>` | full |
| `html-in-math.md` | written for mddog | CC0-1.0 | HTML-like text inside math, which breaks the rest of the page on GitHub | full |

Each upstream repo's license text, with its copyright line, is in `licenses/<owner>_<repo>.txt`. The Apache-2.0 NOTICE for `ai-whys-*.md` is in `licenses/ZHYsfl_ai-whys.NOTICE.txt`.

## Differences from GitHub

mddog differs from GitHub's Markdown API on purpose for these inputs. Each was checked against the API. `\n` stands for a line break.

Kept by choice:

- Math inside italics renders. `*about $x_1$*` and `_$z_2$_`: GitHub shows raw `$x_1$`.
- Math in footnotes renders. `See.[^m]` and `[^m]: The bound $O(n)$ holds.`: GitHub shows raw dollars.
- `\$` stays a literal dollar. On GitHub, `\$x\$ is` gives math `$x$`, `Price \$k$ here` gives math `$k$`, and `$a \$ b$` is not math.

GitHub bugs not copied:

1. `_` and `*` in math become emphasis when a second delimiter on the same line pairs with them. `Stars: $x^* + y^*$ and $z^*$ end.` gives `$x^* + y^<em>$ and $z^</em>$`. In `Sets: $|\mathcal{T}_+| = |\mathcal{T}_-| = N/2$: end.`, the `_` after `}` opens emphasis. `$a_1 + b_2$` and `$x_i * y_j * z$` are fine.
2. Backslashes before punctuation are dropped. `Braces: $\{x\}$ and $a\,b$.` gives `${x}$` and `$a,b$`. In display math `\\` survives, but `\,` and `\$` don't: `$$\n\text{cost} = \$5\n$$` gives `$5`.
3. Display math in a list that already holds inline math doesn't render. `- $a$\n- b:\n\n  $$\n  y\n  $$\n` gives `<p>$$<br>y<br>$$</p>`. Without the `$a$` item, the block renders.
4. HTML-like text in math breaks the rest of the page. `Tag: $<div>$ here.\n\nAfter: $x_1$.` puts everything after it inside the `<div>`.
5. `<br>` after display math comes back escaped. `cmsc426-sfm.md` lines 100 and 101 (`$$\begin{equation}x_i x'_i f_{11} + … + f_{33}=0\end{equation}$$`, then `<br><br>`) give the text `&lt;br&gt;` after the math. `$$x$$\n<br><br>` doesn't trigger it, and there the API drops the breaks. github.com itself showed a third result (the `$$` block stayed raw when text followed), so this case rests on the API alone. mddog renders the math and keeps real breaks.
