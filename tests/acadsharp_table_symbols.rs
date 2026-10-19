// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::{dump_document, new_document};
use acadrust::tables::{LineType, LineTypeElement, TextStyle};
use acadrust::types::Handle;
use acadrust::{CadDocument, DxfReader};
use serde_json::{json, Value};
use std::io::Cursor;

fn dxf(tables: &str) -> CadDocument {
    let input = format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n{tables}0\nENDSEC\n0\nEOF\n");
    DxfReader::from_reader(Cursor::new(input.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

fn props(doc: &CadDocument, h: Handle) -> Value {
    dump_document(doc, "", "dxf")["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["handle"] == format!("#{:X}", h.value()))
        .unwrap()["props"]
        .clone()
}

#[test]
fn symbol_flags_and_layer_source_references_are_retained() {
    let doc = dxf("0\nTABLE\n2\nAPPID\n0\nAPPID\n5\nF001\n2\nCustomApp\n70\n112\n0\nENDTAB\n0\nTABLE\n2\nLAYER\n0\nLAYER\n5\nF002\n2\nDeFpOiNtS\n70\n119\n290\n1\n347\nF100\n390\nF200\n0\nENDTAB\n0\nTABLE\n2\nLTYPE\n0\nLTYPE\n5\nF003\n2\nCustomLine\n70\n112\n72\n66\n0\nENDTAB\n");
    assert_eq!(props(&doc, Handle::new(0xF001))["Flags"], 112);
    let layer = doc.layers.get("defpoints").unwrap();
    assert_eq!(layer.material, Handle::new(0xF100));
    assert_eq!(layer.plotstyle_handle, Handle::new(0xF200));
    let p = props(&doc, layer.handle);
    assert_eq!(p["Flags"], 119);
    assert_eq!(p["PlotFlag"], false);
    assert_eq!(p["Material"], Value::Null); // The reference builder never assigns it.
    assert_eq!(p["PlotStyleName"], 0xF200);
    let p = props(&doc, Handle::new(0xF003));
    assert_eq!(p["Flags"], 112);
    assert_eq!(p["Alignment"], 66);
}

#[test]
fn font_metadata_does_not_replace_style_font_flags() {
    let doc = dxf("0\nTABLE\n2\nSTYLE\n0\nSTYLE\n5\nF001\n2\nCustomFont\n70\n117\n1071\n16777216\n1001\nACAD\n1000\nArial\n1071\n34\n0\nENDTAB\n");
    let style = doc.text_styles.get("customfont").unwrap();
    assert_eq!(style.true_type_font, "Arial");
    assert_eq!(style.true_type_font_flags, 34);
    assert_eq!(style.font_flags, 16777216);
    let p = props(&doc, style.handle);
    assert_eq!(p["TrueType"], 16777216);
    assert_eq!(p["Flags"], 117);
    assert_eq!(p["Filename"], "");
    assert_eq!(p["BigFontFilename"], Value::Null);
    assert_eq!(p["LastHeight"], 0.0);
    assert_eq!(style.font_file, "txt");
    assert_eq!(style.last_height, 2.5);
}

#[test]
fn explicit_empty_font_and_description_values_survive_projection() {
    let doc = dxf("0\nTABLE\n2\nSTYLE\n0\nSTYLE\n5\nF001\n2\nCustomFont\n3\ntxt\n4\n\n42\n2.5\n0\nENDTAB\n0\nTABLE\n2\nLTYPE\n0\nLTYPE\n5\nF002\n2\nEmptyDescription\n3\n\n0\nENDTAB\n");
    let style = doc.text_styles.get("customfont").unwrap();
    let p = props(&doc, style.handle);
    assert_eq!(p["Filename"], "txt");
    assert_eq!(p["BigFontFilename"], "");
    assert_eq!(p["LastHeight"], 2.5);
    let mut same = style.clone();
    same.dxf_present = None;
    same.big_font_file_present = false;
    assert_eq!(*style, same);
    assert_eq!(props(&doc, Handle::new(0xF002))["Description"], "");
}

#[test]
fn segment_defaults_and_computed_pattern_length_are_complete() {
    let mut doc = CadDocument::new();
    let mut line = LineType::new("CustomLine");
    line.handle = Handle::new(0xF001);
    line.pattern_length = 99.0;
    line.elements = vec![
        LineTypeElement::dash(2.0),
        LineTypeElement::space(0.5),
        LineTypeElement::dot(),
    ];
    doc.line_types.add(line).unwrap();
    let p = props(&doc, Handle::new(0xF001));
    assert_eq!(p["PatternLength"], 2.5);
    assert_eq!(
        p["Segments"][0],
        json!({"Flags":0,"Length":2.0,"Offset":[0.0,0.0],"Rotation":0.0,"Scale":1.0,"ShapeNumber":0,"Style":null,"Text":""})
    );
    assert_eq!(
        doc.line_types.get("customline").unwrap().pattern_length,
        99.0
    );
    assert_eq!(p["Description"], Value::Null);
}

#[test]
fn complex_segments_keep_shape_text_and_style_references() {
    let doc = dxf("0\nTABLE\n2\nSTYLE\n0\nSTYLE\n5\nF001\n2\nShapeFont\n0\nENDTAB\n0\nTABLE\n2\nLTYPE\n0\nLTYPE\n5\nF002\n2\nComplexLine\n49\n1\n74\n10\n340\nF001\n46\n0.25\n9\nGAS\n49\n-0.5\n74\n4\n75\n130\n340\nF001\n0\nENDTAB\n");
    let p = props(&doc, Handle::new(0xF002));
    assert_eq!(p["Segments"][0]["Flags"], 10);
    assert_eq!(p["Segments"][0]["Style"], Value::Null);
    assert_eq!(
        doc.line_types.get("ComplexLine").unwrap().elements[0]
            .complex
            .as_ref()
            .unwrap()
            .style_handle,
        Handle::new(0xF001)
    );
    let mut constructed = CadDocument::new();
    constructed
        .text_styles
        .add(doc.text_styles.get("ShapeFont").unwrap().clone())
        .unwrap();
    constructed
        .line_types
        .add(doc.line_types.get("ComplexLine").unwrap().clone())
        .unwrap();
    assert_eq!(
        props(&constructed, Handle::new(0xF002))["Segments"][0]["Style"],
        "#F001"
    );
    assert_eq!(p["Segments"][0]["ShapeNumber"], 0);
    assert_eq!(p["Segments"][0]["Text"], "GAS");
    assert_eq!(p["Segments"][1]["ShapeNumber"], 130);
    assert_eq!(p["Segments"][1]["Text"], "");
}

#[test]
fn unnamed_styles_follow_table_order_and_case_insensitive_names() {
    let mut doc = CadDocument::new();
    for (id, name) in [(0xF001, "UNNAMED0"), (0xF002, ""), (0xF003, "")] {
        let mut style = TextStyle::new(name);
        style.handle = Handle::new(id);
        doc.text_styles.add_allow_duplicate(style);
    }
    assert_eq!(props(&doc, Handle::new(0xF002))["Name"], "unnamed1");
    assert_eq!(props(&doc, Handle::new(0xF003))["Name"], "unnamed2");
    assert_eq!(
        doc.text_styles.get("unnamed0").unwrap().handle,
        Handle::new(0xF001)
    );
    assert_eq!(
        doc.text_styles.iter().filter(|s| s.name.is_empty()).count(),
        2
    );
    let mut table = doc.text_styles.clone();
    table.rename("UNnamed0", "Renamed").unwrap();
    assert_eq!(table.get("RENAMED").unwrap().handle, Handle::new(0xF001));
    assert_eq!(
        table
            .iter()
            .find(|s| s.handle == Handle::new(0xF001))
            .unwrap()
            .name,
        "Renamed"
    );
}

#[test]
fn compatibility_new_document_has_reference_symbol_defaults() {
    let doc = new_document();
    let p = props(&doc, doc.text_styles.get("Standard").unwrap().handle);
    assert_eq!(p["Filename"], "");
    assert_eq!(p["BigFontFilename"], Value::Null);
    assert_eq!(p["LastHeight"], 0.0);
    assert_eq!(p["TrueType"], 0);
    for name in ["ByLayer", "ByBlock", "Continuous"] {
        assert_eq!(
            props(&doc, doc.line_types.get(name).unwrap().handle)["Description"],
            Value::Null
        );
    }
    assert_eq!(TextStyle::standard().font_file, "txt");
    assert_eq!(TextStyle::standard().last_height, 2.5);
    assert_eq!(LineType::continuous().description, "Solid line");
}

#[test]
fn dwg_style_flags_keep_source_dependency_without_resolving_a_null_xref() {
    let mut doc = CadDocument::new();
    doc.dwg_source_version = Some(acadrust::types::DxfVersion::AC1032);
    let mut style = TextStyle::new("External");
    style.handle = Handle::new(0xF001);
    style.source_flags = 112;
    // Native builder deliberately declines to resolve a null xref target.
    style.xref_dependent = false;
    doc.text_styles.add(style).unwrap();
    assert_eq!(props(&doc, Handle::new(0xF001))["Flags"], 80);
    assert!(!doc.text_styles.get("External").unwrap().xref_dependent);
}

#[test]
fn source_synthesized_continuous_description_uses_reference_default() {
    let doc = dxf("");
    let line = doc.line_types.get("continuous").unwrap();
    assert_eq!(line.description, "Solid line");
    assert_eq!(props(&doc, line.handle)["Description"], Value::Null);
}
