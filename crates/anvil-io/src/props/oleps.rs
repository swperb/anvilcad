//! Standard OLE property sets: summary information and user-defined custom properties.
//!
//! Written from Microsoft's published specification, [MS-OLEPS]: Object Linking and
//! Embedding (OLE) Property Set Data Structures. These are the streams Windows Explorer
//! reads for a file's Details and Custom tabs. Nothing here is specific to any CAD program.

use super::cfb::Cfb;
use super::{FileProperties, FileProperty, Source};
use anvil_model::document::PropertyKind;

pub const SUMMARY_STREAM: &str = "\u{5}SummaryInformation";
pub const DOC_SUMMARY_STREAM: &str = "\u{5}DocumentSummaryInformation";

/// FMTID_SummaryInformation, F29F85E0-4FF9-1068-AB91-08002B27B3D9, as stored.
pub const FMTID_SUMMARY: [u8; 16] =
    [0xE0, 0x85, 0x9F, 0xF2, 0xF9, 0x4F, 0x68, 0x10, 0xAB, 0x91, 0x08, 0x00, 0x2B, 0x27, 0xB3, 0xD9];
/// FMTID_DocSummaryInformation, D5CDD502-2E9C-101B-9397-08002B2CF9AE.
pub const FMTID_DOC_SUMMARY: [u8; 16] =
    [0x02, 0xD5, 0xCD, 0xD5, 0x9C, 0x2E, 0x1B, 0x10, 0x93, 0x97, 0x08, 0x00, 0x2B, 0x2C, 0xF9, 0xAE];
/// FMTID_UserDefinedProperties, D5CDD505-2E9C-101B-9397-08002B2CF9AE.
pub const FMTID_USER_DEFINED: [u8; 16] =
    [0x05, 0xD5, 0xCD, 0xD5, 0x9C, 0x2E, 0x1B, 0x10, 0x93, 0x97, 0x08, 0x00, 0x2B, 0x2C, 0xF9, 0xAE];

const PID_DICTIONARY: u32 = 0;
const PID_CODEPAGE: u32 = 1;
const CP_UTF16: u16 = 1200;
const CP_UTF8: u16 = 65001;

const VT_I2: u16 = 0x02;
const VT_I4: u16 = 0x03;
const VT_R4: u16 = 0x04;
const VT_R8: u16 = 0x05;
const VT_CY: u16 = 0x06;
const VT_DATE: u16 = 0x07;
const VT_BOOL: u16 = 0x0B;
const VT_I1: u16 = 0x10;
const VT_UI1: u16 = 0x11;
const VT_UI2: u16 = 0x12;
const VT_UI4: u16 = 0x13;
const VT_I8: u16 = 0x14;
const VT_UI8: u16 = 0x15;
const VT_INT: u16 = 0x16;
const VT_UINT: u16 = 0x17;
const VT_LPSTR: u16 = 0x1E;
const VT_LPWSTR: u16 = 0x1F;
const VT_FILETIME: u16 = 0x40;

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}
fn u64_at(b: &[u8], o: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(o..o + 8)?.try_into().ok()?))
}

/// One section of a property set stream.
struct Section<'a> {
    bytes: &'a [u8],
    /// (property identifier, offset of its typed value within `bytes`)
    entries: Vec<(u32, usize)>,
}

impl<'a> Section<'a> {
    fn value_offset(&self, pid: u32) -> Option<usize> {
        self.entries.iter().find(|(p, _)| *p == pid).map(|(_, o)| *o)
    }
    fn codepage(&self) -> u16 {
        self.value_offset(PID_CODEPAGE)
            .and_then(|o| (u16_at(self.bytes, o)? == VT_I2).then(|| u16_at(self.bytes, o + 4))?)
            .unwrap_or(1252)
    }
}

/// Splits a property set stream into its sections, keyed by FMTID.
fn sections(stream: &[u8]) -> Option<Vec<([u8; 16], Section<'_>)>> {
    if u16_at(stream, 0)? != 0xFFFE {
        return None;
    }
    let n = u32_at(stream, 24)? as usize;
    if n > 2 {
        return None;
    }
    let mut out = Vec::new();
    for i in 0..n {
        let h = 28 + 20 * i;
        let fmtid: [u8; 16] = stream.get(h..h + 16)?.try_into().ok()?;
        let off = u32_at(stream, h + 16)? as usize;
        let size = u32_at(stream, off)? as usize;
        let bytes = stream.get(off..off.checked_add(size)?)?;
        let count = u32_at(bytes, 4)? as usize;
        if count > bytes.len() / 8 {
            return None;
        }
        let entries = (0..count)
            .map(|k| Some((u32_at(bytes, 8 + 8 * k)?, u32_at(bytes, 12 + 8 * k)? as usize)))
            .collect::<Option<Vec<_>>>()?;
        out.push((fmtid, Section { bytes, entries }));
    }
    Some(out)
}

/// Decodes text stored in a Windows code page.
fn decode(bytes: &[u8], codepage: u16) -> String {
    let s = match codepage {
        CP_UTF16 => {
            let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
            String::from_utf16_lossy(&units)
        }
        CP_UTF8 => String::from_utf8_lossy(bytes).into_owned(),
        // Windows-1252 and its subsets; other single-byte code pages read as 1252, which is
        // right for ASCII and the closest available otherwise.
        _ => bytes.iter().map(|&b| cp1252(b)).collect(),
    };
    s.trim_end_matches('\0').to_string()
}

fn cp1252(b: u8) -> char {
    const HIGH: [u16; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152,
        0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122,
        0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
    ];
    match b {
        0x80..=0x9F => char::from_u32(HIGH[(b - 0x80) as usize] as u32).unwrap_or('?'),
        _ => b as char,
    }
}

/// Reads the dictionary (property identifier to name) of a user-defined section.
fn dictionary(sec: &Section, codepage: u16) -> Vec<(u32, String)> {
    let Some(mut o) = sec.value_offset(PID_DICTIONARY) else { return Vec::new() };
    let b = sec.bytes;
    let Some(n) = u32_at(b, o) else { return Vec::new() };
    o += 4;
    let mut out = Vec::new();
    for _ in 0..n.min(10_000) {
        let (Some(pid), Some(len)) = (u32_at(b, o), u32_at(b, o + 4)) else { break };
        o += 8;
        let bytes_len = if codepage == CP_UTF16 { len as usize * 2 } else { len as usize };
        let Some(name) = b.get(o..o + bytes_len) else { break };
        out.push((pid, decode(name, codepage)));
        o += bytes_len;
        if codepage == CP_UTF16 {
            o = o.div_ceil(4) * 4;
        }
    }
    out
}

/// A property value as (kind, display text). `None` for types AnvilCAD does not show.
fn value(sec: &Section, o: usize, codepage: u16) -> Option<Result<(PropertyKind, String), u16>> {
    let b = sec.bytes;
    let vt = u16_at(b, o)?;
    let v = o + 4;
    let num = |x: f64| Some(Ok((PropertyKind::Number, fmt_number(x))));
    match vt {
        VT_LPSTR => {
            let size = u32_at(b, v)? as usize;
            Some(Ok((PropertyKind::Text, decode(b.get(v + 4..v + 4 + size)?, codepage))))
        }
        VT_LPWSTR => {
            let chars = u32_at(b, v)? as usize;
            Some(Ok((PropertyKind::Text, decode(b.get(v + 4..v + 4 + 2 * chars)?, CP_UTF16))))
        }
        VT_I1 => num(*b.get(v)? as i8 as f64),
        VT_UI1 => num(*b.get(v)? as f64),
        VT_I2 => num(u16_at(b, v)? as i16 as f64),
        VT_UI2 => num(u16_at(b, v)? as f64),
        VT_I4 | VT_INT => num(u32_at(b, v)? as i32 as f64),
        VT_UI4 | VT_UINT => num(u32_at(b, v)? as f64),
        VT_I8 => num(u64_at(b, v)? as i64 as f64),
        VT_UI8 => num(u64_at(b, v)? as f64),
        VT_R4 => num(f32::from_bits(u32_at(b, v)?) as f64),
        VT_R8 => num(f64::from_bits(u64_at(b, v)?)),
        VT_CY => num(u64_at(b, v)? as i64 as f64 / 10_000.0),
        VT_BOOL => {
            let yes = u16_at(b, v)? != 0;
            Some(Ok((PropertyKind::YesNo, if yes { "Yes" } else { "No" }.into())))
        }
        VT_FILETIME => {
            // 100 ns intervals since 1601-01-01 UTC.
            let t = u64_at(b, v)?;
            let days = (t / 10_000_000 / 86_400) as i64 - 134_774;
            Some(Ok((PropertyKind::Date, civil_date(days))))
        }
        VT_DATE => {
            // Days since 1899-12-30.
            let d = f64::from_bits(u64_at(b, v)?);
            if !d.is_finite() || d.abs() > 3_000_000.0 {
                return Some(Err(vt));
            }
            Some(Ok((PropertyKind::Date, civil_date(d.floor() as i64 - 25_569))))
        }
        other => Some(Err(other)),
    }
}

fn fmt_number(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

/// YYYY-MM-DD for a count of days since 1970-01-01 (proleptic Gregorian).
pub(crate) fn civil_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

const SUMMARY_NAMES: &[(u32, &str)] = &[
    (2, "Title"),
    (3, "Subject"),
    (4, "Author"),
    (5, "Keywords"),
    (6, "Comments"),
    (8, "Last saved by"),
    (9, "Revision number"),
    (12, "Created"),
    (13, "Last saved"),
];
const DOC_SUMMARY_NAMES: &[(u32, &str)] = &[(2, "Category"), (14, "Manager"), (15, "Company")];

fn named(sec: &Section, names: &[(u32, &str)], out: &mut Vec<FileProperty>) {
    let cp = sec.codepage();
    for &(pid, name) in names {
        if let Some(Some(Ok((kind, value)))) = sec.value_offset(pid).map(|o| value(sec, o, cp)) {
            if !value.is_empty() {
                out.push(FileProperty { name: name.into(), kind, value, expression: None });
            }
        }
    }
}

/// Reads the standard property sets of a compound file. `None` if it has none.
pub fn read(cfb: &Cfb) -> Option<FileProperties> {
    let summary_stream = cfb.stream(&[SUMMARY_STREAM]).ok().flatten();
    let doc_stream = cfb.stream(&[DOC_SUMMARY_STREAM]).ok().flatten();
    if summary_stream.is_none() && doc_stream.is_none() {
        return None;
    }
    let mut fp = FileProperties::new(Source::OlePropertySet);

    if let Some(s) = &summary_stream {
        match sections(s) {
            Some(secs) => {
                for (id, sec) in &secs {
                    if *id == FMTID_SUMMARY {
                        named(sec, SUMMARY_NAMES, &mut fp.summary);
                    }
                }
            }
            None => fp.notes.push("The summary information stream is damaged and was skipped.".into()),
        }
    }
    if let Some(s) = &doc_stream {
        match sections(s) {
            Some(secs) => {
                for (id, sec) in &secs {
                    if *id == FMTID_DOC_SUMMARY {
                        named(sec, DOC_SUMMARY_NAMES, &mut fp.summary);
                    } else if *id == FMTID_USER_DEFINED {
                        let cp = sec.codepage();
                        for (pid, name) in dictionary(sec, cp) {
                            let Some(o) = sec.value_offset(pid) else { continue };
                            match value(sec, o, cp) {
                                Some(Ok((kind, value))) => {
                                    fp.properties.push(FileProperty { name, kind, value, expression: None })
                                }
                                Some(Err(vt)) => fp.notes.push(format!(
                                    "Property '{name}' has a value type (0x{vt:04X}) AnvilCAD does not read yet."
                                )),
                                None => fp.notes.push(format!("Property '{name}' is damaged and was skipped.")),
                            }
                        }
                    }
                }
            }
            None => fp.notes.push("The document summary stream is damaged and was skipped.".into()),
        }
    }
    Some(fp)
}

/// Builds property set streams for tests.
#[cfg(any(test, feature = "fixtures"))]
pub mod writer {
    pub enum V<'a> {
        Str(&'a str),
        WStr(&'a str),
        I4(i32),
        R8(f64),
        Bool(bool),
        FileTime(u64),
        Raw(u16, Vec<u8>),
    }

    fn pad4(v: &mut Vec<u8>) {
        while !v.len().is_multiple_of(4) {
            v.push(0);
        }
    }

    fn typed(v: &V, codepage: u16) -> Vec<u8> {
        let mut out = Vec::new();
        let mut head = |vt: u16| out.extend_from_slice(&(vt as u32).to_le_bytes());
        match v {
            V::Str(s) => {
                head(super::VT_LPSTR);
                let bytes: Vec<u8> = if codepage == super::CP_UTF16 {
                    s.encode_utf16().chain([0]).flat_map(|u| u.to_le_bytes()).collect()
                } else if codepage == super::CP_UTF8 {
                    s.bytes().chain([0]).collect()
                } else {
                    s.chars().map(|c| c as u32 as u8).chain([0]).collect()
                };
                out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                out.extend(bytes);
            }
            V::WStr(s) => {
                head(super::VT_LPWSTR);
                let units: Vec<u16> = s.encode_utf16().chain([0]).collect();
                out.extend_from_slice(&(units.len() as u32).to_le_bytes());
                out.extend(units.iter().flat_map(|u| u.to_le_bytes()));
            }
            V::I4(i) => {
                head(super::VT_I4);
                out.extend_from_slice(&i.to_le_bytes());
            }
            V::R8(x) => {
                head(super::VT_R8);
                out.extend_from_slice(&x.to_le_bytes());
            }
            V::Bool(b) => {
                head(super::VT_BOOL);
                out.extend_from_slice(&(if *b { 0xFFFFu32 } else { 0 }).to_le_bytes());
            }
            V::FileTime(t) => {
                head(super::VT_FILETIME);
                out.extend_from_slice(&t.to_le_bytes());
            }
            V::Raw(vt, bytes) => {
                head(*vt);
                out.extend_from_slice(bytes);
            }
        }
        pad4(&mut out);
        out
    }

    /// A section from (pid, value) pairs, with an optional dictionary for user-defined sets.
    pub fn section(codepage: u16, props: &[(u32, V)], dict: &[(u32, &str)]) -> Vec<u8> {
        let mut values: Vec<(u32, Vec<u8>)> = Vec::new();
        let mut cp = (super::VT_I2 as u32).to_le_bytes().to_vec();
        cp.extend_from_slice(&(codepage as u32).to_le_bytes());
        values.push((super::PID_CODEPAGE, cp));
        if !dict.is_empty() {
            let mut d = (dict.len() as u32).to_le_bytes().to_vec();
            for (pid, name) in dict {
                d.extend_from_slice(&pid.to_le_bytes());
                if codepage == super::CP_UTF16 {
                    let units: Vec<u16> = name.encode_utf16().chain([0]).collect();
                    d.extend_from_slice(&(units.len() as u32).to_le_bytes());
                    d.extend(units.iter().flat_map(|u| u.to_le_bytes()));
                    pad4(&mut d);
                } else {
                    let bytes: Vec<u8> = name.bytes().chain([0]).collect();
                    d.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
                    d.extend(bytes);
                }
            }
            pad4(&mut d);
            values.push((super::PID_DICTIONARY, d));
        }
        for (pid, v) in props {
            values.push((*pid, typed(v, codepage)));
        }
        let header = 8 + 8 * values.len();
        let mut table = Vec::new();
        let mut body = Vec::new();
        for (pid, v) in &values {
            table.extend_from_slice(&pid.to_le_bytes());
            table.extend_from_slice(&((header + body.len()) as u32).to_le_bytes());
            body.extend_from_slice(v);
        }
        let mut out = ((header + body.len()) as u32).to_le_bytes().to_vec();
        out.extend_from_slice(&(values.len() as u32).to_le_bytes());
        out.extend(table);
        out.extend(body);
        out
    }

    pub fn stream(sections: &[([u8; 16], Vec<u8>)]) -> Vec<u8> {
        let mut out = vec![0xFE, 0xFF, 0, 0];
        out.extend_from_slice(&0x0002_0006u32.to_le_bytes()); // system identifier
        out.extend_from_slice(&[0; 16]);
        out.extend_from_slice(&(sections.len() as u32).to_le_bytes());
        let mut off = 28 + 20 * sections.len();
        for (id, s) in sections {
            out.extend_from_slice(id);
            out.extend_from_slice(&(off as u32).to_le_bytes());
            off += s.len();
        }
        for (_, s) in sections {
            out.extend_from_slice(s);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::writer::{section, stream, V};
    use super::*;
    use crate::props::cfb;

    fn sample(codepage: u16) -> Vec<u8> {
        let summary = stream(&[(
            FMTID_SUMMARY,
            section(codepage, &[(2, V::Str("Wheel")), (4, V::Str("J. Doe")), (5, V::Str(""))], &[]),
        )]);
        let user = section(
            codepage,
            &[
                (2, V::Str("large wheel")),
                (3, V::WStr("Lathe")),
                (4, V::Str("galvanized")),
                (5, V::I4(42)),
                (6, V::R8(2.5)),
                (7, V::Bool(true)),
                // 2024-03-15 00:00 UTC
                (8, V::FileTime((1_710_460_800 + 11_644_473_600) * 10_000_000)),
                (9, V::Raw(0x48, vec![0; 16])), // a CLSID: not shown
                (10, V::Str("Größe ÷ 2")),
            ],
            &[
                (2, "Description"),
                (3, "Process"),
                (4, "Finish"),
                (5, "Quantity"),
                (6, "Weight"),
                (7, "Purchased"),
                (8, "Approved"),
                (9, "Odd"),
                (10, "Note"),
            ],
        );
        let doc = stream(&[
            (FMTID_DOC_SUMMARY, section(codepage, &[(15, V::Str("Acme"))], &[])),
            (FMTID_USER_DEFINED, user),
        ]);
        cfb::writer::build(&[(SUMMARY_STREAM, &summary), (DOC_SUMMARY_STREAM, &doc), ("Other", b"x")])
    }

    fn check(codepage: u16) {
        let bytes = sample(codepage);
        let c = Cfb::parse(&bytes).unwrap();
        let fp = read(&c).unwrap();
        let get = |n: &str| fp.properties.iter().find(|p| p.name == n).map(|p| (p.kind, p.value.as_str()));
        assert_eq!(get("Description"), Some((PropertyKind::Text, "large wheel")));
        assert_eq!(get("Process"), Some((PropertyKind::Text, "Lathe")));
        assert_eq!(get("Finish"), Some((PropertyKind::Text, "galvanized")));
        assert_eq!(get("Quantity"), Some((PropertyKind::Number, "42")));
        assert_eq!(get("Weight"), Some((PropertyKind::Number, "2.5")));
        assert_eq!(get("Purchased"), Some((PropertyKind::YesNo, "Yes")));
        assert_eq!(get("Approved"), Some((PropertyKind::Date, "2024-03-15")));
        assert_eq!(get("Note"), Some((PropertyKind::Text, "Größe ÷ 2")));
        assert_eq!(get("Odd"), None);
        assert!(fp.notes.iter().any(|n| n.contains("'Odd'")));
        // Dictionary order is kept.
        assert_eq!(fp.properties[0].name, "Description");
        let s: Vec<_> = fp.summary.iter().map(|p| (p.name.as_str(), p.value.as_str())).collect();
        assert_eq!(s, [("Title", "Wheel"), ("Author", "J. Doe"), ("Company", "Acme")]);
    }

    #[test]
    fn reads_custom_properties_in_each_code_page() {
        check(1252);
        check(CP_UTF8);
        check(CP_UTF16);
    }

    #[test]
    fn files_without_property_sets() {
        let bytes = cfb::writer::build(&[("Contents", b"abc")]);
        assert!(read(&Cfb::parse(&bytes).unwrap()).is_none());
    }

    #[test]
    fn damaged_property_sets_do_not_panic() {
        let good = sample(1252);
        let c = Cfb::parse(&good).unwrap();
        let doc = c.stream(&[DOC_SUMMARY_STREAM]).unwrap().unwrap();
        for i in 0..doc.len() {
            for flip in [0xFFu8, 0x80, 0x01] {
                let mut bad = doc.clone();
                bad[i] ^= flip;
                if let Some(secs) = sections(&bad) {
                    for (_, sec) in &secs {
                        let cp = sec.codepage();
                        for (pid, _) in dictionary(sec, cp) {
                            if let Some(o) = sec.value_offset(pid) {
                                let _ = value(sec, o, cp);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn dates() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(19_797), "2024-03-15");
        assert_eq!(civil_date(-25_569), "1899-12-30");
        assert_eq!(civil_date(11_016), "2000-02-29");
    }
}
