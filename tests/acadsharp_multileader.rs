// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::dump_document;
use acadrust::entities::multileader::*;
use acadrust::entities::{AttributeDefinition, EntityType};
use acadrust::objects::{ObjectContextData, ObjectContextKind, ObjectType};
use acadrust::types::{Color, DxfVersion, Handle, LineWeight, Vector3};
use acadrust::{CadDocument, DxfReader};
use serde_json::{json, Value};
use std::io::Cursor;

fn dxf(codes: &str) -> CadDocument {
    let text = format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nMULTILEADER\n5\n100\n100\nAcDbEntity\n100\nAcDbMLeader\n{codes}0\nENDSEC\n0\nEOF\n");
    DxfReader::from_reader(Cursor::new(text.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

fn native(doc: &CadDocument) -> &MultiLeader {
    doc.entities()
        .find_map(|e| match e {
            EntityType::MultiLeader(v) => Some(v.as_ref()),
            _ => None,
        })
        .unwrap()
}

fn projected(doc: &CadDocument, kind: &str) -> Value {
    dump_document(doc, "", "new")["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["type"] == kind)
        .unwrap()["props"]
        .clone()
}

#[test]
fn dxf_presence_distinguishes_omitted_defaults_and_nested_codes() {
    let empty = dxf("300\nCONTEXT_DATA{\n140\n6\n42\n7\n301\n}\n");
    let n = native(&empty);
    assert_eq!(n.arrowhead_size, 0.18);
    assert!(n.enable_dogleg && n.enable_landing);
    assert_eq!(n.context.arrowhead_size, 6.0);
    let p = projected(&empty, "MultiLeader");
    for field in [
        "ArrowheadSize",
        "LandingDistance",
        "ContentType",
        "PathType",
        "TextAttachmentPoint",
        "TextLeftAttachment",
        "TextRightAttachment",
        "TextTopAttachment",
        "TextBottomAttachment",
    ] {
        assert_eq!(p[field].as_f64(), Some(0.0), "{field}");
    }
    assert_eq!(p["EnableDogleg"], false);
    assert_eq!(p["EnableLanding"], false);
    assert_eq!(p["ContextData"], "#0");
    let explicit = dxf("42\n0.18\n41\n0.36\n172\n2\n170\n1\n179\n2\n173\n2\n95\n2\n272\n9\n273\n9\n290\n1\n291\n1\n");
    let p = projected(&explicit, "MultiLeader");
    assert_eq!(p["ArrowheadSize"], 0.18);
    assert_eq!(p["LandingDistance"], 0.36);
    assert_eq!(p["ContentType"], 2);
    assert_eq!(p["PathType"], 1);
    assert_eq!(p["TextAttachmentPoint"], 2);
    assert_eq!(p["TextTopAttachment"], 9);
    assert_eq!(p["TextBottomAttachment"], 9);
    assert_eq!(p["EnableDogleg"], true);
    assert_eq!(p["EnableLanding"], true);
    let mut clone = n.clone();
    clone.dxf_source = None;
    assert_eq!(&clone, n, "source provenance is not native drawing state");
}

#[test]
fn dxf_ignored_colors_attributes_and_angle_conversion_preserve_native_values() {
    let doc = dxf("91\n-1039727357\n92\n-1039464185\n93\n-1039266806\n43\n90\n342\nABC\n344\nDEF\n330\n555\n177\n7\n44\n2.5\n302\nlabel\n");
    let n = native(&doc);
    assert_eq!(n.line_color, Color::Rgb { r: 7, g: 5, b: 3 });
    assert_eq!(n.block_attributes.len(), 1);
    assert_eq!(n.block_attributes[0].text, "label");
    assert_eq!(n.block_rotation, 90.0);
    assert_eq!(n.arrowhead_handle, Some(Handle::new(0xABC)));
    let p = projected(&doc, "MultiLeader");
    assert_eq!(p["LineColor"]["Index"], 256);
    assert_eq!(p["TextColor"]["Index"], 0);
    assert_eq!(p["BlockContentColor"]["Index"], 0);
    assert_eq!(p["BlockAttributes"], json!([]));
    assert_eq!(p["Arrowhead"], Value::Null);
    assert_eq!(p["BlockContent"], Value::Null);
    assert_eq!(p["BlockContentRotation"], std::f64::consts::FRAC_PI_2);
}

#[test]
fn overrides_references_and_block_attributes_keep_order_and_exact_schema() {
    let mut doc = CadDocument::new();
    let definition = doc
        .add_entity(EntityType::AttributeDefinition(AttributeDefinition::new(
            "TAG".into(),
            "Prompt".into(),
            "value".into(),
        )))
        .unwrap();
    let mut leader = MultiLeader::new();
    leader.property_override_flags = MultiLeaderPropertyOverrideFlags::from_bits_retain(0x84000001);
    leader.line_weight = LineWeight::ByBlock;
    leader.line_color = Color::Rgb { r: 1, g: 2, b: 3 };
    leader.text_align_in_ipe = 7;
    leader.text_direction_negative = true;
    leader.extend_leader_to_text = true;
    leader.block_attributes = vec![
        BlockAttribute {
            attribute_definition_handle: Some(definition),
            index: 8,
            width: 2.5,
            text: "second".into(),
        },
        BlockAttribute {
            attribute_definition_handle: Some(Handle::new(0xFFFF)),
            index: 3,
            width: 0.5,
            text: "first".into(),
        },
    ];
    doc.add_entity(EntityType::MultiLeader(Box::new(leader)))
        .unwrap();
    let p = projected(&doc, "MultiLeader");
    assert_eq!(p["PropertyOverrideFlags"], 0x84000001u32 as i32);
    assert_eq!(p["LeaderLineWeight"], -2);
    assert_eq!(p["LineColor"]["TrueColor"], 0x030201);
    assert_eq!(p["TextAligninIPE"], 7);
    assert_eq!(p["TextDirectionNegative"], true);
    assert_eq!(p["ExtendedToText"], true);
    assert_eq!(p["ContextData"], "#0");
    assert_eq!(
        p["BlockAttributes"],
        json!([
            {"AttributeDefinition":format!("#{:X}", definition.value()),"Index":8,"Text":"second","Width":2.5},
            {"AttributeDefinition":null,"Index":3,"Text":"first","Width":0.5}
        ])
    );
    assert!(!p.as_object().unwrap().values().any(|v| v == "!unmapped"));
}

#[test]
fn pre_2010_dwg_attachments_use_reference_defaults_without_native_mutation() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1021);
    doc.dwg_source_version = Some(DxfVersion::AC1021);
    doc.add_entity(EntityType::MultiLeader(Box::new(MultiLeader::new())))
        .unwrap();
    let p = projected(&doc, "MultiLeader");
    assert_eq!(p["TextTopAttachment"], 0);
    assert_eq!(p["TextBottomAttachment"], 0);
    assert_eq!(
        native(&doc).text_top_attachment,
        TextAttachmentType::CenterOfText
    );
    doc.dwg_source_version = Some(DxfVersion::AC1024);
    assert_eq!(projected(&doc, "MultiLeader")["TextTopAttachment"], 9);
}

#[test]
fn standalone_context_projects_ordered_nested_schema_and_rgb_packing() {
    let mut doc = CadDocument::new();
    let pair = StartEndPointPair {
        start_point: Vector3::new(1.0, 2.0, 3.0),
        end_point: Vector3::new(4.0, 5.0, 6.0),
    };
    let mut line = LeaderLine::new(9);
    line.line_color = Color::Rgb { r: 1, g: 2, b: 3 };
    line.points = vec![pair.end_point, pair.start_point];
    line.break_infos = vec![
        LeaderLineBreakInfo {
            segment_index: 4,
            break_points: vec![pair.clone()],
        },
        LeaderLineBreakInfo {
            segment_index: 1,
            break_points: vec![],
        },
    ];
    let mut root = LeaderRoot::new(6);
    root.lines = vec![line, LeaderLine::new(2)];
    root.break_points = vec![pair];
    let mut context = MultiLeaderAnnotContext::new();
    context.leader_roots = vec![root, LeaderRoot::new(1)];
    context.column_sizes = vec![5.0, 3.0];
    context.text_color = Color::Rgb { r: 4, g: 5, b: 6 };
    let h = doc.allocate_handle();
    doc.objects.insert(
        h,
        ObjectType::ObjectContextData(ObjectContextData {
            handle: h,
            owner_handle: Handle::NULL,
            reactors: vec![],
            xdictionary_handle: None,
            class_version: 4,
            is_default: true,
            scale: Handle::NULL,
            kind: ObjectContextKind::MLeader(context),
        }),
    );
    let p = projected(&doc, "MultiLeaderObjectContextData");
    assert_eq!(p["TextColor"]["TrueColor"], 0x060504);
    assert_eq!(p["ColumnSizes"], json!([5.0, 3.0]));
    let roots = &p["LeaderRoots"];
    assert_eq!(roots[0]["LeaderIndex"], 6);
    assert_eq!(roots[1]["LeaderIndex"], 1);
    let lines = &roots[0]["Lines"];
    assert_eq!(lines[0]["Index"], 9);
    assert_eq!(lines[1]["Index"], 2);
    assert_eq!(lines[0]["LineColor"]["TrueColor"], 0x030201);
    assert_eq!(
        lines[0]["Points"],
        json!([[4.0, 5.0, 6.0], [1.0, 2.0, 3.0]])
    );
    assert_eq!(
        lines[0]["BreakInfoEntries"],
        json!([
            {"SegmentIndex":4,"StartEndPoints":[{"StartPoint":[1.0,2.0,3.0],"EndPoint":[4.0,5.0,6.0]}]},
            {"SegmentIndex":1,"StartEndPoints":[]}
        ])
    );
    assert_eq!(lines[0].as_object().unwrap().len(), 9);
}
