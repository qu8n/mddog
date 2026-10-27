# Escapes, line breaks and braces in math

Escaped dollars: \$x\$ is not math, but $a \$ b$ keeps a dollar sign inside math, and \$5 is money.

Braces in a set: $S = \{1, 2, 3\}$ and $\{x \mid x > 0\}$.

Underscores and stars: $a_i * b_i = c_{ij}^*$ and $x_1 + x_2 + \dots + x_n$ with $f*g$ and $*$ stars in a sentence: *emphasis* and _more emphasis_ outside, a_b_c and 2*3*4 outside.

Two math spans on a line: $a_1$ and $b_2$, then _emphasis between $c_3$ and $d_4$ spans_.

Line breaks in display math:

$$
\begin{aligned}
f(x) &= (x+1)^2 \\
     &= x^2 + 2x + 1 \\
\end{aligned}
$$

Line breaks via a bare `\\` in a matrix:

$$
\begin{pmatrix} a & b \\ c & d \end{pmatrix}
$$

Display on one line: $$E = mc^2$$ inside a paragraph, and $$\sum_{i=1}^n i = \frac{n(n+1)}{2}$$ again.

$$ \int_0^1 x^2\,dx = \tfrac13 $$

Code-span math: $`\sqrt{2}`$ and $`a_1 * b_2`$, plus ``$`x`$`` and an actual `$x$` code span.

```math
\left\{ \begin{array}{ll} 1 & x > 0 \\ 0 & x \le 0 \end{array} \right.
```
