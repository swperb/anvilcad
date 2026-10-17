//! Custom properties through the SOLIDWORKS Document Manager API.
//!
//! The Document Manager is a library Dassault Systèmes publishes so that other programs can
//! read and write SOLIDWORKS file data without SOLIDWORKS. It needs a licence key, which
//! Dassault issues to SOLIDWORKS subscription customers. AnvilCAD binds to the user's
//! installed copy at run time through its documented automation interface (see
//! [`crate::com`]) and only ever opens documents read-only. The key is passed in by the
//! caller and never stored.

use super::{Configuration, FileProperties, FileProperty, ReadError, Source};
use crate::com::{self, Arg, Disp, Value};
use crate::solidworks::{doc_type_from_extension, SwDocType};
use anvil_model::document::PropertyKind;
use std::path::Path;

const PROGID: &str = "SwDocumentMgr.SwDMClassFactory";

// SwDmDocumentType
const DM_PART: i32 = 1;
const DM_ASSEMBLY: i32 = 2;
const DM_DRAWING: i32 = 3;

// SwDmCustomInfoType
fn kind(t: i64) -> PropertyKind {
    match t {
        3 | 5 => PropertyKind::Number,
        11 => PropertyKind::YesNo,
        64 => PropertyKind::Date,
        _ => PropertyKind::Text,
    }
}

fn dm_err(e: com::ComError) -> ReadError {
    ReadError::DocumentManager(e.0)
}

/// Reads one container's custom properties: the document itself or a configuration. Both
/// expose `GetCustomPropertyNames`; newer releases add `GetCustomPropertyValues`, which
/// returns the evaluated value and the link expression, and older ones `GetCustomProperty`.
fn read_container(obj: &Disp, notes: &mut Vec<String>, what: &str) -> Vec<FileProperty> {
    let names = match obj.call("GetCustomPropertyNames", &[]) {
        Ok(v) => v.list(),
        Err(e) => {
            notes.push(format!("{what}: could not list custom properties ({e})"));
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for n in names {
        let Some(name) = n.str().map(str::to_string) else { continue };
        let read = obj
            .call_out("GetCustomPropertyValues", &[name.as_str().into(), Arg::OutInt, Arg::OutStr])
            .map(|(v, outs)| {
                let t = outs.first().and_then(Value::i64).unwrap_or(30);
                let linked = outs.get(1).and_then(|v| v.str()).unwrap_or("").to_string();
                (v, t, linked)
            })
            .or_else(|_| {
                obj.call_out("GetCustomProperty", &[name.as_str().into(), Arg::OutInt])
                    .map(|(v, outs)| (v, outs.first().and_then(Value::i64).unwrap_or(30), String::new()))
            });
        match read {
            Ok((v, t, linked)) => {
                let value = match v {
                    Value::Str(s) => s,
                    Value::Empty => String::new(),
                    other => format!("{other:?}"),
                };
                let expression = (!linked.is_empty() && linked != value).then_some(linked);
                out.push(FileProperty { name, kind: kind(t), value, expression });
            }
            Err(e) => notes.push(format!("{what}: could not read '{name}' ({e})")),
        }
    }
    out
}

/// Reads file-level and configuration custom properties of a SOLIDWORKS file.
pub fn read(path: &Path, key: &str) -> Result<FileProperties, ReadError> {
    let doc_type = match doc_type_from_extension(path) {
        Some(SwDocType::Part | SwDocType::PartTemplate) => DM_PART,
        Some(SwDocType::Assembly | SwDocType::AssemblyTemplate) => DM_ASSEMBLY,
        Some(SwDocType::Drawing | SwDocType::DrawingTemplate) => DM_DRAWING,
        None => return Err(ReadError::DocumentManager("not a SOLIDWORKS file".into())),
    };
    let full = std::path::absolute(path)?;
    // A GUI thread may already be in a single-threaded apartment; COM is usable either way,
    // and if it is not initialized at all, creating the factory reports that.
    let _ = com::init();
    let factory = com::create_inproc(PROGID).map_err(|e| {
        ReadError::DocumentManager(format!("the Document Manager is not installed on this computer ({e})"))
    })?;
    let app = factory
        .opt_obj("GetApplication", &[key.into()])
        .map_err(dm_err)?
        .ok_or_else(|| ReadError::DocumentManager("the licence key was not accepted".into()))?;
    let (doc, outs) = app
        .call_out(
            "GetDocument",
            &[full.to_string_lossy().as_ref().into(), Arg::Int(doc_type), Arg::Bool(true), Arg::OutInt],
        )
        .map_err(dm_err)?;
    let Some(doc) = doc.disp().cloned() else {
        let code = outs.first().and_then(Value::i64).unwrap_or(-1);
        return Err(ReadError::DocumentManager(format!(
            "could not open the file (SwDmDocumentOpenError {code})"
        )));
    };

    let mut fp = FileProperties::new(Source::DocumentManager);
    fp.properties = read_container(&doc, &mut fp.notes, "file");
    if doc_type != DM_DRAWING {
        match doc.opt_obj("ConfigurationManager", &[]) {
            Ok(Some(mgr)) => {
                let names = mgr.call("GetConfigurationNames", &[]).map(|v| v.list()).unwrap_or_default();
                for n in names {
                    let Some(name) = n.str() else { continue };
                    match mgr.opt_obj("GetConfigurationByName", &[name.into()]) {
                        Ok(Some(cfg)) => {
                            let props =
                                read_container(&cfg, &mut fp.notes, &format!("configuration '{name}'"));
                            if !props.is_empty() {
                                fp.configurations
                                    .push(Configuration { name: name.to_string(), properties: props });
                            }
                        }
                        Ok(None) => {}
                        Err(e) => fp.notes.push(format!("configuration '{name}': {e}")),
                    }
                }
            }
            Ok(None) => {}
            Err(e) => fp.notes.push(format!("configurations: {e}")),
        }
    }
    let _ = doc.call("CloseDoc", &[]);
    Ok(fp)
}
