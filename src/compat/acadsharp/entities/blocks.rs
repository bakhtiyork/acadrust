// SPDX-License-Identifier: MPL-2.0
//! Block definitions share specific state with their record, while structural
//! entities keep their own appearance and object metadata.
use super::*;
use crate::tables::BlockRecord;

fn specific(doc: &CadDocument, b: &BlockRecord) -> Vec<(&'static str, Value)> {
    let dwg = doc.dwg_source_version.is_some();
    // A drawing before R2000 keeps the description in the CONTENTBLOCKDESCRIPTION record (`autocad_conversions`), which ACadSharp
    // reads as extended data.
    let comments = if super::super::tables::block_value_from_record(doc, b, "CONTENTBLOCKDESCRIPTION")
        || (b.description.is_empty()
            && (b.dxf_present.is_some_and(|p| p & 8 == 0)
                || (b.dxf_present.is_none() && (!dwg || doc.version < DxfVersion::AC1015))))
    {
        Value::Null
    } else {
        json!(b.description)
    };
    let path = if b.xref_path.is_empty()
        && (b.dxf_present.is_some_and(|p| p & 16 == 0) || (b.dxf_present.is_none() && !dwg))
    {
        Value::Null
    } else {
        json!(b.xref_path)
    };
    vec![
        ("BasePoint", vec3(b.base_point)),
        ("BlockOwner", handle(b.handle)),
        ("Comments", comments),
        ("Flags", json!(super::super::tables::block_flags(doc, b))),
        ("IsUnloaded", json!(b.flags.is_xref_unloaded)),
        ("Name", json!(super::super::tables::block_name(doc, b))),
        ("XRefPath", path),
    ]
}

pub(super) fn block(doc: &CadDocument, b: &crate::entities::Block, out: &mut Vec<Value>) {
    let fields = if let Some(record) = doc
        .block_records
        .get(&b.name)
        .filter(|r| r.handle == b.common.owner_handle)
    {
        specific(doc, record)
    } else {
        vec![
            ("BasePoint", vec3(b.base_point)),
            ("BlockOwner", Value::Null),
            ("Comments", json!(b.description)),
            ("Flags", unmapped()),
            ("IsUnloaded", unmapped()),
            ("Name", json!(b.name)),
            ("XRefPath", json!(b.xref_path)),
        ]
    };
    field_common(
        doc,
        &b.common,
        "Block",
        "BLOCK",
        4,
        "AcDbBlockBegin",
        false,
        fields,
        out,
    );
}

pub(in crate::compat::acadsharp) fn block_record_marker(
    doc: &CadDocument,
    b: &BlockRecord,
    end: bool,
    out: &mut Vec<Value>,
) {
    let default = EntityCommon::default();
    let retained = if end {
        &b.block_end_common
    } else {
        &b.block_entity_common
    }
    .as_deref();
    let common = retained.unwrap_or(&default);
    let (ty, name, code, subclass) = if end {
        ("BlockEnd", "ENDBLK", 5, "AcDbBlockEnd")
    } else {
        ("Block", "BLOCK", 4, "AcDbBlockBegin")
    };
    let mut fields = if end { Vec::new() } else { specific(doc, b) };
    let mut value = if retained.is_none() && !super::super::registration::source_document(doc) {
        // A native record with only marker handles supplies no common state.
        // Preserve unknown fields in explicit inventories. Reader-created
        // missing markers instead have the reference constructor defaults.
        fields.extend([
            ("ObjectName", json!(name)),
            ("ObjectType", json!(code)),
            ("SubclassMarker", json!(subclass)),
            ("ProxyGeometries", json!([])),
        ]);
        record(Handle::NULL, ty, b.handle, props(ty, fields))
    } else {
        entity_record(doc, common, ty, name, code, subclass, false, fields)
    };
    // Source handles can be null or collide in legacy DXF. The document's
    // assigned marker identities take precedence without cloning common data.
    value["handle"] = handle(if end {
        b.block_end_handle
    } else {
        b.block_entity_handle
    });
    value["owner"] = handle(b.handle);
    out.push(value);
}
