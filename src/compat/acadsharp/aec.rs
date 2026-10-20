// SPDX-License-Identifier: MPL-2.0
//! ACadSharp's deliberately partial AEC view. Geometry remains opaque natively.

use super::{bytes, handle, props, record};
use crate::entities::UnknownEntity;
use crate::objects::{AecData, ObjectType};
use crate::types::Handle;
use crate::CadDocument;
use serde_json::{json, Value};

fn encoding(doc: &CadDocument) -> crate::io::dxf::code_page::LegacyEncoding {
    crate::io::dxf::code_page::encoding_from_code_page(&doc.header.code_page)
        .unwrap_or_else(|| encoding_rs::WINDOWS_1252.into())
}

pub(super) fn object(doc: &CadDocument, map_handle: Handle, object: &ObjectType) -> Option<Value> {
    if !matches!(object, ObjectType::Unknown { type_name, .. }
        if matches!(super::type_names::canonical(type_name),
            Some("AecCleanupGroup" | "AecWallStyle" | "AecBinRecord")))
    {
        return None;
    }
    let data = AecData::from_object_with_encoding(object, encoding(doc))
        .ok()
        .flatten()?;
    let ObjectType::Unknown { owner, .. } = object else {
        return None;
    };
    let (class, object_name, marker, version, mut specific) = match data {
        AecData::CleanupGroup(value) => (
            "AecCleanupGroup",
            "AEC_CLEANUP_GROUP_DEF",
            "AecDbCleanupGroupDef",
            value.version,
            vec![
                ("Description", json!(value.description)),
                (
                    "RawData",
                    complete_bytes(&value.raw_data, value.raw_data_bits, true),
                ),
            ],
        ),
        AecData::WallStyle(value) => (
            "AecWallStyle",
            "AEC_WALL_STYLE",
            "AecDbWallStyle",
            value.version,
            vec![("Description", json!(value.description))],
        ),
        AecData::BinRecord(value) => (
            "AecBinRecord",
            "BINRECORD",
            "BinRecord",
            value.version,
            vec![
                // AecBinRecord hides NonGraphicalObject.Name; dictionary
                // assignment changes the base property, not this one.
                ("Name", Value::Null),
                (
                    "BinaryData",
                    complete_bytes(&value.binary_data, value.binary_data_bits, false),
                ),
            ],
        ),
        AecData::Wall(_) => return None,
    };
    specific.extend([
        ("HasDynamicSubclass", json!(false)),
        ("ObjectName", json!(object_name)),
        ("ObjectType", json!(-999)),
        ("SubclassMarker", json!(marker)),
        ("Version", json!(version)),
    ]);
    // Other common data and dictionary-assigned names use the existing
    // non-graphical record enrichment after this projection.
    Some(record(map_handle, class, *owner, props(class, specific)))
}

fn complete_bytes(data: &[u8], bit_count: i64, null_if_empty: bool) -> Value {
    if bit_count < 0 {
        return json!(super::UNMAPPED);
    }
    let count = usize::try_from(bit_count / 8).ok();
    match count.and_then(|count| data.get(..count)) {
        Some([]) if null_if_empty => Value::Null,
        Some(data) => bytes(data),
        None => json!(super::UNMAPPED),
    }
}

pub(super) fn wall(doc: &CadDocument, entity: &UnknownEntity) -> Option<Value> {
    if entity.dxf_name != "AEC_WALL" {
        return None;
    }
    let AecData::Wall(value) = AecData::from_entity_with_encoding(entity, encoding(doc))
        .ok()
        .flatten()?
    else {
        return None;
    };
    let raw = entity.raw_dwg_data.as_deref()?;
    let (style, cleanup_group) = wall_references(
        raw,
        entity.dwg_source_version?,
        value.references_start_bit,
        value.references_end_bit,
    )?;
    let raw_data = wall_raw_data(raw, value.raw_data_start_bit, value.references_end_bit);
    let mut mapped =
        super::entities::common(doc, &entity.common, "AEC_WALL", -999, "AecDbWall", false);
    mapped.extend(wall_properties(
        doc,
        value.version,
        style,
        cleanup_group,
        raw_data,
    ));
    Some(record(
        entity.common.handle,
        "Wall",
        entity.common.owner_handle,
        props("Wall", mapped),
    ))
}

fn wall_references(
    raw: &[u8],
    version: crate::types::DxfVersion,
    start: i64,
    end: i64,
) -> Option<(Handle, Handle)> {
    let total = i64::try_from(raw.len()).ok()?.checked_mul(8)?;
    if start < 0 || end < start || end > total {
        return None;
    }
    let first = usize::try_from(start / 8).ok()?;
    let last = usize::try_from(end.checked_add(7)? / 8).ok()?;
    // Only the few reference bytes need their own reader, not the full record.
    let mut handles = crate::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader::new(
        raw.get(first..last)?.to_vec(),
        crate::io::dwg::dwg_version::DwgVersion::from_dxf_version(version).ok()?,
        version,
    );
    handles.set_position_in_bits(start % 8);
    handles.set_end_bit(end - (start / 8) * 8);
    // The reference uses base zero here, even for relative DWG handles.
    // Preserve the correctly resolved native references in AecWall.
    handles.read_handle_relative(0);
    let style = Handle::new(handles.read_handle_relative(0));
    let cleanup_group = Handle::new(handles.read_handle_relative(0));
    handles.check_error().ok()?;
    Some((style, cleanup_group))
}

fn wall_raw_data(raw: &[u8], start: i64, end: i64) -> Value {
    // C# includes all handles consumed so far and rounds the bit span UP.
    // Copy those bytes only when every requested bit is inside this record.
    let Some(total) = i64::try_from(raw.len()).ok().and_then(|n| n.checked_mul(8)) else {
        return json!(super::UNMAPPED);
    };
    if start < 0 || end < start || end > total {
        return json!(super::UNMAPPED);
    }
    if end == start {
        return Value::Null;
    }
    let Some(count) = (end - start).checked_add(7).map(|n| n / 8) else {
        return json!(super::UNMAPPED);
    };
    if count > (total - start) / 8 {
        return json!(super::UNMAPPED);
    }
    let mut result = Vec::with_capacity(count as usize);
    for offset in 0..count {
        let bit = start + offset * 8;
        let index = (bit / 8) as usize;
        let shift = bit % 8;
        let mut byte = raw[index] << shift;
        if shift != 0 {
            byte |= raw[index + 1] >> (8 - shift);
        }
        result.push(byte);
    }
    bytes(&result)
}

fn reference(doc: &CadDocument, target: Handle, expected: &str) -> Value {
    match doc.objects.get(&target) {
        Some(ObjectType::Unknown { type_name, .. })
            if !target.is_null() && super::type_names::canonical(type_name) == Some(expected) =>
        {
            handle(target)
        }
        _ => Value::Null,
    }
}

fn wall_properties(
    doc: &CadDocument,
    version: i32,
    style: Handle,
    cleanup_group: Handle,
    raw: Value,
) -> Vec<(&'static str, Value)> {
    // DwgObjectReader.readAecWall only reads the version and three handles.
    // CadWallTemplate assigns the two object references, but not the public
    // numeric handle properties. No geometry or BinRecord reference is decoded.
    vec![
        ("BaseHeight", json!(0)),
        ("BinRecord", Value::Null),
        ("BinRecordHandle", json!(0)),
        (
            "CleanupGroup",
            reference(doc, cleanup_group, "AecCleanupGroup"),
        ),
        ("CleanupGroupHandle", json!(0)),
        ("EndPoint", json!([0, 0, 0])),
        ("Height", json!(0)),
        ("Justification", json!(0)),
        ("Length", json!(0)),
        ("Normal", json!([0, 0, 1])),
        ("RawData", raw),
        ("StartPoint", json!([0, 0, 0])),
        ("Style", reference(doc, style, "AecWallStyle")),
        ("Version", json!(version)),
        ("Width", json!(0)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::dwg::dwg_stream_writers::bit_writer::DwgBitWriter;
    use crate::io::dwg::dwg_stream_writers::merged_writer::DwgMergedWriter;
    use crate::io::dwg::dwg_version::DwgVersion;
    use crate::types::DxfVersion;

    fn opaque(type_name: &str, id: u64) -> ObjectType {
        ObjectType::Unknown {
            type_name: type_name.into(),
            handle: Handle::new(id),
            owner: Handle::NULL,
            raw_dxf_codes: None,
            raw_dwg_data: None,
            raw_dwg_handle_bits: 0,
            raw_dwg_version: None,
        }
    }

    #[test]
    fn aec_raw_projection_rounds_each_oracle_range_without_overreading() {
        assert_eq!(complete_bytes(&[0xb2, 0x80], 9, true), "b2");
        assert_eq!(complete_bytes(&[0x80], 1, true), Value::Null);
        assert_eq!(complete_bytes(&[], 0, false), "");
        assert_eq!(complete_bytes(&[], -1, false), super::super::UNMAPPED);
        assert_eq!(complete_bytes(&[1], 16, false), super::super::UNMAPPED);
        let raw = [0xd6, 0x5a, 0xe1];
        assert_eq!(wall_raw_data(&raw, 3, 12), "b2d7");
        assert_eq!(wall_raw_data(&raw, 4, 4), Value::Null);
        for (start, end) in [(-1, 8), (8, 7), (0, 25), (3, 24), (0, i64::MAX)] {
            assert_eq!(wall_raw_data(&raw, start, end), super::super::UNMAPPED);
        }
    }

    #[test]
    fn wall_reference_projection_uses_oracle_zero_base_and_bounded_bits() {
        let mut writer = DwgBitWriter::new(DwgVersion::AC24, DxfVersion::AC1032);
        writer.write_2bits(3);
        writer.write_bit(true);
        let start = writer.position_in_bits();
        writer.write_handle_relative(0x100, 0x101);
        writer.write_handle_relative(0x100, 0x102);
        writer.write_handle_undefined(0x42);
        let end = writer.position_in_bits();
        let raw = writer.into_bytes();
        assert_eq!(
            wall_references(&raw, DxfVersion::AC1032, start, end),
            Some((Handle::new(2), Handle::new(0x42)))
        );
        for (start, end) in [(-1, end), (start, end - 1), (start, i64::MAX)] {
            assert_eq!(wall_references(&raw, DxfVersion::AC1032, start, end), None);
        }
    }

    fn encoded_object(type_name: &str, id: u64, version: i32) -> ObjectType {
        let mut writer = DwgMergedWriter::new(DwgVersion::AC24, DxfVersion::AC1032);
        writer.write_object_type(500);
        writer.main_mut().write_handle_undefined(id);
        writer.write_bit_short(0); // EED terminator
        writer.write_handle_undefined(0); // owner
        writer.write_bit_long(0); // reactors
        writer.write_bit(true); // no extension dictionary
        writer.write_bit(false); // no DS data
        writer.write_bit_long(version);
        if type_name != "BINRECORD" {
            writer.write_variable_text("Synthetic AEC description");
        }
        writer.write_bytes(&[0xab, 0xcd, 0xef]);
        let raw = writer.merge();
        let handle_bits = raw.len() as i64 * 8 - writer.handle_start_bits();
        let mut result = opaque(type_name, id);
        if let ObjectType::Unknown {
            raw_dwg_data,
            raw_dwg_handle_bits,
            raw_dwg_version,
            ..
        } = &mut result
        {
            *raw_dwg_data = Some(raw);
            *raw_dwg_handle_bits = handle_bits;
            *raw_dwg_version = Some(DxfVersion::AC1032);
        }
        result
    }

    #[test]
    fn aec_object_dispatch_decodes_headers_and_preserves_opaque_records() {
        let mut doc = CadDocument::new();
        for (id, name) in [
            (0x900, "AEC_CLEANUP_GROUP_DEF"),
            (0x901, "AEC_WALL_STYLE"),
            (0x902, "BINRECORD"),
        ] {
            doc.objects
                .insert(Handle::new(id), encoded_object(name, id, 81));
        }
        let before = doc.objects[&Handle::new(0x900)].clone();
        let dump = super::super::dump_document(&doc, "", "dwg");
        for (id, class, marker) in [
            ("#900", "AecCleanupGroup", "AecDbCleanupGroupDef"),
            ("#901", "AecWallStyle", "AecDbWallStyle"),
            ("#902", "AecBinRecord", "BinRecord"),
        ] {
            let object = dump["objects"]
                .as_array()
                .unwrap()
                .iter()
                .find(|object| object["handle"] == id)
                .unwrap();
            assert_eq!(object["type"], class);
            assert_eq!(object["props"]["SubclassMarker"], marker);
            assert_eq!(object["props"]["ObjectType"], -999);
            assert_eq!(object["props"]["Version"], 81);
            if class == "AecBinRecord" {
                assert!(object["props"]["Name"].is_null());
                assert!(object["props"]["BinaryData"]
                    .as_str()
                    .unwrap()
                    .starts_with("abcdef"));
            } else {
                assert_eq!(object["props"]["Description"], "Synthetic AEC description");
            }
        }
        assert_eq!(doc.objects[&Handle::new(0x900)], before);
    }

    #[test]
    fn bin_record_hidden_name_survives_reader_dictionary_registration() {
        let mut doc = CadDocument::new();
        doc.header.source_handle_seed = Some(0xa00);
        doc.objects
            .insert(Handle::new(0x902), encoded_object("BINRECORD", 0x902, 80));
        let root = doc.header.named_objects_dict_handle;
        let ObjectType::Dictionary(dictionary) = doc.objects.get_mut(&root).unwrap() else {
            panic!("expected root dictionary");
        };
        dictionary.add_entry("Standards", Handle::new(0x902));
        let dump = super::super::dump_reader_document(&doc, "", "dwg");
        let object = dump["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|object| object["type"] == "AecBinRecord")
            .unwrap();
        assert_eq!(object["owner"], handle(root));
        assert!(object["props"]["Name"].is_null());
    }

    #[test]
    fn wall_view_resolves_typed_references_but_keeps_oracle_geometry_defaults() {
        let mut doc = CadDocument::new();
        doc.objects
            .insert(Handle::new(0x901), opaque("AEC_WALL_STYLE", 0x901));
        doc.objects
            .insert(Handle::new(0x902), opaque("AEC_CLEANUP_GROUP_DEF", 0x902));
        let values = props(
            "Wall",
            wall_properties(
                &doc,
                83,
                Handle::new(0x901),
                Handle::new(0x902),
                bytes(&[0x12, 0x34]),
            ),
        );
        assert_eq!(values["Version"], 83);
        assert_eq!(values["Style"], "#901");
        assert_eq!(values["CleanupGroup"], "#902");
        assert_eq!(values["CleanupGroupHandle"], 0);
        assert_eq!(values["BinRecordHandle"], 0);
        assert!(values["BinRecord"].is_null());
        assert_eq!(values["StartPoint"], json!([0, 0, 0]));
        assert_eq!(values["EndPoint"], json!([0, 0, 0]));
        assert_eq!(values["Normal"], json!([0, 0, 1]));
        for field in ["BaseHeight", "Height", "Length", "Width", "Justification"] {
            assert_eq!(values[field], 0, "{field}");
        }
        assert_eq!(values["RawData"], "1234");
        assert!(matches!(
            &doc.objects[&Handle::new(0x901)],
            ObjectType::Unknown { type_name, .. } if type_name == "AEC_WALL_STYLE"
        ));
    }

    #[test]
    fn wall_view_rejects_missing_and_wrong_type_references() {
        let mut doc = CadDocument::new();
        doc.objects
            .insert(Handle::new(0x901), opaque("BINRECORD", 0x901));
        let values = props(
            "Wall",
            wall_properties(
                &doc,
                80,
                Handle::new(0x901),
                Handle::new(0x902),
                Value::Null,
            ),
        );
        assert!(values["Style"].is_null());
        assert!(values["CleanupGroup"].is_null());
        assert!(reference(&doc, Handle::NULL, "AecWallStyle").is_null());
    }
}
