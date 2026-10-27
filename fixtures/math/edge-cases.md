# Math edge cases

Written for mddog. Each line tries a pattern that real files use.

## Inline

The loss is $L = \sum_i (y_i - \hat{y}_i)^2$ over all samples, with $\alpha_t$ and $\beta_t$ fixed.

Emphasis next to math: *about $x_1$* and **$y^*$ here** and _$z_2$_.

Stars in math: $a * b * c$ and $x^* + y^*$ and $\mathbf{x}_i^*$.

Underscores across math: $a_1$ and $b_2$ and $c_3$ on one line.

Braces and spaces: $\{x \mid x > 0\}$, $\left\{ a \right\}$, $a\ b$, $a\;b\!c$.

Backslash punctuation: $\#$, $\%$, $\&$, $\_$, $\$$, $\|x\|$, $a\\b$.

Dollar next to digits: $5 and $10, but $n$ items cost $3n$ dollars.

Money then math: it costs $5. The value is $x$.

Escaped dollars: \$5 and \$10, with $x$ math and \$ alone.

Escaped dollar in math: $\$x$ and $a \$ b$.

Math after a word: x$y$ and $y$x and ($z$) and [$w$].

Dollar pairs with spaces: $ x $ and $x $ and $ x$.

Code span math: $`\sqrt{2}`$ and $`a_1 * b_2`$ and `$x$` stays code.

Math with HTML-like text: $a<b$ and $a < b > c$ and $x &lt; y$.

Math across lines: $a +
b$ ends here.

Inline display: $$x^2$$ in a sentence, and $$a_1 * b_2$$.

Hard break at the end of math: $x$  
next line.

Link next to math: [the paper](https://example.com) shows $x_1$, and [$y_2$](https://example.com).

Math in a link title-ish: see [$\alpha$-stable](https://example.com/a_b_c).

URL in text next to math: https://example.com/a_b $x_1$.

## Display

$$
\begin{aligned}
f(x) &= x^2 + 2x + 1 \\
     &= (x+1)^2
\end{aligned}
$$

$$
a_1 * b_1 + a_2 * b_2
$$

$$ x = \frac{-b \pm \sqrt{b^2-4ac}}{2a} $$

$$\int_0^1 f(x)\,dx$$

Text before
$$
y = mx + b
$$
text after.

$$
\text{cost} = \$5
$$

```math
\mathbf{A}_{ij} = \sum_k B_{ik} C_{kj}
```

```math
\left\{ x \in \mathbb{R} \mid x > 0 \right\}
```

## Containers

- Item with $x_1$ math
- Item with display:

  $$
  y_1 * y_2
  $$

- Item with $`a_b`$ code math
  1. Nested $\alpha_i$ item

> Quoted $E = mc^2$ math.
>
> $$
> \nabla \cdot \mathbf{E} = \frac{\rho}{\varepsilon_0}
> $$

> [!NOTE]
> Alert with $p_i * q_i$ math.

| Symbol | Meaning |
|--------|---------|
| $\alpha_1$ | learning rate |
| $\beta_*$ | momentum $\in [0, 1)$ |
| $a \| b$ | pipe in math |
| $5 | money, not math |

Heading with math is below.

### The $L_2$ norm

Footnote with math.[^m]

[^m]: The bound $O(n \log n)$ holds for $n_1 > 0$.

<details>
<summary>Summary $x_1$</summary>

Hidden $y_2$ math.

</details>

Term $t_1$
