#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::{self as dump, dump_document, schema};
use acadrust::entities::{EntityType, Line};
use acadrust::objects::{Dictionary, ObjectType, XRecord};
use acadrust::types::{Color, DxfVersion, Handle, LineWeight, Transparency, Vector3};
use acadrust::CadDocument;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

fn object(value: &Value, h: Handle) -> &Value {
    value["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["handle"] == dump::handle(h))
        .unwrap()
}
fn assert_schema(ty: &str, value: &Value) {
    let actual: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(actual, schema()[ty], "schema for {ty}");
}
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target.noindex")
            .join(format!(
                "dump-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn canonical_shape_and_schema_order() {
    let doc = CadDocument::new();
    let value = dump_document(&doc, "", "new");
    assert_eq!(value["format"], 1);
    assert_eq!(value["status"], "ok");
    assert!(value["error"].is_null());
    assert_eq!(value["reader"], "new");
    assert_eq!(value["file"], "");
    assert_eq!(value["version"], 33);
    assert_schema("CadHeader", &value["header"]);
    assert_schema("CadSummaryInfo", &value["summaryInfo"]);
    for o in value["objects"].as_array().unwrap() {
        assert_schema(o["type"].as_str().unwrap(), &o["props"]);
    }
    assert_eq!(
        serde_json::from_slice::<Value>(&serde_json::to_vec(&value).unwrap()).unwrap(),
        value
    );
}
#[test]
fn handles_are_sorted_numerically_and_unique() {
    let mut doc = CadDocument::new();
    for h in [0xFF, 0x20, 0xA0] {
        let mut d = Dictionary::new();
        d.handle = Handle::new(h);
        doc.objects.insert(d.handle, ObjectType::Dictionary(d));
    }
    let value = dump_document(&doc, "", "new");
    let handles: Vec<_> = value["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            u64::from_str_radix(o["handle"].as_str().unwrap().trim_start_matches('#'), 16).unwrap()
        })
        .collect();
    assert!(handles.windows(2).all(|p| p[0] < p[1]));
    assert!(handles.contains(&0xFF));
}
#[test]
fn line_geometry_and_common_references() {
    let mut doc = CadDocument::new();
    let mut line = Line::from_points(Vector3::new(1.25, 2.0, -3.0), Vector3::new(4.0, 5.0, 6.0));
    line.thickness = 7.5;
    line.common.line_weight = LineWeight::Value(35);
    line.common.linetype_handle = Some(Handle::NULL);
    line.common.color = Color::Rgb {
        r: 12,
        g: 34,
        b: 56,
    };
    let h = doc.add_entity(EntityType::Line(line)).unwrap();
    let value = dump_document(&doc, "test.dxf", "dxf");
    let o = object(&value, h);
    let p = &o["props"];
    assert_eq!(o["type"], "Line");
    assert_eq!(p["StartPoint"], json!([1.25, 2.0, -3.0]));
    assert_eq!(p["EndPoint"], json!([4.0, 5.0, 6.0]));
    assert_eq!(p["Thickness"], 7.5);
    assert_eq!(p["Normal"], json!([0.0, 0.0, 1.0]));
    assert_eq!(
        p["Layer"],
        dump::handle(doc.layers.get("0").unwrap().handle)
    );
    assert_eq!(p["LineWeight"], 35);
    assert_eq!(
        p["LineType"],
        dump::handle(doc.line_types.get("ByLayer").unwrap().handle)
    );
    assert_schema("Color", &p["Color"]);
    assert_eq!(p["Color"]["TrueColor"], 0x0C2238);
}
#[test]
fn unavailable_values_are_explicit() {
    let doc = CadDocument::new();
    let value = dump_document(&doc, "", "new");
    assert_eq!(value["summaryInfo"]["CreatedDate"], "!unmapped");
    assert_eq!(value["summaryInfo"]["ModifiedDate"], "!unmapped");
    let block = doc.block_records.get("*Model_Space").unwrap();
    assert_eq!(
        object(&value, block.block_entity_handle)["props"]["Color"],
        "!unmapped"
    );
}
#[test]
fn scalar_and_color_formatting() {
    assert_eq!(dump::handle(Handle::new(0x00ABC)), "#ABC");
    assert_eq!(dump::handle(Handle::NULL), "#0");
    assert_eq!(dump::bytes(&[0, 1, 15, 128, 255]), "00010f80ff");
    assert_eq!(dump::character('A'), 65);
    assert_eq!(dump::character('😀'), 0xD83D);
    assert_eq!(dump::number(f64::NAN), "NaN");
    assert_eq!(dump::number(f64::INFINITY), "Infinity");
    assert_eq!(dump::number(f64::NEG_INFINITY), "-Infinity");
    assert_eq!(
        dump::number(1.2345678901234567).as_f64(),
        Some(1.2345678901234567)
    );
    assert_eq!(dump::color(Color::ByLayer)["Index"], 256);
    assert_eq!(dump::color(Color::ByBlock)["TrueColor"], -1);
    assert_eq!(dump::color(Color::Index(1))["R"], 255);
    assert_eq!(dump::color(Color::Index(12))["R"], 165);
    assert_eq!(dump::color(Color::Index(145))["B"], 217);
    assert_eq!(dump::color(Color::None)["R"], 255);
    assert_eq!(dump::transparency(Transparency::ByLayer)["Value"], -1);
    assert_eq!(dump::transparency(Transparency::ByBlock)["Value"], 100);
    assert_eq!(dump::transparency(Transparency::Explicit(128))["Value"], 50);
}
#[test]
fn version_enums_are_acadsharp_values() {
    for (v, n) in [
        (DxfVersion::Unknown, -1),
        (DxfVersion::AC1009, 9),
        (DxfVersion::AC1012, 19),
        (DxfVersion::AC1014, 21),
        (DxfVersion::AC1015, 23),
        (DxfVersion::AC1018, 25),
        (DxfVersion::AC1021, 27),
        (DxfVersion::AC1024, 29),
        (DxfVersion::AC1027, 31),
        (DxfVersion::AC1032, 33),
    ] {
        assert_eq!(dump::version(v), n);
    }
}
#[test]
fn header_dates_are_julian_days_and_nonfinite_values_survive() {
    let mut doc = CadDocument::new();
    doc.header.create_date_julian = 2451545.25;
    doc.header.update_date_julian = 2451546.5;
    doc.header.total_editing_time = 1.75;
    doc.header.point_display_size = f64::INFINITY;
    let value = dump_document(&doc, "", "new");
    assert_eq!(value["header"]["CreateDateTime"], 2451545.25);
    assert_eq!(value["header"]["UpdateDateTime"], 2451546.5);
    assert_eq!(value["header"]["TotalEditingTime"], 1.75);
    assert_eq!(value["header"]["PointDisplaySize"], "Infinity");
}
#[test]
fn classes_summary_and_dictionary_order() {
    let mut doc = CadDocument::new();
    doc.classes
        .add_or_update(acadrust::classes::DxfClass::new("CUSTOM", "AcDbCustom"));
    doc.summary_info.custom_properties =
        vec![("z".into(), "last".into()), ("a".into(), "first".into())];
    let mut dict = Dictionary::new();
    dict.handle = Handle::new(0x100);
    dict.owner = Handle::new(0x101);
    dict.entries = vec![
        ("z".into(), Handle::new(0xAB)),
        ("a".into(), Handle::new(0xCD)),
    ];
    doc.objects
        .insert(dict.handle, ObjectType::Dictionary(dict));
    let value = dump_document(&doc, "", "new");
    assert_schema("DxfClass", &value["classes"][0]);
    assert_eq!(
        value["summaryInfo"]["Properties"],
        json!([["z", "last"], ["a", "first"]])
    );
    let d = object(&value, Handle::new(0x100));
    assert_eq!(d["owner"], "#101");
    assert_eq!(d["props"]["EntryNames"], json!(["z", "a"]));
    assert_eq!(d["props"]["EntryHandles"], json!([0xAB, 0xCD]));
}
#[test]
fn xrecord_values_and_schema() {
    let mut doc = CadDocument::new();
    let mut x = XRecord::new();
    x.handle = Handle::new(0x1234);
    x.add_double(40, f64::NAN);
    x.add_string(1, "kept");
    let layer_handle = doc.layers.get("0").unwrap().handle;
    x.add_handle(330, layer_handle);
    x.add_handle(330, Handle::new(0xFFFF));
    doc.objects.insert(x.handle, ObjectType::XRecord(x));
    let value = dump_document(&doc, "", "new");
    let o = object(&value, Handle::new(0x1234));
    assert_schema("XRecord", &o["props"]);
    let entries = o["props"]["Entries"].as_array().unwrap();
    assert_eq!(entries[0]["Value"], "NaN");
    assert_eq!(entries[1]["Value"], "kept");
    assert_eq!(entries[2]["Value"], dump::handle(layer_handle));
    assert_eq!(entries[3]["Value"], 0xFFFF);
}
#[test]
fn cli_new_document() {
    let dir = Scratch::new();
    let output = dir.0.join("new.json");
    let result = Command::new(env!("CARGO_BIN_EXE_acad-dump"))
        .args(["dump", "--new-document"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let value: Value = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
    assert_eq!(value, dump_document(&dump::new_document(), "", "new"));
    assert!(String::from_utf8(result.stdout).unwrap().contains("\t"));
}
#[test]
fn cli_usage_and_io_errors_exit_two() {
    let dir = Scratch::new();
    assert_eq!(
        Command::new(env!("CARGO_BIN_EXE_acad-dump"))
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        Command::new(env!("CARGO_BIN_EXE_acad-dump"))
            .arg("dump")
            .arg(dir.0.join("missing.dwg"))
            .arg(dir.0.join("out.json"))
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );
}
#[test]
fn cli_reader_error_is_a_successfully_written_error_dump() {
    let dir = Scratch::new();
    let input = dir.0.join("invalid.dxf");
    let output = dir.0.join("out.json");
    // A truncated binary sentinel fails reader construction, even in failsafe mode.
    std::fs::write(&input, b"AutoCAD Binary DXF").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_acad-dump"))
        .arg("dump")
        .arg(&input)
        .arg(&output)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(0));
    let value: Value = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
    assert_eq!(value["status"], "error");
    assert!(value["error"].as_str().unwrap().contains(": "));
    assert!(value.get("objects").is_none());
    assert!(value.get("header").is_none());
}

#[test]
fn failsafe_dwg_recovery_keeps_the_returned_partial_document() {
    let dir = Scratch::new();
    let input = dir.0.join("partial.dwg");
    std::fs::write(&input, b"not a CAD document").unwrap();
    let value = dump::dump_file(&input);
    assert_eq!(value["status"], "ok");
    assert!(value["objects"].is_array());
    assert!(value["notifications"]["Error"].as_u64().unwrap() > 0);
}

#[test]
fn common_entity_variants_have_exact_schema_and_nested_members() {
    let entities = vec![
        EntityType::Point(Default::default()),
        EntityType::Line(Default::default()),
        EntityType::Circle(Default::default()),
        EntityType::Arc(Default::default()),
        EntityType::Ellipse(Default::default()),
        EntityType::Polyline(Default::default()),
        EntityType::Polyline2D(Default::default()),
        EntityType::Polyline3D(Default::default()),
        EntityType::LwPolyline(Default::default()),
        EntityType::Text(Default::default()),
        EntityType::MText(Default::default()),
        EntityType::Spline(Default::default()),
        EntityType::Helix(Default::default()),
        EntityType::Hatch(Default::default()),
        EntityType::BlockEnd(Default::default()),
        EntityType::Ray(Default::default()),
        EntityType::XLine(Default::default()),
        EntityType::Viewport(Default::default()),
        EntityType::AttributeDefinition(Default::default()),
        EntityType::AttributeEntity(Default::default()),
        EntityType::Leader(Default::default()),
        EntityType::MultiLeader(Default::default()),
        EntityType::MLine(Default::default()),
        EntityType::Mesh(Default::default()),
        EntityType::RasterImage(Default::default()),
        EntityType::Solid3D(Default::default()),
        EntityType::Region(Default::default()),
        EntityType::Body(Default::default()),
        EntityType::Table(Default::default()),
        EntityType::Tolerance(Default::default()),
        EntityType::PolyfaceMesh(Default::default()),
        EntityType::Wipeout(Default::default()),
        EntityType::Shape(Default::default()),
        EntityType::Underlay(Default::default()),
        EntityType::Seqend(Default::default()),
        EntityType::Ole2Frame(Default::default()),
        EntityType::PolygonMesh(Default::default()),
        EntityType::Light(Default::default()),
        EntityType::SectionSymbol(Default::default()),
        EntityType::ViewBorder(Default::default()),
    ];
    let count = entities.len();
    let mut doc = CadDocument::new();
    let mut handles = Vec::new();
    for e in entities {
        handles.push(doc.add_entity(e).unwrap());
    }
    let value = dump_document(&doc, "", "new");
    assert!(count >= 35);
    for h in handles {
        let o = object(&value, h);
        assert_schema(o["type"].as_str().unwrap(), &o["props"]);
        check_nested(&o["props"]);
    }
}
fn check_nested(v: &Value) {
    match v {
        Value::Object(map) => {
            let keys: Vec<_> = map.keys().map(String::as_str).collect();
            assert!(
                schema().values().any(|members| members == &keys),
                "unknown nested schema: {keys:?}"
            );
            for value in map.values() {
                check_nested(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                check_nested(value);
            }
        }
        _ => {}
    }
}

#[test]
fn common_object_variants_have_exact_nested_schema() {
    let mut doc = CadDocument::new();
    let mut material = acadrust::objects::Material::default();
    material.diffuse_map.transform[0] = f64::NAN;
    let mut table_style = acadrust::objects::TableStyle::default();
    table_style.modern_overrides.push((
        1,
        acadrust::objects::NamedTableCellStyle {
            name: "custom".into(),
            ..Default::default()
        },
    ));
    let mut table = acadrust::entities::Table::new(Vector3::ZERO, 1, 1);
    table.rows[0].cells[0] = acadrust::entities::table::TableCell::text("cell");
    table.rows[0].cells[0]
        .custom_data_items
        .push(acadrust::entities::table::TableCustomData {
            name: "data".into(),
            value: acadrust::entities::table::CellValue::text("kept"),
        });
    let values = vec![
        ObjectType::Material(material),
        ObjectType::TableStyle(table_style),
        ObjectType::MultiLeaderStyle(Default::default()),
        ObjectType::MLineStyle(Default::default()),
        ObjectType::PlotSettings(Default::default()),
        ObjectType::VisualStyle(Default::default()),
        ObjectType::Scale(Default::default()),
        ObjectType::TableContent(table),
    ];
    for (i, v) in values.into_iter().enumerate() {
        doc.objects.insert(Handle::new(0x1000 + i as u64), v);
    }
    let value = dump_document(&doc, "", "new");
    for i in 0..8 {
        let o = object(&value, Handle::new(0x1000 + i));
        assert_schema(o["type"].as_str().unwrap(), &o["props"]);
        check_nested(&o["props"]);
    }
    assert_eq!(
        object(&value, Handle::new(0x1000))["props"]["DiffuseMatrix"]["M00"],
        "NaN"
    );
}

#[test]
fn opaque_types_keep_known_identity_and_unknown_schema() {
    let mut doc = CadDocument::new();
    for (i, name) in ["AEC_DISP_REP_2D_SECTION", "AEC_WALL_STYLE"]
        .iter()
        .enumerate()
    {
        let h = Handle::new(0x5000 + i as u64);
        doc.objects.insert(
            h,
            ObjectType::Unknown {
                type_name: name.to_string(),
                handle: h,
                owner: Handle::NULL,
                raw_dxf_codes: None,
                raw_dwg_data: None,
                raw_dwg_handle_bits: 0,
                raw_dwg_version: None,
            },
        );
    }
    let h = doc
        .add_entity(EntityType::Unknown(acadrust::entities::UnknownEntity::new(
            "AEC_WALL",
        )))
        .unwrap();
    let value = dump_document(&doc, "", "new");
    let unknown = object(&value, Handle::new(0x5000));
    assert_eq!(unknown["type"], "UnknownNonGraphicalObject");
    assert_schema("UnknownNonGraphicalObject", &unknown["props"]);
    let wall_style = object(&value, Handle::new(0x5001));
    assert_eq!(wall_style["type"], "AecWallStyle");
    assert_schema("AecWallStyle", &wall_style["props"]);
    assert_eq!(wall_style["props"]["ObjectName"], "AEC_WALL_STYLE");
    assert_eq!(object(&value, h)["type"], "Wall");
}

#[test]
fn partial_child_identities_preserve_known_vertices_without_invented_defaults() {
    let mut doc = CadDocument::new();
    let parent = doc
        .add_entity(EntityType::Polyline3D(Default::default()))
        .unwrap();
    let EntityType::Polyline3D(polyline) = doc.get_entity_mut(parent).unwrap() else {
        panic!("expected polyline");
    };
    let mut known = acadrust::entities::Vertex3DPolyline::from_xyz(1.0, 2.0, 3.0);
    known.handle = Handle::new(0x6000);
    polyline.vertices = vec![known, Default::default()];
    let value = dump_document(&doc, "", "new");
    let vertex = object(&value, Handle::new(0x6000));
    assert_eq!(vertex["owner"], dump::handle(parent));
    assert_eq!(vertex["props"]["Location"], json!([1.0, 2.0, 3.0]));
    assert_eq!(vertex["props"]["Color"], "!unmapped");
    assert_eq!(object(&value, parent)["props"]["Vertices"], "!unmapped");
}

#[test]
fn mtext_height_and_alignment_use_the_retained_fields() {
    let mut doc = CadDocument::new();
    let mut mtext = acadrust::entities::MText::new();
    mtext.height = 2.5;
    mtext.extents_height = 7.0;
    mtext.insertion_point = Vector3::new(4.0, 5.0, 6.0);
    mtext.dwg_x_direction = Some(Vector3::new(0.0, 2.0, 0.0));
    let h = doc.add_entity(EntityType::MText(Box::new(mtext))).unwrap();
    let value = dump_document(&doc, "", "new");
    let p = &object(&value, h)["props"];
    assert_eq!(p["Height"], 2.5);
    assert_eq!(p["VerticalHeight"], 7.0);
    assert_eq!(p["AlignmentPoint"], json!([0.0, 2.0, 0.0]));
    assert_eq!(p["InsertPoint"], json!([4.0, 5.0, 6.0]));
    assert_eq!(p["RectangleHeight"], "!unmapped");
}
