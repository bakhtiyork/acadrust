//! ACadSharp view of the proxy records (`ProxyEntity`, `ProxyObject`): the
//! `IProxy` members shared by both.
//!
//! The native records keep what the file stores (`version` is the combined
//! drawing format, `dwg_version` its low word, `class_id` the class number and
//! the payloads are bit-exact). The reference reader instead resolves the class
//! by number, reads the version fields only from R2000 and keeps the retained
//! data as a stream only from R2010 (`DwgObjectReader.readCommonProxyData`).
use serde_json::{json, Value};

use crate::types::DxfVersion;
use crate::CadDocument;

/// `IProxy` members for one native proxy record; the caller adds the
/// subtype-specific ones.
pub(super) fn members(
    doc: &CadDocument,
    class_id: i32,
    dwg_version: i32,
    maintenance_version: i32,
    from_dxf: bool,
    data_retained: bool,
) -> Vec<(&'static str, Value)> {
    let dwg = doc.classes.source_classes().is_none();
    // The version fields are only read from R2000; earlier records keep zeros.
    let (version, maintenance, from_dxf) = if dwg && doc.version < DxfVersion::AC1015 {
        (0, 0, false)
    } else {
        (dwg_version, maintenance_version, from_dxf)
    };
    // `_handleSectionOffset` is only known for R2010+ files, and then the data
    // always spans at least the string-stream flag bit.
    let data = if dwg {
        doc.version >= DxfVersion::AC1024
    } else {
        data_retained
    };
    let class = super::header::class_by_number(doc, i64::from(class_id));
    vec![
        // `ClassId` reads `DxfClass.ClassNumber` and throws without a class.
        (
            "ClassId",
            match &class {
                Some(class) => class["ClassNumber"].clone(),
                None => json!("!throws:NullReferenceException"),
            },
        ),
        ("DxfClass", class.unwrap_or(Value::Null)),
        (
            "DrawingFormat",
            json!(version.wrapping_add(maintenance.wrapping_shl(16))),
        ),
        ("MaintenanceVersion", json!(maintenance)),
        ("OriginalDataFormatDxf", json!(from_dxf)),
        ("Version", json!(version)),
        // A `Stream` dumps as an object without members.
        ("Data", if data { json!({}) } else { Value::Null }),
    ]
}
