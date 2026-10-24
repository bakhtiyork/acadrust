// SPDX-License-Identifier: MPL-2.0
//! ACadSharp's view of XDATA and DWG extended entity data.
//!
//! The native model intentionally keeps DXF-friendly layer names and opaque
//! DWG EED bytes for round-trip writing.  ACadSharp's dump exposes layer
//! references as numeric handles and reports an EED block that could not be
//! decoded as unavailable.  This adapter performs those conversions only at
//! the compatibility boundary.

use crate::io::dwg::eed_codec::DecodedXDataValue;
use crate::tables::TextStyle;
use crate::types::{DxfVersion, Handle};
use crate::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};
use crate::CadDocument;
use serde_json::{json, Value};

use super::{handle, number, vec3, UNMAPPED};

fn unmapped() -> Value {
    Value::String(UNMAPPED.to_owned())
}

fn bytes(value: &[u8]) -> Value {
    Value::String(value.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn app_handle(doc: &CadDocument, name: &str) -> Handle {
    doc.app_ids
        .get(name)
        .map(|app| app.handle)
        .unwrap_or(Handle::NULL)
}

fn app_name(doc: &CadDocument, value: u64) -> Option<&str> {
    doc.app_ids
        .iter()
        .find(|app| app.handle.value() == value)
        .map(|app| app.name.as_str())
}

fn layer_name(doc: &CadDocument, value: u64) -> Option<String> {
    doc.layers
        .iter()
        .find(|layer| layer.handle.value() == value)
        .map(|layer| layer.name.clone())
}

fn layer_handle(doc: &CadDocument, name: &str) -> u64 {
    if let Some(source_tables) = doc.legacy_dxf_tables.as_deref() {
        let Some(layer_table) = source_tables
            .iter()
            .find(|table| table.name.eq_ignore_ascii_case("LAYER"))
        else {
            return 0;
        };
        return layer_table
            .entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
            .map(|entry| entry.handle.value())
            .unwrap_or(0);
    }
    doc.layers
        .get(name)
        .map(|layer| layer.handle.value())
        .unwrap_or(0)
}

fn control_string(value: &str) -> (Value, bool) {
    let is_closing = value == "}";
    (
        json!(if is_closing { '}' as i64 } else { '{' as i64 }),
        is_closing,
    )
}

fn xdata_value(doc: &CadDocument, value: &XDataValue) -> (i32, Value, Option<bool>) {
    match value {
        XDataValue::String(value) => (1000, Value::String(value.clone()), None),
        XDataValue::ControlString(value) => {
            let (raw, closing) = control_string(value);
            (1002, raw, Some(closing))
        }
        // ACadSharp's ExtendedDataLayer stores the layer object handle.  The
        // native XData value is a name, so resolve it only for this view.
        XDataValue::LayerName(value) => (1003, json!(layer_handle(doc, value)), None),
        XDataValue::BinaryData(value) => (1004, bytes(value), None),
        XDataValue::Handle(value) => (1005, json!(value.value()), None),
        XDataValue::Point3D(value) => (1010, vec3(*value), None),
        XDataValue::Position3D(value) => (1011, vec3(*value), None),
        XDataValue::Displacement3D(value) => (1012, vec3(*value), None),
        XDataValue::Direction3D(value) => (1013, vec3(*value), None),
        XDataValue::Real(value) => (1040, number(*value), None),
        XDataValue::Distance(value) => (1041, number(*value), None),
        XDataValue::ScaleFactor(value) => (1042, number(*value), None),
        XDataValue::Integer16(value) => (1070, json!(*value), None),
        XDataValue::Integer32(value) => (1071, json!(*value), None),
    }
}

fn record_value(doc: &CadDocument, value: &XDataValue) -> Value {
    let (code, raw, closing) = xdata_value(doc, value);
    let mut object = serde_json::Map::new();
    object.insert("Code".to_owned(), json!(code));
    if let Some(closing) = closing {
        object.insert("IsClosing".to_owned(), json!(closing));
    }
    object.insert("RawValue".to_owned(), raw);
    Value::Object(object)
}

fn decoded_record_value(doc: &CadDocument, value: &XDataValue) -> Value {
    // The native DWG decoder removes one UTF-16 terminator; ACadSharp exposes the logical string, which also
    // ends before any further trailing NULs.
    if let XDataValue::String(value) = value {
        let trimmed = value.trim_end_matches('\0');
        if trimmed.len() != value.len() {
            return record_value(doc, &XDataValue::String(trimmed.to_owned()));
        }
    }
    record_value(doc, value)
}

fn decoded_record(doc: &CadDocument, values: &[DecodedXDataValue]) -> Value {
    let records = values
        .iter()
        .map(|value| match value {
            DecodedXDataValue::Value(value) => decoded_record_value(doc, value),
            DecodedXDataValue::Layer { handle, .. } => json!({
                "Code": 1003,
                "RawValue": handle,
            }),
        })
        .collect::<Vec<_>>();
    json!({"Records": records})
}

fn append_record(
    doc: &CadDocument,
    record: &ExtendedDataRecord,
    trim_dwg_strings: bool,
    pairs: &mut Vec<Value>,
    emitted: &mut Vec<String>,
) {
    emitted.push(record.application_name.clone());
    pairs.push(Value::Array(vec![
        handle(app_handle(doc, &record.application_name)),
        json!({
            "Records": record
                .values
                .iter()
                .map(|value| {
                    if trim_dwg_strings {
                        decoded_record_value(doc, value)
                    } else {
                        record_value(doc, value)
                    }
                })
                .collect::<Vec<_>>()
        }),
    ]));
}

fn patch_unresolved_layer_handles(pair: &mut Value, values: &[DecodedXDataValue]) {
    let Some(records) = pair
        .get_mut(1)
        .and_then(|value| value.get_mut("Records"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };

    for (record, value) in records.iter_mut().zip(values) {
        let DecodedXDataValue::Layer { handle, name } = value else {
            continue;
        };
        if name.is_empty()
            && record.get("Code").and_then(Value::as_i64) == Some(1003)
            && record.get("RawValue").and_then(Value::as_i64) == Some(0)
        {
            record["RawValue"] = json!(handle);
        }
    }
}

fn already_emitted(emitted: &[String], name: &str) -> bool {
    emitted
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(name))
}

fn append_raw(
    doc: &CadDocument,
    raw: &[(u64, Vec<u8>)],
    wide: bool,
    pairs: &mut Vec<Value>,
    emitted: &mut Vec<String>,
) -> bool {
    let mut undecoded = false;
    for (app, bytes) in raw {
        let Some(name) = app_name(doc, *app) else {
            // The app handle cannot be named in ACadSharp's ExtendedData
            // dictionary, so keep the property visibly unavailable.
            undecoded = true;
            continue;
        };
        let values =
            crate::io::dwg::eed_codec::decode_values_with_layer_handles(bytes, wide, |layer| {
                layer_name(doc, layer)
            });
        let Some(values) = values else {
            // Preserve the native raw block; the compatibility value must not
            // turn present undecoded data into the same [] as absent data.
            undecoded = true;
            continue;
        };
        // Decode every block before suppressing duplicate app pairs. A second
        // block for an already-emitted application may still be malformed and
        // must make the compatibility property unavailable.
        if already_emitted(emitted, name) {
            let app_pair = handle(Handle::new(*app));
            if let Some(pair) = pairs.iter_mut().find(|pair| pair.get(0) == Some(&app_pair)) {
                patch_unresolved_layer_handles(pair, &values);
            }
            continue;
        }
        emitted.push(name.to_owned());
        pairs.push(Value::Array(vec![
            handle(Handle::new(*app)),
            decoded_record(doc, &values),
        ]));
    }
    undecoded
}

fn append_text_style_font(
    doc: &CadDocument,
    style: Option<&TextStyle>,
    pairs: &mut Vec<Value>,
    emitted: &mut Vec<String>,
) {
    let Some(style) = style else {
        return;
    };
    if style.true_type_font.is_empty() || already_emitted(emitted, "ACAD") {
        return;
    }
    emitted.push("ACAD".to_owned());
    let position = style
        .true_type_font_eed_position
        .map_or(pairs.len(), |position| position.min(pairs.len()));
    pairs.insert(
        position,
        Value::Array(vec![
            handle(app_handle(doc, "ACAD")),
            json!({
                "Records": [
                    record_value(doc, &XDataValue::String(style.true_type_font.clone())),
                    record_value(doc, &XDataValue::Integer32(style.true_type_font_flags)),
                ]
            }),
        ]),
    );
}

/// The records `autocad_conversions` took out of the owner `handle`, for a document read from a file (the model holds the
/// values they state; ACadSharp reads the records).
fn consumed<'a>(doc: &'a CadDocument, handle: Handle) -> &'a [(usize, ExtendedDataRecord)] {
    if super::registration::source_document(doc) {
        doc.consumed_xdata.get(&handle).map_or(&[], Vec::as_slice)
    } else {
        &[]
    }
}

fn project(
    doc: &CadDocument,
    data: Option<&ExtendedData>,
    raw: &[(u64, Vec<u8>)],
    style: Option<&TextStyle>,
    consumed: &[(usize, ExtendedDataRecord)],
) -> Value {
    let data_raw = data.map(|data| data.raw_dwg_eed.as_slice()).unwrap_or(&[]);
    let has_style_font = style.is_some_and(|style| !style.true_type_font.is_empty());
    if data.is_none() && data_raw.is_empty() && raw.is_empty() && !has_style_font && consumed.is_empty() {
        return json!([]);
    }
    if data.is_some_and(|data| data.records().is_empty())
        && data_raw.is_empty()
        && raw.is_empty()
        && !has_style_font
        && consumed.is_empty()
    {
        return json!([]);
    }

    let mut pairs = Vec::new();
    let mut emitted = Vec::new();
    let trim_dwg_strings = doc.dwg_source_version.is_some();
    if let Some(data) = data {
        for record in data.records() {
            append_record(doc, record, trim_dwg_strings, &mut pairs, &mut emitted);
        }
    }

    let wide = doc.dwg_source_version.unwrap_or(doc.version) >= DxfVersion::AC1021;
    let undecoded = append_raw(doc, data_raw, wide, &mut pairs, &mut emitted)
        || append_raw(doc, raw, wide, &mut pairs, &mut emitted);
    append_text_style_font(doc, style, &mut pairs, &mut emitted);
    // The records the native model took out, back at the position the file had them (they came out in ascending order).
    for (index, record) in consumed {
        let mut put_back = Vec::new();
        append_record(doc, record, trim_dwg_strings, &mut put_back, &mut emitted);
        pairs.insert((*index).min(pairs.len()), put_back.remove(0));
    }
    if undecoded {
        unmapped()
    } else {
        Value::Array(pairs)
    }
}

/// Project entity XDATA and any raw EED retained on the entity.
///
/// The handle remains in this boundary API for callers that pass the common
/// entity identity; raw entity EED is carried by `data` itself, so the value
/// is intentionally not used for a document-wide lookup.
pub(super) fn extended_data(
    doc: &CadDocument,
    _object_handle: Handle,
    data: &ExtendedData,
) -> Value {
    project(doc, Some(data), &[], None, consumed(doc, _object_handle))
}

/// Project non-entity XDATA and raw EED stored in the document side maps.
pub(super) fn object_extended_data(doc: &CadDocument, object_handle: Handle) -> Value {
    project(
        doc,
        doc.object_xdata.get(&object_handle),
        doc.eed_by_handle
            .get(&object_handle)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
        None,
        consumed(doc, object_handle),
    )
}

/// Project a STYLE table entry, including its decoded ACAD TrueType metadata.
///
/// STYLE EED is normalized into `TextStyle.true_type_font` during DWG read, so
/// the compatibility view must synthesize the ACAD application record here.
/// Keeping this type-specific entry point avoids scanning the whole STYLE
/// table for every entity and non-STYLE object.
pub(super) fn text_style_extended_data(doc: &CadDocument, style: &TextStyle) -> Value {
    project(
        doc,
        doc.object_xdata.get(&style.handle),
        doc.eed_by_handle
            .get(&style.handle)
            .map(Vec::as_slice)
            .unwrap_or(&[]),
        Some(style),
        &[],
    )
}
