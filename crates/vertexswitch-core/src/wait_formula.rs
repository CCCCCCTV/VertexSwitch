//! 上游繁忙重试前的等待时间（SPEC §4）：关于 `n`（第几次重试，从 1 开始）的算式，结果是秒数。
//!
//! 支持数字、`n`、`+ - * / ^`（`^` 也可写成 `**`）、括号、`min(…)`、`max(…)`；全角括号、逗号与
//! `×` `÷` 当作对应的半角符号。`^` 右结合且比负号优先：`-2^2` = -4，`2^-1` = 0.5。
//! 一个数字本身就是合法的算式（每次等这么久）。结果为负按 0 算，超过 [`MAX_WAIT_SECONDS`] 按上限算；
//! 对 1..=10 次重试中任何一次算不出有限数（如 `1/(n-1)`）的算式视为无效。

use crate::tr;
use crate::transport::MAX_BUSY_RETRIES;

/// 默认：1、2、4、8……秒
pub const DEFAULT: &str = "2^(n-1)";
/// 单次等待的上限（秒）
pub const MAX_WAIT_SECONDS: f64 = 300.0;

#[derive(Debug, Clone, PartialEq)]
enum Node {
    Num(f64),
    N,
    Neg(Box<Node>),
    Bin(Op, Box<Node>, Box<Node>),
    Call(Func, Vec<Node>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Func {
    Min,
    Max,
}

/// 解析好的等待算式。
#[derive(Debug, Clone, PartialEq)]
pub struct WaitFormula {
    node: Node,
}

impl Default for WaitFormula {
    fn default() -> Self {
        Self::parse(DEFAULT).expect("默认算式合法")
    }
}

impl WaitFormula {
    /// 解析并检查算式；出错时返回给用户看的说明（跟随界面语言）。
    pub fn parse(text: &str) -> Result<Self, String> {
        let tokens = lex(text)?;
        if tokens.is_empty() {
            return Err(tr!("等待时间是空的", "The wait time is empty"));
        }
        let mut parser = Parser { tokens, pos: 0 };
        let node = parser.expr()?;
        if let Some(extra) = parser.tokens.get(parser.pos) {
            return Err(unexpected(extra));
        }
        let formula = Self { node };
        for n in 1..=MAX_BUSY_RETRIES {
            let value = formula.raw(n);
            if !value.is_finite() {
                return Err(tr!(
                    "第 {n} 次重试算不出确定的秒数（结果是 {value}）",
                    "Retry {n} doesn't give a finite number of seconds (got {value})"
                ));
            }
        }
        Ok(formula)
    }

    fn raw(&self, n: u32) -> f64 {
        eval(&self.node, f64::from(n))
    }

    /// 第 `n` 次重试前等待的秒数，限制在 0 ~ [`MAX_WAIT_SECONDS`]。
    pub fn seconds(&self, n: u32) -> f64 {
        let value = self.raw(n);
        if value.is_finite() {
            value.clamp(0.0, MAX_WAIT_SECONDS)
        } else {
            0.0
        }
    }
}

fn eval(node: &Node, n: f64) -> f64 {
    match node {
        Node::Num(v) => *v,
        Node::N => n,
        Node::Neg(inner) => -eval(inner, n),
        Node::Bin(op, a, b) => {
            let (a, b) = (eval(a, n), eval(b, n));
            match op {
                Op::Add => a + b,
                Op::Sub => a - b,
                Op::Mul => a * b,
                Op::Div => a / b,
                Op::Pow => a.powf(b),
            }
        }
        Node::Call(func, args) => {
            let values = args.iter().map(|a| eval(a, n));
            match func {
                Func::Min => values.fold(f64::INFINITY, f64::min),
                Func::Max => values.fold(f64::NEG_INFINITY, f64::max),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Name(String),
    Op(char),
    Open,
    Close,
    Comma,
}

/// 词法单元与它在原文里的位置（第几个字符，从 1 开始）。
type Spanned = (Tok, usize);

fn lex(text: &str) -> Result<Vec<Spanned>, String> {
    let chars: Vec<char> = text
        .chars()
        .map(|c| match c {
            '（' => '(',
            '）' => ')',
            '，' => ',',
            '×' => '*',
            '÷' => '/',
            '－' => '-',
            '＋' => '+',
            other => other,
        })
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let at = i + 1;
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let raw: String = chars[start..i].iter().collect();
            let value = raw.parse::<f64>().map_err(|_| {
                tr!(
                    "第 {at} 个字符起的 {raw} 不是数字",
                    "{raw} at character {at} isn't a number"
                )
            })?;
            out.push((Tok::Num(value), at));
        } else if c.is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect::<String>().to_lowercase();
            out.push((Tok::Name(name), at));
        } else if c == '*' && chars.get(i + 1) == Some(&'*') {
            out.push((Tok::Op('^'), at));
            i += 2;
        } else {
            let tok = match c {
                '+' | '-' | '*' | '/' | '^' => Tok::Op(c),
                '(' => Tok::Open,
                ')' => Tok::Close,
                ',' => Tok::Comma,
                _ => {
                    return Err(tr!(
                        "第 {at} 个字符 {c} 不认识（可以用数字、n、+ - * / ^、括号、min、max）",
                        "Character {at} ({c}) isn't allowed (use numbers, n, + - * / ^, brackets, min, max)"
                    ));
                }
            };
            out.push((tok, at));
            i += 1;
        }
    }
    Ok(out)
}

fn describe(tok: &Tok) -> String {
    match tok {
        Tok::Num(v) => v.to_string(),
        Tok::Name(name) => name.clone(),
        Tok::Op(c) => c.to_string(),
        Tok::Open => "(".into(),
        Tok::Close => ")".into(),
        Tok::Comma => ",".into(),
    }
}

fn unexpected((tok, at): &Spanned) -> String {
    let what = describe(tok);
    tr!(
        "第 {at} 个字符附近多了 {what}（两个数之间要写运算符，比如 3*n）",
        "Unexpected {what} near character {at} (put an operator between values, e.g. 3*n)"
    )
}

fn incomplete() -> String {
    tr!(
        "等待时间没写完整（运算符后面缺少数字或 n）",
        "The wait time is incomplete (an operator is missing a number or n after it)"
    )
}

struct Parser {
    tokens: Vec<Spanned>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos).map(|(t, _)| t)
    }

    fn next(&mut self) -> Option<Spanned> {
        let tok = self.tokens.get(self.pos).cloned();
        self.pos += 1;
        tok
    }

    /// expr := term (('+' | '-') term)*
    fn expr(&mut self) -> Result<Node, String> {
        let mut node = self.term()?;
        while let Some(Tok::Op(c @ ('+' | '-'))) = self.peek().cloned() {
            self.pos += 1;
            let op = if c == '+' { Op::Add } else { Op::Sub };
            node = Node::Bin(op, Box::new(node), Box::new(self.term()?));
        }
        Ok(node)
    }

    /// term := unary (('*' | '/') unary)*
    fn term(&mut self) -> Result<Node, String> {
        let mut node = self.unary()?;
        while let Some(Tok::Op(c @ ('*' | '/'))) = self.peek().cloned() {
            self.pos += 1;
            let op = if c == '*' { Op::Mul } else { Op::Div };
            node = Node::Bin(op, Box::new(node), Box::new(self.unary()?));
        }
        Ok(node)
    }

    /// unary := ('-' | '+') unary | power
    fn unary(&mut self) -> Result<Node, String> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.pos += 1;
                Ok(Node::Neg(Box::new(self.unary()?)))
            }
            Some(Tok::Op('+')) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    /// power := atom ('^' unary)?（右结合）
    fn power(&mut self) -> Result<Node, String> {
        let base = self.atom()?;
        if self.peek() == Some(&Tok::Op('^')) {
            self.pos += 1;
            let exponent = self.unary()?;
            return Ok(Node::Bin(Op::Pow, Box::new(base), Box::new(exponent)));
        }
        Ok(base)
    }

    /// atom := 数字 | n | (min | max) '(' expr (',' expr)* ')' | '(' expr ')'
    fn atom(&mut self) -> Result<Node, String> {
        let Some((tok, at)) = self.next() else {
            return Err(incomplete());
        };
        match tok {
            Tok::Num(v) => Ok(Node::Num(v)),
            Tok::Name(name) if name == "n" => Ok(Node::N),
            Tok::Name(name) if name == "min" || name == "max" => {
                let func = if name == "min" { Func::Min } else { Func::Max };
                if self.peek() != Some(&Tok::Open) {
                    return Err(tr!(
                        "{name} 后面要跟括号，比如 {name}(30, 2^n)",
                        "{name} needs brackets, e.g. {name}(30, 2^n)"
                    ));
                }
                self.pos += 1;
                if self.peek() == Some(&Tok::Close) {
                    return Err(tr!(
                        "{name}() 里至少要写一个值",
                        "{name}() needs at least one value"
                    ));
                }
                let mut args = vec![self.expr()?];
                while self.peek() == Some(&Tok::Comma) {
                    self.pos += 1;
                    args.push(self.expr()?);
                }
                self.close()?;
                Ok(Node::Call(func, args))
            }
            Tok::Name(name) => Err(tr!(
                "不认识 {name}（只能用 n、min、max）",
                "Unknown name {name} (only n, min and max are allowed)"
            )),
            Tok::Open => {
                let inner = self.expr()?;
                self.close()?;
                Ok(inner)
            }
            other => Err(unexpected(&(other, at))),
        }
    }

    fn close(&mut self) -> Result<(), String> {
        match self.next() {
            Some((Tok::Close, _)) => Ok(()),
            Some(other) => Err(unexpected(&other)),
            None => Err(tr!("少了右括号 )", "Missing a closing bracket )")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waits(text: &str) -> Vec<f64> {
        let f = WaitFormula::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        (1..=5).map(|n| f.seconds(n)).collect()
    }

    #[test]
    fn default_doubles_from_one_second() {
        assert_eq!(waits(DEFAULT), [1.0, 2.0, 4.0, 8.0, 16.0]);
        assert_eq!(WaitFormula::default(), WaitFormula::parse(DEFAULT).unwrap());
    }

    #[test]
    fn plain_numbers_and_common_shapes() {
        assert_eq!(waits("5"), [5.0; 5]);
        assert_eq!(waits(" 0.5 "), [0.5; 5]);
        assert_eq!(waits("min(30, 3*n)"), [3.0, 6.0, 9.0, 12.0, 15.0]);
        assert_eq!(waits("max(2, n)"), [2.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(waits("1 + 2*(n-1)"), [1.0, 3.0, 5.0, 7.0, 9.0]);
        assert_eq!(waits("2**n / 2"), [1.0, 2.0, 4.0, 8.0, 16.0]);
        assert_eq!(waits("N*N"), [1.0, 4.0, 9.0, 16.0, 25.0]);
        assert_eq!(waits("min(1, 2, n)"), [1.0; 5]);
    }

    #[test]
    fn full_width_symbols_are_accepted() {
        assert_eq!(waits("（n＋1）×2÷2"), [2.0, 3.0, 4.0, 5.0, 6.0]);
        assert_eq!(waits("min（10，n）"), [1.0, 2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn precedence_and_associativity() {
        assert_eq!(waits("2^3^0")[0], 2.0); // 2^(3^0)
        assert_eq!(waits("1 + -2^2 + 10")[0], 7.0); // -(2^2)
        assert_eq!(waits("2^-1")[0], 0.5);
        assert_eq!(waits("8/2/2")[0], 2.0);
        assert_eq!(waits("10-2-3")[0], 5.0);
    }

    #[test]
    fn results_are_clamped() {
        assert_eq!(waits("-5"), [0.0; 5]);
        assert_eq!(waits("1000"), [MAX_WAIT_SECONDS; 5]);
        assert_eq!(waits("10^n")[..3], [10.0, 100.0, MAX_WAIT_SECONDS]);
    }

    #[test]
    fn invalid_formulas_explain_why() {
        let err = |text: &str| WaitFormula::parse(text).unwrap_err();
        assert!(err("").contains("空"));
        assert!(err("   ").contains("空"));
        assert!(err("3n").contains("3*n"), "{}", err("3n"));
        assert!(err("2^").contains("没写完整"));
        assert!(err("(1+2").contains("右括号"));
        assert!(err("foo").contains("不认识 foo"));
        assert!(err("1 $ 2").contains("第 3 个字符"));
        assert!(err("min()").contains("至少"));
        assert!(err("min 3").contains("括号"));
        assert!(err("1..2").contains("不是数字"));
        assert!(err("1)").contains("多了 )"));
        assert!(err("1/(n-1)").contains("第 1 次"));
        assert!(err("1/0").contains("第 1 次"));
    }
}
