// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::dump_document;
use acadrust::entities::hatch::*;
use acadrust::entities::{EntityType, Line};
use acadrust::types::{Color, DxfVersion, Handle, Vector2, Vector3};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader};
use serde_json::{json, Value};
use std::io::Cursor;

fn dxf(codes: &str) -> CadDocument {
    let text = format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nHATCH\n5\n100\n100\nAcDbEntity\n100\nAcDbHatch\n{codes}0\nENDSEC\n0\nEOF\n");
    DxfReader::from_reader(Cursor::new(text.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

fn native(doc: &CadDocument) -> &Hatch {
    doc.entities()
        .find_map(|e| match e {
            EntityType::Hatch(h) => Some(h.as_ref()),
            _ => None,
        })
        .unwrap()
}

fn projected(doc: &CadDocument) -> Value {
    dump_document(doc, "", "new")["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["type"] == "Hatch")
        .unwrap()["props"]
        .clone()
}

#[test]
fn dxf_defaults_are_distinct_from_explicit_values_and_native_state() {
    let doc = dxf("");
    let p = projected(&doc);
    assert_eq!(native(&doc).pattern_scale, 1.0);
    assert!(native(&doc).is_solid);
    assert_eq!(p["PatternScale"], 0.0);
    assert_eq!(p["PatternType"], 0);
    assert_eq!(p["IsSolid"], false);
    assert_eq!(p["GradientColor"]["Name"], Value::Null);
    let doc = dxf("41\n1\n70\n1\n76\n1\n450\n0\n470\n\n");
    let p = projected(&doc);
    assert_eq!(p["PatternScale"], 1.0);
    assert_eq!(p["PatternType"], 1);
    assert_eq!(p["IsSolid"], true);
    assert_eq!(p["GradientColor"]["Name"], "");
    let mut without_source = native(&doc).clone();
    without_source.dxf_source = None;
    assert_eq!(native(&doc), &without_source);
    without_source.pixel_size = 2.0;
    assert_ne!(native(&doc), &without_source);
}

#[test]
fn dxf_elevation_seed_style_and_pattern_trailer_are_projected_only() {
    let doc = dxf("10\n2\n20\n3\n30\n4\n75\n2\n78\n1\n53\n90\n43\n5\n44\n6\n45\n7\n46\n8\n79\n2\n49\n1.5\n49\n-2\n47\n0.25\n98\n1\n10\n9\n20\n10\n");
    let h = native(&doc);
    assert_eq!(h.seed_points, vec![Vector2::new(9.0, 10.0)]);
    assert_eq!(h.style, HatchStyleType::Ignore);
    assert_eq!(h.pixel_size, 0.25);
    let p = projected(&doc);
    assert_eq!(p["Elevation"], 4.0);
    assert_eq!(p["SeedPoints"], json!([[2.0, 3.0], [9.0, 10.0]]));
    assert_eq!(p["Style"], 0);
    assert_eq!(p["PixelSize"], 0.0);
    assert_eq!(
        p["Pattern"],
        json!({"Description":null,"Name":"SOLID","Lines":[{
            "Angle":std::f64::consts::FRAC_PI_2,"BasePoint":[5.0,6.0],"Offset":[7.0,8.0],"DashLengths":[1.5,-2.0]
        }]})
    );
    // A pixel size before the pattern is assigned normally by both readers.
    assert_eq!(projected(&dxf("47\n0.25\n78\n0\n"))["PixelSize"], 0.25);
}

#[test]
fn dxf_gradient_keeps_rgb_native_and_aci_in_reference_view() {
    let doc = dxf("450\n1\n460\n0.5\n461\n0.75\n452\n1\n462\n0.25\n453\n3\n463\n0\n63\n5\n421\n66051\n463\n0.5\n421\n1193046\n63\n2\n463\n1\n421\n16711680\n470\nLINEAR\n");
    assert_eq!(
        native(&doc).gradient_color.colors[0].color,
        Color::Rgb { r: 1, g: 2, b: 3 }
    );
    let p = &projected(&doc)["GradientColor"];
    assert_eq!(p["Colors"][0]["Color"]["Index"], 5);
    assert_eq!(p["Colors"][1]["Color"]["Index"], 2);
    assert_eq!(p["Colors"][2]["Color"]["Index"], 0);
    assert_eq!(p["Colors"][1]["Value"], 0.5);
    assert_eq!(p["Enabled"], true);
    assert_eq!(p["IsSingleColorGradient"], true);
    assert_eq!(p["Angle"], 0.5);
    assert_eq!(p["Shift"], 0.75);
    assert_eq!(p["ColorTint"], 0.25);
    assert_eq!(p["Name"], "LINEAR");
}

#[test]
fn all_boundary_edges_and_resolved_ordered_unique_entity_references() {
    let mut doc = CadDocument::new();
    let first = doc.add_entity(EntityType::Line(Line::default())).unwrap();
    let second = doc.add_entity(EntityType::Line(Line::default())).unwrap();
    let mut hatch = Hatch::new();
    let mut path = BoundaryPath::new();
    path.flags = BoundaryPathFlags::EXTERNAL;
    path.boundary_handles = vec![
        second,
        first,
        second,
        Handle::new(0xDEAD),
        doc.layers.get("0").unwrap().handle,
    ];
    path.edges = vec![
        BoundaryEdge::Line(LineEdge {
            start: Vector2::new(1.0, 2.0),
            end: Vector2::new(3.0, 4.0),
        }),
        BoundaryEdge::CircularArc(CircularArcEdge {
            center: Vector2::new(2.0, 3.0),
            radius: 4.0,
            start_angle: 0.5,
            end_angle: 2.5,
            counter_clockwise: true,
        }),
        BoundaryEdge::EllipticArc(EllipticArcEdge {
            center: Vector2::ZERO,
            major_axis_endpoint: Vector2::new(5.0, 0.0),
            minor_axis_ratio: 0.5,
            start_angle: 0.25,
            end_angle: 2.0,
            counter_clockwise: false,
        }),
        BoundaryEdge::Spline(SplineEdge {
            degree: 2,
            rational: true,
            periodic: false,
            knots: vec![0.0, 1.0],
            control_points: vec![Vector3::new(1.0, 2.0, 0.75)],
            fit_points: vec![Vector2::new(3.0, 4.0)],
            start_tangent: Vector2::new(1.0, 0.0),
            end_tangent: Vector2::new(0.0, 1.0),
        }),
        BoundaryEdge::Polyline(PolylineEdge {
            vertices: vec![Vector3::new(6.0, 7.0, 0.5)],
            is_closed: true,
        }),
    ];
    hatch.paths.push(path);
    doc.add_entity(EntityType::Hatch(Box::new(hatch))).unwrap();
    let p = projected(&doc);
    let path = &p["Paths"][0];
    assert_eq!(
        path["Entities"],
        json!([
            format!("#{:X}", second.value()),
            format!("#{:X}", first.value())
        ])
    );
    assert_eq!(path["Flags"], 3);
    assert_eq!(native(&doc).paths[0].flags.bits(), 1);
    assert_eq!(
        path["Edges"],
        json!([
            {"Start":[1.0,2.0],"End":[3.0,4.0]},
            {"Center":[2.0,3.0],"Radius":4.0,"StartAngle":0.5,"EndAngle":2.5,"CounterClockWise":true},
            {"Center":[0.0,0.0],"MajorAxisEndPoint":[5.0,0.0],"RadiusRatio":0.5,"StartAngle":0.25,"EndAngle":2.0,"CounterClockWise":false},
            {"Degree":2,"IsRational":true,"IsPeriodic":false,"Knots":[0.0,1.0],"ControlPoints":[[1.0,2.0,0.75]],"FitPoints":[[3.0,4.0]],"StartTangent":[1.0,0.0],"EndTangent":[0.0,1.0]},
            {"IsClosed":true,"Vertices":[[6.0,7.0,0.5]]}
        ])
    );
}

#[test]
fn dwg_solid_scale_and_gradient_version_presence_preserve_native_values() {
    for version in [DxfVersion::AC1014, DxfVersion::AC1018, DxfVersion::AC1032] {
        let mut doc = CadDocument::with_version(version);
        let mut hatch = Hatch::solid();
        hatch.style = HatchStyleType::Outer;
        let mut path = BoundaryPath::new();
        path.edges.push(BoundaryEdge::Spline(SplineEdge {
            degree: 1,
            rational: false,
            periodic: false,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Vector3::new(1.0, 2.0, 1.0), Vector3::new(3.0, 4.0, 1.0)],
            fit_points: vec![],
            start_tangent: Vector2::ZERO,
            end_tangent: Vector2::ZERO,
        }));
        hatch.paths.push(path);
        hatch.gradient_color.name = "LINEAR".into();
        hatch
            .gradient_color
            .add_color(0.25, Color::Rgb { r: 1, g: 2, b: 3 });
        doc.add_entity(EntityType::Hatch(Box::new(hatch))).unwrap();
        let bytes = DwgWriter::write_to_vec(&doc).unwrap();
        let doc = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
        assert_eq!(native(&doc).pattern_scale, 1.0);
        let p = projected(&doc);
        assert_eq!(p["PatternScale"], 0.0);
        assert_eq!(p["Style"], 1);
        let BoundaryEdge::Spline(spline) = &native(&doc).paths[0].edges[0] else {
            panic!("spline")
        };
        assert_eq!(spline.control_points[0].z, 1.0);
        assert_eq!(
            p["Paths"][0]["Edges"][0]["ControlPoints"],
            json!([[1.0, 2.0, 0.0], [3.0, 4.0, 0.0]])
        );
        if version < DxfVersion::AC1018 {
            assert_eq!(p["GradientColor"]["Name"], Value::Null);
            assert_eq!(p["GradientColor"]["Colors"], json!([]));
        } else {
            assert_eq!(p["GradientColor"]["Name"], "LINEAR");
            assert_eq!(
                p["GradientColor"]["Colors"][0]["Color"]["TrueColor"],
                197121
            );
        }
    }
}

#[test]
fn legacy_roundtrip_gradient_recovery_remains_native_only() {
    use acadrust::objects::{Dictionary, ObjectType, XRecord};
    use acadrust::xdata::{ExtendedDataRecord, XDataValue};

    let mut document = CadDocument::with_version(DxfVersion::AC1015);
    let mut hatch = Hatch::solid();
    hatch.common.xdictionary_handle = Some(Handle::new(0x900));
    for (name, index) in [("GradientColor1ACI", 5), ("GradientColor2ACI", 2)] {
        document
            .app_ids
            .add(acadrust::tables::AppId::new(name))
            .unwrap();
        let mut record = ExtendedDataRecord::new(name);
        record.add_value(XDataValue::Integer16(index));
        hatch.common.extended_data.add_record(record);
    }
    let hatch_handle = document
        .add_entity(EntityType::Hatch(Box::new(hatch)))
        .unwrap();
    let mut dictionary = Dictionary::new();
    dictionary.handle = Handle::new(0x900);
    dictionary.owner = hatch_handle;
    dictionary.add_entry("ACAD_XREC_ROUNDTRIP", Handle::new(0x901));
    let mut record = XRecord::new();
    record.handle = Handle::new(0x901);
    record.owner = dictionary.handle;
    record.add_string(1, "LINEAR");
    document
        .objects
        .insert(dictionary.handle, ObjectType::Dictionary(dictionary));
    document
        .objects
        .insert(record.handle, ObjectType::XRecord(record));
    let dxf = acadrust::DxfWriter::new(&document).write_to_vec().unwrap();
    let dwg = DwgWriter::write_to_vec(&document).unwrap();
    for doc in [
        DxfReader::from_reader(Cursor::new(dxf))
            .unwrap()
            .read()
            .unwrap(),
        DwgReader::from_stream(Cursor::new(dwg)).read().unwrap(),
    ] {
        let h = native(&doc);
        assert!(h.gradient_color.enabled);
        assert_eq!(h.gradient_color.colors.len(), 2);
        assert_eq!(h.gradient_color.name, "LINEAR");
        assert!(h.gradient_recovery_source.is_some());
        let mut without_source = h.clone();
        without_source.gradient_recovery_source = None;
        assert_eq!(h, &without_source);
        assert_eq!(
            projected(&doc)["GradientColor"],
            json!({
                "Enabled":false,"Name":null,"Colors":[],"Angle":0.0,
                "Shift":0.0,"ColorTint":0.0,"IsSingleColorGradient":false
            })
        );
    }
}

#[test]
fn dxf_elliptic_edge_angles_are_degrees_in_the_file_and_radians_in_the_model() {
    // 91 paths, 92 flags, 93 edges, 72 edge type (3 = ellipse), 10/20 centre,
    // 11/21 major axis end, 40 ratio, 50/51 start/end angle, 73 direction.
    let doc = dxf("91\n1\n92\n1\n93\n1\n72\n3\n10\n1\n20\n2\n11\n3\n21\n0\n40\n0.5\n50\n90\n51\n472.5\n73\n1\n97\n0\n75\n0\n76\n1\n98\n0\n");
    let path = &native(&doc).paths[0];
    let BoundaryEdge::EllipticArc(edge) = &path.edges[0] else {
        panic!("elliptic edge");
    };
    assert!((edge.start_angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    // Not normalised: 472.5 degrees, like the reference.
    assert!((edge.end_angle - 472.5_f64.to_radians()).abs() < 1e-12);
    let p = projected(&doc);
    assert!((p["Paths"][0]["Edges"][0]["EndAngle"].as_f64().unwrap() - 472.5_f64.to_radians()).abs() < 1e-9);
}
