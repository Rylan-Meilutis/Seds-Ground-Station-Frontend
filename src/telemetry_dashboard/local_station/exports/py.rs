//! Python's JSON values and number formatting, for byte-identical ports of
//! the station's Python report worker (`recording_report.py`).
//!
//! [`Py`] keeps what Python's `json.loads` keeps and serde_json does not:
//! object key order, and whether a number was written as an int or a float.
//! [`Py::dumps`] writes like `json.dumps` (ASCII only, `repr` floats), and
//! [`repr_float`] is Python's `repr(float)`.
use serde_json::Value;

#[derive(Clone, Debug)]
pub(crate) enum Py {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Py>),
    Dict(Vec<(String, Py)>),
}

/// `repr(x)` for a Python float: the shortest round-trip digits, fixed
/// notation for decimal exponents -4 to 15, otherwise `d.ddde±XX`.
pub(crate) fn repr_float(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let sci = format!("{x:e}");
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let (sign, mantissa) = mantissa.strip_prefix('-').map_or(("", mantissa), |m| ("-", m));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    if !(-4..16).contains(&exponent) {
        let rest = &digits[1..];
        let sign_e = if exponent < 0 { '-' } else { '+' };
        return if rest.is_empty() {
            format!("{sign}{}e{sign_e}{:02}", &digits[..1], exponent.abs())
        } else {
            format!("{sign}{}.{rest}e{sign_e}{:02}", &digits[..1], exponent.abs())
        };
    }
    let point = exponent + 1;
    let body = if point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else if point as usize >= digits.len() {
        format!("{digits}{}.0", "0".repeat(point as usize - digits.len()))
    } else {
        format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
    };
    format!("{sign}{body}")
}

fn escape(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(c),
            _ => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
}

impl Py {
    pub fn str(text: impl Into<String>) -> Py {
        Py::Str(text.into())
    }

    pub fn dict(items: Vec<(&str, Py)>) -> Py {
        Py::Dict(items.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }

    pub fn get(&self, key: &str) -> Option<&Py> {
        match self {
            Py::Dict(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// `d[key] = value`: replaces in place, else appends (Python dict order).
    pub fn set(&mut self, key: &str, value: Py) {
        if let Py::Dict(items) = self {
            match items.iter_mut().find(|(k, _)| k == key) {
                Some(slot) => slot.1 = value,
                None => items.push((key.to_owned(), value)),
            }
        }
    }

    /// `d.update(other)` for a dict `other`.
    pub fn update(&mut self, other: Py) {
        if let Py::Dict(items) = other {
            for (k, v) in items {
                self.set(&k, v);
            }
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Py::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Py]> {
        match self {
            Py::List(items) => Some(items),
            _ => None,
        }
    }

    pub fn is_dict(&self) -> bool {
        matches!(self, Py::Dict(_))
    }

    /// The number as a float (bools and ints included).
    pub fn number(&self) -> Option<f64> {
        match self {
            Py::Bool(b) => Some(f64::from(u8::from(*b))),
            Py::Int(i) => Some(*i as f64),
            Py::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// Python truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            Py::None => false,
            Py::Bool(b) => *b,
            Py::Int(i) => *i != 0,
            Py::Float(f) => *f != 0.0,
            Py::Str(s) => !s.is_empty(),
            Py::List(items) => !items.is_empty(),
            Py::Dict(items) => !items.is_empty(),
        }
    }

    /// Python `==`: numbers compare by value across bool/int/float, dicts
    /// ignore key order.
    pub fn py_eq(&self, other: &Py) -> bool {
        match (self, other) {
            (Py::None, Py::None) => true,
            (Py::Str(a), Py::Str(b)) => a == b,
            (Py::List(a), Py::List(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.py_eq(y)),
            (Py::Dict(a), Py::Dict(b)) => a.len() == b.len() && a.iter().all(|(k, v)| other.get(k).is_some_and(|w| v.py_eq(w))),
            (Py::Int(a), Py::Int(b)) => a == b,
            _ => match (self.number(), other.number()) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            },
        }
    }

    /// Python `str(x)` for a scalar (`None` is the empty CSV cell, see callers).
    pub fn py_str(&self) -> String {
        match self {
            Py::None => "None".into(),
            Py::Bool(true) => "True".into(),
            Py::Bool(false) => "False".into(),
            Py::Int(i) => i.to_string(),
            Py::Float(f) => repr_float(*f),
            Py::Str(s) => s.clone(),
            other => other.dumps(false, false),
        }
    }

    /// `json.dumps(self)`, with `separators=(",", ":")` when `compact` and
    /// `sort_keys=True` when `sort_keys`.
    pub fn dumps(&self, compact: bool, sort_keys: bool) -> String {
        let mut out = String::new();
        self.write(compact, sort_keys, &mut out);
        out
    }

    fn write(&self, compact: bool, sort_keys: bool, out: &mut String) {
        let (item, key) = if compact { (",", ":") } else { (", ", ": ") };
        match self {
            Py::None => out.push_str("null"),
            Py::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Py::Int(i) => out.push_str(&i.to_string()),
            Py::Float(f) if f.is_nan() => out.push_str("NaN"),
            Py::Float(f) if f.is_infinite() => out.push_str(if *f > 0.0 { "Infinity" } else { "-Infinity" }),
            Py::Float(f) => out.push_str(&repr_float(*f)),
            Py::Str(s) => escape(s, out),
            Py::List(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(item);
                    }
                    v.write(compact, sort_keys, out);
                }
                out.push(']');
            }
            Py::Dict(items) => {
                let mut refs: Vec<&(String, Py)> = items.iter().collect();
                if sort_keys {
                    refs.sort_by(|a, b| a.0.cmp(&b.0));
                }
                out.push('{');
                for (i, (k, v)) in refs.into_iter().enumerate() {
                    if i > 0 {
                        out.push_str(item);
                    }
                    escape(k, out);
                    out.push_str(key);
                    v.write(compact, sort_keys, out);
                }
                out.push('}');
            }
        }
    }

    /// A serde_json value (key order as serde_json keeps it).
    pub fn from_json(value: &Value) -> Py {
        match value {
            Value::Null => Py::None,
            Value::Bool(b) => Py::Bool(*b),
            Value::Number(n) => n.as_i64().map(Py::Int).unwrap_or_else(|| Py::Float(n.as_f64().unwrap_or(f64::NAN))),
            Value::String(s) => Py::Str(s.clone()),
            Value::Array(items) => Py::List(items.iter().map(Py::from_json).collect()),
            Value::Object(map) => Py::Dict(map.iter().map(|(k, v)| (k.clone(), Py::from_json(v))).collect()),
        }
    }

    /// `json.loads(text)`: key order kept, `1` an int and `1.0` a float,
    /// `NaN`/`Infinity` accepted, floats correctly rounded.
    pub fn parse(text: &str) -> Result<Py, String> {
        let mut parser = Parser { b: text.as_bytes(), i: 0 };
        let value = parser.value()?;
        parser.ws();
        if parser.i != parser.b.len() {
            return Err(format!("Extra data: char {}", parser.i));
        }
        Ok(value)
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.b.get(self.i).is_some_and(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }
    fn err<T>(&self) -> Result<T, String> {
        Err(format!("Expecting value: char {}", self.i))
    }
    fn eat(&mut self, word: &str) -> bool {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            true
        } else {
            false
        }
    }
    fn value(&mut self) -> Result<Py, String> {
        self.ws();
        let Some(&c) = self.b.get(self.i) else { return self.err() };
        match c {
            b'{' => {
                self.i += 1;
                let mut items: Vec<(String, Py)> = Vec::new();
                self.ws();
                if self.eat("}") {
                    return Ok(Py::Dict(items));
                }
                loop {
                    self.ws();
                    if self.b.get(self.i) != Some(&b'"') {
                        return self.err();
                    }
                    let key = self.string()?;
                    self.ws();
                    if !self.eat(":") {
                        return self.err();
                    }
                    let value = self.value()?;
                    // A repeated key keeps its first position and the last value, as in Python.
                    match items.iter_mut().find(|(k, _)| *k == key) {
                        Some(slot) => slot.1 = value,
                        None => items.push((key, value)),
                    }
                    self.ws();
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("}") {
                        return Ok(Py::Dict(items));
                    }
                    return self.err();
                }
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.eat("]") {
                    return Ok(Py::List(items));
                }
                loop {
                    items.push(self.value()?);
                    self.ws();
                    if self.eat(",") {
                        continue;
                    }
                    if self.eat("]") {
                        return Ok(Py::List(items));
                    }
                    return self.err();
                }
            }
            b'"' => Ok(Py::Str(self.string()?)),
            b't' if self.eat("true") => Ok(Py::Bool(true)),
            b'f' if self.eat("false") => Ok(Py::Bool(false)),
            b'n' if self.eat("null") => Ok(Py::None),
            b'N' if self.eat("NaN") => Ok(Py::Float(f64::NAN)),
            b'I' if self.eat("Infinity") => Ok(Py::Float(f64::INFINITY)),
            b'-' if self.eat("-Infinity") => Ok(Py::Float(f64::NEG_INFINITY)),
            b'-' | b'0'..=b'9' => {
                let start = self.i;
                if self.b[self.i] == b'-' {
                    self.i += 1;
                }
                let mut float = false;
                while let Some(&c) = self.b.get(self.i) {
                    match c {
                        b'0'..=b'9' => {}
                        b'.' | b'e' | b'E' | b'+' | b'-' => float = true,
                        _ => break,
                    }
                    self.i += 1;
                }
                let text = std::str::from_utf8(&self.b[start..self.i]).map_err(|e| e.to_string())?;
                if !float && let Ok(i) = text.parse::<i64>() {
                    return Ok(Py::Int(i));
                }
                text.parse::<f64>().map(Py::Float).map_err(|_| format!("Expecting value: char {start}"))
            }
            _ => self.err(),
        }
    }
    fn string(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut out = String::new();
        loop {
            let start = self.i;
            while self.b.get(self.i).is_some_and(|&c| c != b'"' && c != b'\\') {
                self.i += 1;
            }
            out.push_str(std::str::from_utf8(&self.b[start..self.i]).map_err(|e| e.to_string())?);
            match self.b.get(self.i) {
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    let c = *self.b.get(self.i + 1).ok_or("Unterminated string")?;
                    self.i += 2;
                    match c {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let unit = self.hex4()?;
                            let code = if (0xd800..0xdc00).contains(&unit) && self.b[self.i..].starts_with(b"\\u") {
                                self.i += 2;
                                let low = self.hex4()?;
                                0x10000 + ((unit - 0xd800) << 10) + (low.wrapping_sub(0xdc00) & 0x3ff)
                            } else {
                                unit
                            };
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        _ => return Err("Invalid \\escape".into()),
                    }
                }
                _ => return Err("Unterminated string".into()),
            }
        }
    }
    fn hex4(&mut self) -> Result<u32, String> {
        let text = self.b.get(self.i..self.i + 4).and_then(|h| std::str::from_utf8(h).ok()).ok_or("Invalid \\uXXXX escape")?;
        self.i += 4;
        u32::from_str_radix(text, 16).map_err(|_| "Invalid \\uXXXX escape".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_print_as_python_repr() {
        for (x, text) in [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (1e-7, "1e-07"),
            (1e16, "1e+16"),
            (1e15, "1000000000000000.0"),
            (0.0001, "0.0001"),
            (0.00012, "0.00012"),
            (1.5e-5, "1.5e-05"),
            (123456789.0, "123456789.0"),
            (0.1 + 0.2, "0.30000000000000004"),
            (-2.5e300, "-2.5e+300"),
            (31.7580169, "31.7580169"),
            (5e-324, "5e-324"),
        ] {
            assert_eq!(repr_float(x), text);
        }
    }

    #[test]
    fn json_round_trips_like_python() {
        let text = r#"{"b": 1, "a": [1.0, 2, -0, 1e5, true, null, "\u00e9\n"], "b": 3}"#;
        let value = Py::parse(text).unwrap();
        assert_eq!(value.dumps(false, false), r#"{"b": 3, "a": [1.0, 2, 0, 100000.0, true, null, "\u00e9\n"]}"#);
        assert_eq!(value.dumps(true, true), r#"{"a":[1.0,2,0,100000.0,true,null,"\u00e9\n"],"b":3}"#);
        assert!(Py::parse("[1]").unwrap().py_eq(&Py::parse("[1.0]").unwrap()));
        assert_eq!(Py::str("\u{1f600}\u{b0}").dumps(true, false), r#""\ud83d\ude00\u00b0""#);
    }
}
