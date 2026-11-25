//! The title block: labelled fields whose text can hold `{Name}` placeholders, filled from
//! the part's custom properties and configuration and from the drawing itself (see
//! `docs/re/drawings.md`, "Title blocks").

use serde::{Deserialize, Serialize};

/// One cell of the title block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TitleField {
    /// The small caption at the top of the cell.
    pub label: String,
    /// The cell's text, with `{Name}` placeholders.
    pub text: String,
}

impl TitleField {
    pub fn new(label: &str, text: &str) -> TitleField {
        TitleField { label: label.into(), text: text.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TitleBlock {
    pub fields: Vec<TitleField>,
}

impl Default for TitleBlock {
    fn default() -> Self {
        TitleBlock::standard()
    }
}

/// Names filled from the drawing and the part rather than from custom properties.
pub const BUILT_IN_NAMES: [&str; 7] =
    ["Configuration", "FileName", "Material", "DrawingName", "Scale", "Size", "Projection"];

impl TitleBlock {
    /// The block new drawings get: the custom properties every new AnvilCAD part has, with
    /// the configuration, material, scale, projection and sheet size.
    pub fn standard() -> TitleBlock {
        TitleBlock {
            fields: vec![
                TitleField::new("Title", "{Description}"),
                TitleField::new("Part number", "{PartNo}"),
                TitleField::new("Revision", "{Revision}"),
                TitleField::new("Material", "{Material}"),
                TitleField::new("Configuration", "{Configuration}"),
                TitleField::new("Drawn by", "{DrawnBy}"),
                TitleField::new("Scale", "{Scale}"),
                TitleField::new("Projection", "{Projection}"),
                TitleField::new("Sheet", "{Size}"),
            ],
        }
    }
}

/// A piece of a field's text: plain words or a placeholder's name.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Piece<'a> {
    Text(String),
    Name(&'a str),
}

/// Splits `text` into plain words and `{Name}` placeholders. `{{` and `}}` are braces; a
/// `{` with no closing `}` is kept as written.
fn pieces(text: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    while let Some(ch) = rest.chars().next() {
        if rest.starts_with("{{") || rest.starts_with("}}") {
            plain.push(ch);
            rest = &rest[2..];
        } else if ch == '{' {
            match rest[1..].find('}') {
                Some(end) => {
                    if !plain.is_empty() {
                        out.push(Piece::Text(std::mem::take(&mut plain)));
                    }
                    out.push(Piece::Name(rest[1..1 + end].trim()));
                    rest = &rest[end + 2..];
                }
                None => {
                    plain.push_str(rest);
                    rest = "";
                }
            }
        } else {
            plain.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    if !plain.is_empty() {
        out.push(Piece::Text(plain));
    }
    out
}

/// Fills a field's placeholders with `lookup`, which returns `None` for a name it does not
/// know. Returns the text and the names that were not found, which print as nothing.
pub fn fill(text: &str, lookup: &dyn Fn(&str) -> Option<String>) -> (String, Vec<String>) {
    let mut out = String::new();
    let mut missing = Vec::new();
    for p in pieces(text) {
        match p {
            Piece::Text(t) => out.push_str(&t),
            Piece::Name(n) => match lookup(n) {
                Some(v) => out.push_str(&v),
                None => {
                    if !missing.iter().any(|m: &String| m.eq_ignore_ascii_case(n)) {
                        missing.push(n.to_string());
                    }
                }
            },
        }
    }
    (out.trim().to_string(), missing)
}

/// The values a title block is filled from.
#[derive(Debug, Clone, Default)]
pub struct Values {
    /// Custom properties of the part, in the configuration shown.
    pub properties: Vec<(String, String)>,
    /// The built-in names, from the drawing and the part.
    pub built_in: Vec<(&'static str, String)>,
}

impl Values {
    /// A custom property of that name wins over a built-in one unless it is empty; names
    /// match without regard to case.
    pub fn get(&self, name: &str) -> Option<String> {
        let custom =
            self.properties.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone());
        let built_in =
            self.built_in.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone());
        match (custom, built_in) {
            (Some(c), Some(b)) if c.trim().is_empty() => Some(b),
            (Some(c), _) => Some(c),
            (None, b) => b,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(name: &str) -> Option<String> {
        match name.to_ascii_lowercase().as_str() {
            "partno" => Some("AB-12".into()),
            "description" => Some("Bracket".into()),
            _ => None,
        }
    }

    #[test]
    fn placeholders_are_filled_and_missing_ones_reported() {
        assert_eq!(fill("{PartNo}", &look), ("AB-12".into(), vec![]));
        assert_eq!(
            fill("{Description}, rev {Revision}", &look),
            ("Bracket, rev".into(), vec!["Revision".into()])
        );
        assert_eq!(fill("{{literal}} { partno }", &look), ("{literal} AB-12".into(), vec![]));
        assert_eq!(fill("open { brace", &look), ("open { brace".into(), vec![]));
        assert_eq!(fill("{X}{x}", &look), (String::new(), vec!["X".into()]));
    }

    #[test]
    fn custom_properties_win_over_built_in_names() {
        let v = Values {
            properties: vec![("material".into(), "As drawn".into())],
            built_in: vec![("Material", "Steel".into()), ("Scale", "1:2".into())],
        };
        assert_eq!(v.get("Material").as_deref(), Some("As drawn"));
        assert_eq!(v.get("scale").as_deref(), Some("1:2"));
        assert_eq!(v.get("Nothing"), None);
    }
}
