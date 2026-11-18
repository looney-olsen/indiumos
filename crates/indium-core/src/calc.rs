//! A small, safe calculator for the Spotlight-style launcher.
//!
//! Supports `+ - * / % ^`, parentheses, unary minus, `pi`, `e`, and the
//! functions `sqrt sin cos tan ln log exp abs round floor ceil`.

use std::fmt;

#[derive(Debug, PartialEq)]
pub enum CalcError {
    Empty,
    Syntax(String),
    DivideByZero,
    Domain,
}

impl fmt::Display for CalcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CalcError::Empty => write!(f, "empty expression"),
            CalcError::Syntax(msg) => write!(f, "syntax error: {msg}"),
            CalcError::DivideByZero => write!(f, "division by zero"),
            CalcError::Domain => write!(f, "result is not a real number"),
        }
    }
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

pub fn eval(input: &str) -> Result<f64, CalcError> {
    let cleaned = input.replace("**", "^").replace('×', "*").replace('÷', "/");
    if cleaned.trim().is_empty() {
        return Err(CalcError::Empty);
    }
    let mut parser = Parser {
        chars: cleaned.chars().collect(),
        pos: 0,
    };
    let value = parser.expr(0)?;
    if let Some(c) = parser.peek() {
        // Leftover input, e.g. "1 2": spaces separate tokens, they never join digits.
        return Err(CalcError::Syntax(format!("unexpected '{c}'")));
    }
    if value.is_finite() {
        Ok(value)
    } else {
        Err(CalcError::Domain)
    }
}

const MAX_DEPTH: usize = 64;

impl Parser {
    /// Next character, skipping whitespace between tokens.
    fn peek(&mut self) -> Option<char> {
        while matches!(self.chars.get(self.pos), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
        self.chars.get(self.pos).copied()
    }

    /// Next character without skipping whitespace (used inside numbers and names).
    fn raw(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expr(&mut self, depth: usize) -> Result<f64, CalcError> {
        if depth > MAX_DEPTH {
            return Err(CalcError::Syntax("too deeply nested".into()));
        }
        let mut value = self.term(depth)?;
        loop {
            if self.eat('+') {
                value += self.term(depth)?;
            } else if self.eat('-') {
                value -= self.term(depth)?;
            } else {
                return Ok(value);
            }
        }
    }

    fn term(&mut self, depth: usize) -> Result<f64, CalcError> {
        let mut value = self.unary(depth)?;
        loop {
            if self.eat('*') {
                value *= self.unary(depth)?;
            } else if self.eat('/') {
                let divisor = self.unary(depth)?;
                if divisor == 0.0 {
                    return Err(CalcError::DivideByZero);
                }
                value /= divisor;
            } else if self.eat('%') {
                let divisor = self.unary(depth)?;
                if divisor == 0.0 {
                    return Err(CalcError::DivideByZero);
                }
                value %= divisor;
            } else {
                return Ok(value);
            }
        }
    }

    fn unary(&mut self, depth: usize) -> Result<f64, CalcError> {
        if depth > MAX_DEPTH {
            return Err(CalcError::Syntax("too deeply nested".into()));
        }
        if self.eat('-') {
            return Ok(-self.unary(depth + 1)?);
        }
        if self.eat('+') {
            return self.unary(depth + 1);
        }
        self.power(depth)
    }

    fn power(&mut self, depth: usize) -> Result<f64, CalcError> {
        let base = self.atom(depth)?;
        if self.eat('^') {
            // Right associative: 2^3^2 = 2^(3^2); allows 2^-1.
            let exponent = self.unary(depth + 1)?;
            return Ok(base.powf(exponent));
        }
        Ok(base)
    }

    fn atom(&mut self, depth: usize) -> Result<f64, CalcError> {
        match self.peek() {
            None => Err(CalcError::Syntax("unexpected end".into())),
            Some('(') => {
                self.pos += 1;
                let value = self.expr(depth + 1)?;
                if !self.eat(')') {
                    return Err(CalcError::Syntax("missing ')'".into()));
                }
                Ok(value)
            }
            Some(c) if c.is_ascii_digit() || c == '.' => self.number(),
            Some(c) if c.is_ascii_alphabetic() => self.ident(depth),
            Some(c) => Err(CalcError::Syntax(format!("unexpected '{c}'"))),
        }
    }

    fn number(&mut self) -> Result<f64, CalcError> {
        let start = self.pos;
        while matches!(self.raw(), Some(c) if c.is_ascii_digit() || c == '.') {
            self.pos += 1;
        }
        // Exponent such as 1e-3, but only when digits follow, so `2e` stays the constant e.
        if matches!(self.raw(), Some('e') | Some('E')) {
            let mut look = self.pos + 1;
            if matches!(self.chars.get(look), Some('+') | Some('-')) {
                look += 1;
            }
            if matches!(self.chars.get(look), Some(c) if c.is_ascii_digit()) {
                self.pos = look;
                while matches!(self.raw(), Some(c) if c.is_ascii_digit()) {
                    self.pos += 1;
                }
            }
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        text.parse::<f64>()
            .map_err(|_| CalcError::Syntax(format!("bad number '{text}'")))
    }

    fn ident(&mut self, depth: usize) -> Result<f64, CalcError> {
        let start = self.pos;
        while matches!(self.raw(), Some(c) if c.is_ascii_alphabetic()) {
            self.pos += 1;
        }
        let name: String = self.chars[start..self.pos]
            .iter()
            .collect::<String>()
            .to_lowercase();

        if self.eat('(') {
            let arg = self.expr(depth + 1)?;
            if !self.eat(')') {
                return Err(CalcError::Syntax("missing ')'".into()));
            }
            return apply(&name, arg);
        }
        match name.as_str() {
            "pi" => Ok(std::f64::consts::PI),
            "e" => Ok(std::f64::consts::E),
            other => Err(CalcError::Syntax(format!("unknown name '{other}'"))),
        }
    }
}

fn apply(name: &str, x: f64) -> Result<f64, CalcError> {
    match name {
        "sqrt" if x < 0.0 => Err(CalcError::Domain),
        "sqrt" => Ok(x.sqrt()),
        "sin" => Ok(x.sin()),
        "cos" => Ok(x.cos()),
        "tan" => Ok(x.tan()),
        "ln" | "log" if x <= 0.0 => Err(CalcError::Domain),
        "ln" => Ok(x.ln()),
        "log" => Ok(x.log10()),
        "exp" => Ok(x.exp()),
        "abs" => Ok(x.abs()),
        "round" => Ok(x.round()),
        "floor" => Ok(x.floor()),
        "ceil" => Ok(x.ceil()),
        other => Err(CalcError::Syntax(format!("unknown function '{other}'"))),
    }
}

/// Format a result for display: no trailing zeros, scientific for extremes.
pub fn format_result(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    if v.abs() >= 1e15 || v.abs() < 1e-9 {
        return format!("{v:e}");
    }
    let mut s = format!("{v:.10}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        s = "0".into();
    }
    s
}

/// If `input` is a calculation (not just a number or a plain app name),
/// return the formatted answer.
pub fn calculate(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let body = trimmed.trim_start_matches(['-', '+']);
    let has_operator = body.chars().any(|c| "+-*/^%()×÷".contains(c));
    let has_function_or_constant = body.chars().any(|c| c.is_ascii_alphabetic());
    let has_digit_or_const = body.chars().any(|c| c.is_ascii_digit()) || has_function_or_constant;
    if !(has_operator || has_function_or_constant) || !has_digit_or_const {
        return None;
    }
    eval(trimmed).ok().map(format_result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(expr: &str) -> f64 {
        eval(expr).unwrap_or_else(|e| panic!("{expr}: {e}"))
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(ok("1+2*3"), 7.0);
        assert_eq!(ok("(1+2)*3"), 9.0);
        assert_eq!(ok("10/4"), 2.5);
        assert_eq!(ok("10%4"), 2.0);
        assert_eq!(ok("2 + 3 × 4"), 14.0);
        assert_eq!(ok("8 ÷ 2"), 4.0);
    }

    #[test]
    fn unary_and_power() {
        assert_eq!(ok("-2^2"), -4.0);
        assert_eq!(ok("2^3^2"), 512.0);
        assert_eq!(ok("2**10"), 1024.0);
        assert_eq!(ok("2^-1"), 0.5);
        assert_eq!(ok("--3"), 3.0);
    }

    #[test]
    fn functions_and_constants() {
        assert_eq!(ok("sqrt(16)"), 4.0);
        assert!((ok("pi") - std::f64::consts::PI).abs() < 1e-12);
        assert!((ok("2*pi") - 2.0 * std::f64::consts::PI).abs() < 1e-12);
        assert_eq!(ok("abs(-5)"), 5.0);
        assert_eq!(ok("log(1000)"), 3.0);
        assert!((ok("ln(e)") - 1.0).abs() < 1e-12);
        assert_eq!(ok("floor(2.7)+ceil(2.1)"), 5.0);
    }

    #[test]
    fn spaces_separate_tokens_but_never_join_digits() {
        assert_eq!(ok("  1 + 2  "), 3.0);
        assert_eq!(ok("sqrt (16)"), 4.0);
        assert!(matches!(eval("1 2"), Err(CalcError::Syntax(_))));
        assert!(matches!(eval("1. 5"), Err(CalcError::Syntax(_))));
        assert!(matches!(eval("2 e"), Err(CalcError::Syntax(_))));
    }

    #[test]
    fn exponent_notation_versus_constant_e() {
        assert_eq!(ok("1e3"), 1000.0);
        assert_eq!(ok("2e-1"), 0.2);
        assert!((ok("2*e") - 2.0 * std::f64::consts::E).abs() < 1e-12);
    }

    #[test]
    fn errors_are_reported_not_panicked() {
        assert_eq!(eval("1/0"), Err(CalcError::DivideByZero));
        assert_eq!(eval("5%0"), Err(CalcError::DivideByZero));
        assert_eq!(eval("sqrt(-1)"), Err(CalcError::Domain));
        assert_eq!(eval("ln(0)"), Err(CalcError::Domain));
        assert_eq!(eval("   "), Err(CalcError::Empty));
        assert!(matches!(eval("2+"), Err(CalcError::Syntax(_))));
        assert!(matches!(eval("(1+2"), Err(CalcError::Syntax(_))));
        assert!(matches!(eval("1 2"), Err(CalcError::Syntax(_))));
        assert!(matches!(eval("foo(2)"), Err(CalcError::Syntax(_))));
        assert!(matches!(eval("bar"), Err(CalcError::Syntax(_))));
    }

    #[test]
    fn deep_nesting_is_rejected_instead_of_overflowing_the_stack() {
        let deep = format!("{}1{}", "(".repeat(5000), ")".repeat(5000));
        assert!(matches!(eval(&deep), Err(CalcError::Syntax(_))));
        let negs = format!("{}1", "-".repeat(5000));
        assert!(matches!(eval(&negs), Err(CalcError::Syntax(_))));
    }

    #[test]
    fn overflow_is_an_error() {
        assert_eq!(eval("10^999"), Err(CalcError::Domain));
    }

    #[test]
    fn formatting() {
        assert_eq!(format_result(0.1 + 0.2), "0.3");
        assert_eq!(format_result(1.0 / 3.0), "0.3333333333");
        assert_eq!(format_result(42.0), "42");
        assert_eq!(format_result(-0.0), "0");
        assert_eq!(format_result(1e20), "1e20");
        assert_eq!(format_result(2.5e-12), "2.5e-12");
    }

    #[test]
    fn launcher_only_shows_real_calculations() {
        assert_eq!(calculate("2+2"), Some("4".into()));
        assert_eq!(calculate("sqrt(81)"), Some("9".into()));
        assert_eq!(calculate("pi"), Some("3.1415926536".into()));
        assert_eq!(calculate("5"), None); // a bare number is not a calculation
        assert_eq!(calculate("-5"), None);
        assert_eq!(calculate("firefox"), None); // unknown name fails to evaluate
        assert_eq!(calculate("files"), None);
        assert_eq!(calculate(""), None);
        assert_eq!(calculate("1/0"), None);
    }
}
