use acadrust::objects::{GeoData, ObjectType};
use acadrust::types::{Handle, Vector2, Vector3};
use acadrust::{CadDocument, DxfReader, DxfWriter};
use std::io::Cursor;

fn read(body: &str) -> CadDocument {
    let dxf = format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1021\n0\nENDSEC\n0\nSECTION\n2\nOBJECTS\n0\nGEODATA\n5\nF000\n330\n0\n100\nAcDbGeoData\n{body}0\nENDSEC\n0\nEOF\n");
    DxfReader::from_reader(Cursor::new(dxf.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

fn geo(doc: &CadDocument) -> &GeoData {
    match doc.objects.get(&Handle::new(0xF000)).unwrap() {
        ObjectType::GeoData(value) => value,
        _ => panic!("expected GeoData"),
    }
}

const LEGACY: &str = "90\n1\n70\n0\n330\n1F\n40\n1\n41\n2\n42\n3\n91\n1\n10\n4\n20\n5\n30\n6\n11\n9\n21\n8\n31\n7\n210\n0\n220\n0\n230\n1\n52\n30\n43\n3\n44\n4\n45\n5\n46\n2\n301\ndefinition^Jtext\n303\ndatum\n93\n1\n12\n1\n22\n2\n13\n3\n23\n4\n96\n1\n97\n0\n98\n0\n99\n0\n";

#[test]
fn r2009_geodata_retains_native_coordinates_scale_north_and_mesh() {
    let doc = read(LEGACY);
    let value = geo(&doc);
    assert_eq!(value.reference_point, Vector3::new(2.0, 1.0, 3.0));
    assert_eq!(
        value.obsolete_observation_point,
        Vector3::new(9.0, 8.0, 7.0)
    );
    assert_eq!(value.obsolete_scale_vector, Vector3::new(3.0, 4.0, 5.0));
    assert_eq!(value.horizontal_unit_scale, 2.0);
    assert_eq!(value.vertical_unit_scale, 2.0);
    assert_eq!((value.horizontal_units, value.vertical_units), (1, 1));
    assert!((value.north_direction.x - 0.5).abs() < 1e-12);
    assert!((value.north_direction.y - 3_f64.sqrt() / 2.0).abs() < 1e-12);
    assert_eq!(value.mesh_points.len(), 1);
    assert_eq!(value.mesh_points[0].source, Vector2::new(1.0, 2.0));
    assert_eq!(value.mesh_points[0].destination, Vector2::new(3.0, 4.0));
    assert_eq!(value.mesh_faces.len(), 1);
    assert_eq!(value.coordinate_system_definition, "definition\ntext");
    assert_eq!(value.coordinate_system_datum, "datum");
    assert_eq!(
        value.raw_dxf_definition.as_deref(),
        Some("definition^Jtextdatum")
    );
}

#[test]
fn r2009_geodata_roundtrips_its_versioned_layout_in_ascii_and_binary() {
    let doc = read(LEGACY);
    for binary in [false, true] {
        let writer = if binary {
            DxfWriter::new_binary(&doc)
        } else {
            DxfWriter::new(&doc)
        };
        let loaded = DxfReader::from_reader(Cursor::new(writer.write_to_vec().unwrap()))
            .unwrap()
            .read()
            .unwrap();
        let (a, b) = (geo(&doc), geo(&loaded));
        assert_eq!(a.reference_point, b.reference_point);
        assert_eq!(a.obsolete_observation_point, b.obsolete_observation_point);
        assert_eq!(a.obsolete_scale_vector, b.obsolete_scale_vector);
        assert_eq!(a.horizontal_unit_scale, b.horizontal_unit_scale);
        assert_eq!(a.vertical_unit_scale, b.vertical_unit_scale);
        assert_eq!(a.mesh_points, b.mesh_points);
        assert_eq!(a.mesh_faces, b.mesh_faces);
        assert!((a.north_direction.x - b.north_direction.x).abs() < 1e-12);
        assert!((a.north_direction.y - b.north_direction.y).abs() < 1e-12);
    }
}

#[test]
fn oversized_legacy_mesh_count_is_bounded_by_records_and_preserves_next_object() {
    let body = LEGACY.replace("93\n1\n", "93\n2147483647\n")
        + "0\nDICTIONARY\n5\nF001\n330\n0\n100\nAcDbDictionary\n280\n0\n281\n1\n";
    let doc = read(&body);
    assert_eq!(geo(&doc).mesh_points.len(), 1);
    assert!(matches!(
        doc.objects.get(&Handle::new(0xF001)),
        Some(ObjectType::Dictionary(_))
    ));
}

#[cfg(feature = "acadsharp-compat")]
fn projected(doc: &CadDocument) -> serde_json::Value {
    let dump = acadrust::compat::acadsharp::dump_document(doc, "fixture.dxf", "dxf");
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["handle"] == "#F000")
        .unwrap()["props"]
        .clone()
}

#[test]
#[cfg(feature = "acadsharp-compat")]
fn r2009_compat_keeps_oracle_overwrites_and_defaults_out_of_native_data() {
    let doc = read(LEGACY);
    let before = geo(&doc).clone();
    let p = projected(&doc);
    assert_eq!(p["ReferencePoint"], serde_json::json!([9.0, 8.0, 7.0]));
    assert_eq!(p["HorizontalUnitScale"], 2.0);
    assert_eq!(p["VerticalUnitScale"], 1.0);
    assert_eq!(p["VerticalUnits"], 6);
    assert_eq!(p["UserSpecifiedScaleFactor"], 0.0);
    assert_eq!(p["CoordinateSystemDefinition"], "definition^Jtextdatum");
    assert!(p["HostBlock"].is_null());
    assert_eq!(p["Points"].as_array().unwrap().len(), 1);
    assert_eq!(geo(&doc), &before);
}

#[test]
#[cfg(feature = "acadsharp-compat")]
fn modern_dxf_geo_projects_presence_defaults_scale_slots_and_exact_escape_text() {
    let doc = read("90\n3\n40\n2\n41\n3\n301\nA^JB^ J^IC\n303\n-end\n");
    let before = geo(&doc).clone();
    assert_eq!(before.coordinate_system_definition, "A\nB^J\tC-end");
    let p = projected(&doc);
    assert_eq!(p["CoordinateSystemDefinition"], "A^JB^ J^IC-end");
    assert_eq!(p["HorizontalUnitScale"], 3.0);
    assert_eq!(p["VerticalUnitScale"], 2.0);
    assert_eq!(p["HorizontalUnits"], 6);
    assert_eq!(p["VerticalUnits"], 6);
    assert_eq!(p["CoordinatesType"], 1);
    assert_eq!(p["NorthDirection"], serde_json::json!([0.0, 1.0]));
    assert_eq!(p["UpDirection"], serde_json::json!([0.0, 0.0, 1.0]));
    assert_eq!(geo(&doc), &before);
}
