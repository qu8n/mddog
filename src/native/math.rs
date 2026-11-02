//! TeX math: a small parser and box layout. Draws with STIX glyphs and rules.
use crate::native::text::{BOLD, FKey, Fonts, ITALIC, MATH};

thread_local! {
    /// Commands the parser did not know, since the last `take_unknown`. Spike instrumentation.
    static UNKNOWN: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn take_unknown() -> Vec<String> {
    UNKNOWN.with(|u| std::mem::take(&mut *u.borrow_mut()))
}

pub enum MItem {
    Text {
        s: String,
        f: FKey,
        x: f32,
        y: f32,
    },
    Rule {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    },
    /// Stroked polyline, coordinates relative to the box origin.
    Path {
        pts: Vec<(f32, f32)>,
        w: f32,
    },
}

pub struct MBox {
    pub w: f32,
    pub asc: f32,
    pub desc: f32,
    pub items: Vec<MItem>,
}

/// TeX's atom classes, which set the space between neighbours.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Cls {
    Ord,
    Op,
    Bin,
    Rel,
    Open,
    Close,
    Punct,
    Inner,
}

/// Where a big operator puts its scripts: beside, above and below in display style, or always above and below.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Lim {
    Side,
    Display,
    Always,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Acc {
    Hat,
    WideHat,
    Tilde,
    WideTilde,
    Vec,
    Dot,
    Ddot,
    Bar,
    Under,
    ArrowR,
    ArrowL,
    ArrowLR,
    OverBrace,
    UnderBrace,
}

#[derive(Debug, Clone)]
enum N {
    Sym(String, Cls, bool),
    Group(Vec<N>),
    /// `style`: `Some(true)` for `\dfrac`, `Some(false)` for `\tfrac`. `bar` is false for `\binom`.
    Frac {
        num: Box<N>,
        den: Box<N>,
        style: Option<bool>,
        bar: bool,
    },
    Sqrt(Option<Box<N>>, Box<N>),
    Script {
        base: Box<N>,
        sup: Option<Box<N>>,
        sub: Option<Box<N>>,
    },
    Text(String),
    Font(u8, Vec<N>),
    /// `\displaystyle` (true) or `\textstyle` (false) for the rest of the group.
    Style(bool, Vec<N>),
    /// `\mathrel{…}` and its kin.
    Class(Cls, Box<N>),
    Delim(String, String, Vec<N>),
    /// Environment, column spec, rows of cells.
    Matrix(String, String, Vec<Vec<Vec<N>>>),
    Space(f32),
    Accent(Acc, Box<N>),
    BigOp(String, Lim),
    Name(String, Lim),
    /// `\overset`, `\underset`, `\stackrel`.
    Stack {
        base: Box<N>,
        over: Option<Box<N>>,
        under: Option<Box<N>>,
    },
    /// `\xrightarrow[under]{over}`, or leftwards.
    XArrow {
        left: bool,
        over: Box<N>,
        under: Option<Box<N>>,
    },
    Logo(&'static str),
}

struct P {
    c: Vec<char>,
    i: usize,
}

fn greek(n: &str) -> Option<&'static str> {
    Some(match n {
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ϵ",
        "varepsilon" => "ε",
        "zeta" => "ζ",
        "eta" => "η",
        "theta" => "θ",
        "vartheta" => "ϑ",
        "iota" => "ι",
        "kappa" => "κ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "xi" => "ξ",
        "pi" => "π",
        "varpi" => "ϖ",
        "rho" => "ρ",
        "varrho" => "ϱ",
        "sigma" => "σ",
        "varsigma" => "ς",
        "tau" => "τ",
        "upsilon" => "υ",
        "phi" => "ϕ",
        "varphi" => "φ",
        "chi" => "χ",
        "psi" => "ψ",
        "omega" => "ω",
        "Gamma" => "Γ",
        "Delta" => "Δ",
        "Theta" => "Θ",
        "Lambda" => "Λ",
        "Xi" => "Ξ",
        "Pi" => "Π",
        "Sigma" => "Σ",
        "Upsilon" => "Υ",
        "Phi" => "Φ",
        "Psi" => "Ψ",
        "Omega" => "Ω",
        _ => return None,
    })
}

fn symbol(n: &str) -> Option<(&'static str, Cls)> {
    Some(match n {
        "le" | "leq" => ("≤", Cls::Rel),
        "ge" | "geq" => ("≥", Cls::Rel),
        "ne" | "neq" => ("≠", Cls::Rel),
        "approx" => ("≈", Cls::Rel),
        "equiv" => ("≡", Cls::Rel),
        "sim" => ("∼", Cls::Rel),
        "simeq" => ("≃", Cls::Rel),
        "propto" => ("∝", Cls::Rel),
        "ll" => ("≪", Cls::Rel),
        "gg" => ("≫", Cls::Rel),
        "to" | "rightarrow" => ("→", Cls::Rel),
        "leftarrow" | "gets" => ("←", Cls::Rel),
        "leftrightarrow" => ("↔", Cls::Rel),
        "Rightarrow" | "implies" => ("⇒", Cls::Rel),
        "Leftarrow" => ("⇐", Cls::Rel),
        "Leftrightarrow" | "iff" => ("⇔", Cls::Rel),
        "mapsto" => ("↦", Cls::Rel),
        "in" => ("∈", Cls::Rel),
        "notin" => ("∉", Cls::Rel),
        "ni" => ("∋", Cls::Rel),
        "subset" => ("⊂", Cls::Rel),
        "supset" => ("⊃", Cls::Rel),
        "subseteq" => ("⊆", Cls::Rel),
        "supseteq" => ("⊇", Cls::Rel),
        "pm" => ("±", Cls::Bin),
        "mp" => ("∓", Cls::Bin),
        "times" => ("×", Cls::Bin),
        "div" => ("÷", Cls::Bin),
        "cdot" => ("⋅", Cls::Bin),
        "ast" => ("∗", Cls::Bin),
        "star" => ("⋆", Cls::Bin),
        "circ" => ("∘", Cls::Bin),
        "cup" => ("∪", Cls::Bin),
        "cap" => ("∩", Cls::Bin),
        "wedge" | "land" => ("∧", Cls::Bin),
        "vee" | "lor" => ("∨", Cls::Bin),
        "oplus" => ("⊕", Cls::Bin),
        "otimes" => ("⊗", Cls::Bin),
        "setminus" => ("∖", Cls::Bin),
        "infty" => ("∞", Cls::Ord),
        "partial" => ("∂", Cls::Ord),
        "nabla" => ("∇", Cls::Ord),
        "forall" => ("∀", Cls::Ord),
        "exists" => ("∃", Cls::Ord),
        "emptyset" | "varnothing" => ("∅", Cls::Ord),
        "neg" | "lnot" => ("¬", Cls::Ord),
        "ell" => ("ℓ", Cls::Ord),
        "hbar" => ("ℏ", Cls::Ord),
        "Re" => ("ℜ", Cls::Ord),
        "Im" => ("ℑ", Cls::Ord),
        "aleph" => ("ℵ", Cls::Ord),
        "angle" => ("∠", Cls::Ord),
        "prime" => ("′", Cls::Ord),
        "ldots" | "dots" | "cdots" => ("…", Cls::Ord),
        "vdots" => ("⋮", Cls::Ord),
        "ddots" => ("⋱", Cls::Ord),
        "langle" => ("⟨", Cls::Open),
        "rangle" => ("⟩", Cls::Close),
        "lbrace" => ("{", Cls::Open),
        "rbrace" => ("}", Cls::Close),
        "lfloor" => ("⌊", Cls::Open),
        "rfloor" => ("⌋", Cls::Close),
        "lceil" => ("⌈", Cls::Open),
        "rceil" => ("⌉", Cls::Close),
        "vert" => ("|", Cls::Ord),
        "mid" => ("|", Cls::Rel),
        "Vert" | "lVert" | "rVert" => ("‖", Cls::Ord),
        "lvert" | "rvert" => ("|", Cls::Ord),
        "top" => ("⊤", Cls::Ord),
        "perp" => ("⊥", Cls::Rel),
        "odot" => ("⊙", Cls::Bin),
        "dagger" => ("†", Cls::Bin),
        "ddagger" => ("‡", Cls::Bin),
        "sqcup" => ("⊔", Cls::Bin),
        "longrightarrow" => ("⟶", Cls::Rel),
        "longleftarrow" => ("⟵", Cls::Rel),
        "Longrightarrow" => ("⟹", Cls::Rel),
        "triangleq" => ("≜", Cls::Rel),
        "gtrsim" => ("≳", Cls::Rel),
        "lesssim" => ("≲", Cls::Rel),
        "bigvee" => ("⋁", Cls::Op),
        "bigwedge" => ("⋀", Cls::Op),
        "backslash" => ("\\", Cls::Ord),
        "degree" => ("°", Cls::Ord),
        _ => return None,
    })
}

const FUNCS: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh", "tanh", "log", "ln", "lg", "exp", "det", "dim", "gcd", "deg", "max", "min", "sup", "inf", "lim", "limsup",
    "liminf", "arg", "ker", "hom", "Pr", "mod",
];

/// The functions that take limits above and below in display style, as in TeX.
const LIMIT_FUNCS: &[&str] = &["det", "gcd", "max", "min", "sup", "inf", "lim", "limsup", "liminf", "Pr"];

/// The negated form of a relation, for `\not`.
fn negate(s: &str) -> Option<&'static str> {
    Some(match s {
        "∈" => "∉",
        "=" => "≠",
        "⊂" => "⊄",
        "⊃" => "⊅",
        "⊆" => "⊈",
        "⊇" => "⊉",
        "≡" => "≢",
        "∼" => "≁",
        "≃" => "≄",
        "≈" => "≉",
        "<" => "≮",
        ">" => "≯",
        "≤" => "≰",
        "≥" => "≱",
        "∋" => "∌",
        "|" => "∤",
        _ => return None,
    })
}

/// Parses a whole TeX string, skipping stray closers.
fn parse_all(tex: &str) -> Vec<N> {
    let mut p = P { c: tex.chars().collect(), i: 0 };
    let mut nodes = vec![];
    while p.i < p.c.len() {
        let mut v = p.seq(&[]);
        nodes.append(&mut v);
        if p.i < p.c.len() {
            p.i += 1;
        }
    }
    nodes
}

/// `\text{…}` content: text, with `$…$` inside it read as math again.
fn text_node(s: &str) -> N {
    let s = s.replace("\\,", "\u{2009}").replace("\\ ", " ").replace("\\%", "%").replace("\\&", "&").replace("\\_", "_").replace("\\#", "#").replace("\\{", "{").replace("\\}", "}");
    if !s.contains('$') {
        return N::Text(s.replace("\\$", "$"));
    }
    let mut out = vec![];
    let mut cur = String::new();
    let mut math = false;
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' && it.peek() == Some(&'$') {
            cur.push('$');
            it.next();
        } else if c == '$' {
            if !cur.is_empty() {
                out.push(if math { N::Group(parse_all(&cur)) } else { N::Text(std::mem::take(&mut cur)) });
                cur.clear();
            }
            math = !math;
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(if math { N::Group(parse_all(&cur)) } else { N::Text(cur) });
    }
    N::Group(out)
}

impl P {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }
    fn ws(&mut self) {
        while self.peek().map(|c| c.is_whitespace()).unwrap_or(false) {
            self.i += 1;
        }
    }
    fn ident(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphabetic() {
                s.push(c);
                self.i += 1;
            } else {
                break;
            }
        }
        s
    }
    /// The command name at the cursor, without moving it.
    fn peek_command(&mut self) -> Option<String> {
        if self.peek() != Some('\\') {
            return None;
        }
        let save = self.i;
        self.i += 1;
        let name = self.ident();
        self.i = save;
        Some(name)
    }
    /// `{...}` group or a single token. A digit argument is one digit, as in `\frac12`.
    fn arg(&mut self) -> N {
        self.ws();
        match self.peek() {
            Some('{') => {
                self.i += 1;
                let v = self.seq(&['}']);
                if self.peek() == Some('}') {
                    self.i += 1;
                }
                N::Group(v)
            }
            Some(c) if c.is_ascii_digit() => {
                self.i += 1;
                N::Sym(c.to_string(), Cls::Ord, false)
            }
            Some(_) => self.atom().unwrap_or(N::Group(vec![])),
            None => N::Group(vec![]),
        }
    }
    /// `[...]` if one comes next.
    fn opt_arg(&mut self) -> Option<N> {
        self.ws();
        if self.peek() != Some('[') {
            return None;
        }
        self.i += 1;
        let v = self.seq(&[']']);
        if self.peek() == Some(']') {
            self.i += 1;
        }
        Some(N::Group(v))
    }
    fn raw_arg(&mut self) -> String {
        self.ws();
        let mut s = String::new();
        if self.peek() == Some('{') {
            self.i += 1;
            let mut depth = 1;
            while let Some(c) = self.peek() {
                self.i += 1;
                // `\{` and `\}` are characters, not nesting
                if c == '\\'
                    && let Some(e) = self.peek()
                {
                    self.i += 1;
                    s.push(c);
                    s.push(e);
                    continue;
                }
                if c == '{' {
                    depth += 1;
                } else if c == '}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                s.push(c);
            }
        }
        s
    }
    fn seq(&mut self, stop: &[char]) -> Vec<N> {
        let mut out: Vec<N> = vec![];
        loop {
            self.ws();
            let Some(c) = self.peek() else { break };
            if stop.contains(&c) {
                break;
            }
            if let Some(name) = self.peek_command() {
                if matches!(name.as_str(), "right" | "end") || (stop.contains(&'&') && self.c.get(self.i + 1) == Some(&'\\')) {
                    break;
                }
                match name.as_str() {
                    "limits" | "nolimits" => {
                        self.i += 1 + name.len();
                        let lim = if name == "limits" { Lim::Always } else { Lim::Side };
                        match out.last_mut() {
                            Some(N::BigOp(_, l) | N::Name(_, l)) => *l = lim,
                            Some(N::Script { base, .. }) => {
                                if let N::BigOp(_, l) | N::Name(_, l) = &mut **base {
                                    *l = lim;
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }
                    "rm" | "bf" | "it" | "cal" => {
                        self.i += 1 + name.len();
                        let rest = self.seq(stop);
                        let f = match name.as_str() {
                            "rm" => 0,
                            "bf" => 1,
                            "it" => 5,
                            _ => 3,
                        };
                        out.push(N::Font(f, rest));
                        continue;
                    }
                    "displaystyle" | "textstyle" => {
                        self.i += 1 + name.len();
                        let rest = self.seq(stop);
                        out.push(N::Style(name == "displaystyle", rest));
                        continue;
                    }
                    _ => {}
                }
            }
            // `x'` is `x^{\prime}`, so a prime joins the base's scripts.
            if c == '\'' {
                let mut primes = String::new();
                while self.peek() == Some('\'') {
                    self.i += 1;
                    primes.push('′');
                }
                let p = N::Sym(primes, Cls::Ord, false);
                let (sup, sub, base) = match out.pop().unwrap_or(N::Group(vec![])) {
                    N::Script { base, sup, sub } => (
                        Some(Box::new(match sup {
                            Some(s) => N::Group(vec![*s, p]),
                            None => p,
                        })),
                        sub,
                        *base,
                    ),
                    b => (Some(Box::new(p)), None, b),
                };
                out.push(N::Script { base: Box::new(base), sup, sub });
                continue;
            }
            if c == '^' || c == '_' {
                self.i += 1;
                let a = self.arg();
                let base = out.pop().unwrap_or(N::Group(vec![]));
                let (mut sup, mut sub, base) = match base {
                    N::Script { base, sup, sub } => (sup, sub, *base),
                    b => (None, None, b),
                };
                if c == '^' {
                    sup = Some(Box::new(a));
                } else {
                    sub = Some(Box::new(a));
                }
                out.push(N::Script { base: Box::new(base), sup, sub });
                continue;
            }
            match self.atom() {
                Some(n) => out.push(n),
                None => break,
            }
        }
        out
    }
    fn atom(&mut self) -> Option<N> {
        self.ws();
        let c = self.peek()?;
        self.i += 1;
        Some(match c {
            '{' => {
                let v = self.seq(&['}']);
                if self.peek() == Some('}') {
                    self.i += 1;
                }
                N::Group(v)
            }
            '\\' => return Some(self.command()),
            '+' | '-' => N::Sym(if c == '-' { "−".into() } else { "+".into() }, Cls::Bin, false),
            '*' => N::Sym("∗".into(), Cls::Bin, false),
            '=' | '<' | '>' => N::Sym(c.to_string(), Cls::Rel, false),
            ':' => N::Sym(":".into(), Cls::Rel, false),
            '(' | '[' => N::Sym(c.to_string(), Cls::Open, false),
            ')' | ']' => N::Sym(c.to_string(), Cls::Close, false),
            ',' | ';' => N::Sym(c.to_string(), Cls::Punct, false),
            '!' | '?' | '.' => N::Sym(c.to_string(), Cls::Ord, false),
            '|' => N::Sym("|".into(), Cls::Ord, false),
            '\'' => N::Sym("′".into(), Cls::Ord, false),
            '~' => N::Space(0.33),
            '&' | '}' => return None,
            c if c.is_ascii_digit() => {
                let mut s = c.to_string();
                while let Some(d) = self.peek() {
                    if d.is_ascii_digit() || d == '.' && self.c.get(self.i + 1).map(|x| x.is_ascii_digit()).unwrap_or(false) {
                        s.push(d);
                        self.i += 1;
                    } else {
                        break;
                    }
                }
                N::Sym(s, Cls::Ord, false)
            }
            c if c.is_alphabetic() => N::Sym(c.to_string(), Cls::Ord, true),
            c => N::Sym(c.to_string(), Cls::Ord, false),
        })
    }
    fn command(&mut self) -> N {
        // escaped single characters
        if let Some(c) = self.peek() {
            if !c.is_ascii_alphabetic() {
                self.i += 1;
                return match c {
                    '{' => N::Sym("{".into(), Cls::Open, false),
                    '}' => N::Sym("}".into(), Cls::Close, false),
                    '|' => N::Sym("‖".into(), Cls::Ord, false),
                    ',' => N::Space(0.17),
                    ':' | '>' => N::Space(0.22),
                    ';' => N::Space(0.28),
                    '!' => N::Space(-0.17),
                    ' ' => N::Space(0.33),
                    '$' | '%' | '#' | '&' | '_' => N::Sym(c.to_string(), Cls::Ord, false),
                    '\\' => N::Space(0.0),
                    c => N::Sym(c.to_string(), Cls::Ord, false),
                };
            }
        }
        let name = self.ident();
        let n = name.as_str();
        if let Some(g) = greek(n) {
            return N::Sym(g.into(), Cls::Ord, !n.chars().next().unwrap().is_uppercase());
        }
        if let Some((s, c)) = symbol(n) {
            return N::Sym(s.into(), c, false);
        }
        let acc = |p: &mut P, a: Acc| N::Accent(a, Box::new(p.arg()));
        match n {
            "frac" | "dfrac" | "tfrac" | "cfrac" | "binom" | "dbinom" | "tbinom" => {
                let num = Box::new(self.arg());
                let den = Box::new(self.arg());
                let style = match n {
                    "dfrac" | "cfrac" | "dbinom" => Some(true),
                    "tfrac" | "tbinom" => Some(false),
                    _ => None,
                };
                let f = N::Frac { num, den, style, bar: !n.ends_with("binom") };
                if n.ends_with("binom") { N::Delim("(".into(), ")".into(), vec![f]) } else { f }
            }
            "sqrt" => {
                let idx = self.opt_arg().map(Box::new);
                let b = self.arg();
                N::Sqrt(idx, Box::new(b))
            }
            "text" | "mbox" | "textrm" | "textnormal" | "textup" => text_node(&self.raw_arg()),
            "textit" | "textbf" => {
                let t = text_node(&self.raw_arg());
                N::Font(if n == "textbf" { 1 } else { 5 }, vec![t])
            }
            "operatorname" => {
                self.ws();
                let star = self.peek() == Some('*');
                if star {
                    self.i += 1;
                }
                let s = self.raw_arg().replace("\\,", "\u{2009}").replace("\\!", "").replace('\\', "");
                N::Name(s, if star { Lim::Display } else { Lim::Side })
            }
            "mathrm" | "mathbf" | "boldsymbol" | "bm" | "mathbb" | "mathcal" | "mathscr" | "mathit" | "mathfrak" | "mathsf" | "mathtt" => {
                let f = match n {
                    "mathrm" => 0,
                    "mathbf" => 1,
                    "mathbb" => 2,
                    "mathcal" | "mathscr" => 3,
                    "boldsymbol" | "bm" => 4,
                    "mathit" => 5,
                    "mathfrak" => 6,
                    "mathsf" => 7,
                    _ => 8,
                };
                N::Font(f, vec![self.arg()])
            }
            "left" => {
                self.ws();
                let l = self.delim_char();
                let body = self.seq(&[]);
                // expect \right
                let save = self.i;
                self.ws();
                let mut r = String::new();
                if self.peek() == Some('\\') {
                    self.i += 1;
                    if self.ident() == "right" {
                        r = self.delim_char();
                    } else {
                        self.i = save;
                    }
                }
                N::Delim(l, r, body)
            }
            "begin" => {
                let env = self.raw_arg();
                let spec = if matches!(env.as_str(), "array" | "subarray" | "alignedat" | "alignat" | "alignat*") { self.raw_arg() } else { String::new() };
                let rows = self.matrix_rows();
                N::Matrix(env, spec, rows)
            }
            "hat" => acc(self, Acc::Hat),
            "widehat" => acc(self, Acc::WideHat),
            "tilde" => acc(self, Acc::Tilde),
            "widetilde" => acc(self, Acc::WideTilde),
            "vec" => acc(self, Acc::Vec),
            "dot" => acc(self, Acc::Dot),
            "ddot" => acc(self, Acc::Ddot),
            "bar" | "overline" => acc(self, Acc::Bar),
            "underline" => acc(self, Acc::Under),
            "overrightarrow" => acc(self, Acc::ArrowR),
            "overleftarrow" => acc(self, Acc::ArrowL),
            "overleftrightarrow" => acc(self, Acc::ArrowLR),
            "overbrace" => acc(self, Acc::OverBrace),
            "underbrace" => acc(self, Acc::UnderBrace),
            "sum" => N::BigOp("∑".into(), Lim::Display),
            "prod" => N::BigOp("∏".into(), Lim::Display),
            "coprod" => N::BigOp("∐".into(), Lim::Display),
            "bigcup" => N::BigOp("⋃".into(), Lim::Display),
            "bigcap" => N::BigOp("⋂".into(), Lim::Display),
            "bigoplus" => N::BigOp("⨁".into(), Lim::Display),
            "bigotimes" => N::BigOp("⨂".into(), Lim::Display),
            "int" => N::BigOp("∫".into(), Lim::Side),
            "iint" => N::BigOp("∬".into(), Lim::Side),
            "iiint" => N::BigOp("∭".into(), Lim::Side),
            "oint" => N::BigOp("∮".into(), Lim::Side),
            "quad" => N::Space(1.0),
            "qquad" => N::Space(2.0),
            "scriptstyle" | "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl" | "biggr" | "Biggl" | "Biggr" | "bigm" | "Bigm" | "hline" | "nonumber" | "notag" => {
                N::Space(0.0)
            }
            "space" => N::Space(0.33),
            "mathbin" => N::Class(Cls::Bin, Box::new(self.arg())),
            "mathrel" => N::Class(Cls::Rel, Box::new(self.arg())),
            "mathord" => N::Class(Cls::Ord, Box::new(self.arg())),
            "mathop" => N::Class(Cls::Op, Box::new(self.arg())),
            "mathopen" => N::Class(Cls::Open, Box::new(self.arg())),
            "mathclose" => N::Class(Cls::Close, Box::new(self.arg())),
            "mathpunct" => N::Class(Cls::Punct, Box::new(self.arg())),
            "overset" | "stackrel" | "underset" => {
                let a = Box::new(self.arg());
                let base = Box::new(self.arg());
                if n == "underset" { N::Stack { base, over: None, under: Some(a) } } else { N::Stack { base, over: Some(a), under: None } }
            }
            "xrightarrow" | "xleftarrow" => {
                let under = self.opt_arg().map(Box::new);
                let over = Box::new(self.arg());
                N::XArrow { left: n == "xleftarrow", over, under }
            }
            "not" => match self.atom() {
                Some(N::Sym(s, c, v)) => match negate(&s) {
                    Some(neg) => N::Sym(neg.into(), c, v),
                    None => N::Sym(format!("{s}\u{338}"), c, v),
                },
                Some(other) => other,
                None => N::Sym("/".into(), Cls::Rel, false),
            },
            "TeX" => N::Logo("TeX"),
            "LaTeX" => N::Logo("LaTeX"),
            "KaTeX" => N::Logo("KaTeX"),
            _ if FUNCS.contains(&n) => N::Name(n.to_string(), if LIMIT_FUNCS.contains(&n) { Lim::Display } else { Lim::Side }),
            _ => {
                UNKNOWN.with(|u| u.borrow_mut().push(n.to_string()));
                N::Text(format!("\\{}", n))
            }
        }
    }
    fn delim_char(&mut self) -> String {
        self.ws();
        match self.peek() {
            Some('.') => {
                self.i += 1;
                String::new()
            }
            Some('\\') => {
                self.i += 1;
                if let Some(c) = self.peek() {
                    if !c.is_ascii_alphabetic() {
                        self.i += 1;
                        return match c {
                            '{' => "{".into(),
                            '}' => "}".into(),
                            '|' => "‖".into(),
                            c => c.to_string(),
                        };
                    }
                }
                let n = self.ident();
                symbol(&n).map(|s| s.0.to_string()).unwrap_or_default()
            }
            Some(c) => {
                self.i += 1;
                c.to_string()
            }
            None => String::new(),
        }
    }
    fn matrix_rows(&mut self) -> Vec<Vec<Vec<N>>> {
        let mut rows = vec![];
        let mut row: Vec<Vec<N>> = vec![];
        loop {
            let cell = self.seq(&['&']);
            row.push(cell);
            self.ws();
            match self.peek() {
                Some('&') => {
                    self.i += 1;
                }
                Some('\\') => {
                    let save = self.i;
                    self.i += 1;
                    if self.peek() == Some('\\') {
                        self.i += 1;
                        rows.push(std::mem::take(&mut row));
                    } else if self.ident() == "end" {
                        self.raw_arg();
                        break;
                    } else {
                        self.i = save + 1;
                    }
                }
                _ => break,
            }
        }
        if !row.is_empty() && !(row.len() == 1 && row[0].is_empty()) {
            rows.push(row);
        }
        rows
    }
}

struct Lay<'a> {
    fonts: &'a mut Fonts,
}

#[derive(Clone, Copy)]
struct St {
    size: f32,
    display: bool,
    /// Script style: inside a sub- or superscript, a limit or an inline fraction. Drops the wide spaces.
    script: bool,
    font: u8, // 0 rm, 1 bold, 2 blackboard, 3 cal, 4 bold italic, 5 italic, 6 fraktur, 7 sans, 8 mono, 255 default
}

fn mbox() -> MBox {
    MBox { w: 0.0, asc: 0.0, desc: 0.0, items: vec![] }
}

fn append(dst: &mut MBox, src: MBox, dy: f32) {
    for it in src.items {
        dst.items.push(match it {
            MItem::Text { s, f, x, y } => MItem::Text { s, f, x: x + dst.w, y: y + dy },
            MItem::Rule { x, y, w, h } => MItem::Rule { x: x + dst.w, y: y + dy, w, h },
            MItem::Path { pts, w } => MItem::Path { pts: pts.into_iter().map(|(px, py)| (px + dst.w, py + dy)).collect(), w },
        });
    }
    dst.asc = dst.asc.max(src.asc - dy);
    dst.desc = dst.desc.max(src.desc + dy);
    dst.w += src.w;
}

fn shift(b: &mut MBox, dx: f32, dy: f32) {
    for it in b.items.iter_mut() {
        match it {
            MItem::Text { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            MItem::Rule { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            MItem::Path { pts, .. } => {
                for p in pts.iter_mut() {
                    p.0 += dx;
                    p.1 += dy;
                }
            }
        }
    }
}

/// TeX's space between two atom classes, in em. Script style drops all but the thin spaces around operators.
fn class_space(l: Cls, r: Cls, script: bool) -> f32 {
    use Cls::*;
    let (thin, med, thick) = (0.167, 0.222, 0.278);
    let (v, always) = match (l, r) {
        (Ord | Close | Op, Op) | (Op, Ord) => (thin, true),
        (Ord | Op | Close | Inner, Bin) | (Bin, Ord | Op | Open | Inner) => (med, false),
        (Ord | Op | Close | Inner, Rel) | (Rel, Ord | Op | Open | Inner) => (thick, false),
        (Punct, Ord | Op | Rel | Open | Close | Punct | Inner) => (thin, false),
        (Ord | Close | Inner, Inner) | (Inner, Ord | Op | Open | Punct | Inner) => (thin, false),
        (Op, Inner) => (thin, false),
        _ => (0.0, true),
    };
    if script && !always { 0.0 } else { v }
}

/// Letters with no ascender, so an accent over them sits low.
const SHORT: &str = "acegmnopqrsuvwxyzıȷαγεηικμνοπρστυχωϵϱςϖ";

/// A tall delimiter drawn as a stroked path. Returns its width.
fn delim_path(ch: &str, x: f32, top: f32, bot: f32, size: f32) -> Option<(MItem, f32)> {
    let h = bot - top;
    let w = (0.08 * size).max(1.0);
    let mid = (top + bot) / 2.0;
    let arc = |cx: f32, cy: f32, rx: f32, ry: f32, a0: f32, a1: f32, n: usize| -> Vec<(f32, f32)> {
        (0..=n)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / n as f32;
                (cx + rx * a.cos(), cy + ry * a.sin())
            })
            .collect()
    };
    let pi = std::f32::consts::PI;
    let (pts, bw) = match ch {
        "(" => {
            let rx = (h * 0.12).min(0.5 * size);
            let k = 0.32 * size;
            (arc(x + k, mid, k, h / 2.0, pi * 0.62, pi * 1.38, 16).into_iter().collect(), rx.max(k) + 0.1 * size)
        }
        ")" => {
            let k = 0.32 * size;
            (arc(x + 0.1 * size, mid, k, h / 2.0, -pi * 0.38, pi * 0.38, 16), k + 0.2 * size)
        }
        "[" => (vec![(x + 0.25 * size, top), (x + 0.1 * size, top), (x + 0.1 * size, bot), (x + 0.25 * size, bot)], 0.35 * size),
        "]" => (vec![(x + 0.05 * size, top), (x + 0.2 * size, top), (x + 0.2 * size, bot), (x + 0.05 * size, bot)], 0.35 * size),
        "|" => (vec![(x + 0.1 * size, top), (x + 0.1 * size, bot)], 0.25 * size),
        "‖" => return None,
        "{" | "}" => {
            let q = 0.16 * size;
            let (xr, xs, xt) = if ch == "{" { (x + 0.38 * size, x + 0.24 * size, x + 0.06 * size) } else { (x + 0.07 * size, x + 0.21 * size, x + 0.39 * size) };
            let quad = |p0: (f32, f32), c: (f32, f32), p1: (f32, f32)| -> Vec<(f32, f32)> {
                (0..=6)
                    .map(|i| {
                        let t = i as f32 / 6.0;
                        let u = 1.0 - t;
                        (u * u * p0.0 + 2.0 * u * t * c.0 + t * t * p1.0, u * u * p0.1 + 2.0 * u * t * c.1 + t * t * p1.1)
                    })
                    .collect()
            };
            let mut v = quad((xr, top), (xs, top), (xs, top + q));
            v.extend(quad((xs, mid - q), (xs, mid), (xt, mid)));
            v.extend(quad((xt, mid), (xs, mid), (xs, mid + q)));
            v.extend(quad((xs, bot - q), (xs, bot), (xr, bot)));
            (v, 0.45 * size)
        }
        _ => return None,
    };
    Some((MItem::Path { pts, w }, bw))
}

/// A letter from Unicode's mathematical alphanumeric block at `base`, or the Letterlike Symbols letter that
/// fills one of its holes.
fn math_letter(c: char, upper: u32, lower: u32, holes: &[(char, char)]) -> char {
    if let Some(&(_, h)) = holes.iter().find(|(k, _)| *k == c) {
        return h;
    }
    let at = match c {
        'A'..='Z' => upper + (c as u32 - 'A' as u32),
        'a'..='z' if lower > 0 => lower + (c as u32 - 'a' as u32),
        _ => return c,
    };
    char::from_u32(at).unwrap_or(c)
}

fn blackboard(c: char) -> char {
    if c.is_ascii_digit() {
        return char::from_u32(0x1D7D8 + (c as u32 - '0' as u32)).unwrap_or(c);
    }
    math_letter(c, 0x1D538, 0x1D552, &[('C', 'ℂ'), ('H', 'ℍ'), ('N', 'ℕ'), ('P', 'ℙ'), ('Q', 'ℚ'), ('R', 'ℝ'), ('Z', 'ℤ')])
}

fn calligraphic(c: char) -> char {
    math_letter(c, 0x1D49C, 0x1D4B6, &[('B', 'ℬ'), ('E', 'ℰ'), ('F', 'ℱ'), ('H', 'ℋ'), ('I', 'ℐ'), ('L', 'ℒ'), ('M', 'ℳ'), ('R', 'ℛ'), ('e', 'ℯ'), ('g', 'ℊ'), ('o', 'ℴ')])
}

fn fraktur(c: char) -> char {
    math_letter(c, 0x1D504, 0x1D51E, &[('C', 'ℭ'), ('H', 'ℌ'), ('I', 'ℑ'), ('R', 'ℜ'), ('Z', 'ℨ')])
}

fn sans(c: char) -> char {
    if c.is_ascii_digit() {
        return char::from_u32(0x1D7E2 + (c as u32 - '0' as u32)).unwrap_or(c);
    }
    math_letter(c, 0x1D5A0, 0x1D5BA, &[])
}

fn mono(c: char) -> char {
    if c.is_ascii_digit() {
        return char::from_u32(0x1D7F6 + (c as u32 - '0' as u32)).unwrap_or(c);
    }
    math_letter(c, 0x1D670, 0x1D68A, &[])
}

/// Whether a node's letters all lack ascenders (`Some(true)`), or some have one (`Some(false)`). `None` when the
/// node is not plain letters and its box height has to do.
fn short(n: &N) -> Option<bool> {
    match n {
        N::Sym(s, _, _) => Some(s.chars().all(|c| SHORT.contains(c))),
        N::Font(2 | 3 | 6, _) => Some(false),
        N::Group(v) | N::Font(_, v) if !v.is_empty() => {
            let all: Option<Vec<bool>> = v.iter().map(short).collect();
            all.map(|a| a.iter().all(|&s| s))
        }
        N::Script { base, .. } => short(base),
        _ => None,
    }
}

fn class_of(n: &N) -> Cls {
    match n {
        N::Sym(_, c, _) | N::Class(c, _) => *c,
        N::BigOp(..) | N::Name(..) => Cls::Op,
        N::Script { base, .. } => class_of(base),
        // amsmath gives `\overset{!}{=}` the class of its base, braces and all
        N::Stack { base, .. } => match &**base {
            N::Group(v) if v.len() == 1 => class_of(&v[0]),
            b => class_of(b),
        },
        N::XArrow { .. } => Cls::Rel,
        N::Delim(..) => Cls::Inner,
        _ => Cls::Ord,
    }
}

/// Classes for a list, with TeX's rule that a binary operator with nothing to join is ordinary (`-x`, `(+`).
/// Spaces are skipped over.
fn classes(v: &[N]) -> Vec<Cls> {
    let mut c: Vec<Cls> = v.iter().map(class_of).collect();
    let idx: Vec<usize> = (0..v.len()).filter(|&i| !matches!(v[i], N::Space(_))).collect();
    for k in 0..idx.len() {
        let prev = k.checked_sub(1).map(|p| c[idx[p]]);
        if c[idx[k]] == Cls::Bin && prev.is_none_or(|p| matches!(p, Cls::Bin | Cls::Op | Cls::Rel | Cls::Open | Cls::Punct)) {
            c[idx[k]] = Cls::Ord;
        }
    }
    for k in 0..idx.len() {
        let next = idx.get(k + 1).map(|&j| c[j]);
        if c[idx[k]] == Cls::Bin && next.is_none_or(|n| matches!(n, Cls::Rel | Cls::Close | Cls::Punct)) {
            c[idx[k]] = Cls::Ord;
        }
    }
    c
}

/// A quadratic curve from `p0` to `p1` with control point `c`, as polyline points.
fn quad(p0: (f32, f32), c: (f32, f32), p1: (f32, f32)) -> Vec<(f32, f32)> {
    (0..=6)
        .map(|i| {
            let t = i as f32 / 6.0;
            let u = 1.0 - t;
            (u * u * p0.0 + 2.0 * u * t * c.0 + t * t * p1.0, u * u * p0.1 + 2.0 * u * t * c.1 + t * t * p1.1)
        })
        .collect()
}

/// A horizontal curly brace from `x0` to `x1`. Its ends sit at `edge` and its point at `tip`.
fn hbrace(x0: f32, x1: f32, edge: f32, tip: f32) -> Vec<(f32, f32)> {
    let mid = (edge + tip) / 2.0;
    let xm = (x0 + x1) / 2.0;
    let q = ((x1 - x0) / 4.0).min((tip - edge).abs() * 1.2);
    let mut v = quad((x0, edge), (x0, mid), (x0 + q, mid));
    v.extend(quad((xm - q, mid), (xm, mid), (xm, tip)));
    v.extend(quad((xm, tip), (xm, mid), (xm + q, mid)));
    v.extend(quad((x1 - q, mid), (x1, mid), (x1, edge)));
    v
}

/// A horizontal arrow from `x0` to `x1` at height `y`, with heads on the chosen ends.
fn arrow(items: &mut Vec<MItem>, x0: f32, x1: f32, y: f32, left: bool, right: bool, size: f32) {
    let w = (0.05 * size).max(1.0);
    let (hl, hh) = (0.13 * size, 0.1 * size);
    items.push(MItem::Path { pts: vec![(x0, y), (x1, y)], w });
    if right {
        items.push(MItem::Path { pts: vec![(x1 - hl, y - hh), (x1, y), (x1 - hl, y + hh)], w });
    }
    if left {
        items.push(MItem::Path { pts: vec![(x0 + hl, y - hh), (x0, y), (x0 + hl, y + hh)], w });
    }
}

impl<'a> Lay<'a> {
    fn text(&mut self, s: &str, st: St, bold: bool, italic: bool) -> MBox {
        let f = FKey::new(st.size, MATH | if bold { BOLD } else { 0 } | if italic { ITALIC } else { 0 });
        let w = self.fonts.measure(f, s);
        MBox { w, asc: 0.73 * st.size, desc: 0.22 * st.size, items: vec![MItem::Text { s: s.to_string(), f, x: 0.0, y: 0.0 }] }
    }

    fn seq(&mut self, v: &[N], st: St) -> MBox {
        let mut out = mbox();
        let mut prev: Option<Cls> = None;
        for (n, cls) in v.iter().zip(classes(v)) {
            if !matches!(n, N::Space(_)) {
                if let Some(p) = prev {
                    out.w += class_space(p, cls, st.script) * st.size;
                }
                prev = Some(cls);
            }
            let b = self.node(n, st);
            append(&mut out, b, 0.0);
        }
        out
    }

    /// Style for scripts and limits: smaller, and no wide spaces.
    fn script(st: St) -> St {
        St { size: st.size * 0.7, display: false, script: true, ..st }
    }

    fn node(&mut self, n: &N, st: St) -> MBox {
        match n {
            N::Sym(s, _, var) => {
                let bold = matches!(st.font, 1 | 4);
                if *var && matches!(st.font, 255 | 4 | 5) && s.chars().all(|c| c.is_alphabetic()) {
                    return self.text(s, st, bold, true);
                }
                let out: String = s
                    .chars()
                    .map(|c| match st.font {
                        2 => blackboard(c),
                        3 => calligraphic(c),
                        6 => fraktur(c),
                        7 => sans(c),
                        8 => mono(c),
                        _ => c,
                    })
                    .collect();
                // \mathbf is upright, as MathJax draws it.
                self.text(&out, st, bold, false)
            }
            N::Group(v) => self.seq(v, st),
            N::Font(f, v) => self.seq(v, St { font: *f, ..st }),
            N::Style(d, v) => self.seq(v, St { display: *d, ..st }),
            N::Class(_, b) => self.node(b, st),
            N::Text(s) => {
                let mut b = self.text(s, St { font: 0, ..st }, matches!(st.font, 1 | 4), st.font == 5);
                b.asc = b.asc.max(0.73 * st.size);
                b
            }
            N::Name(s, _) => self.text(s, St { font: 0, ..st }, false, false),
            N::Space(e) => MBox { w: e * st.size, asc: 0.0, desc: 0.0, items: vec![] },
            N::Frac { num, den, style, bar } => {
                let big = style.unwrap_or(st.display);
                let s2 = St { size: st.size * if big { 1.0 } else { 0.8 }, display: false, script: st.script || !big, ..st };
                let mut num = self.node(num, s2);
                let mut den = self.node(den, s2);
                let pad = 0.1 * st.size;
                let w = num.w.max(den.w) + 2.0 * pad;
                let axis = 0.25 * st.size;
                let th = (0.055 * st.size).max(1.0);
                let gap = if *bar { 0.18 } else { 0.3 } * s2.size;
                let nx = (w - num.w) / 2.0;
                let dx = (w - den.w) / 2.0;
                let n_up = axis + th / 2.0 + gap + num.desc;
                let d_dn = -axis + th / 2.0 + gap + den.asc;
                shift(&mut num, nx, -n_up);
                shift(&mut den, dx, d_dn);
                let mut out = MBox { w, asc: n_up + num.asc, desc: d_dn + den.desc, items: vec![] };
                out.items.extend(num.items);
                out.items.extend(den.items);
                if *bar {
                    out.items.push(MItem::Rule { x: pad * 0.5, y: -axis - th / 2.0, w: w - pad, h: th });
                }
                out
            }
            N::Sqrt(idx, body) => {
                let b = self.node(body, St { display: false, ..st });
                let th = (0.055 * st.size).max(1.0);
                let h = b.asc + b.desc + 0.15 * st.size;
                let mut out = mbox();
                if let Some(i) = idx {
                    let mut ib = self.node(i, St { size: st.size * 0.6, display: false, script: true, ..st });
                    shift(&mut ib, 0.0, -h * 0.55);
                    out.w = ib.w;
                    out.items.extend(ib.items);
                    out.w -= 0.3 * st.size;
                    out.w = out.w.max(0.0);
                }
                let top = b.asc + 0.08 * st.size;
                let bot = b.desc + 0.02 * st.size;
                let rw = 0.55 * st.size;
                let x0 = out.w;
                out.items.push(MItem::Path {
                    pts: vec![(x0, bot - 0.45 * (top + bot)), (x0 + 0.14 * rw * 2.0, bot - 0.5 * (top + bot)), (x0 + 0.5 * rw, bot), (x0 + rw, -top)],
                    w: (0.07 * st.size).max(1.0),
                });
                let body_x = out.w + rw;
                out.items.push(MItem::Rule { x: body_x - 0.02 * st.size, y: -top - th, w: b.w + 0.1 * st.size, h: th });
                let mut bb = b;
                shift(&mut bb, body_x + 0.04 * st.size, 0.0);
                out.items.extend(bb.items);
                out.w = body_x + bb.w + 0.1 * st.size;
                out.asc = top + th + 0.05 * st.size;
                out.desc = bb.desc + 0.05 * st.size;
                out
            }
            N::Script { base, sup, sub } => {
                let lim = match &**base {
                    N::BigOp(_, l) | N::Name(_, l) => *l,
                    N::Accent(Acc::OverBrace | Acc::UnderBrace, _) => Lim::Always,
                    _ => Lim::Side,
                };
                let ss = Self::script(st);
                if lim == Lim::Always || lim == Lim::Display && st.display {
                    let b = self.node(base, st);
                    let sp = sup.as_ref().map(|s| self.node(s, ss));
                    let sb = sub.as_ref().map(|s| self.node(s, ss));
                    return self.limits_box(b, sp, sb, st);
                }
                let mut b = self.node(base, st);
                // The prime glyph is already raised, so primes alone keep full size on the baseline.
                let primes = matches!(sup.as_deref(), Some(N::Sym(s, ..)) if s.chars().all(|c| c == '′'));
                let mut sp = sup.as_ref().map(|s| self.node(s, if primes { St { script: true, ..st } } else { ss }));
                let mut sb = sub.as_ref().map(|s| self.node(s, ss));
                let up = if primes { 0.0 } else { (b.asc - 0.35 * ss.size).max(0.4 * st.size) };
                let dn = 0.2 * st.size;
                let mut w = 0.0f32;
                let bw = b.w;
                if let Some(p) = &mut sp {
                    shift(p, bw, -up);
                    w = w.max(p.w);
                }
                let mut asc = b.asc;
                let mut desc = b.desc;
                if let Some(q) = &mut sb {
                    let dy = if sp.is_some() { dn + 0.05 * st.size } else { dn };
                    shift(q, bw, dy);
                    w = w.max(q.w);
                    desc = desc.max(q.desc + dy);
                }
                if let Some(p) = &sp {
                    asc = asc.max(p.asc + up);
                }
                b.w += w + 0.03 * st.size;
                b.asc = asc;
                b.desc = desc;
                if let Some(p) = sp {
                    b.items.extend(p.items);
                }
                if let Some(q) = sb {
                    b.items.extend(q.items);
                }
                b
            }
            N::BigOp(sym, _) => {
                let k = if st.display { if sym == "∫" { 1.6 } else { 1.35 } } else { 1.1 };
                let s2 = St { size: st.size * k, ..st };
                let mut b = self.text(sym, s2, false, false);
                // center on the math axis
                let drop = 0.25 * st.size + (s2.size - st.size) * 0.22;
                shift(&mut b, 0.0, drop);
                b.asc = 0.73 * s2.size - drop;
                b.desc = 0.22 * s2.size + drop;
                b.w += 0.1 * st.size;
                b
            }
            N::Delim(l, r, body) => {
                let b = self.seq(body, st);
                let h = (b.asc + b.desc).max(st.size * 1.0);
                let mut out = mbox();
                let big = h > st.size * 1.25;
                let (top, bot) = (-(b.asc + 0.05 * st.size), b.desc + 0.05 * st.size);
                self.put_delim(&mut out, l, big, top, bot, st);
                out.w += 0.05 * st.size;
                append(&mut out, b, 0.0);
                out.w += 0.05 * st.size;
                self.put_delim(&mut out, r, big, top, bot, st);
                out.asc = out.asc.max(-top);
                out.desc = out.desc.max(bot);
                out
            }
            N::Matrix(env, spec, rows) => self.matrix(env, spec, rows, st),
            N::Accent(a, body) => self.accent(*a, body, st),
            N::Stack { base, over, under } => {
                let b = self.node(base, st);
                let ss = Self::script(st);
                let sp = over.as_ref().map(|s| self.node(s, ss));
                let sb = under.as_ref().map(|s| self.node(s, ss));
                self.limits_box(b, sp, sb, st)
            }
            N::XArrow { left, over, under } => {
                let sz = st.size;
                let ss = Self::script(st);
                let mut o = self.node(over, ss);
                let mut u = under.as_ref().map(|s| self.node(s, ss));
                let lw = o.w.max(u.as_ref().map_or(0.0, |u| u.w));
                let w = (lw + 0.5 * sz).max(1.2 * sz);
                let (x0, axis) = (0.1 * sz, -0.25 * sz);
                let mut out = MBox { w: w + 0.2 * sz, asc: 0.5 * sz, desc: 0.2 * sz, items: vec![] };
                arrow(&mut out.items, x0, x0 + w, axis, *left, !*left, sz);
                let dy = axis - 0.12 * sz - o.desc;
                let ox = x0 + (w - o.w) / 2.0;
                shift(&mut o, ox, dy);
                out.asc = out.asc.max(o.asc - dy);
                out.items.extend(o.items);
                if let Some(u) = &mut u {
                    let dy = axis + 0.15 * sz + u.asc;
                    let ux = x0 + (w - u.w) / 2.0;
                    shift(u, ux, dy);
                    out.desc = out.desc.max(dy + u.desc);
                }
                if let Some(u) = u {
                    out.items.extend(u.items);
                }
                out
            }
            N::Logo(name) => {
                // (letter, size, raise in em, kern before in em), after TeX's own logo macros
                let parts: &[(&str, f32, f32, f32)] = match *name {
                    "LaTeX" => &[("L", 1.0, 0.0, 0.0), ("A", 0.75, -0.21, -0.32), ("T", 1.0, 0.0, -0.12), ("E", 1.0, 0.22, -0.14), ("X", 1.0, 0.0, -0.11)],
                    "KaTeX" => &[("K", 1.0, 0.0, 0.0), ("A", 0.75, -0.21, -0.17), ("T", 1.0, 0.0, -0.12), ("E", 1.0, 0.22, -0.14), ("X", 1.0, 0.0, -0.11)],
                    _ => &[("T", 1.0, 0.0, 0.0), ("E", 1.0, 0.22, -0.14), ("X", 1.0, 0.0, -0.11)],
                };
                let mut out = mbox();
                for &(s, k, dy, kern) in parts {
                    out.w += kern * st.size;
                    let t = self.text(s, St { size: st.size * k, font: 0, ..st }, false, false);
                    append(&mut out, t, dy * st.size);
                }
                out
            }
        }
    }

    fn accent(&mut self, a: Acc, body: &N, st: St) -> MBox {
        let mut b = self.node(body, st);
        let sz = st.size;
        let th = (0.055 * sz).max(1.0);
        let ink = match short(body) {
            Some(true) => 0.46 * sz,
            Some(false) => 0.69 * sz,
            None => (b.asc - 0.04 * sz).max(0.46 * sz),
        };
        // An italic letter leans right, so its accent does too.
        let skew = if st.font == 255 && matches!(body, N::Sym(s, _, true) if s.chars().count() == 1) { 0.07 * sz } else { 0.0 };
        let cx = b.w / 2.0 + skew;
        let bottom = -(ink + 0.08 * sz);
        let top = match a {
            Acc::Hat | Acc::WideHat => {
                let (aw, h) = if a == Acc::Hat { (0.42 * sz, 0.14 * sz) } else { ((b.w * 0.95).max(0.42 * sz), 0.18 * sz) };
                b.items.push(MItem::Path { pts: vec![(cx - aw / 2.0, bottom), (cx, bottom - h), (cx + aw / 2.0, bottom)], w: th });
                bottom - h - th
            }
            Acc::Tilde | Acc::WideTilde => {
                let aw = if a == Acc::Tilde { 0.45 * sz } else { (b.w * 0.95).max(0.45 * sz) };
                let h = 0.1 * sz;
                let pts = (0..=16)
                    .map(|i| {
                        let t = i as f32 / 16.0;
                        (cx - aw / 2.0 + aw * t, bottom - h / 2.0 - h / 2.0 * (2.0 * std::f32::consts::PI * t).sin())
                    })
                    .collect();
                b.items.push(MItem::Path { pts, w: th });
                bottom - h - th
            }
            Acc::Vec | Acc::ArrowR | Acc::ArrowL | Acc::ArrowLR => {
                let (x0, x1) = if a == Acc::Vec { (cx - 0.22 * sz, cx + 0.22 * sz) } else { (0.0, b.w) };
                let y = bottom - 0.1 * sz;
                arrow(&mut b.items, x0, x1, y, matches!(a, Acc::ArrowL | Acc::ArrowLR), a != Acc::ArrowL, sz);
                y - 0.12 * sz
            }
            Acc::Dot | Acc::Ddot => {
                let d = 0.1 * sz;
                let offs: &[f32] = if a == Acc::Dot { &[0.0] } else { &[-0.12, 0.12] };
                for o in offs {
                    b.items.push(MItem::Rule { x: cx + o * sz - d / 2.0, y: bottom - d, w: d, h: d });
                }
                bottom - d
            }
            Acc::Bar => {
                let y = bottom - th;
                let (x, w) = if matches!(body, N::Sym(s, _, _) if s.chars().count() == 1) { (cx - b.w * 0.4, b.w * 0.8) } else { (0.0, b.w) };
                b.items.push(MItem::Rule { x, y, w, h: th });
                y
            }
            Acc::OverBrace => {
                let tip = bottom - 0.25 * sz;
                b.items.push(MItem::Path { pts: hbrace(0.0, b.w, bottom, tip), w: th });
                tip - th
            }
            Acc::Under | Acc::UnderBrace => {
                let edge = b.desc + 0.08 * sz;
                if a == Acc::Under {
                    b.items.push(MItem::Rule { x: 0.0, y: edge, w: b.w, h: th });
                    b.desc = edge + th + 0.04 * sz;
                } else {
                    let tip = edge + 0.25 * sz;
                    b.items.push(MItem::Path { pts: hbrace(0.0, b.w, edge, tip), w: th });
                    b.desc = tip + th + 0.04 * sz;
                }
                return b;
            }
        };
        b.asc = b.asc.max(-top + 0.04 * sz);
        b
    }

    fn put_delim(&mut self, out: &mut MBox, ch: &str, big: bool, top: f32, bot: f32, st: St) {
        if ch.is_empty() {
            return;
        }
        if big {
            if let Some((item, w)) = delim_path(ch, out.w, top, bot, st.size) {
                out.items.push(item);
                out.w += w;
                return;
            }
        }
        let fs = if big { ((bot - top) * 1.05 / 0.93).max(st.size) } else { st.size };
        let f = FKey::new(fs, MATH);
        let w = self.fonts.measure(f, ch);
        let y = if big { (top + bot) / 2.0 + 0.27 * fs } else { 0.0 };
        out.items.push(MItem::Text { s: ch.to_string(), f, x: out.w, y });
        out.w += w;
    }

    /// `base` with `over` centered above it and `under` centered below.
    fn limits_box(&mut self, base: MBox, over: Option<MBox>, under: Option<MBox>, st: St) -> MBox {
        let (mut sp, mut sb) = (over, under);
        let mut w = base.w;
        for b in [&sp, &sb].into_iter().flatten() {
            w = w.max(b.w);
        }
        let mut out = MBox { w, asc: base.asc, desc: base.desc, items: vec![] };
        let bx = (w - base.w) / 2.0;
        let mut bb = base;
        shift(&mut bb, bx, 0.0);
        out.items.extend(bb.items);
        if let Some(p) = &mut sp {
            let y = -(out.asc + 0.1 * st.size + p.desc);
            let dx = (w - p.w) / 2.0;
            shift(p, dx, y);
            out.asc = -y + p.asc;
        }
        if let Some(q) = &mut sb {
            let y = out.desc + 0.1 * st.size + q.asc;
            let dx = (w - q.w) / 2.0;
            shift(q, dx, y);
            out.desc = y + q.desc;
        }
        if let Some(p) = sp {
            out.items.extend(p.items);
        }
        if let Some(q) = sb {
            out.items.extend(q.items);
        }
        out
    }

    fn matrix(&mut self, env: &str, spec: &str, rows: &[Vec<Vec<N>>], st: St) -> MBox {
        let cs = St { display: false, ..st };
        let aligned = matches!(env, "aligned" | "align" | "align*" | "split" | "eqnarray" | "alignedat" | "alignat" | "alignat*");
        // The l, c and r letters of an array's column spec, outside any `{…}` (as in `p{2cm}` or `@{}`).
        let mut depth = 0;
        let mut spec_cols = vec![];
        for c in spec.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                'l' | 'c' | 'r' if depth == 0 => spec_cols.push(c),
                _ => {}
            }
        }
        let align = |ci: usize| {
            if aligned {
                if ci % 2 == 0 { 'r' } else { 'l' }
            } else if env == "cases" {
                'l'
            } else {
                spec_cols.get(ci).copied().unwrap_or('c')
            }
        };
        // aligned pairs columns as `right&left`, with no gap inside a pair and 2em between pairs
        let gap = |ci: usize| if aligned { if ci % 2 == 0 { 0.0 } else { 2.0 * st.size } } else { st.size };
        let ncol = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut cells: Vec<Vec<MBox>> = vec![];
        let mut colw = vec![0f32; ncol];
        for r in rows {
            let mut row = vec![];
            for (c, cell) in r.iter().enumerate() {
                // The left column of a pair starts with `{}`, so `&= b` spaces the `=` as a relation.
                let b = if aligned && c % 2 == 1 {
                    let mut v = vec![N::Group(vec![])];
                    v.extend(cell.iter().cloned());
                    self.seq(&v, cs)
                } else {
                    self.seq(cell, cs)
                };
                colw[c] = colw[c].max(b.w);
                row.push(b);
            }
            cells.push(row);
        }
        let rgap = 0.3 * st.size;
        let mut heights = vec![];
        for r in &cells {
            let a = r.iter().map(|b| b.asc).fold(0.73 * st.size, f32::max);
            let d = r.iter().map(|b| b.desc).fold(0.22 * st.size, f32::max);
            heights.push((a, d));
        }
        let total_h: f32 = heights.iter().map(|h| h.0 + h.1).sum::<f32>() + rgap * (rows.len().saturating_sub(1)) as f32;
        let axis = 0.25 * st.size;
        let top = -(total_h / 2.0 + axis); // top edge relative to baseline (negative up)
        let total_w: f32 = colw.iter().sum::<f32>() + (0..ncol.saturating_sub(1)).map(gap).sum::<f32>();
        let mut body = MBox { w: total_w, asc: total_h / 2.0 + axis, desc: total_h / 2.0 - axis, items: vec![] };
        let mut y = top;
        for (ri, row) in cells.into_iter().enumerate() {
            let (a, d) = heights[ri];
            let base = y + a;
            let mut x = 0.0;
            for (ci, mut b) in row.into_iter().enumerate() {
                let off = match align(ci) {
                    'l' => 0.0,
                    'r' => colw[ci] - b.w,
                    _ => (colw[ci] - b.w) / 2.0,
                };
                shift(&mut b, x + off, base);
                body.items.extend(b.items);
                x += colw[ci] + gap(ci);
            }
            y += a + d + rgap;
        }
        let (l, r) = match env {
            "pmatrix" => ("(", ")"),
            "bmatrix" => ("[", "]"),
            "Bmatrix" => ("{", "}"),
            "vmatrix" => ("|", "|"),
            "Vmatrix" => ("‖", "‖"),
            "cases" => ("{", ""),
            _ => ("", ""),
        };
        if l.is_empty() && r.is_empty() {
            return body;
        }
        let (top, bot) = (-(body.asc + 0.05 * st.size), body.desc + 0.05 * st.size);
        let mut out = mbox();
        self.put_delim(&mut out, l, true, top, bot, st);
        out.w += 0.1 * st.size;
        let w0 = out.w;
        let (bw, basc, bdesc) = (body.w, body.asc, body.desc);
        shift(&mut body, w0, 0.0);
        out.items.extend(body.items);
        out.w += bw + 0.1 * st.size;
        self.put_delim(&mut out, r, true, top, bot, st);
        out.asc = basc;
        out.desc = bdesc;
        out
    }
}

pub fn layout(fonts: &mut Fonts, tex: &str, size: f32, display: bool) -> MBox {
    let nodes = parse_all(tex);
    let mut l = Lay { fonts };
    let mut b = l.seq(&nodes, St { size, display, script: false, font: 255 });
    b.asc = b.asc.max(0.8 * size);
    b.desc = b.desc.max(0.25 * size);
    b
}
