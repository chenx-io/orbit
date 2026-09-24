// ─── Arithmetic evaluator ─────────────────────────────────────────────
// Minimal, safe recursive-descent evaluator: numeric operations only, avoiding arbitrary code execution.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
enum ArithTok {
    Num(f64),
    Ident(String),
    Op(char),
    LParen,
    RParen,
}

fn tokenize_arith(expr: &str) -> Result<Vec<ArithTok>, String> {
    let chars: Vec<char> = expr.chars().collect();
    let mut idx = 0;
    let mut toks = Vec::new();
    while idx < chars.len() {
        let c = chars[idx];
        if c.is_whitespace() {
            idx += 1;
            continue;
        }
        if c.is_ascii_digit() || c == '.' {
            let start = idx;
            while idx < chars.len() && (chars[idx].is_ascii_digit() || chars[idx] == '.') {
                idx += 1;
            }
            let s: String = chars[start..idx].iter().collect();
            let n: f64 = s.parse().map_err(|_| format!("invalid number '{}'", s))?;
            toks.push(ArithTok::Num(n));
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = idx;
            while idx < chars.len()
                && (chars[idx].is_alphanumeric() || chars[idx] == '_' || chars[idx] == '.')
            {
                idx += 1;
            }
            let s: String = chars[start..idx].iter().collect();
            toks.push(ArithTok::Ident(s));
            continue;
        }
        match c {
            '+' | '-' | '*' | '/' | '%' => {
                toks.push(ArithTok::Op(c));
                idx += 1;
            }
            '(' => {
                toks.push(ArithTok::LParen);
                idx += 1;
            }
            ')' => {
                toks.push(ArithTok::RParen);
                idx += 1;
            }
            _ => return Err(format!("unexpected character '{}'", c)),
        }
    }
    Ok(toks)
}

struct ArithParser<'a> {
    tokens: Vec<ArithTok>,
    pos: usize,
    vars: &'a HashMap<String, String>,
}

impl<'a> ArithParser<'a> {
    fn next_op(&self) -> Option<char> {
        match self.tokens.get(self.pos) {
            Some(ArithTok::Op(c)) => Some(*c),
            _ => None,
        }
    }

    fn peek(&self) -> Option<ArithTok> {
        self.tokens.get(self.pos).cloned()
    }

    fn parse_expr(&mut self) -> Result<f64, String> {
        let mut lhs = self.parse_term()?;
        while let Some(c @ ('+' | '-')) = self.next_op() {
            self.pos += 1;
            let rhs = self.parse_term()?;
            lhs = if c == '+' { lhs + rhs } else { lhs - rhs };
        }
        Ok(lhs)
    }

    fn parse_term(&mut self) -> Result<f64, String> {
        let mut lhs = self.parse_factor()?;
        while let Some(c @ ('*' | '/' | '%')) = self.next_op() {
            self.pos += 1;
            let rhs = self.parse_factor()?;
            lhs = match c {
                '*' => lhs * rhs,
                '/' => {
                    if rhs == 0.0 {
                        return Err("division by zero".into());
                    }
                    lhs / rhs
                }
                '%' => {
                    if rhs == 0.0 {
                        return Err("modulo by zero".into());
                    }
                    lhs % rhs
                }
                _ => unreachable!(),
            };
        }
        Ok(lhs)
    }

    fn parse_factor(&mut self) -> Result<f64, String> {
        if let Some(c) = self.next_op() {
            if c == '-' {
                self.pos += 1;
                return Ok(-self.parse_factor()?);
            }
            if c == '+' {
                self.pos += 1;
                return self.parse_factor();
            }
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some(ArithTok::Num(n)) => {
                self.pos += 1;
                Ok(n)
            }
            Some(ArithTok::Ident(name)) => {
                self.pos += 1;
                let raw = self
                    .vars
                    .get(&name)
                    .cloned()
                    .or_else(|| std::env::var(&name).ok())
                    .ok_or_else(|| format!("unknown variable '{}'", name))?;
                raw.parse::<f64>()
                    .map_err(|_| format!("variable '{}' is not numeric: '{}'", name, raw))
            }
            Some(ArithTok::LParen) => {
                self.pos += 1;
                let v = self.parse_expr()?;
                match self.peek() {
                    Some(ArithTok::RParen) => {
                        self.pos += 1;
                        Ok(v)
                    }
                    _ => Err("expected ')'".into()),
                }
            }
            _ => Err("unexpected token in expression".into()),
        }
    }
}

/// Evaluate an arithmetic expression, resolving variables from `vars` (falling back to environment variables).
pub(crate) fn eval_arithmetic(expr: &str, vars: &HashMap<String, String>) -> Result<f64, String> {
    let tokens = tokenize_arith(expr)?;
    if tokens.is_empty() {
        return Err("empty expression".into());
    }
    let mut p = ArithParser {
        tokens,
        pos: 0,
        vars,
    };
    let v = p.parse_expr()?;
    if p.pos != p.tokens.len() {
        return Err("unexpected trailing tokens".into());
    }
    Ok(v)
}
