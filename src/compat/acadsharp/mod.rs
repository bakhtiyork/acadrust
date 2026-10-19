//! Canonical ACadSharp differential dump, with unavailable values marked `!unmapped`.
//!
//! Regenerate `schema.tsv` with CadKit's `Tools/AcadDump/run.sh gen-rust-checklist`.
//! Dumping never changes the document. [`new_document`] explicitly creates
//! ACadSharp's default graph without changing the native constructor's policy.

use crate::types::{Color, DxfVersion, Handle, Transparency, Vector2, Vector3};
use crate::CadDocument;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

mod entities;
mod header;
mod new_document;
mod objects;
mod palette;
mod tables;
mod type_names;

pub use new_document::new_document;

pub const UNMAPPED: &str = "!unmapped";
pub const SCHEMA: &str = include_str!("schema.tsv");

/// Ordered member names, parsed once from the reference dumper's checklist.
pub fn schema() -> &'static BTreeMap<&'static str, Vec<&'static str>> {
    static SCHEMA_MAP: OnceLock<BTreeMap<&'static str, Vec<&'static str>>> = OnceLock::new();
    SCHEMA_MAP.get_or_init(|| {
        let mut result = BTreeMap::<_, Vec<_>>::new();
        for line in SCHEMA.lines().skip(1) {
            let mut cells = line.split('\t');
            let ty = cells.next().expect("schema type");
            let member = cells.next().expect("schema member");
            result.entry(ty).or_default().push(member);
        }
        result
    })
}

pub(super) fn props(ty: &str, mapped: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    let mut mapped: BTreeMap<_, _> = mapped.into_iter().collect();
    let members = schema()
        .get(ty)
        .unwrap_or_else(|| panic!("missing ACadSharp schema: {ty}"));
    Value::Object(
        members
            .iter()
            .map(|&key| {
                (
                    key.to_owned(),
                    mapped.remove(key).unwrap_or_else(|| json!(UNMAPPED)),
                )
            })
            .collect(),
    )
}

pub(super) fn record(h: Handle, ty: &str, owner: Handle, props: Value) -> Value {
    json!({"handle": handle(h), "type": ty, "owner": if owner.is_null() {Value::Null} else {handle(owner)}, "props": props})
}

pub fn handle(h: Handle) -> Value {
    json!(format!("#{:X}", h.value()))
}
pub(super) fn optional_handle(h: Option<Handle>) -> Value {
    h.filter(|h| !h.is_null())
        .map(handle)
        .unwrap_or(Value::Null)
}
pub fn number(n: f64) -> Value {
    if n.is_nan() {
        json!("NaN")
    } else if n == f64::INFINITY {
        json!("Infinity")
    } else if n == f64::NEG_INFINITY {
        json!("-Infinity")
    } else {
        json!(n)
    }
}
pub(super) fn vec2(v: Vector2) -> Value {
    json!([number(v.x), number(v.y)])
}
pub(super) fn vec3(v: Vector3) -> Value {
    json!([number(v.x), number(v.y), number(v.z)])
}
pub fn character(c: char) -> Value {
    // System.Char is a single UTF-16 code unit.
    json!(c.encode_utf16(&mut [0; 2])[0])
}
pub fn bytes(data: &[u8]) -> Value {
    use std::fmt::Write;
    let mut s = String::with_capacity(data.len() * 2);
    for byte in data {
        write!(&mut s, "{byte:02x}").expect("string write");
    }
    json!(s)
}
pub fn color(c: Color) -> Value {
    let (r, g, b) = c.true_color_rgb().unwrap_or_else(|| {
        // C# casts the index to a byte, so None (257) uses palette entry 1.
        let [r, g, b] = palette::INDEX_RGB[c.index().unwrap_or(0) as u8 as usize];
        (r, g, b)
    });
    props(
        "Color",
        [
            ("R", json!(r)),
            ("G", json!(g)),
            ("B", json!(b)),
            ("Index", json!(c.index().map(i32::from).unwrap_or(-1))),
            ("IsByBlock", json!(matches!(c, Color::ByBlock))),
            ("IsByLayer", json!(matches!(c, Color::ByLayer))),
            ("IsTrueColor", json!(c.is_true_color())),
            ("TrueColor", json!(c.to_true_color_value().unwrap_or(-1))),
        ],
    )
}
pub fn transparency(t: Transparency) -> Value {
    let value = match t {
        Transparency::ByLayer => -1,
        Transparency::ByBlock => 100,
        Transparency::Explicit(a) => {
            let p = (100.0 - (255 - a) as f64 / 255.0 * 100.0) as i16;
            if p == 100 {
                100
            } else {
                p.min(90)
            }
        }
    };
    props(
        "Transparency",
        [
            ("Value", json!(value)),
            ("IsByBlock", json!(value == 100)),
            ("IsByLayer", json!(value == -1)),
        ],
    )
}
pub fn version(v: DxfVersion) -> i16 {
    match v {
        DxfVersion::Unknown => -1,
        DxfVersion::AC1009 => 9,
        DxfVersion::AC1012 => 19,
        DxfVersion::AC1014 => 21,
        DxfVersion::AC1015 => 23,
        DxfVersion::AC1018 => 25,
        DxfVersion::AC1021 => 27,
        DxfVersion::AC1024 => 29,
        DxfVersion::AC1027 => 31,
        DxfVersion::AC1032 => 33,
    }
}
fn notifications(doc: Option<&CadDocument>) -> Value {
    let mut counts = Map::new();
    for name in ["NotImplemented", "None", "NotSupported", "Warning", "Error"] {
        counts.insert(name.into(), json!(0));
    }
    if let Some(doc) = doc {
        for n in doc.notifications.iter() {
            let count = counts
                .get_mut(&n.notification_type.to_string())
                .expect("notification kind");
            *count = json!(count.as_u64().unwrap() + 1);
        }
    }
    Value::Object(counts)
}
/// Dump an existing document. `file` is the basename, or empty for a new document.
pub fn dump_document(doc: &CadDocument, file: &str, reader: &str) -> Value {
    let mut objects = Vec::new();
    tables::collect(doc, &mut objects);
    entities::collect(doc, &mut objects);
    objects::collect(doc, &mut objects);
    // A registered handle may be exposed through multiple model collections.
    // Prefer the first (concrete entity/table) representation over sidecar records.
    let mut sorted = BTreeMap::new();
    for object in objects {
        let h = u64::from_str_radix(
            object["handle"].as_str().unwrap().trim_start_matches('#'),
            16,
        )
        .unwrap();
        if h != 0 {
            sorted.entry(h).or_insert(object);
        }
    }
    json!({"format":1,"file":file,"reader":reader,"status":"ok","error":null,
        "notifications":notifications(Some(doc)),"version":version(doc.version),
        "header":header::header(doc),"classes":header::classes(doc),"summaryInfo":header::summary(doc),
        "objects":sorted.into_values().collect::<Vec<_>>()})
}
/// Error dumps intentionally omit document sections.
pub fn error_dump(file: &str, reader: &str, error: &str) -> Value {
    json!({"format":1,"file":file,"reader":reader,"status":"error","error":error,"notifications":notifications(None)})
}
/// Read once in failsafe mode. Reader panics become canonical error dumps.
pub fn dump_file(path: &Path) -> Value {
    use crate::io::dwg::{DwgReadOptions, DwgReader};
    use crate::io::dxf::{DxfReader, DxfReaderConfiguration};
    let file = path.file_name().unwrap_or_default().to_string_lossy();
    let reader = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    let result = std::panic::catch_unwind(|| -> crate::Result<CadDocument> {
        match reader.as_str() {
            "dxf" => DxfReader::from_file(path)?
                .with_configuration(DxfReaderConfiguration {
                    failsafe: true,
                    ..Default::default()
                })
                .read(),
            "dwg" => DwgReader::from_file_with_options(path, DwgReadOptions::failsafe())?.read(),
            _ => Err(crate::DxfError::InvalidFormat(
                "expected .dwg or .dxf".into(),
            )),
        }
    });
    match result {
        Ok(Ok(doc)) => dump_document(&doc, &file, &reader),
        Ok(Err(e)) => {
            let debug = format!("{e:?}");
            let kind = debug.split(['(', ' ', '{']).next().unwrap_or("DxfError");
            error_dump(&file, &reader, &format!("{kind}: {e}"))
        }
        Err(payload) => {
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("reader panicked");
            error_dump(&file, &reader, &format!("Panic: {message}"))
        }
    }
}
