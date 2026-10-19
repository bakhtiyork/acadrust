// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::dump_document;
use acadrust::entities::{
    Dimension, DimensionArc, DimensionDiameter, DimensionLinear, DimensionOrdinate,
    DimensionRadius, EntityType, Tolerance,
};
use acadrust::types::Vector3;
use acadrust::{CadDocument, DxfReader};
use serde_json::{json, Value};
use std::io::Cursor;

fn object<'a>(dump: &'a Value, type_name: &str) -> &'a Value {
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["type"] == type_name)
        .unwrap_or_else(|| panic!("missing {type_name} object"))
}

fn properties(doc: &CadDocument, type_name: &str) -> Value {
    object(&dump_document(doc, "", "new"), type_name)["props"].clone()
}

#[test]
fn dimension_compatibility_projects_geometry_and_angular_flags() {
    let mut doc = CadDocument::new();

    let mut linear = DimensionLinear::new(Vector3::ZERO, Vector3::new(10.0, 0.0, 0.0));
    linear.definition_point = Vector3::new(10.0, 3.0, 0.0);
    doc.add_entity(EntityType::Dimension(Box::new(Dimension::Linear(linear))))
        .unwrap();

    let radius = DimensionRadius::new(Vector3::new(1.0, 2.0, 0.0), Vector3::new(4.0, 2.0, 0.0));
    doc.add_entity(EntityType::Dimension(Box::new(Dimension::Radius(radius))))
        .unwrap();

    let diameter = DimensionDiameter::new(Vector3::new(1.0, 0.0, 0.0), Vector3::new(5.0, 0.0, 0.0));
    doc.add_entity(EntityType::Dimension(Box::new(Dimension::Diameter(
        diameter,
    ))))
    .unwrap();

    let mut angular = acadrust::entities::DimensionAngular2Ln::default();
    angular.definition_point = Vector3::new(0.0, 0.0, 0.0);
    angular.angle_vertex = Vector3::new(10.0, 0.0, 0.0);
    angular.first_point = Vector3::new(5.0, -5.0, 0.0);
    angular.second_point = Vector3::new(5.0, 5.0, 0.0);
    angular.dimension_arc = Vector3::new(5.0, 2.0, 0.0);
    doc.add_entity(EntityType::Dimension(Box::new(Dimension::Angular2Ln(
        angular,
    ))))
    .unwrap();

    let ordinate =
        DimensionOrdinate::x_ordinate(Vector3::new(4.0, 7.0, 0.0), Vector3::new(8.0, 7.0, 0.0));
    doc.add_entity(EntityType::Dimension(Box::new(Dimension::Ordinate(
        ordinate,
    ))))
    .unwrap();

    let mut arc = DimensionArc::default();
    arc.center_point = Vector3::ZERO;
    arc.first_extension_point = Vector3::new(3.0, 0.0, 0.0);
    arc.second_extension_point = Vector3::new(0.0, 4.0, 0.0);
    arc.arc_start_parameter = 0.0;
    arc.arc_end_parameter = 0.5;
    doc.add_entity(EntityType::Dimension(Box::new(Dimension::Arc(arc))))
        .unwrap();

    let p = properties(&doc, "DimensionLinear");
    assert_eq!(p["Offset"], 3.0);

    let p = properties(&doc, "DimensionRadius");
    assert_eq!(p["DefinitionPoint"], json!([1.0, 2.0, 0.0]));
    assert_eq!(p["AngleVertex"], json!([4.0, 2.0, 0.0]));

    assert_eq!(properties(&doc, "DimensionDiameter")["IsAngular"], true);
    assert_eq!(properties(&doc, "DimensionOrdinate")["IsAngular"], true);

    let p = properties(&doc, "DimensionAngular2Line");
    assert_eq!(p["Center"], json!([5.0, 0.0, 0.0]));
    assert!((p["Offset"].as_f64().unwrap() - 50.0_f64.sqrt()).abs() < 1e-12);

    let p = properties(&doc, "DimensionArc");
    assert_eq!(p["Flags"], 5);
    assert_eq!(p["IsAngular"], true);
    let expected = 3.0 * std::f64::consts::FRAC_PI_2;
    assert!((p["Measurement"].as_f64().unwrap() - expected).abs() < 1e-12);
}

fn tolerance_dxf(codes: &str) -> CadDocument {
    let text = format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nTOLERANCE\n5\n2A\n100\nAcDbEntity\n8\n0\n100\nAcDbFcf\n{codes}0\nENDSEC\n0\nEOF\n"
    );
    DxfReader::from_reader(Cursor::new(text.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

#[test]
fn tolerance_source_projection_keeps_native_defaults_and_decoded_text() {
    let doc = tolerance_dxf("10\n1\n20\n2\n1\nline1^Jline2\n");
    let native = doc
        .entities()
        .find_map(|entity| match entity {
            EntityType::Tolerance(value) => Some(value),
            _ => None,
        })
        .expect("tolerance entity");
    assert_eq!(native.direction, Vector3::UNIT_X);
    assert_eq!(native.text, "line1\nline2");
    assert_eq!(native.dxf_source.as_ref().unwrap().has_direction, false);
    assert_eq!(
        native.dxf_source.as_ref().unwrap().raw_text.as_deref(),
        Some("line1^Jline2")
    );

    let p = properties(&doc, "Tolerance");
    assert_eq!(p["Direction"], json!([0.0, 0.0, 0.0]));
    assert_eq!(p["Text"], "line1^Jline2");
}

#[test]
fn programmatic_tolerance_keeps_native_direction_and_text_in_compatibility_view() {
    let mut doc = CadDocument::new();
    let tolerance = Tolerance::with_text(Vector3::new(1.0, 2.0, 0.0), "line1\nline2");
    doc.add_entity(EntityType::Tolerance(tolerance)).unwrap();
    let p = properties(&doc, "Tolerance");
    assert_eq!(p["Direction"], json!([1.0, 0.0, 0.0]));
    assert_eq!(p["Text"], "line1\nline2");
}

#[test]
fn angular_center_preserves_parallel_skew_and_degenerate_oracle_results() {
    for (first, second, expected) in [
        (
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 1.0, 0.0),
            json!(["NaN", "NaN", "NaN"]),
        ),
        (
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 1.0, 1.0),
            json!(["NaN", "NaN", "NaN"]),
        ),
        (
            Vector3::ZERO,
            Vector3::ZERO,
            json!("!throws:TargetInvocationException"),
        ),
    ] {
        let mut doc = CadDocument::new();
        let mut dim = acadrust::entities::DimensionAngular2Ln::default();
        dim.definition_point = Vector3::ZERO;
        dim.angle_vertex = Vector3::UNIT_X;
        dim.first_point = first;
        dim.second_point = second;
        doc.add_entity(EntityType::Dimension(Box::new(Dimension::Angular2Ln(dim))))
            .unwrap();
        assert_eq!(
            properties(&doc, "DimensionAngular2Line")["Center"],
            expected
        );
    }
}

#[test]
fn arc_parallel_and_zero_vector_quirks_stay_in_compatibility_view() {
    for (first, second, expected) in [
        (
            Vector3::new(1.0, 1.0, 1.0),
            Vector3::new(2.0, 2.0, 2.0),
            json!(3.0_f64.sqrt() * std::f64::consts::PI),
        ),
        (
            Vector3::UNIT_X,
            Vector3::ZERO,
            json!("!throws:InvalidOperationException"),
        ),
        (Vector3::UNIT_X, Vector3::UNIT_X, json!(0.0)),
    ] {
        let mut doc = CadDocument::new();
        let mut dim = DimensionArc::default();
        dim.first_extension_point = first;
        dim.second_extension_point = second;
        doc.add_entity(EntityType::Dimension(Box::new(Dimension::Arc(dim))))
            .unwrap();
        assert_eq!(properties(&doc, "DimensionArc")["Measurement"], expected);
    }
}

#[test]
fn tolerance_explicit_direction_and_text_edits_override_source_defaults() {
    let mut doc = tolerance_dxf("11\n1\n21\n0\n31\n0\n1\nfirst^Jsecond\n");
    assert_eq!(
        properties(&doc, "Tolerance")["Direction"],
        json!([1.0, 0.0, 0.0])
    );
    let handle = doc
        .entities()
        .find_map(|e| match e {
            EntityType::Tolerance(t) => Some(t.common.handle),
            _ => None,
        })
        .unwrap();
    let EntityType::Tolerance(t) = doc.get_entity_mut(handle).unwrap() else {
        panic!()
    };
    let mut without_source = t.clone();
    without_source.dxf_source = None;
    assert_eq!(t, &without_source);
    t.text = "edited".into();
    assert_ne!(t, &without_source);
    assert_eq!(properties(&doc, "Tolerance")["Text"], "edited");
}
