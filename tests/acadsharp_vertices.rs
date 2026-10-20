// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::dump_document;
use acadrust::entities::lwpolyline::{LwPolyline, LwVertex};
use acadrust::entities::mline::{MLine, MLineFlags, MLineSegment, MLineVertex};
use acadrust::entities::EntityType;
use acadrust::types::{DxfVersion, Vector2, Vector3};
use acadrust::CadDocument;
use serde_json::{json, Value};

fn props(doc: &CadDocument, type_name: &str, reader: &str) -> Value {
    dump_document(doc, "synthetic", reader)["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["type"] == type_name)
        .unwrap_or_else(|| panic!("missing {type_name} object"))["props"]
        .clone()
}

#[test]
fn lwpolyline_vertices_project_widths_and_acadsharp_defaults() {
    let mut doc = CadDocument::new();
    let mut polyline = LwPolyline::new();
    polyline.vertices.push(LwVertex {
        location: Vector2::new(1.0, 2.0),
        bulge: 0.25,
        start_width: 3.0,
        end_width: 4.0,
        vertex_id: 17,
    });
    doc.add_entity(EntityType::LwPolyline(polyline)).unwrap();

    assert_eq!(
        props(&doc, "LwPolyline", "new")["Vertices"],
        json!([{
            "Bulge": 0.25,
            "CurveTangent": 0.0,
            "EndWidth": 4.0,
            "Flags": 0,
            "Id": 17,
            "Location": [1.0, 2.0],
            "StartWidth": 3.0,
        }])
    );
}

#[test]
fn mline_vertices_project_nested_segment_parameters_in_order() {
    let mut doc = CadDocument::new();
    let mut mline = MLine::new();
    let mut vertex = MLineVertex::new(Vector3::new(5.0, 6.0, 0.0));

    let mut first = MLineSegment::new();
    first.parameters.extend([1.0, 2.0]);
    let mut second = MLineSegment::new();
    second.area_fill_parameters.push(3.0);
    second.parameters.push(-4.0);
    vertex.segments.extend([first, second]);
    mline.vertices.push(vertex);
    doc.add_entity(EntityType::MLine(mline)).unwrap();

    assert_eq!(
        props(&doc, "MLine", "new")["Vertices"],
        json!([{
            "Direction": [1.0, 0.0, 0.0],
            "Miter": [0.0, 1.0, 0.0],
            "Position": [5.0, 6.0, 0.0],
            "Segments": [
                {"AreaFillParameters": [], "Parameters": [1.0, 2.0]},
                {"AreaFillParameters": [3.0], "Parameters": [-4.0]},
            ],
        }])
    );
}

#[test]
fn mline_dwg_flags_follow_acadsharp_open_closed_reader() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    doc.dwg_source_version = Some(DxfVersion::AC1032);
    let mut mline = MLine::new();
    mline.flags = MLineFlags::HAS_VERTICES | MLineFlags::NO_START_CAPS;
    doc.add_entity(EntityType::MLine(mline)).unwrap();

    let native = doc
        .entities()
        .find_map(|entity| match entity {
            EntityType::MLine(value) => Some(value),
            _ => None,
        })
        .unwrap();
    assert!(native.flags.contains(MLineFlags::NO_START_CAPS));
    assert_eq!(props(&doc, "MLine", "dwg")["Flags"], json!(1));
}
