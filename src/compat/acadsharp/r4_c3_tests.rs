// SPDX-License-Identifier: MPL-2.0
//! Focused tests for the R4-C3 corpus fixes: collection dictionaries and the
//! reader root, defaults ACadSharp creates, unnamed table entries, and the
//! compatibility projections for surfaces, OLE frames and table records.
use super::{dump_document, dump_reader_document};
use crate::entities::{EntityType, Surface, SurfaceKind};
use crate::objects::{Dictionary, ObjectType, SourceFlag};
use crate::tables::{DimStyle, Layer, LineType, LineTypeElement, TextStyle};
use crate::types::{Color, DxfVersion, Handle};
use crate::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};
use crate::CadDocument;
use serde_json::{json, Value};

fn dict(doc: &mut CadDocument, h: u64, entries: &[(&str, u64)]) {
    let mut d = Dictionary::new();
    d.handle = Handle::new(h);
    for (name, child) in entries {
        d.add_entry(*name, Handle::new(*child));
    }
    doc.objects.insert(d.handle, ObjectType::Dictionary(d));
}

fn find<'a>(dump: &'a Value, h: u64) -> &'a Value {
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["handle"] == format!("#{h:X}"))
        .unwrap_or_else(|| panic!("missing object #{h:X}"))
}

fn rec(h: u64, ty: &str, owner: u64, props: Value) -> Value {
    json!({"handle": format!("#{h:X}"), "type": ty, "owner": format!("#{owner:X}"), "props": props})
}

fn root_record(h: u64, entries: &[(&str, u64)]) -> Value {
    rec(
        h,
        "CadDictionary",
        0,
        json!({
            "Name": "",
            "SubclassMarker": "AcDbDictionary",
            "EntryNames": entries.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            "EntryHandles": entries.iter().map(|(_, c)| *c).collect::<Vec<_>>(),
        }),
    )
}

fn source_doc(version: DxfVersion) -> CadDocument {
    let mut doc = CadDocument::new();
    doc.version = version;
    doc.header.source_handle_seed = Some(0x1000);
    doc.header.named_objects_dict_handle = Handle::new(0x10);
    doc
}

#[test]
fn damaged_group_dictionary_with_owner_zero_becomes_the_reader_root() {
    let mut doc = CadDocument::new();
    doc.header.source_handle_seed = Some(0x1000);
    doc.header.named_objects_dict_handle = Handle::new(0x100);
    doc.header.acad_group_dict_handle = Handle::new(0x101);
    dict(&mut doc, 0x100, &[("ACAD_GROUP", 0x101)]);
    dict(&mut doc, 0x101, &[]);

    let dump = dump_reader_document(&doc, "", "dwg");
    assert!(!dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .any(|o| o["handle"] == "#100"));
    assert_eq!(find(&dump, 0x101)["owner"], "#100");
    assert_eq!(find(&dump, 0x101)["props"]["Name"], "ACAD_GROUP");
    let layout = dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["props"]["Name"] == "ACAD_LAYOUT")
        .expect("created collection dictionary");
    assert_eq!(layout["owner"], "#101");
}

#[test]
fn empty_table_entries_get_unnamed_names_and_empty_vports_are_dropped() {
    let doc = source_doc(DxfVersion::AC1032);
    let table_props = json!({"Name": "", "SubclassMarker": "AcDbSymbolTable"});
    let entry = |marker: &str| json!({"Name": "", "SubclassMarker": marker, "Layout": "#BB"});
    let mut records = vec![
        root_record(0x10, &[]),
        rec(0x20, "BlockRecordsTable", 0, table_props.clone()),
        rec(0x21, "BlockRecord", 0x20, entry("AcDbBlockTableRecord")),
        rec(0x24, "BlockRecord", 0x20, entry("AcDbBlockTableRecord")),
        rec(0x30, "VPortsTable", 0, table_props),
        rec(0x31, "VPort", 0x30, entry("AcDbViewportTableRecord")),
    ];
    super::registration::apply(&doc, "dwg", &mut records);
    let by = |h: u64| records.iter().find(|r| r["handle"] == format!("#{h:X}"));
    assert_eq!(by(0x21).unwrap()["props"]["Name"], "unnamed0");
    assert_eq!(by(0x24).unwrap()["props"]["Name"], "unnamed1");
    // No layout record exists behind the stale reference.
    assert_eq!(by(0x21).unwrap()["props"]["Layout"], Value::Null);
    assert!(by(0x31).is_none(), "VPortsTable refuses an empty key");
    assert_eq!(by(0x30).unwrap()["props"]["Count"], 0);
}

#[test]
fn group_unnamed_flag_follows_the_dictionary_assigned_name() {
    let doc = source_doc(DxfVersion::AC1032);
    let mut records = vec![
        root_record(0x10, &[("ACAD_GROUP", 0x41)]),
        rec(
            0x41,
            "CadDictionary",
            0,
            json!({"Name": "", "SubclassMarker": "AcDbDictionary",
                   "EntryNames": ["*A1", "Named"], "EntryHandles": [0x40, 0x42]}),
        ),
        rec(0x40, "Group", 0, json!({"Name": "", "IsUnnamed": false, "SubclassMarker": "AcDbGroup"})),
        rec(0x42, "Group", 0, json!({"Name": "", "IsUnnamed": true, "SubclassMarker": "AcDbGroup"})),
    ];
    super::registration::apply(&doc, "dwg", &mut records);
    let by = |h: u64| records.iter().find(|r| r["handle"] == format!("#{h:X}")).unwrap();
    assert_eq!(by(0x40)["props"]["Name"], "*A1");
    assert_eq!(by(0x40)["props"]["IsUnnamed"], true);
    assert_eq!(by(0x42)["props"]["Name"], "Named");
    assert_eq!(by(0x42)["props"]["IsUnnamed"], false);
}

#[test]
fn evaluation_graph_clears_expressions_the_reader_never_produced() {
    let doc = source_doc(DxfVersion::AC1032);
    let mut records = vec![
        root_record(0x10, &[("graph", 0x50)]),
        rec(
            0x50,
            "EvaluationGraph",
            0,
            json!({"Name": "", "SubclassMarker": "AcDbEvalGraph",
                   "Nodes": [{"Expression": "#51"}, {"Expression": "#52"}, {"Expression": null}]}),
        ),
        rec(0x51, "BlockMoveAction", 0x50, json!({"Name": "", "SubclassMarker": "AcDbBlockMoveAction"})),
        rec(0x52, "UnknownNonGraphicalObject", 0x50, json!({"Name": ""})),
    ];
    super::registration::apply(&doc, "dwg", &mut records);
    let graph = records.iter().find(|r| r["handle"] == "#50").unwrap();
    assert_eq!(graph["props"]["Nodes"][0]["Expression"], "#51");
    assert_eq!(graph["props"]["Nodes"][1]["Expression"], Value::Null);
}

#[test]
fn r14_drawings_do_not_register_placeholders() {
    let make = |version| {
        let doc = source_doc(version);
        let mut records = vec![
            root_record(0x10, &[("ACAD_PLOTSTYLENAME", 0x60)]),
            rec(
                0x60,
                "CadDictionaryWithDefault",
                0,
                json!({"Name": "", "SubclassMarker": "AcDbDictionaryWithDefault",
                       "EntryNames": ["Normal"], "EntryHandles": [0x61]}),
            ),
            rec(0x61, "AcdbPlaceHolder", 0x60, json!({"Name": "", "SubclassMarker": "AcDbPlaceHolder"})),
        ];
        super::registration::apply(&doc, "dwg", &mut records);
        records
    };
    let r14 = make(DxfVersion::AC1014);
    assert!(!r14.iter().any(|r| r["handle"] == "#61"));
    assert_eq!(r14.iter().find(|r| r["handle"] == "#60").unwrap()["props"]["EntryNames"], json!([]));
    let r2000 = make(DxfVersion::AC1015);
    assert!(r2000.iter().any(|r| r["handle"] == "#61"));
}

#[test]
fn mtext_attribute_context_data_is_not_registered_from_dwg() {
    let doc = source_doc(DxfVersion::AC1032);
    let mut records = vec![
        root_record(0x10, &[("ctx", 0x70)]),
        rec(0x70, "MTextAttributeObjectContextData", 0, json!({"Name": "", "SubclassMarker": "x"})),
    ];
    super::registration::apply(&doc, "dwg", &mut records);
    assert!(!records.iter().any(|r| r["handle"] == "#70"));
}

#[test]
fn default_dimension_style_takes_the_next_identity_when_a_dimension_exists() {
    let mut doc = source_doc(DxfVersion::AC1032);
    let default = doc.dim_styles.get("Standard").unwrap().handle.value();
    assert!(!doc.dim_styles.get("Standard").unwrap().source_present);
    let mut records = vec![
        root_record(0x10, &[]),
        rec(default, "DimensionStyle", 0xA, json!({"Name": "Standard", "SubclassMarker": "AcDbDimStyleTableRecord"})),
        rec(
            0x200,
            "DimensionLinear",
            0x1F,
            json!({"Style": format!("#{default:X}"), "SubclassMarker": "AcDbRotatedDimension"}),
        ),
    ];
    super::registration_order::apply(&doc, "dwg", &mut records);
    let style = records.iter().find(|r| r["type"] == "DimensionStyle").unwrap();
    // The collection dictionaries are created first (#1000-#100A); the style
    // is created while its dimension joins a block.
    assert_eq!(style["handle"], "#100B");
    let linear = records.iter().find(|r| r["type"] == "DimensionLinear").unwrap();
    assert_eq!(linear["props"]["Style"], "#100B");
    assert!(records.iter().any(|r| r["handle"] == "#1000" && r["type"] == "CadDictionary"));
    // Without a dimension entity the default is not part of the reader view.
    let mut without = vec![
        root_record(0x10, &[]),
        rec(default, "DimensionStyle", 0xA, json!({"Name": "Standard", "SubclassMarker": "AcDbDimStyleTableRecord"})),
    ];
    super::registration::apply(&doc, "dwg", &mut without);
    assert!(!without.iter().any(|r| r["type"] == "DimensionStyle"));
    doc.dim_styles.get_mut("Standard").unwrap().source_present = true;
}

#[test]
fn created_standard_dimension_style_projects_constructor_defaults() {
    let doc = source_doc(DxfVersion::AC1032);
    let style = doc.dim_styles.get("Standard").unwrap();
    let values = super::table_dimstyle::map(&doc, style);
    let get = |name: &str| values.iter().find(|(k, _)| *k == name).unwrap().1.clone();
    assert_eq!(get("PostFix"), "");
    assert_eq!(get("DimensionFit"), 0);
    assert_eq!(get("SeparateArrowBlocks"), false);
}

#[test]
fn dwg_layer_color_by_block_reads_as_the_default_color() {
    let mut doc = CadDocument::new();
    doc.dwg_source_version = Some(DxfVersion::AC1015);
    let mut layer = Layer::new("L");
    layer.handle = Handle::new(0x300);
    layer.color = Color::ByBlock;
    doc.layers.add(layer).unwrap();
    let dump = dump_reader_document(&doc, "", "dwg");
    assert_eq!(find(&dump, 0x300)["props"]["Color"]["Index"], 7);
    assert_eq!(find(&dump, 0x300)["props"]["Color"]["IsByBlock"], false);
}

#[test]
fn dwg_header_extension_line_suppression_reads_the_dimension_line_bits() {
    let mut doc = CadDocument::new();
    doc.dwg_source_version = Some(DxfVersion::AC1015);
    doc.header.dim_suppress_line1 = true;
    doc.header.dim_suppress_ext1 = false;
    doc.header.dim_suppress_line2 = false;
    doc.header.dim_suppress_ext2 = true;
    let dump = dump_reader_document(&doc, "", "dwg");
    assert_eq!(dump["header"]["DimensionSuppressFirstExtensionLine"], true);
    assert_eq!(dump["header"]["DimensionSuppressSecondExtensionLine"], false);
}

#[test]
fn surface_subtypes_dump_as_their_own_classes() {
    let mut doc = CadDocument::new();
    let handle = doc
        .add_entity(EntityType::Surface(Box::new(Surface::new(SurfaceKind::Extruded))))
        .unwrap();
    let dump = dump_document(&doc, "", "new");
    let object = find(&dump, handle.value());
    assert_eq!(object["type"], "ExtrudedSurface");
    assert_eq!(object["props"]["SubclassMarker"], "AcDbExtrudedSurface");
    assert_eq!(object["props"]["ObjectName"], "EXTRUDEDSURFACE");
}

#[test]
fn line_type_segments_expose_the_stored_scale() {
    let mut doc = CadDocument::new();
    let mut line_type = LineType::new("SCALED");
    line_type.handle = Handle::new(0x310);
    line_type.elements.push(LineTypeElement {
        length: 1.0,
        source_scale: Some(0.25),
        complex: None,
    });
    line_type.elements.push(LineTypeElement::dash(2.0));
    doc.line_types.add(line_type).unwrap();
    let dump = dump_document(&doc, "", "new");
    let segments = &find(&dump, 0x310)["props"]["Segments"];
    assert_eq!(segments[0]["Scale"], 0.25);
    assert_eq!(segments[1]["Scale"], 1.0);
    // Source metadata never makes elements unequal.
    assert_eq!(
        LineTypeElement { length: 3.0, source_scale: Some(0.5), complex: None },
        LineTypeElement::dash(3.0)
    );
}

#[test]
fn ole_frame_dumps_the_stored_payload_and_skips_unassigned_dwg_fields() {
    use crate::entities::ole2frame::SourcePayload;
    let mut doc = CadDocument::new();
    doc.dwg_source_version = Some(DxfVersion::AC1015);
    let mut ole = crate::entities::Ole2Frame::new();
    ole.is_paper_space = true;
    ole.source_application = "Excel.Sheet.12".into();
    ole.source_payload = Some(SourcePayload(vec![1, 2, 3]));
    let handle = doc.add_entity(EntityType::Ole2Frame(Box::new(ole))).unwrap();
    let dump = dump_document(&doc, "", "dwg");
    let object = find(&dump, handle.value());
    assert_eq!(object["props"]["BinaryData"], "010203");
    assert_eq!(object["props"]["IsPaperSpace"], false);
    assert_eq!(object["props"]["SourceApplication"], Value::Null);
}

#[test]
fn dictionary_without_cloning_group_projects_the_constructor_default() {
    let mut doc = CadDocument::new();
    let mut absent = Dictionary::new();
    absent.handle = Handle::new(0x320);
    absent.cloning_absent = SourceFlag(true);
    let mut present = Dictionary::new();
    present.handle = Handle::new(0x321);
    doc.objects.insert(absent.handle, ObjectType::Dictionary(absent));
    doc.objects.insert(present.handle, ObjectType::Dictionary(present));
    let dump = dump_document(&doc, "", "new");
    assert_eq!(find(&dump, 0x320)["props"]["ClonningFlags"], 0);
    assert_eq!(find(&dump, 0x321)["props"]["ClonningFlags"], 1);
}

#[test]
fn text_style_typeface_keeps_its_source_position_among_xdata() {
    let mut doc = CadDocument::new();
    let mut style = TextStyle::new("FontStyle");
    style.handle = Handle::new(0x330);
    style.true_type_font = "Arial".to_owned();
    style.true_type_font_eed_position = Some(0);
    doc.text_styles.add(style).unwrap();
    let mut app = crate::tables::AppId::new("STYLE_CUSTOM");
    app.handle = Handle::new(0x331);
    doc.app_ids.add(app).unwrap();
    let mut data = ExtendedData::new();
    let mut record = ExtendedDataRecord::new("STYLE_CUSTOM");
    record.add_value(XDataValue::String("later".to_owned()));
    data.add_record(record);
    doc.object_xdata.insert(Handle::new(0x330), data);
    let dump = dump_document(&doc, "", "new");
    let xdata = &find(&dump, 0x330)["props"]["ExtendedData"];
    assert_eq!(xdata[1][0], "#331");
    assert_eq!(xdata[0][1]["Records"][0]["RawValue"], "Arial");
}

#[test]
fn block_context_data_and_embedded_scale_defaults() {
    let doc = source_doc(DxfVersion::AC1032);
    let mut records = vec![
        json!({"handle":"#20", "type":"CadDictionary", "owner":"#10",
            "props":{"Name":"ACAD_SCALELIST", "EntryNames":["1\" = 1\""], "EntryHandles":[33]}}),
        rec(0x21, "Scale", 0x20, json!({"Name": "1\" = 1\""})),
        rec(0x30, "MultiLeader", 0x1F, json!({})),
    ];
    super::registration_order::apply(&doc, "dwg", &mut records);
    let names = records.iter().find(|r| r["handle"] == "#20").unwrap()["props"]["EntryNames"].clone();
    assert!(names.as_array().unwrap().iter().any(|n| n == "1:1"));
}

#[test]
fn dxf_reader_keeps_source_provenance() {
    use crate::io::dxf::DxfReader;
    use std::io::Cursor;
    let text = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n0\nENDSEC\n\
        0\nSECTION\n2\nCLASSES\n0\nCLASS\n1\n\n2\nAcDbBEditSession\n3\nObjectDBX Classes\n90\n0\n91\n0\n280\n0\n281\n0\n0\nENDSEC\n\
        0\nSECTION\n2\nTABLES\n\
        0\nTABLE\n2\nBLOCK_RECORD\n5\n1\n330\n0\n100\nAcDbSymbolTable\n70\n1\n\
        0\nBLOCK_RECORD\n5\n1B\n330\n1\n100\nAcDbSymbolTableRecord\n100\nAcDbBlockTableRecord\n2\n*PAPER_SPACE\n0\nENDTAB\n\
        0\nTABLE\n2\nDIMSTYLE\n5\nA\n330\n0\n100\nAcDbSymbolTable\n70\n1\n\
        0\nDIMSTYLE\n105\n27\n330\nA\n100\nAcDbSymbolTableRecord\n100\nAcDbDimStyleTableRecord\n2\nStandard\n70\n0\n0\nENDTAB\n0\nENDSEC\n\
        0\nSECTION\n2\nOBJECTS\n\
        0\nDICTIONARY\n5\nC\n330\n0\n100\nAcDbDictionary\n3\nACAD_GROUP\n350\nD\n\
        0\nDICTIONARY\n5\nD\n330\nC\n100\nAcDbDictionary\n281\n1\n0\nENDSEC\n0\nEOF\n";
    let doc = DxfReader::from_reader(Cursor::new(text.as_bytes())).unwrap().read().unwrap();
    let classes = doc.classes.source_classes().expect("source classes");
    assert!(classes.iter().any(|class| class.dxf_name.is_empty()));
    assert!(doc.block_records.iter().any(|record| record.name == "*PAPER_SPACE"));
    assert!(doc.dim_styles.get("Standard").unwrap().source_present);
    let ObjectType::Dictionary(root) = doc.objects.get(&Handle::new(0xC)).unwrap() else {
        panic!("root dictionary");
    };
    assert!(root.cloning_absent.0);
    let ObjectType::Dictionary(group) = doc.objects.get(&Handle::new(0xD)).unwrap() else {
        panic!("group dictionary");
    };
    assert!(!group.cloning_absent.0);
    // Constructed styles stay native defaults.
    assert!(!CadDocument::new().dim_styles.get("Standard").unwrap().source_present);
    let _ = DimStyle::standard();
}
