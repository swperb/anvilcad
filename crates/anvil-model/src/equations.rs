//! Expression evaluation for global variables and driven dimensions.
//!
//! Grammar: numbers, variable names, `+ - * / ^`, parentheses, unary minus and the
//! functions sin, cos, tan (degrees), sqrt, abs, min, max, round. Variable names may be
//! written bare (`width`) or quoted the way SOLIDWORKS writes them (`"width"`).

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum EvalError {
    Syntax(String),
    UnknownVariable(String),
    UnknownFunction(String),
    Cycle(Vec<String>),
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvalError::Syntax(s) => write!(f, "syntax error: {s}"),
            EvalError::UnknownVariable(v) => write!(f, "unknown variable '{v}'"),
            EvalError::UnknownFunction(v) => write!(f, "unknown function '{v}'"),
            EvalError::Cycle(v) => write!(f, "circular reference among: {}", v.join(", ")),
        }
    }
}

impl std::error::Error for EvalError {}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
}

fn lex(s: &str) -> Result<Vec<Tok>, EvalError> {
    let mut out = Vec::new();
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let st = i;
            while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.') {
                i += 1;
            }
            // Exponent part.
            if i < cs.len() && (cs[i] == 'e' || cs[i] == 'E') {
                let save = i;
                i += 1;
                if i < cs.len() && (cs[i] == '+' || cs[i] == '-') {
                    i += 1;
                }
                if i < cs.len() && cs[i].is_ascii_digit() {
                    while i < cs.len() && cs[i].is_ascii_digit() {
                        i += 1;
                    }
                } else {
                    i = save;
                }
            }
            let t: String = cs[st..i].iter().collect();
            out.push(Tok::Num(t.parse().map_err(|_| EvalError::Syntax(format!("bad number '{t}'")))?));
        } else if c.is_alphabetic() || c == '_' {
            let st = i;
            while i < cs.len() && (cs[i].is_alphanumeric() || cs[i] == '_' || cs[i] == '@') {
                i += 1;
            }
            out.push(Tok::Ident(cs[st..i].iter().collect()));
        } else if c == '"' {
            let st = i + 1;
            i += 1;
            while i < cs.len() && cs[i] != '"' {
                i += 1;
            }
            if i >= cs.len() {
                return Err(EvalError::Syntax("unterminated quote".into()));
            }
            out.push(Tok::Ident(cs[st..i].iter().collect()));
            i += 1;
        } else if "+-*/^(),".contains(c) {
            out.push(Tok::Op(c));
            i += 1;
        } else {
            return Err(EvalError::Syntax(format!("unexpected '{c}'")));
        }
    }
    Ok(out)
}

struct Parser<'a> {
    toks: Vec<Tok>,
    pos: usize,
    vars: &'a dyn Fn(&str) -> Option<f64>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(&Tok::Op(c)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expr(&mut self) -> Result<f64, EvalError> {
        let mut v = self.term()?;
        loop {
            if self.eat('+') {
                v += self.term()?;
            } else if self.eat('-') {
                v -= self.term()?;
            } else {
                return Ok(v);
            }
        }
    }
    fn term(&mut self) -> Result<f64, EvalError> {
        let mut v = self.unary()?;
        loop {
            if self.eat('*') {
                v *= self.unary()?;
            } else if self.eat('/') {
                v /= self.unary()?;
            } else {
                return Ok(v);
            }
        }
    }
    fn power(&mut self) -> Result<f64, EvalError> {
        let b = self.atom()?;
        if self.eat('^') {
            Ok(b.powf(self.unary()?))
        } else {
            Ok(b)
        }
    }
    fn unary(&mut self) -> Result<f64, EvalError> {
        if self.eat('-') {
            Ok(-self.unary()?)
        } else if self.eat('+') {
            self.unary()
        } else {
            self.power()
        }
    }
    fn atom(&mut self) -> Result<f64, EvalError> {
        match self.peek().cloned() {
            Some(Tok::Num(n)) => {
                self.pos += 1;
                Ok(n)
            }
            Some(Tok::Op('(')) => {
                self.pos += 1;
                let v = self.expr()?;
                if !self.eat(')') {
                    return Err(EvalError::Syntax("missing ')'".into()));
                }
                Ok(v)
            }
            Some(Tok::Ident(name)) => {
                self.pos += 1;
                if self.eat('(') {
                    let mut args = vec![self.expr()?];
                    while self.eat(',') {
                        args.push(self.expr()?);
                    }
                    if !self.eat(')') {
                        return Err(EvalError::Syntax("missing ')'".into()));
                    }
                    call(&name, &args)
                } else if name.eq_ignore_ascii_case("pi") {
                    Ok(std::f64::consts::PI)
                } else {
                    (self.vars)(&name).ok_or(EvalError::UnknownVariable(name))
                }
            }
            other => Err(EvalError::Syntax(format!("unexpected {other:?}"))),
        }
    }
}

fn call(name: &str, a: &[f64]) -> Result<f64, EvalError> {
    let one = |f: fn(f64) -> f64| {
        if a.len() == 1 {
            Ok(f(a[0]))
        } else {
            Err(EvalError::Syntax(format!("{name} takes one argument")))
        }
    };
    match name.to_ascii_lowercase().as_str() {
        "sin" => one(|x| x.to_radians().sin()),
        "cos" => one(|x| x.to_radians().cos()),
        "tan" => one(|x| x.to_radians().tan()),
        "sqrt" => one(f64::sqrt),
        "abs" => one(f64::abs),
        "round" => one(f64::round),
        "min" => Ok(a.iter().copied().fold(f64::INFINITY, f64::min)),
        "max" => Ok(a.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
        _ => Err(EvalError::UnknownFunction(name.to_string())),
    }
}

/// Evaluates `src` with variables looked up through `vars`.
pub fn eval(src: &str, vars: &dyn Fn(&str) -> Option<f64>) -> Result<f64, EvalError> {
    let src = src.trim().trim_start_matches('=');
    let mut p = Parser { toks: lex(src)?, pos: 0, vars };
    let v = p.expr()?;
    if p.pos != p.toks.len() {
        return Err(EvalError::Syntax("trailing input".into()));
    }
    Ok(v)
}

/// Evaluates a set of named expressions that may reference each other, in dependency order.
pub fn eval_all(defs: &[(String, String)]) -> (BTreeMap<String, f64>, BTreeMap<String, EvalError>) {
    let mut vals: BTreeMap<String, f64> = BTreeMap::new();
    let mut errs: BTreeMap<String, EvalError> = BTreeMap::new();
    let mut pending: Vec<&(String, String)> = defs.iter().collect();
    loop {
        let before = pending.len();
        pending.retain(|(name, expr)| {
            let lookup = |n: &str| vals.get(n).copied();
            match eval(expr, &lookup) {
                Ok(v) => {
                    vals.insert(name.clone(), v);
                    false
                }
                Err(EvalError::UnknownVariable(v)) if defs.iter().any(|(n, _)| *n == v) => true,
                Err(e) => {
                    errs.insert(name.clone(), e);
                    false
                }
            }
        });
        if pending.is_empty() || pending.len() == before {
            break;
        }
    }
    if !pending.is_empty() {
        let names: Vec<String> = pending.iter().map(|(n, _)| n.clone()).collect();
        for n in &names {
            errs.insert(n.clone(), EvalError::Cycle(names.clone()));
        }
    }
    (vals, errs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic() {
        let none = |_: &str| None;
        assert_eq!(eval("1 + 2 * 3", &none).unwrap(), 7.0);
        assert_eq!(eval("(1 + 2) * 3", &none).unwrap(), 9.0);
        assert_eq!(eval("-2^2", &none).unwrap(), -4.0);
        assert_eq!(eval("2^3^2", &none).unwrap(), 512.0);
        assert!((eval("= sin(30)", &none).unwrap() - 0.5).abs() < 1e-12);
        assert_eq!(eval("max(1, 5, 3)", &none).unwrap(), 5.0);
        assert_eq!(eval("1.5e2", &none).unwrap(), 150.0);
        assert!(eval("1 +", &none).is_err());
    }

    #[test]
    fn variables_any_order() {
        let defs = vec![
            ("depth".to_string(), "\"width\" / 2".to_string()),
            ("width".to_string(), "40".to_string()),
            ("hole".to_string(), "depth - 5".to_string()),
        ];
        let (v, e) = eval_all(&defs);
        assert!(e.is_empty(), "{e:?}");
        assert_eq!(v["depth"], 20.0);
        assert_eq!(v["hole"], 15.0);
    }

    #[test]
    fn cycle_detected() {
        let defs = vec![("a".to_string(), "b + 1".to_string()), ("b".to_string(), "a + 1".to_string())];
        let (_, e) = eval_all(&defs);
        assert!(matches!(e["a"], EvalError::Cycle(_)));
    }
}
