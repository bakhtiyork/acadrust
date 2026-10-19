// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::{bytes, dump_document};
use acadrust::entities::solid3d::source::{AcisSource, DwgAcisSource};
use acadrust::entities::solid3d::Wire;
use acadrust::entities::{Arc as CadArc, Ellipse, Solid3D};
use acadrust::io::dwg::{dwg_stream_writers::bit_writer::DwgBitWriter, DwgVersion};
use acadrust::{CadDocument, DxfReader, DxfVersion, DxfWriter, EntityType, Handle, Vector3};
use serde_json::{json, Value};
use std::{io::Cursor, sync::Arc};

fn read_entity(kind: &str, data: &str) -> CadDocument {
    let text = format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\n{kind}\n5\n900\n{data}0\nENDSEC\n0\nEOF\n");
    DxfReader::from_reader(Cursor::new(text.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

fn props(doc: &CadDocument, h: Handle) -> Value {
    let dump = dump_document(doc, "", "new");
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["handle"] == format!("#{:X}", h.value()))
        .unwrap()["props"]
        .clone()
}

#[test]
fn ellipse_thickness_reads_and_roundtrips_without_changing_coordinates() {
    let doc = read_entity(
        "ELLIPSE",
        "100\nAcDbEllipse\n10\n2\n20\n3\n30\n4\n11\n5\n21\n-6\n31\n0\n40\n0.5\n39\n-7.25\n",
    );
    let EntityType::Ellipse(ellipse) = doc.get_entity(Handle::new(0x900)).unwrap() else {
        panic!()
    };
    assert_eq!(ellipse.thickness, -7.25);
    assert_eq!(ellipse.center, Vector3::new(2.0, 3.0, 4.0));
    assert_eq!(ellipse.major_axis, Vector3::new(5.0, -6.0, 0.0));
    let p = props(&doc, Handle::new(0x900));
    assert_eq!(p["Thickness"], -7.25);
    assert_eq!(p["Rotation"], (-6.0_f64).atan2(5.0));
    for writer in [DxfWriter::new(&doc), DxfWriter::new_binary(&doc)] {
        let doc = DxfReader::from_reader(Cursor::new(writer.write_to_vec().unwrap()))
            .unwrap()
            .read()
            .unwrap();
        assert_eq!(props(&doc, Handle::new(0x900))["Thickness"], -7.25);
    }
}

#[test]
fn primitive_getters_keep_native_arc_direction_and_signed_ellipse_rotation() {
    for (start, end, expected) in [
        (0.0, 1.0, -1.0),
        (5.0, 1.0, 4.0 - std::f64::consts::TAU),
        (2.0, 2.0, 0.0),
    ] {
        let arc = CadArc::from_center_radius_angles(Vector3::ZERO, 2.0, start, end);
        assert_eq!(arc.sweep_angle(), -expected);
        let mut doc = CadDocument::new();
        let h = doc.add_entity(EntityType::Arc(arc)).unwrap();
        assert_eq!(props(&doc, h)["Sweep"], expected);
    }
    let mut doc = CadDocument::new();
    let ellipse = Ellipse::from_center_axes(Vector3::ZERO, Vector3::new(0.0, -2.0, 0.0), 0.5);
    let h = doc.add_entity(EntityType::Ellipse(ellipse)).unwrap();
    let p = props(&doc, h);
    assert_eq!(p["Rotation"], -std::f64::consts::FRAC_PI_2);
    assert_eq!(p["Thickness"], 0.0);
}

fn encode(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c > ' ' && c < '\u{9f}' {
                char::from(159 - c as u8)
            } else {
                c
            }
        })
        .collect()
}

#[test]
fn dxf_sat_preserves_chunks_terminator_and_absent_version_without_payload_copy() {
    for kind in ["3DSOLID", "REGION", "BODY"] {
        for encoded in [false, true] {
            let chunks = ["400 0 1 0", "body", " $-1", "End-of-ACIS-data"];
            let raw: Vec<_> = chunks
                .iter()
                .map(|s| if encoded { encode(s) } else { s.to_string() })
                .collect();
            let input = format!(
                "100\nAcDbModelerGeometry\n2\nsource-id\n1\n{}\n1\n{}\n3\n{}\n1\n{}\n",
                raw[0], raw[1], raw[2], raw[3]
            );
            let doc = read_entity(kind, &input);
            let e = doc.get_entity(Handle::new(0x900)).unwrap();
            let (data, uid) = match e {
                EntityType::Solid3D(e) => (&e.acis_data, &e.uid),
                EntityType::Region(e) => (&e.acis_data, &e.uid),
                EntityType::Body(e) => (&e.acis_data, &e.uid),
                _ => panic!(),
            };
            assert_eq!(uid, "source-id");
            assert_eq!(data.sat_data, "400 0 1 0\nbody $-1\n");
            let AcisSource::Dxf(source) = data.source.as_deref().unwrap() else {
                panic!()
            };
            assert!(source.chunks.iter().all(|c| c.source_override.is_none()));
            assert_eq!(source.layout.line_endings.len(), 2);
            assert_eq!(source.layout.suffix, "End-of-ACIS-data\n");
            let p = props(&doc, Handle::new(0x900));
            assert_eq!(
                p["AcisData"],
                bytes(b"400 0 1 0\nbody $-1\nEnd-of-ACIS-data")
            );
            assert_eq!(p["ProprietaryData"], raw.join("\n") + "\n");
            assert_eq!(p["ModelerFormatVersion"], 0);
            assert_eq!(p["Point"], json!([0.0, 0.0, 0.0]));
            assert_eq!(p["Wires"], json!([]));
            if kind != "3DSOLID" {
                assert_eq!(p["SubclassMarker"], "AcDbModelerGeometry");
            }
        }
    }
}

#[test]
fn dxf_explicit_modeler_version_is_independent_of_binary_kernel_signature() {
    let mut doc = read_entity("3DSOLID", "100\nAcDbModelerGeometry\n70\n2\n");
    let EntityType::Solid3D(solid) = doc.get_entity_mut(Handle::new(0x900)).unwrap() else {
        panic!()
    };
    solid.acis_data.is_binary = true;
    solid.acis_data.sab_data = b"ASM BinaryFile4\0large-native-binary".to_vec();
    let p = props(&doc, Handle::new(0x900));
    assert_eq!(p["ModelerFormatVersion"], 2);
    assert_eq!(p["IsBinaryAcisData"], false);
    assert_eq!(
        p["AcisData"],
        bytes(b"ASM BinaryFile4\0large-native-binary")
    );
    let EntityType::Solid3D(solid) = doc.get_entity_mut(Handle::new(0x900)).unwrap() else {
        panic!()
    };
    assert!(solid.acis_data.is_binary);
    solid.acis_data.sab_data = b"ACIS BinaryFile\0".to_vec();
    assert_eq!(props(&doc, Handle::new(0x900))["IsBinaryAcisData"], true);
}

fn source_solid(version: DxfVersion, build: impl FnOnce(&mut DwgBitWriter)) -> Solid3D {
    let mut w = DwgBitWriter::new(DwgVersion::from_dxf_version(version).unwrap(), version);
    for _ in 0..3 {
        w.write_bit(true);
    }
    build(&mut w);
    let end = w.position_in_bits();
    let data: Arc<[u8]> = w.into_bytes().into();
    let mut solid = Solid3D::new();
    solid.acis_data.source = Some(Arc::new(AcisSource::Dwg(DwgAcisSource {
        data: data.clone(),
        start_bit: 3,
        end_bit: end,
        version,
    })));
    let AcisSource::Dwg(source) = solid.acis_data.source.as_deref().unwrap() else {
        panic!()
    };
    assert!(Arc::ptr_eq(&data, &source.data));
    solid
}

#[test]
fn old_dwg_sat_retains_crlf_and_returns_before_native_wire_cache() {
    let original = b"400 0 1 0\r\nbody $-1\r\n";
    let mut solid = source_solid(DxfVersion::AC1015, |w| {
        w.write_bit(false);
        w.write_bit(false);
        w.write_bit_short(1);
        w.write_bit_long(original.len() as i32);
        w.write_bytes(
            &original
                .iter()
                .map(|&b| if b > 32 && b < 159 { 159 - b } else { b })
                .collect::<Vec<_>>(),
        );
        w.write_bit_long(0);
        // Native cache remains available after the oracle's early return.
        w.write_bit(true);
    });
    solid.acis_data.sat_data = "400 0 1 0\nbody $-1\n".into();
    solid
        .wires
        .push(Wire::from_points(vec![Vector3::new(9.0, 8.0, 7.0)]));
    solid.point_of_reference = Vector3::new(3.0, 4.0, 5.0);
    let mut doc = CadDocument::new();
    let h = doc.add_entity(EntityType::Solid3D(solid)).unwrap();
    let p = props(&doc, h);
    assert_eq!(p["AcisData"], bytes(original));
    assert_eq!(p["ModelerFormatVersion"], 0);
    assert_eq!(p["Point"], json!([0.0, 0.0, 0.0]));
    assert_eq!(p["Wires"], json!([]));
    let EntityType::Solid3D(solid) = doc.get_entity(h).unwrap() else {
        panic!()
    };
    assert_eq!(solid.wires.len(), 1);
    assert_eq!(solid.point_of_reference, Vector3::new(3.0, 4.0, 5.0));
}

fn write_wire(w: &mut DwgBitWriter, transform: bool) {
    w.write_byte(203);
    w.write_bit_long(42);
    w.write_bit_short(300);
    w.write_bit_long(-1);
    w.write_bit_long(1);
    w.write_3bit_double(Vector3::new(2.0, 3.0, 4.0));
    w.write_bit(transform);
    if transform {
        w.write_3bit_double(Vector3::UNIT_X);
        w.write_3bit_double(Vector3::UNIT_Y);
        w.write_3bit_double(Vector3::UNIT_Z);
        w.write_3bit_double(Vector3::new(5.0, 6.0, 7.0));
        w.write_bit_double(2.5);
        w.write_bit(true);
        w.write_bit(false);
        w.write_bit(true);
    }
}

#[test]
fn dwg_raw_wire_and_silhouette_projection_preserves_non_enum_type_and_oracle_scalar_scale() {
    let solid = source_solid(DxfVersion::AC1027, |w| {
        w.write_bit(true);
        w.write_bit(true);
        w.write_3bit_double(Vector3::new(1.0, 2.0, 3.0));
        w.write_bit_long(7);
        w.write_bit(true);
        w.write_bit_long(2);
        write_wire(w, false);
        write_wire(w, true);
        w.write_bit_long(1);
        w.write_bit_long_long(0x123456789);
        w.write_3bit_double(Vector3::new(8.0, 9.0, 10.0));
        w.write_3bit_double(Vector3::UNIT_Z);
        w.write_3bit_double(Vector3::UNIT_Y);
        w.write_bit(true);
        w.write_bit(true);
        w.write_bit_long(1);
        write_wire(w, false);
        w.write_bit(true);
    });
    let mut doc = CadDocument::new();
    let h = doc.add_entity(EntityType::Solid3D(solid)).unwrap();
    let p = props(&doc, h);
    assert_eq!(p["Point"], json!([1.0, 2.0, 3.0]));
    assert_eq!(p["Wires"][0]["Type"], 203);
    assert_eq!(p["Wires"][0]["Color"]["Index"], 256);
    assert_eq!(p["Wires"][0]["XAxis"], json!([0.0, 0.0, 0.0]));
    assert_eq!(p["Wires"][0]["Scale"], 0.0);
    assert_eq!(p["Wires"][1]["Scale"], 2.5);
    assert_eq!(p["Wires"][1]["HasRotation"], true);
    assert_eq!(p["Wires"][1]["HasShear"], true);
    assert_eq!(p["Silhouettes"][0]["ViewportId"], 0x123456789_i64);
    assert_eq!(
        p["Silhouettes"][0]["Wires"][0]["Points"],
        json!([[2.0, 3.0, 4.0]])
    );
}

#[test]
fn malformed_modeler_counts_remain_bounded_and_explicitly_unmapped() {
    for count in [-1, i32::MAX] {
        let solid = source_solid(DxfVersion::AC1027, |w| {
            w.write_bit(true);
            w.write_bit(false);
            w.write_bit_long(0);
            w.write_bit(true);
            w.write_bit_long(count);
        });
        let mut doc = CadDocument::new();
        let h = doc.add_entity(EntityType::Solid3D(solid)).unwrap();
        assert_eq!(props(&doc, h)["Wires"], "!unmapped");
    }
}

#[test]
fn dwg_inline_sab_is_trimmed_at_marker_without_changing_native_binary_payload() {
    let raw = b"ACIS BinaryFile\0\x0e\x03End\x0e\x02of\x0e\x04ACIS\x0d\x04data";
    let mut solid = source_solid(DxfVersion::AC1018, |w| {
        w.write_bit(false);
        w.write_bit(false);
        w.write_bit_short(2);
        w.write_bytes(raw);
        w.write_bytes(b"trailing-cache");
    });
    solid.acis_data.sab_data = [raw.as_slice(), b"\x11"].concat();
    solid.acis_data.is_binary = true;
    let mut doc = CadDocument::new();
    let h = doc.add_entity(EntityType::Solid3D(solid)).unwrap();
    let p = props(&doc, h);
    assert_eq!(p["AcisData"], bytes(raw));
    assert_eq!(p["IsBinaryAcisData"], true);
    let EntityType::Solid3D(solid) = doc.get_entity(h).unwrap() else {
        panic!()
    };
    assert_eq!(solid.acis_data.sab_data.last(), Some(&0x11));
}

#[test]
fn dxf_lexical_source_differences_are_projected_without_changing_native_decoding() {
    let doc = read_entity(
        "3DSOLID",
        "100\nAcDbModelerGeometry\n70\n1\n1\n400 0 1 0\n1\n\\U+0041\n",
    );
    let EntityType::Solid3D(solid) = doc.get_entity(Handle::new(0x900)).unwrap() else {
        panic!()
    };
    assert_eq!(solid.acis_data.sat_data, "400 0 1 0\nA\n");
    let p = props(&doc, Handle::new(0x900));
    assert_eq!(p["AcisData"], bytes(b"400 0 1 0\n\\U+0041"));
    assert_eq!(p["ProprietaryData"], "400 0 1 0\n\\U+0041\n");
}

#[test]
fn dxf_large_sat_chunks_retain_only_sparse_lexical_differences() {
    let body = format!(
        "{}\\U+0041{}^ {}\\U+0042{}",
        "x".repeat(16384),
        "y".repeat(16384),
        "z".repeat(16384),
        "w".repeat(16384)
    );
    let doc = read_entity("3DSOLID", &format!("70\n1\n1\n400 0 1 0\n1\n{body}\n"));
    let EntityType::Solid3D(solid) = doc.get_entity(Handle::new(0x900)).unwrap() else {
        panic!()
    };
    let Some(AcisSource::Dxf(source)) = solid.acis_data.source.as_deref() else {
        panic!()
    };
    let retained: usize = source
        .chunks
        .iter()
        .filter_map(|c| c.source_override.as_ref())
        .flatten()
        .map(|p| p.insert.len())
        .sum();
    assert!(
        retained < 32,
        "retained {retained} bytes from a 64 KiB chunk"
    );
    let p = props(&doc, Handle::new(0x900));
    assert_eq!(
        p["AcisData"],
        bytes(format!("400 0 1 0\n{}", body.replace("^ ", "^")).as_bytes())
    );
}

#[test]
fn dwg_builder_retains_modeler_source_for_all_three_entity_types() {
    use acadrust::entities::solid3d::{Body, Region};
    use acadrust::{DwgReader, DwgWriter};
    for version in [DxfVersion::AC1015, DxfVersion::AC1027] {
        let mut doc = CadDocument::with_version(version);
        let mut handles = Vec::new();
        for entity in [
            EntityType::Solid3D(Solid3D::new()),
            EntityType::Region(Region::new()),
            EntityType::Body(Body::new()),
        ] {
            handles.push(doc.add_entity(entity).unwrap());
        }
        let data = DwgWriter::write_to_vec(&doc).unwrap();
        let read = DwgReader::from_stream(Cursor::new(data)).read().unwrap();
        for h in handles {
            let data = match read.get_entity(h).unwrap() {
                EntityType::Solid3D(e) => &e.acis_data,
                EntityType::Region(e) => &e.acis_data,
                EntityType::Body(e) => &e.acis_data,
                _ => panic!(),
            };
            let AcisSource::Dwg(source) = data.source.as_deref().unwrap() else {
                panic!()
            };
            assert_eq!(source.version, version);
            assert!(source.start_bit < source.end_bit);
            assert!(source.end_bit <= source.data.len() as i64 * 8);
            let p = props(&read, h);
            assert_eq!(p["ModelerFormatVersion"], 0);
            assert_eq!(p["AcisData"], Value::Null);
        }
    }
}

#[test]
fn dxf_empty_chunks_keep_native_line_layout_and_oracle_separators_distinct() {
    let doc = read_entity(
        "3DSOLID",
        "100\nAcDbModelerGeometry\n1\n\n1\n400 0 1 0\n1\n\n3\nbody\n",
    );
    let EntityType::Solid3D(solid) = doc.get_entity(Handle::new(0x900)).unwrap() else {
        panic!()
    };
    assert_eq!(solid.acis_data.sat_data, "\n400 0 1 0\nbody\n");
    let p = props(&doc, Handle::new(0x900));
    assert_eq!(p["AcisData"], bytes(b"400 0 1 0\nbody"));
    assert_eq!(p["ProprietaryData"], "\n400 0 1 0\n\nbody\n");
}

#[test]
fn dwg_sab_end_markers_follow_oracle_priority() {
    let raw = b"ACIS BinaryFile\0End-of-ASM-data followed by End-of-ACIS-data";
    let solid = source_solid(DxfVersion::AC1018, |w| {
        w.write_bit(false);
        w.write_bit(false);
        w.write_bit_short(2);
        w.write_bytes(raw);
        w.write_bytes(b"discarded tail");
    });
    let mut doc = CadDocument::new();
    let h = doc.add_entity(EntityType::Solid3D(solid)).unwrap();
    assert_eq!(props(&doc, h)["AcisData"], bytes(raw));
}
