//! P-01: expression evaluator for the user parameter table.
//!
//! Grammar (precedence climbing, all values are `f64`, angles in radians):
//!
//! ```text
//! expr   := term (("+" | "-") term)*
//! term   := unary (("*" | "/" | "%") unary)*
//! unary  := ("-" | "+") unary | power
//! power  := primary ("^" unary)?          // right-associative
//! primary:= NUMBER | IDENT | IDENT "(" expr ("," expr)* ")" | "(" expr ")"
//! ```
//!
//! Identifiers resolve to parameters, the constants `pi`, `tau` and `e`,
//! or one of the supported functions. `deg(x)` converts degrees to
//! radians so angle parameters can be written readably
//! (`sweep = deg(45)`).

use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// A parse or evaluation failure.
#[derive(Debug, Clone, PartialEq)]
pub struct ExprError {
    /// Human-readable message (includes position when available).
    pub message: String,
}

impl std::fmt::Display for ExprError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExprError {}

fn err(message: impl Into<String>) -> ExprError {
    ExprError {
        message: message.into(),
    }
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq)]
enum UnOp {
    Neg,
    Pos,
}

/// Expression tree.
#[derive(Debug, Clone, PartialEq)]
enum Ast {
    Num(f64),
    Var(String),
    Bin {
        op: BinOp,
        lhs: Box<Ast>,
        rhs: Box<Ast>,
    },
    Un {
        op: UnOp,
        operand: Box<Ast>,
    },
    Call {
        name: String,
        args: Vec<Ast>,
    },
}

/// A lexical token.
#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    LParen,
    RParen,
    Comma,
}

fn tokenize(input: &str) -> Result<Vec<(Tok, usize)>, ExprError> {
    let mut out = Vec::new();
    let bytes: Vec<char> = input.chars().collect();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        let pos = i;
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '+' => {
                out.push((Tok::Plus, pos));
                i += 1;
            }
            '-' => {
                out.push((Tok::Minus, pos));
                i += 1;
            }
            '*' => {
                out.push((Tok::Star, pos));
                i += 1;
            }
            '/' => {
                out.push((Tok::Slash, pos));
                i += 1;
            }
            '%' => {
                out.push((Tok::Percent, pos));
                i += 1;
            }
            '^' => {
                out.push((Tok::Caret, pos));
                i += 1;
            }
            '(' => {
                out.push((Tok::LParen, pos));
                i += 1;
            }
            ')' => {
                out.push((Tok::RParen, pos));
                i += 1;
            }
            ',' => {
                out.push((Tok::Comma, pos));
                i += 1;
            }
            '0'..='9' | '.' => {
                let start = i;
                let mut seen_dot = false;
                while i < bytes.len() {
                    let d = bytes[i];
                    if d.is_ascii_digit() {
                        i += 1;
                    } else if d == '.' && !seen_dot {
                        seen_dot = true;
                        i += 1;
                    } else {
                        break;
                    }
                }
                let text: String = bytes[start..i].iter().collect();
                let value: f64 = text
                    .parse()
                    .map_err(|_| err(format!("invalid number `{text}` at offset {start}")))?;
                out.push((Tok::Num(value), start));
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_alphanumeric() || bytes[i] == '_') {
                    i += 1;
                }
                let name: String = bytes[start..i].iter().collect();
                out.push((Tok::Ident(name), start));
            }
            other => {
                return Err(err(format!(
                    "unexpected character `{other}` at offset {pos}"
                )));
            }
        }
    }
    Ok(out)
}

/// Parser over the token stream.
struct Parser {
    toks: Vec<(Tok, usize)>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|(t, _)| t)
    }

    fn bump(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned().map(|(t, _)| t);
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn expect(&mut self, want: &Tok) -> Result<(), ExprError> {
        match self.peek() {
            Some(t) if t == want => {
                self.bump();
                Ok(())
            }
            Some(t) => Err(err(format!("expected {want:?}, found {t:?}"))),
            None => Err(err(format!("expected {want:?}, found end of input"))),
        }
    }

    fn parse_expr(&mut self) -> Result<Ast, ExprError> {
        let mut lhs = self.parse_term()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => BinOp::Add,
                Some(Tok::Minus) => BinOp::Sub,
                _ => return Ok(lhs),
            };
            self.bump();
            let rhs = self.parse_term()?;
            lhs = Ast::Bin {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
    }

    fn parse_term(&mut self) -> Result<Ast, ExprError> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Star) => BinOp::Mul,
                Some(Tok::Slash) => BinOp::Div,
                Some(Tok::Percent) => BinOp::Mod,
                _ => return Ok(lhs),
            };
            self.bump();
            let rhs = self.parse_unary()?;
            lhs = Ast::Bin {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
    }

    fn parse_unary(&mut self) -> Result<Ast, ExprError> {
        match self.peek() {
            Some(Tok::Minus) => {
                self.bump();
                let operand = self.parse_unary()?;
                Ok(Ast::Un {
                    op: UnOp::Neg,
                    operand: Box::new(operand),
                })
            }
            Some(Tok::Plus) => {
                self.bump();
                let operand = self.parse_unary()?;
                Ok(Ast::Un {
                    op: UnOp::Pos,
                    operand: Box::new(operand),
                })
            }
            _ => self.parse_power(),
        }
    }

    fn parse_power(&mut self) -> Result<Ast, ExprError> {
        let base = self.parse_primary()?;
        if matches!(self.peek(), Some(Tok::Caret)) {
            self.bump();
            // Right-associative: 2^3^2 = 2^(3^2).
            let exponent = self.parse_unary()?;
            return Ok(Ast::Bin {
                op: BinOp::Pow,
                lhs: Box::new(base),
                rhs: Box::new(exponent),
            });
        }
        Ok(base)
    }

    fn parse_primary(&mut self) -> Result<Ast, ExprError> {
        match self.bump() {
            Some(Tok::Num(v)) => Ok(Ast::Num(v)),
            Some(Tok::Ident(name)) => {
                if matches!(self.peek(), Some(Tok::LParen)) {
                    self.bump();
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Tok::RParen)) {
                        loop {
                            args.push(self.parse_expr()?);
                            match self.peek() {
                                Some(Tok::Comma) => {
                                    self.bump();
                                }
                                _ => break,
                            }
                        }
                    }
                    self.expect(&Tok::RParen)?;
                    Ok(Ast::Call { name, args })
                } else {
                    Ok(Ast::Var(name))
                }
            }
            Some(Tok::LParen) => {
                let inner = self.parse_expr()?;
                self.expect(&Tok::RParen)?;
                Ok(inner)
            }
            Some(tok) => Err(err(format!("unexpected token {tok:?}"))),
            None => Err(err("unexpected end of input")),
        }
    }
}

fn parse(input: &str) -> Result<Ast, ExprError> {
    let toks = tokenize(input)?;
    if toks.is_empty() {
        return Err(err("empty expression"));
    }
    let mut parser = Parser { toks, pos: 0 };
    let ast = parser.parse_expr()?;
    if parser.pos != parser.toks.len() {
        return Err(err(format!(
            "trailing input after expression: {}",
            input.trim()
        )));
    }
    Ok(ast)
}

fn constant(name: &str) -> Option<f64> {
    match name {
        "pi" => Some(std::f64::consts::PI),
        "tau" => Some(std::f64::consts::TAU),
        "e" => Some(std::f64::consts::E),
        _ => None,
    }
}

fn is_function(name: &str) -> bool {
    matches!(
        name,
        "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "atan2"
            | "sqrt"
            | "abs"
            | "exp"
            | "ln"
            | "log"
            | "floor"
            | "ceil"
            | "round"
            | "deg"
            | "min"
            | "max"
            | "hypot"
            | "pow"
    )
}

fn check_domain(name: &str, v: f64) -> Result<f64, ExprError> {
    if v.is_nan() {
        return Err(err(format!("{name}() produced NaN (domain error?)")));
    }
    Ok(v)
}

fn eval_f64(name: &str, args: &[f64]) -> Result<f64, ExprError> {
    let unary = |name: &str, f: &dyn Fn(f64) -> f64, args: &[f64]| -> Result<f64, ExprError> {
        let a = args.first().copied().unwrap_or(f64::NAN);
        check_domain(name, f(a))
    };
    match name {
        "sin" => unary("sin", &f64::sin, args),
        "cos" => unary("cos", &f64::cos, args),
        "tan" => unary("tan", &f64::tan, args),
        "asin" => unary("asin", &f64::asin, args),
        "acos" => unary("acos", &f64::acos, args),
        "atan" => unary("atan", &f64::atan, args),
        "sqrt" => unary("sqrt", &f64::sqrt, args),
        "abs" => unary("abs", &f64::abs, args),
        "exp" => unary("exp", &f64::exp, args),
        "ln" => unary("ln", &f64::ln, args),
        "log" => unary("log", &f64::log10, args),
        "floor" => unary("floor", &f64::floor, args),
        "ceil" => unary("ceil", &f64::ceil, args),
        "round" => unary("round", &f64::round, args),
        // deg(x): interpret x as degrees, return radians.
        "deg" => unary("deg", &|d: f64| d.to_radians(), args),
        "atan2" => {
            let (y, x) = (args[0], args[1]);
            check_domain("atan2", y.atan2(x))
        }
        "min" => Ok(args[0].min(args[1])),
        "max" => Ok(args[0].max(args[1])),
        "hypot" => check_domain("hypot", args[0].hypot(args[1])),
        "pow" => check_domain("pow", args[0].powf(args[1])),
        _ => Err(err(format!("unknown function `{name}`"))),
    }
}

fn expected_args(name: &str) -> usize {
    match name {
        "atan2" | "min" | "max" | "hypot" | "pow" => 2,
        _ => 1,
    }
}

fn eval_ast(ast: &Ast, vars: &BTreeMap<String, f64>) -> Result<f64, ExprError> {
    match ast {
        Ast::Num(v) => Ok(*v),
        Ast::Var(name) => {
            if let Some(v) = vars.get(name) {
                Ok(*v)
            } else if let Some(c) = constant(name) {
                Ok(c)
            } else {
                Err(err(format!("unknown name `{name}`")))
            }
        }
        Ast::Un { op, operand } => {
            let v = eval_ast(operand, vars)?;
            Ok(match op {
                UnOp::Neg => -v,
                UnOp::Pos => v,
            })
        }
        Ast::Bin { op, lhs, rhs } => {
            let l = eval_ast(lhs, vars)?;
            let r = eval_ast(rhs, vars)?;
            match op {
                BinOp::Add => Ok(l + r),
                BinOp::Sub => Ok(l - r),
                BinOp::Mul => Ok(l * r),
                BinOp::Div => {
                    if r == 0.0 {
                        Err(err("division by zero"))
                    } else {
                        Ok(l / r)
                    }
                }
                BinOp::Mod => {
                    if r == 0.0 {
                        Err(err("modulo by zero"))
                    } else {
                        Ok(l % r)
                    }
                }
                BinOp::Pow => check_domain("^", l.powf(r)),
            }
        }
        Ast::Call { name, args } => {
            let want = expected_args(name);
            if args.len() != want {
                return Err(err(format!(
                    "`{name}` expects {want} argument(s), got {}",
                    args.len()
                )));
            }
            let mut vals = Vec::with_capacity(args.len());
            for a in args {
                vals.push(eval_ast(a, vars)?);
            }
            eval_f64(name, &vals)
        }
    }
}

fn collect_vars(ast: &Ast, out: &mut BTreeSet<String>) {
    match ast {
        Ast::Num(_) => {}
        Ast::Var(name) => {
            if constant(name).is_none() {
                out.insert(name.clone());
            }
        }
        Ast::Un { operand, .. } => collect_vars(operand, out),
        Ast::Bin { lhs, rhs, .. } => {
            collect_vars(lhs, out);
            collect_vars(rhs, out);
        }
        Ast::Call { args, .. } => {
            for a in args {
                collect_vars(a, out);
            }
        }
    }
}

/// Evaluate an expression with the given variable values.
///
/// ```text
/// eval("2*th + 1", { "th": 5 }) == 11
/// ```
pub fn eval(expr: &str, vars: &BTreeMap<String, f64>) -> Result<f64, ExprError> {
    let ast = parse(expr)?;
    let v = eval_ast(&ast, vars)?;
    if v.is_nan() {
        return Err(err("expression evaluated to NaN"));
    }
    Ok(v)
}

/// Names referenced by an expression (excluding functions and the
/// constants `pi`, `tau`, `e`). Used to build the parameter dependency
/// graph for cycle detection.
pub fn referenced_names(expr: &str) -> Result<BTreeSet<String>, ExprError> {
    let ast = parse(expr)?;
    let mut names = BTreeSet::new();
    collect_vars(&ast, &mut names);
    Ok(names)
}

/// Validate an expression without a variable context (syntax, function
/// names, arities). Returns the referenced names on success.
pub fn validate(expr: &str) -> Result<BTreeSet<String>, ExprError> {
    let ast = parse(expr)?;
    // Check calls without evaluating.
    fn walk(ast: &Ast) -> Result<(), ExprError> {
        match ast {
            Ast::Num(_) | Ast::Var(_) => Ok(()),
            Ast::Un { operand, .. } => walk(operand),
            Ast::Bin { lhs, rhs, .. } => {
                walk(lhs)?;
                walk(rhs)
            }
            Ast::Call { name, args } => {
                if !is_function(name) {
                    return Err(err(format!("unknown function `{name}`")));
                }
                let want = expected_args(name);
                if args.len() != want {
                    return Err(err(format!(
                        "`{name}` expects {want} argument(s), got {}",
                        args.len()
                    )));
                }
                for a in args {
                    walk(a)?;
                }
                Ok(())
            }
        }
    }
    walk(&ast)?;
    let mut names = BTreeSet::new();
    collect_vars(&ast, &mut names);
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    fn vars(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
        pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn arithmetic_precedence() {
        let v = BTreeMap::new();
        assert_abs_diff_eq!(eval("2 + 3 * 4", &v).unwrap(), 14.0, epsilon = 1e-12);
        assert_abs_diff_eq!(eval("(2 + 3) * 4", &v).unwrap(), 20.0, epsilon = 1e-12);
        assert_abs_diff_eq!(eval("2 ^ 3 ^ 2", &v).unwrap(), 512.0, epsilon = 1e-9);
        assert_abs_diff_eq!(eval("-2 ^ 2", &v).unwrap(), -4.0, epsilon = 1e-12);
        assert_abs_diff_eq!(eval("10 % 3", &v).unwrap(), 1.0, epsilon = 1e-12);
        assert_abs_diff_eq!(eval("1 + -2", &v).unwrap(), -1.0, epsilon = 1e-12);
    }

    #[test]
    fn variables_and_functions() {
        let v = vars(&[("th", 5.0), ("width", 12.0)]);
        assert_abs_diff_eq!(eval("2*th + 1", &v).unwrap(), 11.0, epsilon = 1e-12);
        assert_abs_diff_eq!(eval("width / 4", &v).unwrap(), 3.0, epsilon = 1e-12);
        assert_abs_diff_eq!(
            eval("sqrt(width + th*4 + 4)", &v).unwrap(),
            6.0,
            epsilon = 1e-12
        );
        assert_abs_diff_eq!(eval("hypot(3, 4)", &v).unwrap(), 5.0, epsilon = 1e-12);
        assert_abs_diff_eq!(eval("min(width, th)", &v).unwrap(), 5.0, epsilon = 1e-12);
        assert_abs_diff_eq!(
            eval("deg(180)", &v).unwrap(),
            std::f64::consts::PI,
            epsilon = 1e-12
        );
        assert_abs_diff_eq!(
            eval("pi", &v).unwrap(),
            std::f64::consts::PI,
            epsilon = 1e-12
        );
        assert_abs_diff_eq!(
            eval("tau / 2", &v).unwrap(),
            std::f64::consts::PI,
            epsilon = 1e-12
        );
    }

    #[test]
    fn error_reporting() {
        let v = vars(&[("a", 1.0)]);
        assert!(eval("a +", &v).is_err());
        assert!(eval("(a", &v).is_err());
        assert!(eval("a b", &v).is_err());
        assert!(eval("unknown_name", &v).is_err());
        assert!(eval("1/0", &v).is_err());
        assert!(eval("sqrt(a - 2)", &v).is_err());
        assert!(eval("foo(1)", &v).is_err());
        assert!(eval("min(1)", &v).is_err());
        assert!(eval("", &v).is_err());
        assert!(eval("1 $ 2", &v).is_err());
    }

    #[test]
    fn reference_extraction() {
        let names = referenced_names("2*th + width*deg(angle) - pi").unwrap();
        let expected: BTreeSet<String> =
            ["th".to_string(), "width".to_string(), "angle".to_string()]
                .into_iter()
                .collect();
        assert_eq!(names, expected);
        // Functions are not references.
        let names = referenced_names("sqrt(2)").unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn validation_checks_calls() {
        assert!(validate("min(1, 2)").is_ok());
        assert!(validate("nosuchfn(1)").is_err());
        assert!(validate("sin(1, 2)").is_err());
    }
}
