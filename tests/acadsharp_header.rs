// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::{dump_document, new_document};
use acadrust::tables::{BlockRecord, DimStyle, TextStyle};
use acadrust::types::{Color, DxfVersion, Handle};
use acadrust::{CadDocument, DxfReader, DxfWriter};
use serde_json::{json, Value};
use std::io::Cursor;

fn header(doc: &CadDocument) -> Value {
    dump_document(doc, "", "new")["header"].clone()
}

fn dxf(variables: &str) -> CadDocument {
    let text =
        format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n{variables}0\nENDSEC\n0\nEOF\n");
    DxfReader::from_reader(Cursor::new(text.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

#[test]
fn omitted_header_variables_use_oracle_defaults_but_explicit_zero_survives() {
    let omitted = dxf("");
    let explicit =
        dxf("9\n$OSMODE\n70\n0\n9\n$CMLSCALE\n40\n0.0\n9\n$DIMDEC\n70\n0\n9\n$USRTIMER\n70\n0\n");
    let a = header(&omitted);
    let b = header(&explicit);
    assert_eq!(a["ObjectSnapMode"], 4133);
    assert_eq!(a["CurrentMultilineScale"], 20.0);
    assert_eq!(a["DimensionDecimalPlaces"], 2);
    assert_eq!(b["ObjectSnapMode"], 0);
    assert_eq!(b["CurrentMultilineScale"], 0.0);
    assert_eq!(b["DimensionDecimalPlaces"], 0);
    assert_eq!(b["UserTimer"], false);
    // The compatibility defaults never overwrite native data.
    assert_eq!(omitted.header.object_snap_mode, 0);
    assert_eq!(omitted.header.multiline_scale, 1.0);
}

#[test]
fn guid_strings_and_codepage_follow_their_source_representation() {
    let mut doc = dxf("9\n$FINGERPRINTGUID\n2\n{ABCDEF00-1234-5678-9012-ABCDEFABCDEF}\n9\n$VERSIONGUID\n2\nNOT-A-GUID\n9\n$DWGCODEPAGE\n3\nANSI_1252\n");
    let a = header(&doc);
    assert_eq!(
        a["FingerPrintGuid"],
        "{ABCDEF00-1234-5678-9012-ABCDEFABCDEF}"
    );
    assert_eq!(a["VersionGuid"], "NOT-A-GUID");
    assert_eq!(a["CodePage"], "ANSI_1252");
    doc.dwg_source_version = Some(DxfVersion::AC1032);
    doc.header.dxf_source_variables = None;
    assert_eq!(header(&doc)["CodePage"], "ansi_1252");
    assert_eq!(doc.header.code_page, "ANSI_1252");
}

#[test]
fn maintenance_variable_is_independent_of_framing_and_survives_dxf_writes() {
    for value in [0, 329] {
        let mut doc = dxf(&format!("9\n$ACADMAINTVER\n90\n{value}\n"));
        doc.maintenance_version = 7;
        assert_eq!(header(&doc)["MaintenanceVersion"], value);
        for binary in [false, true] {
            let mut writer = DxfWriter::new(&doc);
            writer.set_binary(binary);
            let bytes = writer.write_to_vec().unwrap();
            let loaded = DxfReader::from_reader(Cursor::new(bytes))
                .unwrap()
                .read()
                .unwrap();
            assert_eq!(loaded.header.maintenance_version, value);
            assert!(loaded
                .header
                .dxf_source_variables
                .as_ref()
                .unwrap()
                .contains("$ACADMAINTVER"));
        }
    }
    for (version, expected) in [
        (DxfVersion::AC1014, 0),
        (DxfVersion::AC1015, 20),
        (DxfVersion::AC1018, 104),
        (DxfVersion::AC1021, 50),
        (DxfVersion::AC1024, 226),
        (DxfVersion::AC1027, 125),
        (DxfVersion::AC1032, 228),
    ] {
        let mut doc = CadDocument::with_version(version);
        doc.dwg_source_version = Some(version);
        doc.maintenance_version = 7;
        assert_eq!(header(&doc)["MaintenanceVersion"], expected);
    }
}

#[test]
fn compatibility_new_header_uses_unattached_override_and_native_defaults_are_unchanged() {
    let doc = new_document();
    let h = header(&doc);
    assert_eq!(h["AngularDirection"], 1);
    assert_eq!(h["DimensionExtensionLineExtension"], 1.25);
    assert_eq!(h["DimensionExtensionLineOffset"], 0.625);
    assert_eq!(h["DimensionLineIncrement"], 3.75);
    assert_eq!(h["DimensionLineGap"], 0.625);
    assert_eq!(h["DimensionTextVerticalAlignment"], 1);
    assert_eq!(h["DimensionToleranceZeroHandling"], 8);
    assert_eq!(h["ModelSpaceExtMin"], json!([0.0, 0.0, 0.0]));
    assert_eq!(h["DimensionstyleOverrides"], "#0");
    assert_eq!(h["ModelSpaceUcs"], "#0");
    assert!(!doc.dim_styles.contains("override"));
    let native = CadDocument::new();
    assert_eq!(native.header.dim_ext_line_extension, 0.18);
    assert_eq!(native.header.dim_line_increment, 0.38);
}

#[test]
fn current_references_resolve_dxf_names_and_dwg_handles_without_mutating_slots() {
    let mut doc = dxf("");
    let mut text = TextStyle::new("CustomText");
    text.handle = Handle::new(0xF01);
    doc.text_styles.add(text).unwrap();
    let mut dim = DimStyle::new("CustomDim");
    dim.handle = Handle::new(0xF02);
    doc.dim_styles.add(dim).unwrap();
    let stale = doc.header.current_text_style_handle;
    doc.header.current_text_style_name = "CustomText".into();
    doc.header.dim_text_style_name = "CustomText".into();
    doc.header.current_dimstyle_name = "CustomDim".into();
    let h = header(&doc);
    assert_eq!(h["CurrentTextStyle"], "#F01");
    assert_eq!(h["DimensionTextStyle"], "#F01");
    assert_eq!(h["CurrentDimensionStyle"], "#F02");
    assert_eq!(doc.header.current_text_style_handle, stale);
    doc.header.dxf_source_variables = None;
    doc.dwg_source_version = Some(DxfVersion::AC1032);
    doc.header.current_text_style_handle = Handle::new(0xF01);
    doc.header.current_text_style_name = "Standard".into();
    assert_eq!(header(&doc)["CurrentTextStyleName"], "CustomText");
    assert_eq!(doc.header.current_text_style_name, "Standard");
}

#[test]
fn dwg_arrow_and_unattached_ucs_projection_preserves_native_handles() {
    let mut doc = CadDocument::new();
    doc.dwg_source_version = Some(DxfVersion::AC1032);
    for (handle, name) in [(0xF10, "DimensionArrow"), (0xF11, "LeaderArrow")] {
        let mut block = BlockRecord::new(name);
        block.handle = Handle::new(handle);
        doc.block_records.add(block).unwrap();
    }
    doc.header.dim_arrow_block_handle = Handle::new(0xF10);
    doc.header.dim_leader_arrow_block_handle = Handle::new(0xF11);
    doc.header.model_space_ucs_handle = Handle::new(0xF12);
    doc.header.model_space_ucs_name = "NamedUcs".into();
    doc.header.dim_linetype_name = "DASHED".into();
    let h = header(&doc);
    assert_eq!(h["DimensionBlockName"], "LeaderArrow");
    assert_eq!(h["ArrowBlockName"], "");
    assert_eq!(h["DimensionBlockNameFirst"], Value::Null);
    assert_eq!(h["ModelSpaceUcs"], "#0");
    assert_eq!(h["UcsName"], "");
    assert_eq!(h["DimensionLineType"], "ByBlock");
    assert_eq!(doc.header.model_space_ucs_name, "NamedUcs");
    assert_eq!(doc.header.dim_arrow_block_handle, Handle::new(0xF10));
}

#[test]
fn dwg_header_flags_project_the_oracle_bit_masks_only_in_compat() {
    let mut doc = CadDocument::new();
    doc.dwg_source_version = Some(DxfVersion::AC1032);
    doc.header.end_caps = 2;
    doc.header.join_style = 3;
    doc.header.lineweight_display = true;
    doc.header.current_line_weight = -1;
    let h = header(&doc);
    assert_eq!(h["EndCaps"], 64);
    assert_eq!(h["JoinStyle"], 384);
    assert_eq!(h["PlotStyleMode"], 8192);
    assert_eq!(h["CurrentEntityLineWeight"], 29);
    assert_eq!(h["DisplayLineWeight"], false);
    assert_eq!(h["ExtendedNames"], false);
    assert!(doc.header.lineweight_display);
    assert_eq!(doc.header.current_line_weight, -1);
}

#[test]
fn dwg_dates_and_duration_units_are_projected_from_raw_pairs() {
    let mut doc = CadDocument::new();
    doc.dwg_source_version = Some(DxfVersion::AC1032);
    doc.header.dwg_timestamps = Some([
        (2_450_000, 43_200_000),
        (0, 0),
        (24, 43_200_000),
        (-1, 1000),
    ]);
    doc.header.create_date_julian = 2_450_000.5;
    doc.header.update_date_julian = 0.0;
    doc.header.total_editing_time = 24.5;
    doc.header.user_elapsed_time = -1.0 + 1000.0 / 86_400_000.0;
    let h = header(&doc);
    assert_eq!(h["CreateDateTime"], 2_450_001.0);
    assert_eq!(h["UpdateDateTime"], 1_721_426.0);
    assert_eq!(h["TotalEditingTime"], 1.5);
    assert_eq!(h["UserElapsedTimeSpan"], 0.0);
    assert_eq!(doc.header.total_editing_time, 24.5);
    doc.header.create_date_julian = 2_450_002.0;
    doc.header.total_editing_time = 48.0;
    let edited = header(&doc);
    assert_eq!(edited["CreateDateTime"], 2_450_002.5);
    assert_eq!(edited["TotalEditingTime"], 2.0);
}

#[test]
fn dxf_duration_truncates_to_oracle_milliseconds() {
    let doc = dxf("9\n$TDINDWG\n40\n0.4024074074\n9\n$TDUSRTIMER\n40\n0.4024074074\n");
    let h = header(&doc);
    let expected = 34_767_999.0 / 86_400_000.0;
    assert!((h["TotalEditingTime"].as_f64().unwrap() - expected).abs() < 1e-14);
    assert!((h["UserElapsedTimeSpan"].as_f64().unwrap() - expected).abs() < 1e-14);
    assert_eq!(doc.header.total_editing_time, 0.4024074074);
}

#[test]
fn absent_strings_are_null_and_explicit_empty_strings_are_retained() {
    let missing = header(&dxf(""));
    let stored = header(&dxf(
        "9\n$HYPERLINKBASE\n1\n\n9\n$STYLESHEET\n1\n\n9\n$PROJECTNAME\n1\n\n9\n$DIMBLK1\n1\n\n",
    ));
    for key in [
        "HyperLinkBase",
        "StyleSheetName",
        "ProjectName",
        "DimensionBlockNameFirst",
    ] {
        assert_eq!(missing[key], Value::Null, "{key}");
        assert_eq!(stored[key], "", "{key}");
    }
}

#[test]
fn header_color_packing_is_local_to_the_compatibility_view() {
    let mut doc = CadDocument::new();
    doc.header.current_entity_color = Color::Rgb {
        r: 0x12,
        g: 0x34,
        b: 0x56,
    };
    let h = header(&doc);
    assert_eq!(h["CurrentEntityColor"]["TrueColor"], 0x563412);
    assert_eq!(h["CurrentEntityColor"]["R"], 0x12);
    assert_eq!(
        doc.header.current_entity_color.to_true_color_value(),
        Some(0x123456)
    );
}

#[test]
fn dxf_summary_last_saved_by_stays_separate_in_the_oracle_view() {
    let doc = dxf("9\n$LASTSAVEDBY\n1\nAlice\n");
    let dumped = dump_document(&doc, "test.dxf", "dxf");
    assert_eq!(dumped["header"]["LastSavedBy"], "Alice");
    assert_eq!(dumped["summaryInfo"]["LastSavedBy"], "");
    assert_eq!(doc.summary_info.last_saved_by, "Alice");
}

#[test]
fn template_measurement_projects_byte_count_quirk_and_absent_section_default() {
    let mut doc = CadDocument::new();
    doc.dwg_source_version = Some(DxfVersion::AC1032);
    assert_eq!(header(&doc)["MeasurementUnits"], 1);
    // One UTF-16 character (two bytes), followed by the native metric value.
    doc.header.dwg_template_data = Some(vec![1, 0, 0, 0, 1, 0].into());
    doc.header.measurement = 1;
    assert_eq!(header(&doc)["MeasurementUnits"], 256);
    assert_eq!(doc.header.measurement, 1);
    doc.header.measurement = 0;
    assert_eq!(header(&doc)["MeasurementUnits"], 0);
    doc.header.dwg_template_data = Some(vec![5, 0, 0, 0].into());
    assert_eq!(header(&doc)["MeasurementUnits"], 1);
    doc.header.dwg_template_data = Some(vec![0, 0, 0, 0].into());
    assert_eq!(header(&doc)["MeasurementUnits"], 0);
}

#[test]
fn summary_timestamps_keep_zero_presence_and_negative_components_separate_from_header_dates() {
    let mut doc = CadDocument::new();
    doc.dwg_source_version = Some(DxfVersion::AC1032);
    doc.dwg_summary_timestamps = Some([(0, 0), (2_450_000, -1000)]);
    doc.summary_info.modified_date_julian = Some(2_450_000.0 - 1000.0 / 86_400_000.0);
    doc.header.create_date_julian = 2_460_000.0;
    let value = dump_document(&doc, "", "dwg");
    assert_eq!(value["summaryInfo"]["CreatedDate"], 1_721_426.0);
    assert_eq!(value["summaryInfo"]["ModifiedDate"], 2_450_000.5);
    assert_eq!(value["header"]["CreateDateTime"], 2_460_000.5);
    doc.summary_info.modified_date_julian = Some(2_470_000.25);
    assert_eq!(
        dump_document(&doc, "", "dwg")["summaryInfo"]["ModifiedDate"],
        2_470_000.75
    );
}

#[test]
fn original_handseed_is_retained_without_overriding_the_live_allocator_seed() {
    let mut doc = dxf("9\n$HANDSEED\n5\n40\n");
    assert_eq!(doc.header.source_handle_seed, Some(0x40));
    doc.header.handle_seed = 0x999;
    assert_eq!(header(&doc)["HandleSeed"], 0x999);
    assert_eq!(doc.header.source_handle_seed, Some(0x40));
}
