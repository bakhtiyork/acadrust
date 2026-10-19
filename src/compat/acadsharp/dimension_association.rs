//! ACadSharp's typed view of a DIMASSOC associative object.
//!
//! acadrust keeps each on-disk osnap continuation in a native vector, while
//! ACadSharp exposes one `OsnapPointRef` property for each of the four slots.
//! The compatibility view reproduces each reader's consumption and resolution
//! quirks, leaving native continuation records untouched.

use crate::entities::EntityType;
use crate::objects::{AssocDimensionAssociation, AssocDimensionReference, AssociativeObject};
use crate::types::Handle;
use serde_json::Value;

/// Project a native DIMASSOC payload to ACadSharp's `DimensionAssociation`.
///
/// The caller handles the `ObjectType::Associative` dispatch.  This adapter
/// deliberately takes the owning associative object separately so its common
/// object metadata remains available without changing the native payload.
pub(super) fn dump_object(
    doc: &crate::CadDocument,
    handle_value: Handle,
    object: &AssociativeObject,
    value: &AssocDimensionAssociation,
    reader: &str,
) -> Value {
    let mut mapped = super::common(
        "DIMASSOC",
        super::UNLISTED,
        "AcDbDimAssoc",
        None,
        Some(&object.reactors),
        object.xdictionary_handle,
    );
    let refs = projected_references(doc, object, value, reader);
    mapped.extend([
        ("AssociativityFlags", super::int(value.associativity as i64)),
        ("Dimension", dimension_handle(doc, value.dimension)),
        ("FirstPointRef", refs[0].clone()),
        ("FourthPointRef", refs[3].clone()),
        ("IsTransSpace", Value::Bool(value.trans_space)),
        (
            "RotatedDimensionType",
            super::int(value.rotated_type as i64),
        ),
        ("SecondPointRef", refs[1].clone()),
        ("ThirdPointRef", refs[2].clone()),
    ]);
    super::record(
        handle_value,
        "DimensionAssociation",
        object.owner,
        super::props("DimensionAssociation", mapped),
    )
}

/// Reproduce the reference reader's consumption without changing native chains.
fn projected_references(
    doc: &crate::CadDocument,
    object: &AssociativeObject,
    value: &AssocDimensionAssociation,
    reader: &str,
) -> [Value; 4] {
    use crate::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader;
    use crate::io::dwg::dwg_version::DwgVersion;
    use crate::types::DxfVersion;

    let version = object.source_version.unwrap_or(doc.version);
    let mut stream = value
        .dwg_reference_stream
        .as_ref()
        .map(|(data, start, end)| {
            let mut stream = DwgBitReader::from_shared(
                data.clone(),
                DwgVersion::from_dxf_version(version).unwrap_or(DwgVersion::AC15),
                version,
            );
            stream.set_end_bit(*end);
            stream.set_position_in_bits(*start);
            stream
        });
    let mut dxf_refs = value.references.iter().flatten();
    let mut dwg_handles = value.references.iter().flatten().flat_map(|reference| {
        reference
            .xrefs
            .iter()
            .chain(reference.intersection_objects.iter())
    });
    std::array::from_fn(|slot| {
        if value.associativity & (1 << slot) == 0 {
            return Value::Null;
        }
        if reader == "dwg" {
            // DwgObjectReader.readOsnapPointRef reads only class name, byte
            // osnap type, and one handle. It does not advance over the rest of
            // the native record, so subsequent types must be read from source
            // bits, rather than guessed from the next native slot.
            let osnap_type = if let Some(stream) = stream.as_mut() {
                if version < DxfVersion::AC1021 {
                    let _ = stream.read_variable_text();
                }
                stream.read_byte()
            } else {
                value.references[slot]
                    .first()
                    .map_or(0, |reference| reference.osnap_type)
            };
            let geometry = dwg_handles.next().copied();
            let defaults = AssocDimensionReference {
                osnap_type,
                ..Default::default()
            };
            let mut projected = dump_reference(doc, &defaults);
            projected["Geometry"] = geometry
                .filter(|handle| super::resolves_handle(doc, *handle))
                .map(super::handle)
                .unwrap_or(Value::Null);
            projected
        } else if reader == "dxf" {
            // The DXF callback advances to the next active property for every
            // class marker, even when native records form a continuation chain.
            // It drops the returned OsnapPointRefTemplate, so Geometry stays null.
            dxf_refs
                .next()
                .map(|reference| {
                    let mut projected = dump_reference(doc, reference);
                    projected["Geometry"] = Value::Null;
                    projected
                })
                .unwrap_or(Value::Null)
        } else {
            value.references[slot]
                .first()
                .map(|reference| dump_reference(doc, reference))
                .unwrap_or(Value::Null)
        }
    })
}

fn dump_reference(doc: &crate::CadDocument, value: &AssocDimensionReference) -> Value {
    super::props(
        "DimensionAssociation.OsnapPointRef",
        [
            (
                "Geometry",
                value
                    .xrefs
                    .first()
                    .copied()
                    .filter(|handle| super::resolves_handle(doc, *handle))
                    .map(super::handle)
                    .unwrap_or(Value::Null),
            ),
            ("GeometryParameter", super::number(value.osnap_distance)),
            ("GsMarker", super::int(value.main_gs_marker as i64)),
            (
                "HasLastPointRef",
                Value::Bool(value.has_last_point_reference),
            ),
            (
                "IntersectionGsMarker",
                super::int(value.intersection_gs_marker as i64),
            ),
            (
                "IntersectionSubType",
                super::int(value.intersection_subent_type as i64),
            ),
            ("ObjectOsnapType", super::int(value.osnap_type as i64)),
            ("OsnapPoint", super::vec3(value.osnap_point)),
            ("SubentType", super::int(value.main_subent_type as i64)),
        ],
    )
}

/// Resolve a DIMASSOC dimension pointer with the same typed lookup as
/// ACadSharp's `TryGetCadObject<Dimension>`. A registered non-dimension
/// handle must not be projected into the `Dimension` property.
fn dimension_handle(doc: &crate::CadDocument, value: Handle) -> Value {
    if matches!(doc.get_entity(value), Some(EntityType::Dimension(_))) {
        super::handle(value)
    } else {
        Value::Null
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::dimension::{Dimension, DimensionAligned};
    use crate::objects::{AssociativeData, Group, ObjectType};
    use crate::types::Vector3;
    use crate::{CadDocument, EntityType, Line};
    use serde_json::json;

    fn reference(
        osnap_type: u8,
        geometry: Option<Handle>,
        point: Vector3,
        distance: f64,
        subent_type: i32,
        gs_marker: i32,
        has_last_point_reference: bool,
    ) -> AssocDimensionReference {
        AssocDimensionReference {
            class_name: "AcDbOsnapPointRef".to_owned(),
            osnap_type,
            xrefs: geometry.into_iter().collect(),
            main_subent_type: subent_type,
            main_gs_marker: gs_marker,
            xref_paths: Vec::new(),
            osnap_distance: distance,
            osnap_point: point,
            intersection_objects: Vec::new(),
            intersection_subent_type: 4,
            intersection_gs_marker: 5,
            intersection_xref_paths: Vec::new(),
            has_last_point_reference,
        }
    }

    fn object(value: &AssocDimensionAssociation) -> AssociativeObject {
        AssociativeObject {
            handle: Handle::new(0x70),
            owner: Handle::new(0x71),
            reactors: vec![Handle::new(0x72)],
            xdictionary_handle: Some(Handle::new(0x73)),
            dxf_name: "DIMASSOC".to_owned(),
            cpp_class_name: "AcDbDimAssoc".to_owned(),
            data: AssociativeData::DimensionAssociation(value.clone()),
            source_version: None,
        }
    }

    fn document_with_registered_handles() -> CadDocument {
        let mut doc = CadDocument::new();

        let mut dimension = DimensionAligned::new(Vector3::ZERO, Vector3::UNIT_X);
        dimension.base.common.handle = Handle::new(0x74);
        doc.add_entity(EntityType::Dimension(Box::new(Dimension::Aligned(
            dimension,
        ))))
        .unwrap();

        for handle in [0x101, 0x103, 0x104] {
            let mut line = Line::new();
            line.common.handle = Handle::new(handle);
            doc.add_entity(EntityType::Line(line)).unwrap();
        }

        let mut group = Group::new("geometry-object");
        group.handle = Handle::new(0x105);
        doc.objects.insert(group.handle, ObjectType::Group(group));
        doc
    }

    #[test]
    fn projects_all_slots_and_first_continuation_record() {
        let mut value = AssocDimensionAssociation {
            associativity: 0b1111,
            trans_space: true,
            rotated_type: 2,
            dimension: Handle::new(0x74),
            references: std::array::from_fn(|_| Vec::new()),
            dwg_reference_stream: None,
        };
        value.references[0].push(reference(
            13,
            Some(Handle::new(0x101)),
            Vector3::new(1.0, 2.0, 3.0),
            4.5,
            2,
            6,
            true,
        ));
        // The continuation belongs to the native chain but is not exposed by
        // ACadSharp's singular FirstPointRef property.
        value.references[0].push(reference(
            9,
            Some(Handle::new(0x1FF)),
            Vector3::new(9.0, 9.0, 9.0),
            99.0,
            8,
            9,
            false,
        ));
        value.references[1].push(reference(
            6,
            None,
            Vector3::new(10.0, 11.0, 12.0),
            13.0,
            3,
            7,
            false,
        ));
        value.references[2].push(reference(
            7,
            Some(Handle::new(0x103)),
            Vector3::new(14.0, 15.0, 16.0),
            17.0,
            4,
            8,
            false,
        ));
        value.references[3].push(reference(
            8,
            Some(Handle::new(0x104)),
            Vector3::new(18.0, 19.0, 20.0),
            21.0,
            5,
            9,
            false,
        ));

        let doc = document_with_registered_handles();
        let object = object(&value);
        let output = dump_object(&doc, object.handle, &object, &value, "new");
        let props = &output["props"];

        assert_eq!(output["type"], json!("DimensionAssociation"));
        assert_eq!(output["owner"], json!("#71"));
        assert_eq!(props["AssociativityFlags"], json!(15));
        assert_eq!(props["Dimension"], json!("#74"));
        assert_eq!(props["IsTransSpace"], json!(true));
        assert_eq!(props["RotatedDimensionType"], json!(2));
        assert_eq!(props["Reactors"], json!(["#72"]));
        assert_eq!(props["XDictionary"], json!("#73"));
        assert_eq!(props["FirstPointRef"]["Geometry"], json!("#101"));
        assert_eq!(props["FirstPointRef"]["GeometryParameter"], json!(4.5));
        assert_eq!(props["FirstPointRef"]["GsMarker"], json!(6));
        assert_eq!(props["FirstPointRef"]["HasLastPointRef"], json!(true));
        assert_eq!(props["FirstPointRef"]["OsnapPoint"], json!([1.0, 2.0, 3.0]));
        assert_eq!(props["FirstPointRef"]["ObjectOsnapType"], json!(13));
        assert_eq!(props["FirstPointRef"]["SubentType"], json!(2));
        assert_eq!(props["SecondPointRef"]["Geometry"], Value::Null);
        assert_eq!(props["SecondPointRef"]["IntersectionSubType"], json!(4));
        assert_eq!(props["ThirdPointRef"]["Geometry"], json!("#103"));
        assert_eq!(props["FourthPointRef"]["Geometry"], json!("#104"));
        assert_eq!(
            props["ExtendedData"],
            json!(crate::compat::acadsharp::UNMAPPED)
        );
    }

    #[test]
    fn inactive_and_empty_slots_project_as_null() {
        let mut value = AssocDimensionAssociation::default();
        value.dimension = Handle::NULL;
        value.references[0].push(reference(
            1,
            Some(Handle::new(0x201)),
            Vector3::new(1.0, 1.0, 1.0),
            1.0,
            1,
            1,
            false,
        ));
        let doc = CadDocument::new();
        let object = object(&value);
        let props = dump_object(&doc, object.handle, &object, &value, "new")["props"].clone();

        assert_eq!(props["AssociativityFlags"], json!(0));
        assert_eq!(props["Dimension"], Value::Null);
        assert_eq!(props["FirstPointRef"], Value::Null);
        assert_eq!(props["SecondPointRef"], Value::Null);
        assert_eq!(props["ThirdPointRef"], Value::Null);
        assert_eq!(props["FourthPointRef"], Value::Null);
    }

    #[test]
    fn resolves_only_typed_dimension_and_registered_geometry_handles() {
        let mut value = AssocDimensionAssociation {
            associativity: 1,
            dimension: Handle::new(0x101), // registered line, not a dimension
            ..Default::default()
        };
        value.references[0].push(reference(
            1,
            Some(Handle::new(0x105)),
            Vector3::ZERO,
            0.0,
            0,
            0,
            false,
        ));
        let doc = document_with_registered_handles();
        let object = object(&value);
        let props = dump_object(&doc, object.handle, &object, &value, "new")["props"].clone();

        assert_eq!(props["Dimension"], Value::Null);
        assert_eq!(props["FirstPointRef"]["Geometry"], json!("#105"));

        value.references[0][0].xrefs = vec![Handle::new(0x1FF)];
        let props = dump_object(&doc, object.handle, &object, &value, "new")["props"].clone();
        assert_eq!(props["FirstPointRef"]["Geometry"], Value::Null);
    }
    #[test]
    fn dwg_incomplete_osnap_read_uses_source_bits_in_both_stream_layouts() {
        use crate::io::dwg::dwg_reference_type::DwgReferenceType;
        use crate::io::dwg::dwg_stream_readers::bit_reader::DwgBitReader;
        use crate::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader;
        use crate::io::dwg::dwg_stream_readers::object_reader::associative::read_associative_data;
        use crate::io::dwg::dwg_stream_writers::bit_writer::DwgBitWriter;
        use crate::io::dwg::dwg_version::DwgVersion;
        use crate::types::DxfVersion;

        for (version, second_type) in [(DxfVersion::AC1018, 144), (DxfVersion::AC1032, 64)] {
            let dwg = DwgVersion::from_dxf_version(version).unwrap();
            let mut main = DwgBitWriter::new(dwg, version);
            let mut text = DwgBitWriter::new(dwg, version);
            let mut handles = DwgBitWriter::new(dwg, version);
            main.write_bit_long(3);
            main.write_bit(false);
            main.write_byte(0);
            handles.write_handle(DwgReferenceType::SoftPointer, 0x74);
            for (osnap, geometry) in [(13, 0x101), (1, 0x103)] {
                if version < DxfVersion::AC1021 {
                    main.write_variable_text("AcDbOsnapPointRef");
                } else {
                    text.write_variable_text("AcDbOsnapPointRef");
                }
                main.write_byte(osnap);
                main.write_bit_long(1); // One geometry handle.
                handles.write_handle(DwgReferenceType::SoftPointer, geometry);
                main.write_bit_long(2); // Subentity type.
                main.write_bit_long(6); // Graphics marker.
                main.write_bit_long(0); // No xref paths.
                main.write_bit_double(0.25);
                main.write_3bit_double(Vector3::new(1.0, 2.0, 3.0));
                main.write_bit(false);
            }
            let end = main.position_in_bits();
            let mut main = DwgBitReader::new(main.into_bytes(), dwg, version);
            main.set_end_bit(end);
            let text = (version >= DxfVersion::AC1021)
                .then(|| DwgBitReader::new(text.into_bytes(), dwg, version));
            let handles = DwgBitReader::new(handles.into_bytes(), dwg, version);
            let mut reader = DwgMergedReader::from_readers(main, text, Some(handles), version);
            let Some(AssociativeData::DimensionAssociation(value)) =
                read_associative_data(&mut reader, "DIMASSOC", dwg, version)
            else {
                panic!("typed DIMASSOC");
            };
            assert_eq!(value.references[1][0].osnap_type, 1);
            assert_eq!(value.references[0][0].main_subent_type, 2);
            assert_eq!(
                value.references[0][0].osnap_point,
                Vector3::new(1.0, 2.0, 3.0)
            );
            assert!(value.dwg_reference_stream.is_some());
            let doc = document_with_registered_handles();
            let mut object = object(&value);
            object.source_version = Some(version);
            let before = value.clone();
            let p = dump_object(&doc, object.handle, &object, &value, "dwg")["props"].clone();
            assert_eq!(p["FirstPointRef"]["ObjectOsnapType"], 13);
            assert_eq!(p["SecondPointRef"]["ObjectOsnapType"], second_type);
            assert_eq!(p["FirstPointRef"]["Geometry"], "#101");
            assert_eq!(p["SecondPointRef"]["Geometry"], "#103");
            assert_eq!(p["FirstPointRef"]["SubentType"], 0);
            assert_eq!(p["FirstPointRef"]["OsnapPoint"], json!([0.0, 0.0, 0.0]));
            assert_eq!(p["FirstPointRef"]["GeometryParameter"], 0.0);
            assert_eq!(value, before);
        }
    }

    #[test]
    fn dxf_continuations_fill_next_active_slot_and_leave_geometry_unresolved() {
        let mut value = AssocDimensionAssociation {
            associativity: 0b0101,
            dimension: Handle::new(0x74),
            ..Default::default()
        };
        value.references[0] = vec![
            reference(
                1,
                Some(Handle::new(0x101)),
                Vector3::UNIT_X,
                0.25,
                2,
                3,
                true,
            ),
            reference(
                3,
                Some(Handle::new(0x103)),
                Vector3::UNIT_Y,
                0.75,
                4,
                5,
                false,
            ),
        ];
        value.references[2] = vec![reference(
            9,
            Some(Handle::new(0x104)),
            Vector3::UNIT_Z,
            1.0,
            6,
            7,
            false,
        )];
        let doc = document_with_registered_handles();
        let object = object(&value);
        let p = dump_object(&doc, object.handle, &object, &value, "dxf")["props"].clone();
        assert_eq!(p["Dimension"], "#74");
        assert_eq!(p["FirstPointRef"]["Geometry"], Value::Null);
        assert_eq!(p["ThirdPointRef"]["Geometry"], Value::Null);
        assert_eq!(p["ThirdPointRef"]["ObjectOsnapType"], 3);
        assert_eq!(p["ThirdPointRef"]["GeometryParameter"], 0.75);
        assert_eq!(value.references[2][0].osnap_type, 9);
    }
}
