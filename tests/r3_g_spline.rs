#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::{dump_document, handle};
use acadrust::entities::{EntityType, Spline};
use acadrust::types::{DxfVersion, Vector3};
use acadrust::CadDocument;
use serde_json::Value;

fn spline_props<'a>(dump: &'a Value, handle_value: acadrust::Handle) -> &'a Value {
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["handle"] == handle(handle_value))
        .map(|object| &object["props"])
        .expect("spline object")
}

#[test]
fn dxf_spline_projection_keeps_raw_flags_and_computed_values() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut spline = Spline::from_control_points(
        3,
        vec![
            Vector3::ZERO,
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(2.0, 1.0, 0.0),
            Vector3::new(3.0, 0.0, 0.0),
        ],
    );
    spline.dxf_flags = 1160; // planar + parameterization/fit bits.
    spline.knot_tolerance = 1e-9;
    spline.control_tolerance = 1e-10;
    spline.fit_tolerance = 1e-10;
    let handle_value = doc.add_entity(EntityType::Spline(spline)).unwrap();

    let dump = dump_document(&doc, "synthetic.dxf", "dxf");
    let props = spline_props(&dump, handle_value);
    assert_eq!(props["ControlPointTolerance"], 1e-10);
    assert_eq!(props["FitTolerance"], 1e-10);
    assert_eq!(props["KnotTolerance"], 1e-9);
    assert_eq!(props["Flags"], 1160);
    assert_eq!(props["Flags1"], 0);
    assert_eq!(props["KnotParametrization"], 0);
    assert_eq!(props["IsClosed"], false);
    assert_eq!(props["IsPeriodic"], false);
    assert_eq!(props["HasValidKnotCount"], true);
}

#[test]
fn dwg_spline_projection_uses_flags1_and_source_version_defaults() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1027);
    let mut spline = Spline::from_fit_points(vec![Vector3::ZERO, Vector3::UNIT_X]);
    spline.flags.closed = true;
    spline.dwg_scenario = Some(2);
    spline.dwg_flags1 = 13; // fit method + closed + use knot parameter.
    spline.knot_parameterization = 2;
    let handle_value = doc.add_entity(EntityType::Spline(spline)).unwrap();

    let dump = dump_document(&doc, "synthetic.dwg", "dwg");
    let props = spline_props(&dump, handle_value);
    assert_eq!(props["Flags"], 1);
    assert_eq!(props["Flags1"], 13);
    assert_eq!(props["ControlPointTolerance"], 1e-7);
    assert_eq!(props["KnotTolerance"], 1e-7);
    assert_eq!(props["FitTolerance"], 0.0);
    assert_eq!(props["KnotParametrization"], 2);
    assert_eq!(props["IsClosed"], true);
    assert_eq!(props["IsPeriodic"], true);
    assert_eq!(props["HasValidKnotCount"], false);

    doc.version = DxfVersion::AC1018;
    let mut control_spline = Spline::from_control_points(
        3,
        vec![
            Vector3::ZERO,
            Vector3::UNIT_X,
            Vector3::new(2.0, 1.0, 0.0),
            Vector3::new(3.0, 0.0, 0.0),
        ],
    );
    control_spline.dwg_scenario = Some(1);
    let control_handle = doc.add_entity(EntityType::Spline(control_spline)).unwrap();
    let dump = dump_document(&doc, "synthetic.dwg", "dwg");
    let props = spline_props(&dump, control_handle);
    assert_eq!(props["Flags1"], 0);
    assert_eq!(props["KnotParametrization"], 15);
    assert_eq!(props["FitTolerance"], 1e-10);

    // ACadSharp seeds Flags.Closed from Flags1 before reading the scenario-1
    // body. A false body bit therefore does not clear a true Flags1 bit.
    doc.version = DxfVersion::AC1027;
    let mut conflicting_spline = Spline::from_control_points(
        3,
        vec![
            Vector3::ZERO,
            Vector3::UNIT_X,
            Vector3::new(2.0, 1.0, 0.0),
            Vector3::new(3.0, 0.0, 0.0),
        ],
    );
    conflicting_spline.dwg_scenario = Some(1);
    conflicting_spline.dwg_flags1 = 4;
    conflicting_spline.flags.closed = false;
    let conflicting_handle = doc
        .add_entity(EntityType::Spline(conflicting_spline))
        .unwrap();
    let dump = dump_document(&doc, "synthetic.dwg", "dwg");
    let props = spline_props(&dump, conflicting_handle);
    assert_eq!(props["Flags"], 1);
    assert_eq!(props["IsClosed"], true);
}

#[test]
fn dwg_spline_flags1_projects_as_ushort() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1027);
    let mut spline = Spline::from_control_points(
        3,
        vec![
            Vector3::ZERO,
            Vector3::UNIT_X,
            Vector3::new(2.0, 1.0, 0.0),
            Vector3::new(3.0, 0.0, 0.0),
        ],
    );
    spline.dwg_scenario = Some(1);
    spline.dwg_flags1 = 0x1_0005;
    let handle_value = doc.add_entity(EntityType::Spline(spline)).unwrap();

    let dump = dump_document(&doc, "synthetic.dwg", "dwg");
    let props = spline_props(&dump, handle_value);
    assert_eq!(props["Flags1"], 5);
    assert_eq!(props["IsClosed"], true);
}

#[test]
fn spline_knot_count_uses_wrapping_csharp_integer_arithmetic() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut spline = Spline::new();
    spline.degree = i32::MIN;
    spline.flags.closed = true;
    spline.control_points = vec![
        Vector3::ZERO,
        Vector3::UNIT_X,
        Vector3::new(2.0, 1.0, 0.0),
        Vector3::new(3.0, 0.0, 0.0),
    ];
    spline.knots = vec![0.0; 5];
    let handle_value = doc.add_entity(EntityType::Spline(spline)).unwrap();

    let dump = dump_document(&doc, "synthetic.dxf", "dxf");
    let props = spline_props(&dump, handle_value);
    assert_eq!(props["Degree"], i32::MIN as i64);
    // 4 + (2 * i32::MIN wrapping to 0) + 1 == 5.
    assert_eq!(props["HasValidKnotCount"], true);
}
