# Notes on the central limit theorem

The sample mean $\bar{X}_n$ of $n$ independent draws converges in distribution.[^clt] The rate depends on the variance $\sigma^2$.[^rate]

Here is a second reference to the first note.[^clt]

[^clt]: For i.i.d. $X_i$ with mean $\mu$ and variance $\sigma^2 < \infty$,

    $$\sqrt{n}\,(\bar{X}_n - \mu) \xrightarrow{d} \mathcal{N}(0, \sigma^2).$$

[^rate]: The Berry-Esseen bound gives $\sup_x |F_n(x) - \Phi(x)| \le \frac{C \rho}{\sigma^3 \sqrt{n}}$ with $C < 0.48$, where $\rho = E|X - \mu|^3$.
