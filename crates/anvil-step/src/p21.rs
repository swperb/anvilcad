//! Reader for the STEP exchange structure (ISO 10303-21, "Part 21").
//!
//! Turns the file text into entity instances keyed by their `#id`. The reader is
//! defensive: it never panics on malformed input, nesting depth is bounded, and
//! every problem is reported with its line number.

use std::collections::HashMap;

/// Maximum nesting of lists and typed parameters.
const MAX_DEPTH: usize = 256;

#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    Int(i64),
    Real(f64),
    Str(String),
    /// An enumeration value such as `.T.` or `.MILLI.`, without the dots.
    Enum(String),
    Ref(u64),
    List(Vec<Param>),
    /// A typed parameter such as `LENGTH_MEASURE(1.E-06)`.
    Typed(String, Box<Param>),
    Binary(String),
    /// `$`: no value.
    Unset,
    /// `*`: value derived elsewhere.
    Derived,
}

impl Param {
    pub fn as_real(&self) -> Option<f64> {
        match self {
            Param::Real(v) => Some(*v),
            Param::Int(v) => Some(*v as f64),
            Param::Typed(_, p) => p.as_real(),
            _ => None,
        }
    }

    pub fn as_ref(&self) -> Option<u64> {
        match self {
            Param::Ref(r) => Some(*r),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Param]> {
        match self {
            Param::List(l) => Some(l),
            _ => None,
        }
    }

    pub fn as_enum(&self) -> Option<&str> {
        match self {
            Param::Enum(e) => Some(e),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Param::Str(s) => Some(s),
            _ => None,
        }
    }

    /// `.T.` or `.F.`.
    pub fn as_bool(&self) -> Option<bool> {
        match self.as_enum()? {
            "T" => Some(true),
            "F" => Some(false),
            _ => None,
        }
    }
}

/// One entity instance. Complex instances (`#1=(A()B()C());`) have several parts.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub parts: Vec<(String, Vec<Param>)>,
}

impl Entity {
    /// The type name of a simple instance, or the first part of a complex one.
    pub fn name(&self) -> &str {
        self.parts.first().map(|(n, _)| n.as_str()).unwrap_or("")
    }

    pub fn is_complex(&self) -> bool {
        self.parts.len() > 1
    }

    /// Parameters of the part with this type name.
    pub fn part(&self, name: &str) -> Option<&[Param]> {
        self.parts.iter().find(|(n, _)| n == name).map(|(_, p)| p.as_slice())
    }

    pub fn has(&self, name: &str) -> bool {
        self.parts.iter().any(|(n, _)| n == name)
    }

    /// Parameters of a simple instance.
    pub fn params(&self) -> &[Param] {
        self.parts.first().map(|(_, p)| p.as_slice()).unwrap_or(&[])
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ParseError {
    #[error("line {line}: {msg}")]
    Syntax { line: usize, msg: String },
    #[error("this is not a STEP file (it does not start with ISO-10303-21)")]
    NotStep,
}

/// A parsed exchange file.
#[derive(Debug, Clone, Default)]
pub struct Exchange {
    /// Header entities, in order (FILE_DESCRIPTION, FILE_NAME, FILE_SCHEMA, ...).
    pub header: Vec<(String, Vec<Param>)>,
    pub entities: HashMap<u64, Entity>,
}

impl Exchange {
    pub fn get(&self, id: u64) -> Option<&Entity> {
        self.entities.get(&id)
    }

    /// The schema names listed in FILE_SCHEMA, e.g. `AUTOMOTIVE_DESIGN`.
    pub fn schemas(&self) -> Vec<String> {
        self.header
            .iter()
            .filter(|(n, _)| n == "FILE_SCHEMA")
            .flat_map(|(_, p)| p.first().and_then(Param::as_list).unwrap_or(&[]).to_vec())
            .filter_map(|p| p.as_str().map(str::to_string))
            .collect()
    }

    /// Ids of every instance with this type (simple or as part of a complex one).
    pub fn of_type<'a>(&'a self, name: &'a str) -> impl Iterator<Item = u64> + 'a {
        let mut ids: Vec<u64> =
            self.entities.iter().filter(|(_, e)| e.has(name)).map(|(id, _)| *id).collect();
        ids.sort_unstable();
        ids.into_iter()
    }
}

struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    line: usize,
}

impl<'a> Lexer<'a> {
    fn err<T>(&self, msg: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError::Syntax { line: self.line, msg: msg.into() })
    }

    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        if c == b'\n' {
            self.line += 1;
        }
        Some(c)
    }

    /// Skips white space and comments.
    fn skip(&mut self) -> Result<(), ParseError> {
        loop {
            match self.peek() {
                Some(c) if c.is_ascii_whitespace() => {
                    self.bump();
                }
                Some(b'/') if self.src.get(self.pos + 1) == Some(&b'*') => {
                    self.pos += 2;
                    loop {
                        match self.bump() {
                            None => return self.err("unterminated comment"),
                            Some(b'*') if self.peek() == Some(b'/') => {
                                self.pos += 1;
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), ParseError> {
        self.skip()?;
        match self.bump() {
            Some(x) if x == c => Ok(()),
            Some(x) => self.err(format!("expected '{}', found '{}'", c as char, x as char)),
            None => self.err(format!("expected '{}', found end of file", c as char)),
        }
    }

    fn keyword(&mut self) -> Result<String, ParseError> {
        self.skip()?;
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' {
                self.bump();
            } else {
                break;
            }
        }
        if self.pos == start {
            return self.err("expected a keyword");
        }
        Ok(String::from_utf8_lossy(&self.src[start..self.pos]).to_ascii_uppercase())
    }

    fn number(&mut self) -> Result<Param, ParseError> {
        let start = self.pos;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.bump();
        }
        let mut real = false;
        while let Some(c) = self.peek() {
            match c {
                b'0'..=b'9' => {
                    self.bump();
                }
                b'.' => {
                    real = true;
                    self.bump();
                }
                b'E' | b'e' => {
                    real = true;
                    self.bump();
                    if matches!(self.peek(), Some(b'+' | b'-')) {
                        self.bump();
                    }
                }
                _ => break,
            }
        }
        let text = String::from_utf8_lossy(&self.src[start..self.pos]).to_string();
        if real {
            // "1." and "1.E-06" are valid STEP reals; Rust needs a digit after the point.
            let fixed = text.replacen(".E", ".0E", 1).replacen(".e", ".0e", 1);
            let fixed = if fixed.ends_with('.') { format!("{fixed}0") } else { fixed };
            match fixed.parse::<f64>() {
                Ok(v) if v.is_finite() => Ok(Param::Real(v)),
                _ => self.err(format!("bad real number '{text}'")),
            }
        } else {
            match text.parse::<i64>() {
                Ok(v) => Ok(Param::Int(v)),
                Err(_) => self.err(format!("bad integer '{text}'")),
            }
        }
    }

    fn string(&mut self) -> Result<Param, ParseError> {
        self.bump(); // opening quote
        let mut raw = Vec::new();
        loop {
            match self.bump() {
                None => return self.err("unterminated string"),
                Some(b'\'') => {
                    if self.peek() == Some(b'\'') {
                        self.bump();
                        raw.push(b'\'');
                    } else {
                        break;
                    }
                }
                Some(b'\n' | b'\r') => {}
                Some(c) => raw.push(c),
            }
        }
        Ok(Param::Str(decode_string(&raw)))
    }

    fn param(&mut self, depth: usize) -> Result<Param, ParseError> {
        if depth > MAX_DEPTH {
            return self.err("parameters are nested too deeply");
        }
        self.skip()?;
        match self.peek() {
            None => self.err("unexpected end of file in parameters"),
            Some(b'$') => {
                self.bump();
                Ok(Param::Unset)
            }
            Some(b'*') => {
                self.bump();
                Ok(Param::Derived)
            }
            Some(b'#') => {
                self.bump();
                let start = self.pos;
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.bump();
                }
                let digits = String::from_utf8_lossy(&self.src[start..self.pos]).to_string();
                digits.parse().map(Param::Ref).or_else(|_| self.err("bad entity reference"))
            }
            Some(b'\'') => self.string(),
            Some(b'"') => {
                self.bump();
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if c == b'"' {
                        break;
                    }
                    self.bump();
                }
                let s = String::from_utf8_lossy(&self.src[start..self.pos]).to_string();
                self.expect(b'"')?;
                Ok(Param::Binary(s))
            }
            Some(b'.') => {
                self.bump();
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if c == b'.' {
                        break;
                    }
                    if !(c.is_ascii_alphanumeric() || c == b'_') {
                        return self.err("bad enumeration value");
                    }
                    self.bump();
                }
                let s = String::from_utf8_lossy(&self.src[start..self.pos]).to_ascii_uppercase();
                self.expect(b'.')?;
                Ok(Param::Enum(s))
            }
            Some(b'(') => Ok(Param::List(self.list(depth + 1)?)),
            Some(c) if c == b'+' || c == b'-' || c.is_ascii_digit() => self.number(),
            Some(c) if c.is_ascii_alphabetic() => {
                let name = self.keyword()?;
                self.expect(b'(')?;
                let inner = self.param(depth + 1)?;
                self.expect(b')')?;
                Ok(Param::Typed(name, Box::new(inner)))
            }
            Some(c) => self.err(format!("unexpected '{}' in parameters", c as char)),
        }
    }

    /// A parenthesised, comma-separated list. The opening parenthesis is next.
    fn list(&mut self, depth: usize) -> Result<Vec<Param>, ParseError> {
        self.expect(b'(')?;
        let mut out = Vec::new();
        self.skip()?;
        if self.peek() == Some(b')') {
            self.bump();
            return Ok(out);
        }
        loop {
            out.push(self.param(depth)?);
            self.skip()?;
            match self.bump() {
                Some(b',') => {}
                Some(b')') => return Ok(out),
                Some(c) => return self.err(format!("expected ',' or ')', found '{}'", c as char)),
                None => return self.err("unexpected end of file in a list"),
            }
        }
    }

    /// `NAME(params)` or, for a complex instance, `(NAME(params) NAME(params) ...)`.
    fn instance(&mut self) -> Result<Vec<(String, Vec<Param>)>, ParseError> {
        self.skip()?;
        if self.peek() == Some(b'(') {
            self.bump();
            let mut parts = Vec::new();
            loop {
                self.skip()?;
                if self.peek() == Some(b')') {
                    self.bump();
                    break;
                }
                let name = self.keyword()?;
                let params = self.list(1)?;
                parts.push((name, params));
                if parts.len() > 64 {
                    return self.err("complex instance has too many parts");
                }
            }
            if parts.is_empty() {
                return self.err("empty complex instance");
            }
            Ok(parts)
        } else {
            let name = self.keyword()?;
            let params = self.list(1)?;
            Ok(vec![(name, params)])
        }
    }
}

/// Decodes the Part 21 string escapes \X\hh, \X2\...\X0\, \X4\...\X0\ and \S\c.
fn decode_string(raw: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0;
    let hex = |b: &[u8]| u32::from_str_radix(&String::from_utf8_lossy(b), 16).ok();
    while i < raw.len() {
        if raw[i] == b'\\' && i + 1 < raw.len() {
            match raw[i + 1] {
                b'X' if raw.get(i + 2) == Some(&b'\\') => {
                    if let Some(c) = raw.get(i + 3..i + 5).and_then(hex).and_then(char::from_u32) {
                        out.push(c);
                        i += 5;
                        continue;
                    }
                }
                b'X' if matches!(raw.get(i + 2), Some(b'2' | b'4')) && raw.get(i + 3) == Some(&b'\\') => {
                    let width = if raw[i + 2] == b'2' { 4 } else { 8 };
                    let mut j = i + 4;
                    let mut ok = false;
                    while j + width <= raw.len() {
                        if raw[j..].starts_with(b"\\X0\\") {
                            ok = true;
                            break;
                        }
                        if let Some(c) = hex(&raw[j..j + width]).and_then(char::from_u32) {
                            out.push(c);
                        }
                        j += width;
                    }
                    if ok {
                        i = j + 4;
                        continue;
                    }
                }
                b'S' if raw.get(i + 2) == Some(&b'\\') => {
                    if let Some(&c) = raw.get(i + 3) {
                        out.push(char::from(c.wrapping_add(128)));
                        i += 4;
                        continue;
                    }
                }
                b'\\' => {
                    out.push('\\');
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }
        out.push(char::from(raw[i]));
        i += 1;
    }
    out
}

/// Parses a whole exchange file.
pub fn parse(text: &str) -> Result<Exchange, ParseError> {
    let mut lx = Lexer { src: text.as_bytes(), pos: 0, line: 1 };
    lx.skip()?;
    let magic = lx.keyword().map_err(|_| ParseError::NotStep)?;
    if magic != "ISO-10303-21" {
        return Err(ParseError::NotStep);
    }
    lx.expect(b';')?;
    let mut ex = Exchange::default();
    loop {
        lx.skip()?;
        if lx.peek().is_none() {
            return lx.err("missing END-ISO-10303-21");
        }
        let section = lx.keyword()?;
        match section.as_str() {
            "END-ISO-10303-21" => {
                lx.expect(b';')?;
                return Ok(ex);
            }
            "HEADER" => {
                lx.expect(b';')?;
                loop {
                    let name = lx.keyword()?;
                    if name == "ENDSEC" {
                        lx.expect(b';')?;
                        break;
                    }
                    let params = lx.list(1)?;
                    lx.expect(b';')?;
                    ex.header.push((name, params));
                }
            }
            "DATA" => {
                // Optional DATA('name', ('schema')) in later editions.
                lx.skip()?;
                if lx.peek() == Some(b'(') {
                    lx.list(1)?;
                }
                lx.expect(b';')?;
                loop {
                    lx.skip()?;
                    match lx.peek() {
                        Some(b'#') => {
                            lx.bump();
                            let start = lx.pos;
                            while matches!(lx.peek(), Some(b'0'..=b'9')) {
                                lx.bump();
                            }
                            let id: u64 = String::from_utf8_lossy(&lx.src[start..lx.pos])
                                .parse()
                                .or_else(|_| lx.err("bad entity id"))?;
                            lx.expect(b'=')?;
                            let parts = lx.instance()?;
                            lx.expect(b';')?;
                            if ex.entities.insert(id, Entity { parts }).is_some() {
                                return lx.err(format!("entity #{id} is defined twice"));
                            }
                        }
                        _ => {
                            let kw = lx.keyword()?;
                            if kw != "ENDSEC" {
                                return lx.err(format!("expected an entity or ENDSEC, found {kw}"));
                            }
                            lx.expect(b';')?;
                            break;
                        }
                    }
                }
            }
            other => {
                // Unknown sections (ANCHOR, REFERENCE, SIGNATURE) are skipped.
                loop {
                    lx.skip()?;
                    if lx.peek().is_none() {
                        return lx.err(format!("unterminated section {other}"));
                    }
                    if lx.src[lx.pos..].starts_with(b"ENDSEC") {
                        lx.keyword()?;
                        lx.expect(b';')?;
                        break;
                    }
                    lx.bump();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const SAMPLE: &str = "ISO-10303-21;
HEADER;
FILE_DESCRIPTION(('a part'),'2;1');
FILE_NAME('plate.step','2026-10-12',('Jay'),(''),'','','');
FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));
ENDSEC;
DATA;
/* a comment */
#1=CARTESIAN_POINT('',(0.,1.5,-2.E-03));
#2=DIRECTION('dir',(0.,0.,1.));
#3=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
#4=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#3,'it''s',$);
#5=ADVANCED_FACE('',(#6),#7,.T.);
#6=PRODUCT('\\X2\\00E9\\X0\\t\\X\\E9','',\"0F\",());
ENDSEC;
END-ISO-10303-21;
";

    #[test]
    fn parses_a_sample() {
        let ex = parse(SAMPLE).expect("parse");
        assert_eq!(ex.schemas(), vec!["AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }"]);
        let p = ex.get(1).expect("point");
        assert_eq!(p.name(), "CARTESIAN_POINT");
        let coords = p.params()[1].as_list().expect("list");
        assert_eq!(coords[2].as_real(), Some(-0.002));
        let unit = ex.get(3).expect("unit");
        assert!(unit.is_complex() && unit.has("SI_UNIT"));
        assert_eq!(unit.part("SI_UNIT").and_then(|p| p[0].as_enum()), Some("MILLI"));
        let m = ex.get(4).expect("measure");
        assert_eq!(m.params()[0], Param::Typed("LENGTH_MEASURE".into(), Box::new(Param::Real(1e-6))));
        assert_eq!(m.params()[2].as_str(), Some("it's"));
        assert_eq!(m.params()[3], Param::Unset);
        assert_eq!(ex.get(5).expect("face").params()[3].as_bool(), Some(true));
        assert_eq!(ex.get(6).expect("product").params()[0].as_str(), Some("\u{e9}t\u{e9}"));
        assert_eq!(ex.of_type("SI_UNIT").collect::<Vec<_>>(), vec![3]);
    }

    #[test]
    fn reports_errors_with_lines() {
        assert!(matches!(parse("hello"), Err(ParseError::NotStep)));
        let bad = "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n#1=POINT((1.,2.);\nENDSEC;\nEND-ISO-10303-21;";
        match parse(bad) {
            Err(ParseError::Syntax { line, .. }) => assert_eq!(line, 5),
            other => panic!("{other:?}"),
        }
        let dup = "ISO-10303-21;\nDATA;\n#1=A();\n#1=B();\nENDSEC;\nEND-ISO-10303-21;";
        assert!(parse(dup).is_err());
    }

    #[test]
    fn deep_nesting_is_refused_not_overflowed() {
        let deep = format!(
            "ISO-10303-21;\nDATA;\n#1=A({}1{});\nENDSEC;\nEND-ISO-10303-21;",
            "(".repeat(100_000),
            ")".repeat(100_000)
        );
        assert!(parse(&deep).is_err());
    }

    proptest! {
        /// Arbitrary text never panics the reader.
        #[test]
        fn never_panics(s in ".{0,400}") {
            let _ = parse(&format!("ISO-10303-21;\nDATA;\n{s}"));
            let _ = parse(&s);
        }

        /// Mutating a valid file never panics either.
        #[test]
        fn mutations_never_panic(pos in 0usize..600, byte in any::<u8>()) {
            let mut b = SAMPLE.as_bytes().to_vec();
            let i = pos % b.len();
            b[i] = byte;
            let _ = parse(&String::from_utf8_lossy(&b));
        }
    }
}
