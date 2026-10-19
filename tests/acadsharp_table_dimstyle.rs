// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::dump_document;
use acadrust::tables::DimStyle;
use acadrust::types::{DxfVersion, Handle};
use acadrust::{CadDocument, DxfReader};
use serde_json::Value;
use std::io::Cursor;

fn read_tables(tables: &str) -> CadDocument {
    let input = format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n{tables}0\nENDSEC\n0\nEOF\n"
    );
    DxfReader::from_reader(Cursor::new(input.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

fn props(doc: &CadDocument, name: &str) -> Value {
    dump_document(doc, "", "dxf")["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|object| object["type"] == "DimensionStyle" && object["props"]["Name"] == name)
        .unwrap()["props"]
        .clone()
}

#[test]
fn omitted_dxf_groups_use_acadsharp_dimension_style_defaults() {
    let mut doc = CadDocument::new();
    let mut style = DimStyle::dxf_defaults("Sparse");
    style.handle = Handle::new(0x501);
    style.dxf_present_codes = Some([1 << 2; 6]);
    doc.dim_styles.add(style).unwrap();

    let value = props(&doc, "Sparse");
    assert_eq!(value["PostFix"], "");
    assert_eq!(value["AlternateUnitDecimalPlaces"], 3);
    assert_eq!(value["DimensionLineIncrement"], 3.75);
    assert_eq!(value["DimensionLineGap"], 0.625);
    assert_eq!(value["SeparateArrowBlocks"], false);
    assert_eq!(value["TextVerticalAlignment"], 1);
    assert_eq!(value["ZeroHandling"], 8);
}

#[test]
fn explicit_dxf_groups_and_prefix_suffix_are_preserved() {
    let mut doc = CadDocument::new();
    let mut style = DimStyle::dxf_defaults("Explicit");
    style.handle = Handle::new(0x502);
    style.dimpost = "PRE<>POST".into();
    style.dimdli = 7.5;
    style.dimdec = 6;
    style.dimsah = true;
    let mut present = [0; 6];
    for code in [2, 3, 43, 171, 173, 271] {
        present[code / 64] |= 1 << (code % 64);
    }
    style.dxf_present_codes = Some(present);
    doc.dim_styles.add(style).unwrap();

    let value = props(&doc, "Explicit");
    assert_eq!(value["PostFix"], "PRE<>POST");
    assert_eq!(value["Prefix"], "PRE");
    assert_eq!(value["Suffix"], "POST");
    assert_eq!(value["DimensionLineIncrement"], 7.5);
    assert_eq!(value["DecimalPlaces"], 6);
    assert_eq!(value["SeparateArrowBlocks"], true);
    // Omitted code 147 still gets ACadSharp's object default.
    assert_eq!(value["DimensionLineGap"], 0.625);
}

#[test]
fn ascii_dxf_reader_projects_defaults_getters_and_references() {
    let doc = read_tables(
        "0\nTABLE\n2\nSTYLE\n0\nSTYLE\n5\nA001\n2\nDimText\n70\n0\n3\ntxt\n0\nENDTAB\n\
         0\nTABLE\n2\nLTYPE\n0\nLTYPE\n5\nC001\n2\nDimLine\n70\n0\n3\n\n72\n65\n73\n0\n40\n0\n0\nENDTAB\n\
         0\nTABLE\n2\nBLOCK_RECORD\n0\nBLOCK_RECORD\n5\nB001\n2\nArrow\n70\n0\n0\nENDTAB\n\
         0\nTABLE\n2\nDIMSTYLE\n0\nDIMSTYLE\n105\nD001\n2\nReadStyle\n70\n0\n3\nPRE<>POST\n40\n-2\n41\n-1\n43\n7.5\n50\n-1\n140\n0\n270\n9\n277\n3\n340\nA001\n342\nB001\n345\nC001\n0\nENDTAB\n",
    );

    let style = doc.dim_styles.get("ReadStyle").unwrap();
    assert!(style.dxf_code_present(40));
    assert!(style.dxf_code_present(340));
    assert!(!style.dxf_code_present(147));
    assert_eq!(style.dxf_last_linear_unit_code, Some(277));

    let value = props(&doc, "ReadStyle");
    assert_eq!(value["PostFix"], "PRE<>POST");
    assert_eq!(value["Prefix"], "PRE");
    assert_eq!(value["Suffix"], "POST");
    assert_eq!(value["DimensionLineIncrement"], 7.5);
    assert_eq!(value["DimensionLineGap"], 0.625);
    assert_eq!(value["SeparateArrowBlocks"], false);
    assert_eq!(value["TextVerticalAlignment"], 1);
    assert_eq!(value["ZeroHandling"], 8);
    assert_eq!(value["ScaleFactor"], 1.0e-12);
    assert_eq!(value["ArrowSize"], 0.18);
    assert_eq!(value["TextHeight"], 0.18);
    assert_eq!(
        value["JoggedRadiusDimensionTransverseSegmentAngle"],
        std::f64::consts::FRAC_PI_4
    );
    assert_eq!(value["LinearUnitFormat"], 3);
    assert_eq!(value["DimensionUnit"], 2);
    assert_eq!(value["ArrowBlock"], "#B001");
    assert_eq!(value["Style"], "#A001");
    assert_eq!(value["LineType"], "#C001");
    assert_eq!(value["DimArrow1"], Value::Null);
}

#[test]
fn dxf_unit_format_uses_the_last_270_or_277_group() {
    let mut doc = CadDocument::new();
    let mut style = DimStyle::dxf_defaults("ReverseUnits");
    style.handle = Handle::new(0x505);
    style.dimunit = 9;
    style.dimlunit = 3;
    style.dxf_present_codes = Some({
        let mut present = [0; 6];
        for code in [2, 270, 277] {
            present[code / 64] |= 1 << (code % 64);
        }
        present
    });
    style.dxf_last_linear_unit_code = Some(270);
    doc.dim_styles.add(style).unwrap();

    assert_eq!(props(&doc, "ReverseUnits")["LinearUnitFormat"], 9);
}

#[test]
fn r13_dwg_bit_order_and_pre_r2000_defaults_match_acadsharp() {
    let mut doc = CadDocument::new();
    doc.dwg_source_version = Some(DxfVersion::AC1012);
    let mut style = DimStyle::new("R13");
    style.handle = Handle::new(0x503);
    // Native R13 reader fields in stream order: inside, outside, se1, se2.
    style.dimtih = true;
    style.dimtoh = false;
    style.dimse1 = true;
    style.dimse2 = false;
    style.dimfxl = 0.0;
    style.dimjogang = 0.0;
    style.dimlwd = 0;
    style.dimlwe = 0;
    doc.dim_styles.add(style).unwrap();

    let value = props(&doc, "R13");
    assert_eq!(value["TextOutsideHorizontal"], true);
    assert_eq!(value["SuppressFirstExtensionLine"], false);
    assert_eq!(value["SuppressSecondExtensionLine"], true);
    assert_eq!(value["TextInsideHorizontal"], false);
    assert_eq!(value["FixedExtensionLineLength"], 1.0);
    assert_eq!(value["DimensionLineWeight"], -2);
    assert_eq!(value["ExtensionLineWeight"], -2);
    assert_eq!(
        value["JoggedRadiusDimensionTransverseSegmentAngle"],
        std::f64::consts::FRAC_PI_4
    );
}

#[test]
fn modern_dwg_uses_the_dimatfit_slot_for_dimension_fit() {
    let mut doc = CadDocument::new();
    doc.dwg_source_version = Some(DxfVersion::AC1024);
    let mut style = DimStyle::new("Modern");
    style.handle = Handle::new(0x504);
    style.dimfit = 9;
    style.dimatfit = 1;
    doc.dim_styles.add(style).unwrap();

    let value = props(&doc, "Modern");
    assert_eq!(value["DimensionFit"], 1);
    assert_eq!(value["DimensionTextArrowFit"], 3);
}

#[test]
fn source_presence_is_metadata_and_does_not_change_style_equality() {
    let mut style = DimStyle::new("Equal");
    let mut read = style.clone();
    read.dxf_present_codes = Some({
        let mut present = [0; 6];
        present[2 / 64] |= 1 << (2 % 64);
        present[43 / 64] |= 1 << (43 % 64);
        present
    });
    assert_eq!(style, read);
    style.dxf_present_codes = Some({
        let mut present = [0; 6];
        present[43 / 64] |= 1 << (43 % 64);
        present
    });
    assert!(style.dxf_code_present(43));
    assert!(!style.dxf_code_present(44));
}
