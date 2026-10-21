// SPDX-License-Identifier: MPL-2.0
//! DXF writer round-trip fixes found by the C#/Rust round-trip gate (G4). Each test writes a synthetic
//! document (or one marked as read from a file) and checks the group codes written or the read-back value.
use crate::entities::mtext::MTextDxfSource;
use crate::entities::*;
use crate::io::dxf::{DxfReader, DxfWriter};
use crate::objects::{ObjectType, Scale};
use crate::tables::*;
use crate::types::{Color, DxfVersion, Handle, Transparency, Vector3};
use crate::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};
use crate::CadDocument;
use std::collections::HashSet;
use std::io::Cursor;

fn mtext_source(decoded: &str) -> MTextDxfSource {
    MTextDxfSource {
        alignment_point: Vector3::UNIT_X,
        line_spacing_style: None,
        has_background_color: false,
        has_background_transparency: false,
        has_embedded_columns: false,
        decoded_value: Some(decoded.to_string()),
    }
}

fn write(doc: &CadDocument) -> String {
    String::from_utf8_lossy(&DxfWriter::new(doc).write_to_vec().expect("write")).into_owned()
}

fn write_binary(doc: &CadDocument) -> Vec<u8> {
    DxfWriter::new_binary(doc).write_to_vec().expect("write binary")
}

fn read(text: &str) -> CadDocument {
    DxfReader::from_reader(Cursor::new(text.as_bytes().to_vec()))
        .expect("reader")
        .read()
        .expect("read")
}

/// The records `0 KIND ... ` of a text DXF, each up to the next group code 0.
fn records<'a>(text: &'a str, kind: &str) -> Vec<&'a str> {
    // A group code 0 line is preceded by the CRLF ending the previous value (a value such as `     0` ends in
    // the same characters).
    let marker = format!("\r\n  0\r\n{kind}\r\n");
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(start) = text[from..].find(&marker) {
        let start = from + start + 2;
        let body = start + marker.len() - 2;
        let end = text[body..].find("\r\n  0\r\n").map_or(text.len(), |end| body + end + 2);
        found.push(&text[start..end]);
        from = end - 2;
    }
    found
}

/// The record of `kind` whose first group 2 (the name) is `name`.
fn named<'a>(text: &'a str, kind: &str, name: &str) -> &'a str {
    records(text, kind)
        .into_iter()
        .find(|record| record.contains(&format!("\r\n  2\r\n{name}\r\n")))
        .unwrap_or_else(|| panic!("no {kind} named {name}"))
}

fn values(record: &str, code: i32) -> Vec<String> {
    let lines: Vec<&str> = record.trim_start_matches("\r\n").split("\r\n").collect();
    let wanted = format!("{code}");
    lines
        .chunks(2)
        .filter(|pair| pair.len() == 2 && pair[0].trim() == wanted)
        .map(|pair| pair[1].trim().to_string())
        .collect()
}

fn first(record: &str, code: i32) -> Option<String> {
    values(record, code).into_iter().next()
}

fn with_handles(mut doc: CadDocument) -> CadDocument {
    doc.assign_table_entry_handles();
    doc
}

fn xdata(records: Vec<ExtendedDataRecord>) -> ExtendedData {
    let mut data = ExtendedData::new();
    for record in records {
        data.add_record(record);
    }
    data
}

fn record(app: &str, values: Vec<XDataValue>) -> ExtendedDataRecord {
    let mut record = ExtendedDataRecord::new(app);
    record.values = values;
    record
}

#[test]
fn text_style_font_xdata_is_written_once_after_a_round_trip() {
    let mut doc = CadDocument::new();
    let mut style = TextStyle::new("Arial");
    style.true_type_font = "Arial".to_string();
    style.true_type_font_flags = 34;
    doc.text_styles.add(style).unwrap();

    let reread = read(&write(&doc));
    let text = write(&reread);
    let style = named(&text, "STYLE", "Arial");
    // The record read back keeps its own ACAD font xdata; a second 1001 ACAD block would be a duplicate application.
    assert_eq!(values(style, 1001), vec!["ACAD"]);
    assert_eq!(values(style, 1071), vec!["34"]);
}

#[test]
fn layer_xdata_merges_by_application_name_ignoring_case_and_keeps_order() {
    let mut doc = CadDocument::new();
    let mut layer = Layer::new("L1");
    layer.transparency = Transparency::Explicit(128);
    layer.description = "desc".to_string();
    doc.layers.add(layer).unwrap();
    let mut doc = with_handles(doc);
    let handle = doc.layers.get("L1").unwrap().handle;
    // A DWG R14 source stores application names in upper case, here ahead of the description record.
    doc.object_xdata.insert(
        handle,
        xdata(vec![
            record(
                "ACAECLAYERSTANDARD",
                vec![XDataValue::String(String::new()), XDataValue::String("old".to_string())],
            ),
            record("ACCMTRANSPARENCY", vec![XDataValue::Integer32(33554560)]),
        ]),
    );

    let text = write(&doc);
    let layer = named(&text, "LAYER", "L1");
    assert_eq!(values(layer, 1001), vec!["ACAECLAYERSTANDARD", "ACCMTRANSPARENCY"]);
    assert!(values(layer, 1000).contains(&"desc".to_string()));
}

#[test]
fn layer_transparency_record_that_still_decodes_to_the_layer_value_is_written_unchanged() {
    let mut doc = CadDocument::new();
    doc.layers.add(Layer::new("L1")).unwrap();
    let mut doc = with_handles(doc);
    let handle = doc.layers.get("L1").unwrap().handle;
    // A packed zero is the opaque default; re-encoding it would give the explicit 33554687.
    doc.object_xdata.insert(
        handle,
        xdata(vec![record("AcCmTransparency", vec![XDataValue::Integer32(0)])]),
    );
    let mut layer = doc.layers.get("L1").unwrap().clone();
    layer.transparency = Transparency::from_layer_alpha_value(0).unwrap();
    doc.layers.add_or_replace(layer);

    let text = write(&doc);
    let layer = named(&text, "LAYER", "L1");
    assert_eq!(values(layer, 1071), vec!["0"]);
}

#[test]
fn symbol_table_flags_keep_the_source_bits_the_model_has_no_field_for() {
    let mut doc = CadDocument::new();
    let mut layer = Layer::new("L1");
    layer.source_flags = 64;
    doc.layers.add(layer).unwrap();
    let mut ltype = LineType::new("DASHED");
    ltype.source_flags = 64;
    doc.line_types.add(ltype).unwrap();
    let mut style = TextStyle::new("S1");
    style.source_flags = 64;
    doc.text_styles.add(style).unwrap();
    let mut appid = AppId::new("MYAPP");
    appid.source_flags = 64;
    doc.app_ids.add(appid).unwrap();

    let text = write(&doc);
    assert_eq!(first(named(&text, "LAYER", "L1"), 70).as_deref(), Some("64"));
    assert_eq!(first(named(&text, "LTYPE", "DASHED"), 70).as_deref(), Some("64"));
    assert_eq!(first(named(&text, "STYLE", "S1"), 70).as_deref(), Some("64"));
    assert_eq!(first(named(&text, "APPID", "MYAPP"), 70).as_deref(), Some("64"));
}

#[test]
fn layer_without_plot_style_gets_the_normal_one_only_in_a_document_built_in_memory() {
    let mut built = CadDocument::new();
    built.layers.add(Layer::new("L1")).unwrap();
    let text = write(&built);
    assert!(first(named(&text, "LAYER", "L1"), 390).is_some());

    let mut from_file = CadDocument::new();
    from_file.layers.add(Layer::new("L1")).unwrap();
    from_file.header.dxf_source_variables = Some(HashSet::new());
    let text = write(&from_file);
    assert!(first(named(&text, "LAYER", "L1"), 390).is_none());
}

#[test]
fn scale_object_keeps_its_reactors() {
    let mut doc = CadDocument::new();
    let owner = doc.header.named_objects_dict_handle;
    let mut scale = Scale::new("1:7", 1.0, 7.0);
    scale.handle = Handle::new(0x7000);
    scale.owner_handle = owner;
    doc.objects.insert(scale.handle, ObjectType::Scale(scale));
    doc.reactors_by_handle.insert(Handle::new(0x7000), vec![owner]);

    let text = write(&doc);
    let scale = records(&text, "SCALE")
        .into_iter()
        .find(|record| record.contains("\r\n  5\r\n7000\r\n"))
        .expect("scale record");
    assert_eq!(values(scale, 102), vec!["{ACAD_REACTORS", "}"]);
    assert_eq!(first(scale, 330), Some(format!("{:X}", owner.value())));
}

#[test]
fn block_markers_write_their_retained_appearance_flags_and_empty_comment() {
    let mut doc = CadDocument::new();
    let mut block = BlockRecord::new("MYBLOCK");
    let mut begin = EntityCommon::new();
    begin.layer = "0".to_string();
    begin.color = Color::Index(1);
    begin.extended_data = xdata(vec![record("MYAPP", vec![XDataValue::Integer16(5)])]);
    block.block_entity_common = Some(Box::new(begin));
    let mut end = EntityCommon::new();
    end.layer = "0".to_string();
    end.color = Color::Index(2);
    block.block_end_common = Some(Box::new(end));
    block.source_flags = 64;
    block.dxf_present = Some(8);
    doc.block_records.add(block).unwrap();
    doc.app_ids.add(AppId::new("MYAPP")).unwrap();

    let text = write(&doc);
    let begin = records(&text, "BLOCK")
        .into_iter()
        .find(|record| record.contains("\r\n  2\r\nMYBLOCK\r\n"))
        .expect("BLOCK");
    assert_eq!(first(begin, 62).as_deref(), Some("1"));
    assert_eq!(first(begin, 70).as_deref(), Some("64"));
    // The source had the description group, even though it is empty.
    assert_eq!(values(begin, 4), vec![""]);
    assert_eq!(values(begin, 1001), vec!["MYAPP"]);
    let end = records(&text, "ENDBLK")
        .into_iter()
        .find(|record| record.contains("\r\n 62\r\n     2\r\n"))
        .expect("ENDBLK with its color");
    assert!(end.contains("AcDbBlockEnd"));
}

#[test]
fn dwg_block_keeps_its_anonymous_bit_without_the_name_saying_so() {
    let mut doc = CadDocument::new();
    doc.dwg_source_version = Some(DxfVersion::AC1018);
    let mut block = BlockRecord::new("my-dynamic-block");
    block.source_flags = 1;
    doc.block_records.add(block).unwrap();

    let text = write(&doc);
    let begin = records(&text, "BLOCK")
        .into_iter()
        .find(|record| record.contains("\r\n  2\r\nmy-dynamic-block\r\n"))
        .expect("BLOCK");
    assert_eq!(first(begin, 70).as_deref(), Some("1"));
    // A DWG R2000+ record always has a comment, empty or not.
    assert_eq!(values(begin, 4), vec![""]);
}

#[test]
fn block_record_groups_the_source_left_out_stay_out() {
    let mut doc = CadDocument::new();
    let mut absent = BlockRecord::new("ABSENT");
    absent.dxf_present = Some(0);
    doc.block_records.add(absent).unwrap();
    let mut present = BlockRecord::new("PRESENT");
    present.dxf_present = Some(1 | 2 | 32);
    present.explodable = true;
    doc.block_records.add(present).unwrap();
    let mut changed = BlockRecord::new("CHANGED");
    changed.dxf_present = Some(0);
    changed.explodable = false;
    doc.block_records.add(changed).unwrap();

    let text = write(&doc);
    let absent = named(&text, "BLOCK_RECORD", "ABSENT");
    assert!(first(absent, 280).is_none() && first(absent, 281).is_none() && first(absent, 70).is_none());
    let present = named(&text, "BLOCK_RECORD", "PRESENT");
    assert_eq!(first(present, 280).as_deref(), Some("1"));
    assert_eq!(first(present, 70).as_deref(), Some("0"));
    let changed = named(&text, "BLOCK_RECORD", "CHANGED");
    assert_eq!(first(changed, 280).as_deref(), Some("0"));
}

#[test]
fn dimension_style_omits_groups_its_dxf_source_lacked_and_keeps_changed_ones() {
    let mut doc = CadDocument::new();
    let mut style = DimStyle::dxf_defaults("FROMFILE");
    style.dxf_present_codes = Some([0; 6]);
    style.record_dxf_code(70);
    style.record_dxf_code(41);
    doc.dim_styles.add(style).unwrap();
    let mut edited = DimStyle::dxf_defaults("EDITED");
    edited.dxf_present_codes = Some([0; 6]);
    edited.dimdec = 2;
    doc.dim_styles.add(edited).unwrap();
    let programmatic = DimStyle::new("BUILT");
    doc.dim_styles.add(programmatic).unwrap();

    let text = write(&doc);
    let from_file = named(&text, "DIMSTYLE", "FROMFILE");
    assert!(first(from_file, 271).is_none(), "absent DIMDEC must stay absent");
    assert!(first(from_file, 41).is_some(), "present DIMASZ is written");
    assert_eq!(first(named(&text, "DIMSTYLE", "EDITED"), 271).as_deref(), Some("2"));
    assert!(first(named(&text, "DIMSTYLE", "BUILT"), 271).is_some());
}

#[test]
fn dimension_style_keeps_its_reactors() {
    let mut doc = CadDocument::new();
    doc.dim_styles.add(DimStyle::new("WITHDIMS")).unwrap();
    let mut doc = with_handles(doc);
    let handle = doc.dim_styles.get("WITHDIMS").unwrap().handle;
    let owner = doc.header.named_objects_dict_handle;
    doc.reactors_by_handle.insert(handle, vec![owner]);

    let text = write(&doc);
    let style = named(&text, "DIMSTYLE", "WITHDIMS");
    assert_eq!(values(style, 102), vec!["{ACAD_REACTORS", "}"]);
}

#[test]
fn dimension_style_r2007_variables_are_not_invented_for_records_read_from_an_older_dwg() {
    let mut style = DimStyle::new("OLD");
    style.dimfxl = 0.0;
    let mut old_dwg = CadDocument::with_version(DxfVersion::AC1015);
    old_dwg.dwg_source_version = Some(DxfVersion::AC1015);
    old_dwg.dim_styles.add(style.clone()).unwrap();
    let text = write(&old_dwg);
    assert!(first(named(&text, "DIMSTYLE", "OLD"), 49).is_none());
    assert!(first(named(&text, "DIMSTYLE", "OLD"), 50).is_none());

    let mut new_dwg = CadDocument::with_version(DxfVersion::AC1032);
    new_dwg.dwg_source_version = Some(DxfVersion::AC1032);
    new_dwg.dim_styles.add(style.clone()).unwrap();
    let text = write(&new_dwg);
    assert_eq!(first(named(&text, "DIMSTYLE", "OLD"), 49).as_deref(), Some("0.0"));

    // A document that did not come from an older DWG keeps writing them (BricsCAD wants a valid DIMJOGANG).
    let mut built = CadDocument::with_version(DxfVersion::AC1015);
    built.dim_styles.add(style).unwrap();
    let text = write(&built);
    assert_eq!(first(named(&text, "DIMSTYLE", "OLD"), 49).as_deref(), Some("0.0"));
}

#[test]
fn current_dimension_style_name_comes_from_the_header_handle_when_the_name_is_the_default() {
    let mut doc = CadDocument::new();
    doc.dim_styles.add(DimStyle::new("CUSTOM")).unwrap();
    let mut doc = with_handles(doc);
    doc.header.current_dimstyle_handle = doc.dim_styles.get("CUSTOM").unwrap().handle;
    assert_eq!(doc.header.current_dimstyle_name, "Standard");

    let text = write(&doc);
    assert!(text.contains("  9\r\n$DIMSTYLE\r\n  2\r\nCUSTOM\r\n"));
}

#[test]
fn linetype_shape_element_survives_an_empty_text_group() {
    use crate::tables::linetype::{LineTypeComplexData, LineTypeComplexContent};
    let mut doc = CadDocument::new();
    let mut ltype = LineType::new("TRACKS");
    let mut element = LineTypeElement::dash(0.15);
    let mut complex = LineTypeComplexData::default();
    complex.content = LineTypeComplexContent::Shape { shape_number: 130 };
    complex.source_flags = 4;
    element.complex = Some(complex);
    ltype.elements.push(element);
    ltype.elements.push(LineTypeElement::dash(0.15));
    doc.line_types.add(ltype).unwrap();

    // ACadSharp writes an empty group 9 after a shape element.
    let text = write(&doc).replace("\r\n 46\r\n", "\r\n  9\r\n\r\n 46\r\n");
    assert!(text.contains("  9\r\n\r\n"), "test setup: empty text group present");
    let reread = read(&text);
    let ltype = reread.line_types.get("TRACKS").expect("TRACKS");
    let complex = ltype.elements[0].complex.as_ref().expect("complex element");
    assert!(complex.is_shape(), "an empty 9 must not turn the shape into text");
    assert!(matches!(
        complex.content,
        LineTypeComplexContent::Shape { shape_number: 130 }
    ));
}

#[test]
fn mtext_source_spelling_and_line_breaks_round_trip_in_ascii_dxf() {
    let mut doc = CadDocument::new();
    let mut mtext = MText::new();
    mtext.value = "94\u{b0}\nnext".to_string();
    // As the reader leaves it: caret escapes decoded, \U+ escapes still spelled.
    mtext.source_value = Some("94\\U+00B0\nnext".to_string());
    mtext.dxf_source = Some(mtext_source(&mtext.value));
    doc.add_entity(EntityType::MText(Box::new(mtext))).unwrap();

    let text = write(&doc);
    let mtext = records(&text, "MTEXT").pop().expect("MTEXT");
    assert_eq!(first(mtext, 1).as_deref(), Some("94\\U+00B0^Jnext"));
    let reread = read(&text);
    let value = reread
        .entities()
        .find_map(|e| match e {
            EntityType::MText(m) => Some(m.value.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(value, "94\u{b0}\nnext");
}

#[test]
fn mtext_without_source_spelling_writes_a_line_feed_as_a_caret_escape() {
    // `\P` is a paragraph break, not a line feed: the line feed keeps its meaning, spelled as AutoCAD spells it.
    let mut doc = CadDocument::new();
    let mut mtext = MText::new();
    mtext.value = "one\ntwo\\Pthree".to_string();
    doc.add_entity(EntityType::MText(Box::new(mtext))).unwrap();
    let text = write(&doc);
    let mtext = records(&text, "MTEXT").pop().expect("MTEXT");
    assert_eq!(first(mtext, 1).as_deref(), Some("one^Jtwo\\Pthree"));
}

#[test]
fn mtext_background_transparency_is_written_even_when_zero() {
    let mut doc = CadDocument::new();
    let mut mtext = MText::new();
    mtext.value = "x".to_string();
    mtext.background_fill_flags = 1;
    mtext.background_transparency = 0;
    doc.add_entity(EntityType::MText(Box::new(mtext.clone()))).unwrap();
    // A DXF source without the group keeps it out.
    let mut source = mtext_source("x");
    source.has_background_transparency = false;
    mtext.dxf_source = Some(source);
    doc.add_entity(EntityType::MText(Box::new(mtext))).unwrap();
    let text = write(&doc);
    let mtexts = records(&text, "MTEXT");
    assert_eq!(mtexts.len(), 2);
    assert_eq!(first(mtexts[0], 441).as_deref(), Some("0"));
    assert!(first(mtexts[1], 441).is_none());
}

#[test]
fn tolerance_text_uses_caret_line_breaks_and_an_omitted_direction_stays_omitted() {
    let mut doc = CadDocument::new();
    let mut tolerance = Tolerance::new();
    tolerance.text = "a\nb".to_string();
    tolerance.dxf_source = Some(ToleranceDxfSource {
        has_direction: false,
        raw_text: None,
    });
    doc.add_entity(EntityType::Tolerance(tolerance.clone())).unwrap();
    tolerance.dxf_source = None;
    doc.add_entity(EntityType::Tolerance(tolerance)).unwrap();

    let text = write(&doc);
    let tolerances = records(&text, "TOLERANCE");
    assert_eq!(tolerances.len(), 2);
    assert_eq!(first(tolerances[0], 1).as_deref(), Some("a^Jb"));
    assert!(first(tolerances[0], 11).is_none());
    assert!(first(tolerances[1], 11).is_some(), "a programmatic tolerance writes its direction");
}

#[test]
fn attribute_definition_text_generation_flags_are_omitted_when_zero() {
    let mut doc = CadDocument::new();
    let mut attdef = AttributeDefinition::new("TAG".to_string(), "Prompt".to_string(), "default".to_string());
    attdef.text_generation_flags = 0;
    doc.add_entity(EntityType::AttributeDefinition(attdef.clone())).unwrap();
    attdef.text_generation_flags = 2;
    doc.add_entity(EntityType::AttributeDefinition(attdef)).unwrap();

    let text = write(&doc);
    let attdefs = records(&text, "ATTDEF");
    assert_eq!(attdefs.len(), 2);
    let text_part = |record: &str| record.split("AcDbAttributeDefinition").next().unwrap().to_string();
    assert!(first(&text_part(attdefs[0]), 71).is_none());
    assert_eq!(first(&text_part(attdefs[1]), 71).as_deref(), Some("2"));
}

#[test]
fn dimension_keeps_the_shared_block_bit_and_omitted_attachment_and_spacing_groups() {
    let mut doc = CadDocument::new();
    let mut from_file = DimensionLinear::new(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
    from_file.base.dxf_source = Some(DimensionDxfSource {
        raw_flags: Some(32),
        ..Default::default()
    });
    doc.add_entity(EntityType::Dimension(Box::new(Dimension::Linear(from_file)))).unwrap();
    let built = DimensionLinear::new(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
    doc.add_entity(EntityType::Dimension(Box::new(Dimension::Linear(built)))).unwrap();

    let text = write(&doc);
    let dims = records(&text, "DIMENSION");
    assert_eq!(first(dims[0], 70).as_deref(), Some("32"));
    assert!(first(dims[0], 71).is_none() && first(dims[0], 72).is_none());
    assert_eq!(first(dims[1], 70).as_deref(), Some("0"));
    assert!(first(dims[1], 71).is_some() && first(dims[1], 72).is_some());
}

#[test]
fn header_variables_absent_from_a_dxf_source_and_unchanged_are_not_written() {
    let mut doc = CadDocument::new();
    doc.header.dxf_source_variables = Some(["$ACADVER".to_string(), "$LTSCALE".to_string()].into());
    let text = write(&doc);
    assert!(!text.contains("$OSMODE"), "unchanged variable the source lacked");
    assert!(text.contains("  9\r\n$LTSCALE\r\n"), "variable the source had");
    assert!(text.contains("  9\r\n$ACADVER\r\n"));

    doc.header.object_snap_mode = 5;
    let text = write(&doc);
    assert!(text.contains("  9\r\n$OSMODE\r\n 70\r\n     5\r\n"), "changed variable is written");

    let built = CadDocument::new();
    assert!(write(&built).contains("$OSMODE"), "a document built in memory writes everything");
}

#[test]
fn header_multiline_scale_and_justification_are_written() {
    let mut doc = CadDocument::new();
    doc.header.multiline_scale = 1.0;
    doc.header.multiline_justification = 1;
    let text = write(&doc);
    assert!(text.contains("  9\r\n$CMLJUST\r\n 70\r\n     1\r\n"));
    assert!(text.contains("  9\r\n$CMLSCALE\r\n 40\r\n1.0\r\n"));
    let reread = read(&text);
    assert_eq!(reread.header.multiline_scale, 1.0);
    assert_eq!(reread.header.multiline_justification, 1);
}

#[test]
fn layout_without_a_plot_view_name_does_not_write_group_6() {
    let doc = CadDocument::new();
    let text = write(&doc);
    let layouts = records(&text, "LAYOUT");
    assert!(!layouts.is_empty());
    for layout in layouts {
        let plot = layout
            .split("AcDbPlotSettings")
            .nth(1)
            .and_then(|rest| rest.split("AcDbLayout").next())
            .unwrap_or("");
        assert!(first(plot, 6).is_none(), "empty plot view name written: {plot}");
    }
}

#[test]
fn binary_dxf_keeps_source_spelling_and_line_breaks_of_mtext() {
    let mut doc = CadDocument::new();
    let mut mtext = MText::new();
    mtext.value = "a\u{b0}\nb".to_string();
    mtext.source_value = Some("a\\U+00B0\nb".to_string());
    mtext.dxf_source = Some(mtext_source(&mtext.value));
    doc.add_entity(EntityType::MText(Box::new(mtext))).unwrap();
    let bytes = write_binary(&doc);
    let needle = b"a\\U+00B0\nb\0";
    assert!(
        bytes.windows(needle.len()).any(|window| window == needle),
        "binary DXF should hold the source spelling with its literal line break"
    );
}

#[test]
fn binary_mtext_chunks_do_not_cut_a_unicode_escape() {
    let mut doc = CadDocument::new();
    let mut mtext = MText::new();
    let prefix = "a".repeat(247);
    mtext.value = format!("{prefix}\u{b0}\nb");
    mtext.source_value = Some(format!("{prefix}\\U+00B0\nb"));
    mtext.dxf_source = Some(mtext_source(&mtext.value));
    let value = mtext.value.clone();
    doc.add_entity(EntityType::MText(Box::new(mtext))).unwrap();
    let bytes = write_binary(&doc);
    let reread = DxfReader::from_reader(Cursor::new(bytes)).expect("reader").read().expect("read");
    let text = reread
        .entities()
        .find_map(|entity| match entity {
            EntityType::MText(mtext) => Some(mtext.value.clone()),
            _ => None,
        })
        .expect("mtext");
    assert_eq!(text, value);
}

#[test]
fn alignment_grip_orientation_uses_groups_140_141_142() {
    use crate::objects::{BlockOrientedGrip, DynamicBlockData, DynamicBlockObject};
    let mut doc = CadDocument::new();
    let mut object = DynamicBlockObject::new("BLOCKALIGNMENTGRIP", "AcDbBlockAlignmentGrip");
    object.handle = Handle::new(0x7100);
    object.owner = doc.header.named_objects_dict_handle;
    object.data = DynamicBlockData::AlignmentGrip(BlockOrientedGrip {
        orientation: Vector3::new(14.5, 8.25, 0.5),
        ..Default::default()
    });
    doc.objects.insert(object.handle, ObjectType::DynamicBlock(object));

    let text = write(&doc);
    let grip = records(&text, "BLOCKALIGNMENTGRIP").pop().expect("grip");
    let tail = grip.split("AcDbBlockAlignmentGrip").nth(1).expect("alignment subclass");
    assert_eq!(first(tail, 140).as_deref(), Some("14.5"));
    assert_eq!(first(tail, 141).as_deref(), Some("8.25"));
    assert_eq!(first(tail, 142).as_deref(), Some("0.5"));
    assert!(first(tail, 150).is_none() && first(tail, 160).is_none());
}

#[test]
fn leader_groups_the_dxf_source_left_out_stay_out_unless_changed() {
    let mut doc = CadDocument::new();
    let mut leader = Leader::new();
    leader.add_vertex(Vector3::ZERO);
    leader.add_vertex(Vector3::new(1.0, 1.0, 0.0));
    leader.dxf_source.mark_source();
    leader.dxf_source.insert(71);
    doc.add_entity(EntityType::Leader(leader.clone())).unwrap();
    leader.text_height = 3.5;
    doc.add_entity(EntityType::Leader(leader)).unwrap();
    let built = {
        let mut built = Leader::new();
        built.add_vertex(Vector3::ZERO);
        built
    };
    doc.add_entity(EntityType::Leader(built)).unwrap();

    let text = write(&doc);
    let leaders = records(&text, "LEADER");
    assert_eq!(leaders.len(), 3);
    assert!(first(leaders[0], 71).is_some());
    assert!(first(leaders[0], 40).is_none() && first(leaders[0], 41).is_none() && first(leaders[0], 73).is_none());
    assert_eq!(first(leaders[1], 40).as_deref(), Some("3.5"));
    assert!(first(leaders[2], 40).is_some() && first(leaders[2], 73).is_some());
}

#[test]
fn multileader_text_attachments_the_source_left_out_stay_out() {
    use crate::entities::multileader::MultiLeaderDxfSource;
    let mut doc = CadDocument::new();
    let mut mleader = MultiLeader::new();
    mleader.dxf_source = Some(Box::new(MultiLeaderDxfSource::default()));
    doc.add_entity(EntityType::MultiLeader(Box::new(mleader.clone()))).unwrap();
    mleader.dxf_source = None;
    doc.add_entity(EntityType::MultiLeader(Box::new(mleader))).unwrap();

    let text = write(&doc);
    let mleaders = records(&text, "MULTILEADER");
    assert_eq!(mleaders.len(), 2);
    // The top-level 272/273 pair is the only difference between the two records.
    for code in [272, 273] {
        assert_eq!(
            values(mleaders[0], code).len() + 1,
            values(mleaders[1], code).len(),
            "group {code}"
        );
    }
}

#[test]
fn multileader_style_groups_the_source_left_out_stay_out_unless_changed() {
    use crate::objects::MultiLeaderStyle;
    let mut doc = CadDocument::new();
    let mut from_file = MultiLeaderStyle::new("FROMFILE");
    from_file.handle = Handle::new(0x7200);
    from_file.owner_handle = doc.header.named_objects_dict_handle;
    from_file.dxf_present_codes = Some([0; 6]);
    doc.objects.insert(from_file.handle, ObjectType::MultiLeaderStyle(from_file.clone()));
    let mut edited = from_file.clone();
    edited.handle = Handle::new(0x7201);
    edited.align_space = 7.0;
    doc.objects.insert(edited.handle, ObjectType::MultiLeaderStyle(edited));

    let text = write(&doc);
    let styles = records(&text, "MLEADERSTYLE");
    let by_handle = |handle: &str| {
        styles
            .iter()
            .find(|record| record.contains(&format!("\r\n  5\r\n{handle}\r\n")))
            .copied()
            .unwrap_or_else(|| panic!("no style {handle}"))
    };
    assert!(first(by_handle("7200"), 46).is_none() && first(by_handle("7200"), 174).is_none());
    assert_eq!(first(by_handle("7201"), 46).as_deref(), Some("7.0"));
}

#[test]
fn viewport_status_grid_bits_and_default_grid_major_follow_the_source() {
    use crate::entities::viewport::ViewportSource;
    let mut doc = CadDocument::with_version(DxfVersion::AC1018);
    let mut viewport = Viewport::new();
    viewport.grid_major = 5;
    viewport.grid_flags = crate::entities::viewport::GridFlags::from_bits(3);
    let grid_word = i32::from(viewport.grid_flags.to_bits()) << 18;
    viewport.source = Some(Box::new(ViewportSource {
        dxf_codes: Some([0; 7]),
        status_flags: Some(viewport.status.to_bits() | grid_word),
    }));
    doc.add_entity(EntityType::Viewport(Box::new(viewport.clone()))).unwrap();
    viewport.source = None;
    doc.add_entity(EntityType::Viewport(Box::new(viewport))).unwrap();

    let text = write(&doc);
    let viewports = records(&text, "VIEWPORT");
    assert_eq!(viewports.len(), 2);
    let status = |record: &str| first(record, 90).unwrap().parse::<i32>().unwrap();
    assert_ne!(grid_word, 0);
    assert_eq!(status(viewports[0]) & (0xFF << 18), grid_word, "pre-R2007 source keeps its grid bits");
    assert_eq!(status(viewports[1]) & (0xFF << 18), 0, "a programmatic viewport writes none before R2007");
    assert!(first(viewports[0], 61).is_none(), "grid major absent from the DXF source stays absent");
    assert_eq!(first(viewports[1], 61).as_deref(), Some("5"));
}

#[test]
fn xrecord_string_keeps_the_source_spelling_and_the_geodata_definition_uses_caret_line_breaks() {
    use crate::objects::{GeoData, XRecord, XRecordEntry};
    let mut doc = CadDocument::new();
    let mut xrecord = XRecord::new();
    xrecord.handle = Handle::new(0x7300);
    xrecord.owner = doc.header.named_objects_dict_handle;
    xrecord.add_entry(XRecordEntry::string(1, "\u{2205}45"));
    xrecord.set_source_value(0, "\\U+220545".to_string());
    doc.objects.insert(xrecord.handle, ObjectType::XRecord(xrecord));
    let mut geo = GeoData::new();
    geo.handle = Handle::new(0x7301);
    geo.owner = doc.header.named_objects_dict_handle;
    geo.coordinate_system_definition = "<a>\n</a>".to_string();
    doc.objects.insert(geo.handle, ObjectType::GeoData(geo));

    let text = write(&doc);
    let xrecord = records(&text, "XRECORD")
        .into_iter()
        .find(|record| record.contains("\r\n  5\r\n7300\r\n"))
        .expect("xrecord");
    assert_eq!(first(xrecord, 1).as_deref(), Some("\\U+220545"));
    let geo = records(&text, "GEODATA").pop().expect("geodata");
    assert_eq!(first(geo, 301).as_deref(), Some("<a>^J</a>"));
}

#[test]
fn spatial_filter_keeps_its_reactors() {
    use crate::objects::SpatialFilter;
    let mut doc = CadDocument::new();
    let owner = doc.header.named_objects_dict_handle;
    let mut filter = SpatialFilter::new();
    filter.handle = Handle::new(0x7400);
    filter.owner = owner;
    doc.objects.insert(filter.handle, ObjectType::SpatialFilter(filter));
    doc.reactors_by_handle.insert(Handle::new(0x7400), vec![owner]);

    let text = write(&doc);
    let filter = records(&text, "SPATIAL_FILTER").pop().expect("filter");
    assert_eq!(values(filter, 102), vec!["{ACAD_REACTORS", "}"]);
}

#[test]
fn r2000_dimension_and_mtext_groups_are_not_written_into_an_r14_file() {
    for (version, expect) in [(DxfVersion::AC1014, false), (DxfVersion::AC1015, true)] {
        let mut doc = CadDocument::with_version(version);
        let mut dim = DimensionLinear::new(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
        dim.base.line_spacing_factor = 2.0;
        doc.add_entity(EntityType::Dimension(Box::new(Dimension::Linear(dim)))).unwrap();
        let mut mtext = MText::new();
        mtext.value = "x".to_string();
        doc.add_entity(EntityType::MText(Box::new(mtext))).unwrap();

        let text = write(&doc);
        let dim = records(&text, "DIMENSION").pop().expect("DIMENSION");
        for code in [71, 72, 41] {
            assert_eq!(first(dim, code).is_some(), expect, "dimension group {code} in {version:?}");
        }
        let mtext = records(&text, "MTEXT").pop().expect("MTEXT");
        for code in [73, 44] {
            assert_eq!(first(mtext, code).is_some(), expect, "mtext group {code} in {version:?}");
        }
    }
}

#[test]
fn a_zero_dimension_line_spacing_factor_is_the_unset_value_and_is_not_written() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut dim = DimensionLinear::new(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
    dim.base.line_spacing_factor = 0.0;
    doc.add_entity(EntityType::Dimension(Box::new(Dimension::Linear(dim)))).unwrap();
    let text = write(&doc);
    let dim = records(&text, "DIMENSION").pop().expect("DIMENSION");
    assert!(first(dim, 41).is_none());
}

#[test]
fn pre_r2004_background_and_gradient_restored_from_legacy_data_stay_in_that_data() {
    use crate::entities::hatch::HatchGradientPattern;
    let mut doc = CadDocument::with_version(DxfVersion::AC1015);
    let mut mtext = MText::new();
    mtext.value = "x".to_string();
    mtext.background_fill_flags = 1;
    mtext.common.extended_data = xdata(vec![record(
        "ACAD",
        vec![XDataValue::String("ACAD_MTEXT_BBRT".to_string())],
    )]);
    doc.add_entity(EntityType::MText(Box::new(mtext.clone()))).unwrap();
    mtext.common.extended_data = ExtendedData::new();
    doc.add_entity(EntityType::MText(Box::new(mtext))).unwrap();
    let mut hatch = Hatch::new();
    hatch.gradient_color.enabled = true;
    hatch.gradient_recovery_source = Some(Box::new(HatchGradientPattern::default()));
    doc.add_entity(EntityType::Hatch(Box::new(hatch.clone()))).unwrap();
    hatch.gradient_recovery_source = None;
    doc.add_entity(EntityType::Hatch(Box::new(hatch))).unwrap();

    let text = write(&doc);
    let mtexts = records(&text, "MTEXT");
    assert!(first(mtexts[0], 90).is_none(), "restored fill: legacy data only");
    assert!(first(mtexts[1], 90).is_some(), "no legacy data: the native groups are the only carrier");
    let hatches = records(&text, "HATCH");
    assert!(first(hatches[0], 450).is_none());
    assert!(first(hatches[1], 450).is_some());
}

#[test]
fn book_color_without_a_source_color_writes_none_and_tolerance_keeps_its_raw_text() {
    use crate::objects::BookColor;
    let mut doc = CadDocument::new();
    let mut color = BookColor::default();
    color.handle = Handle::new(0x7600);
    color.owner = doc.header.named_objects_dict_handle;
    color.dxf_color_present = Some(false);
    doc.objects.insert(color.handle, ObjectType::BookColor(color.clone()));
    color.handle = Handle::new(0x7601);
    color.dxf_color_present = None;
    doc.objects.insert(color.handle, ObjectType::BookColor(color));
    let mut tolerance = Tolerance::new();
    tolerance.text = "a\nb".to_string();
    tolerance.dxf_source = Some(ToleranceDxfSource {
        has_direction: true,
        raw_text: Some("a^Jb".to_string()),
    });
    doc.add_entity(EntityType::Tolerance(tolerance)).unwrap();

    let text = write(&doc);
    let colors = records(&text, "DBCOLOR");
    let by_handle = |handle: &str| {
        colors
            .iter()
            .find(|record| record.contains(&format!("\r\n  5\r\n{handle}\r\n")))
            .copied()
            .unwrap()
    };
    assert!(first(by_handle("7600"), 62).is_none());
    assert!(first(by_handle("7601"), 62).is_some());
    let tolerance = records(&text, "TOLERANCE").pop().unwrap();
    assert_eq!(first(tolerance, 1).as_deref(), Some("a^Jb"));
    // Binary DXF holds the line break itself (as AutoCAD writes it); a caret escape there would be read as text.
    let bytes = write_binary(&doc);
    let needle = b"a\nb\0";
    assert!(bytes.windows(needle.len()).any(|window| window == needle), "binary holds the line break");
    let reread = DxfReader::from_reader(Cursor::new(bytes)).expect("reader").read().expect("read");
    let text = reread
        .entities()
        .find_map(|entity| match entity {
            EntityType::Tolerance(tolerance) => Some(tolerance.text.clone()),
            _ => None,
        })
        .expect("tolerance");
    assert_eq!(text, "a\nb");
}

#[test]
fn xrecord_text_dxf_spelling_is_written_to_binary_with_the_characters() {
    use crate::objects::{XRecord, XRecordEntry};
    let mut doc = CadDocument::new();
    let mut xrecord = XRecord::new();
    xrecord.handle = Handle::new(0x7310);
    xrecord.owner = doc.header.named_objects_dict_handle;
    xrecord.add_entry(XRecordEntry::string(1, "a\nb\u{2205}^c"));
    // As the text DXF reader retains it: caret escapes and the \U+ escape spelled out.
    xrecord.set_source_value(0, "a^Jb\\U+2205^ c".to_string());
    doc.objects.insert(xrecord.handle, ObjectType::XRecord(xrecord));

    let text = write(&doc);
    let record = records(&text, "XRECORD")
        .into_iter()
        .find(|record| record.contains("\r\n  5\r\n7310\r\n"))
        .expect("xrecord");
    assert_eq!(first(record, 1).as_deref(), Some("a^Jb\\U+2205^ c"));
    let bytes = write_binary(&doc);
    let needle = b"a\nb\\U+2205^c\0";
    assert!(bytes.windows(needle.len()).any(|window| window == needle), "binary holds the characters");
    for doc in [read(&text), DxfReader::from_reader(Cursor::new(bytes)).expect("reader").read().expect("read")] {
        let Some(ObjectType::XRecord(xrecord)) = doc.objects.get(&Handle::new(0x7310)) else {
            panic!("xrecord missing");
        };
        assert_eq!(xrecord.entries[0].value.as_string(), Some("a\nb\u{2205}^c"));
    }
}
