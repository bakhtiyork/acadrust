// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]
//! R5b-11 (gate G5-RR and the sample_V cells): the round-trip records AutoCAD's down-saves hold for the dropped chains of the
//! sample_V report, and the extension dictionaries of the layers in a DXF. Synthetic documents only; each test fails with its fix
//! reverted. Evidence: AutoCAD's own files of one drawing (sample_AC1014.dwg to sample_AC1032.dwg and the DXF twins; the C# dumps
//! under Tests/CadCoreTests/Fixtures/dumps).

use std::io::Cursor;

use acadrust::objects::{Dictionary, ObjectType};
use acadrust::tables::Layer;
use acadrust::types::{Color, DxfVersion, Handle};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfWriter};

fn write_and_read(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    let bytes = DwgWriter::write_to_vec(&document).unwrap_or_else(|error| panic!("{version:?}: DWG write failed: {error}"));
    DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .unwrap_or_else(|error| panic!("{version:?}: DWG read failed: {error}"))
}

fn dxf_write_and_read(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    let bytes = DxfWriter::new(&document).write_to_vec().unwrap_or_else(|error| panic!("{version:?}: DXF write failed: {error}"));
    DxfReader::from_reader(Cursor::new(bytes))
        .unwrap_or_else(|error| panic!("{version:?}: DXF reader failed: {error}"))
        .read()
        .unwrap_or_else(|error| panic!("{version:?}: DXF read failed: {error}"))
}

fn layer<'a>(document: &'a CadDocument, name: &str) -> &'a Layer {
    document.layers.iter().find(|layer| layer.name.eq_ignore_ascii_case(name)).unwrap_or_else(|| panic!("no layer {name}"))
}

/// An empty extension dictionary on the layer `name`, as AutoCAD's files hold on layer 0 and `Layer2`.
fn give_empty_dictionary(document: &mut CadDocument, name: &str) -> Handle {
    let owner = layer(document, name).handle;
    let handle = document.allocate_handle();
    let mut dictionary = Dictionary::new();
    dictionary.handle = handle;
    dictionary.owner = owner;
    dictionary.hard_owner = true;
    document.objects.insert(handle, ObjectType::Dictionary(dictionary));
    handle
}

/// AutoCAD's DXF of R2000 has an extension dictionary on the layers `0` and `Layer2` (empty in sample_AC1015.dwg too) and none on the
/// layers that held only a colour record in the DWG (`Layer_true_color`, `Layer_color_book`, `color_125_33_79`): the dictionary a
/// consumed record left empty is not written to a DXF; an empty dictionary the file held stays.
#[test]
fn a_dxf_has_no_extension_dictionary_a_consumed_layer_record_left_empty() {
    let mut source = CadDocument::new();
    let mut true_color = Layer::new("TRUE_COLOR");
    true_color.color = Color::Rgb { r: 17, g: 146, b: 238 };
    source.layers.add(true_color).unwrap();
    source.layers.add(Layer::new("HELD_EMPTY")).unwrap();
    // read back once so that the table records hold their handles
    let mut source = write_and_read(&source, DxfVersion::AC1032);
    give_empty_dictionary(&mut source, "HELD_EMPTY");
    let dwg = write_and_read(&source, DxfVersion::AC1015);
    assert!(dwg.extension_dictionary_handle(layer(&dwg, "TRUE_COLOR").handle).is_some(), "the DWG keeps the dictionary the record left");
    assert!(dwg.extension_dictionary_handle(layer(&dwg, "HELD_EMPTY").handle).is_some());
    for version in [DxfVersion::AC1015, DxfVersion::AC1018] {
        let dxf = dxf_write_and_read(&dwg, version);
        assert!(dxf.extension_dictionary_handle(layer(&dxf, "TRUE_COLOR").handle).is_none(), "{version:?}: the emptied dictionary is not written");
        assert!(dxf.extension_dictionary_handle(layer(&dxf, "HELD_EMPTY").handle).is_some(), "{version:?}: a dictionary the file held empty stays");
    }
    // the DWG written again keeps the record and its dictionary
    let again = write_and_read(&dwg, DxfVersion::AC1015);
    assert!(again.extension_dictionary_handle(layer(&again, "TRUE_COLOR").handle).is_some());
}

fn multiline_attribute_with_suffixed_source() -> CadDocument {
    let mut text = acadrust::entities::MText::with_value("first line\\Psecond line", acadrust::types::Vector3::new(12.0, 3.0, 0.0));
    text.height = 2.5;
    let mut document = CadDocument::new();
    let mut definition = acadrust::entities::AttributeDefinition::new("MLTAG".into(), "prompt".into(), String::new());
    definition.mtext_flag = acadrust::entities::MTextFlag::ConstantMultiLine;
    definition.is_multiline = true;
    definition.embedded_mtext = Some(Box::new(text));
    // as a reader of a file before R2018 leaves it: the outer tag spelled `MLTAG_001`, the recovered tag `MLTAG`
    let mut source = acadrust::entities::text::TextSource::default();
    source.tag = Some(("MLTAG_001".into(), "MLTAG".into()));
    definition.source = Some(Box::new(source));
    document.add_entity(acadrust::entities::EntityType::AttributeDefinition(definition)).unwrap();
    document
}

fn first_definition_tag(document: &CadDocument) -> String {
    document
        .entities()
        .find_map(|entity| match entity {
            acadrust::entities::EntityType::AttributeDefinition(definition) => Some(definition.tag.clone()),
            _ => None,
        })
        .expect("the attribute definition")
}

/// AutoCAD's R2018 file holds a multi-line attribute with its own tag (sample_AC1032.dwg: `MULTI_LINE_ATT`), the files before R2018
/// spell the outer tag `MULTI_LINE_ATT_001` (sample_AC1018.dwg: #79D). A drawing read from the older spelling writes the recovered
/// tag to R2018, in a DWG and in a DXF.
#[test]
fn a_multiline_attribute_read_from_the_suffixed_spelling_is_written_with_its_own_tag_at_r2018() {
    let source = multiline_attribute_with_suffixed_source();
    assert_eq!(first_definition_tag(&write_and_read(&source, DxfVersion::AC1032)), "MLTAG");
    assert_eq!(first_definition_tag(&dxf_write_and_read(&source, DxfVersion::AC1032)), "MLTAG");
    // the older versions keep the file's spelling, which the reader recovers from the record
    assert_eq!(first_definition_tag(&write_and_read(&source, DxfVersion::AC1018)), "MLTAG");
}

use acadrust::entities::{BoundaryEdge, BoundaryPath, EntityType, Hatch, PolylineEdge};
use acadrust::objects::XRecordValue;
use acadrust::types::{Vector2, Vector3};

/// The two records of sample_AC1015.dwg: the hatches #371 (LINEAR) and #376 (SPHERICAL), the colours 0x0000FF and 0xFFFF00.
const LINEAR_RECORD: &str = "7b000000020000000100000000000000000000000000000000000000000000000000000000000000000000000200000000000000000000001800000001000000ff0000c2000000000000000000000000000000000000f03f180000000100000000ffffc2000000000000000000000000070000004c494e45415200";
const SPHERICAL_RECORD: &str = "7e000000020000000100000000000000000000000000000000000000000000000000000000000000000000000200000000000000000000001800000001000000ff0000c2000000000000000000000000000000000000f03f180000000100000000ffffc20000000000000000000000000a00000053504845524943414c00";

fn hex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap()).collect()
}

fn gradient_hatch(name: &str) -> Hatch {
    let mut hatch = Hatch::solid();
    let mut path = BoundaryPath::new();
    path.edges.push(BoundaryEdge::Polyline(PolylineEdge::new(
        vec![Vector2::new(0.0, 0.0), Vector2::new(4.0, 0.0), Vector2::new(4.0, 3.0), Vector2::new(0.0, 3.0)],
        true,
    )));
    hatch.paths.push(path);
    hatch.gradient_color.enabled = true;
    hatch.gradient_color.name = name.into();
    hatch.gradient_color.add_color(0.0, Color::Rgb { r: 0, g: 0, b: 255 });
    hatch.gradient_color.add_color(1.0, Color::Rgb { r: 255, g: 255, b: 0 });
    // the nearest indexes AutoCAD keeps in the extended data of the hatch in every version (sample_AC1015.dwg #371: 5 and 2)
    for (application, index) in [("GradientColor1ACI", 5), ("GradientColor2ACI", 2)] {
        let mut record = acadrust::xdata::ExtendedDataRecord::new(application);
        record.add_value(acadrust::xdata::XDataValue::Integer16(index));
        hatch.common.extended_data.add_record(record);
    }
    hatch
}

fn document_with_gradient(name: &str) -> CadDocument {
    let mut document = CadDocument::new();
    for application in ["GradientColor1ACI", "GradientColor2ACI"] {
        document.app_ids.add(acadrust::tables::AppId::new(application)).unwrap();
    }
    document.add_entity(EntityType::Hatch(Box::new(gradient_hatch(name)))).unwrap();
    document
}

fn the_hatch(document: &CadDocument) -> &Hatch {
    document
        .entities()
        .find_map(|entity| match entity {
            EntityType::Hatch(hatch) => Some(&**hatch),
            _ => None,
        })
        .expect("the hatch")
}

/// The chunk of the `Gradient1` group of the round-trip record of the hatch, as the file held it.
fn gradient_chunk(document: &CadDocument) -> Option<Vec<u8>> {
    let held = document.with_round_trip_records();
    let hatch = the_hatch(&held).common.handle;
    let record = held.xrecord(hatch, "ACAD_XREC_ROUNDTRIP")?;
    let chunk = record.entries.iter().find_map(|entry| match &entry.value {
        XRecordValue::Chunk(bytes) if entry.code == 1004 => Some(bytes.clone()),
        _ => None,
    })?;
    assert_eq!(record.entries[0].value.as_string(), Some("Gradient1"));
    Some(chunk)
}

/// AutoCAD's R14 and R2000 files hold a gradient hatch as a solid fill with the nearest indexes in `GradientColor1ACI` and
/// `GradientColor2ACI` and the gradient in `ACAD_XREC_ROUNDTRIP {Gradient1}` (sample_AC1014.dwg, sample_AC1015.dwg: #371 LINEAR, #376
/// SPHERICAL, the colours 0x0000FF and 0xFFFF00). The writer states that chunk byte for byte, the reader rebuilds the gradient from
/// it and takes the record out of the model.
#[test]
fn a_gradient_hatch_keeps_its_gradient_in_the_record_autocad_states_before_r2004() {
    for (name, record) in [("LINEAR", LINEAR_RECORD), ("SPHERICAL", SPHERICAL_RECORD)] {
        for version in [DxfVersion::AC1014, DxfVersion::AC1015] {
            let out = write_and_read(&document_with_gradient(name), version);
            let hatch = the_hatch(&out);
            assert!(hatch.gradient_color.enabled, "{name} {version:?}");
            assert_eq!(hatch.gradient_color.name, name, "{version:?}");
            assert_eq!(hatch.gradient_color.colors.len(), 2);
            assert_eq!(hatch.gradient_color.colors[0].color, Color::Rgb { r: 0, g: 0, b: 255 });
            assert_eq!(hatch.gradient_color.colors[1].color, Color::Rgb { r: 255, g: 255, b: 0 });
            assert!(out.xrecord(hatch.common.handle, "ACAD_XREC_ROUNDTRIP").is_none(), "{version:?}: the record is out of the model");
            assert_eq!(gradient_chunk(&out).as_deref(), Some(hex(record).as_slice()), "{name} {version:?}: the chunk of the sample");
        }
        // R2004 holds the gradient in the entity
        let out = write_and_read(&document_with_gradient(name), DxfVersion::AC1018);
        assert!(the_hatch(&out).gradient_color.enabled);
        assert!(gradient_chunk(&out).is_none());
    }
}

/// An edit after the read wins; a gradient the version cannot state in the sample's shape (another angle) gets no record.
#[test]
fn a_gradient_record_follows_an_edit_and_is_not_made_for_values_no_sample_shows() {
    let out = write_and_read(&document_with_gradient("LINEAR"), DxfVersion::AC1015);
    let mut edited = out.clone();
    for entity in edited.entities_mut() {
        if let EntityType::Hatch(hatch) = entity {
            hatch.gradient_color.name = "SPHERICAL".into();
        }
    }
    let again = write_and_read(&edited, DxfVersion::AC1015);
    assert_eq!(gradient_chunk(&again).as_deref(), Some(hex(SPHERICAL_RECORD).as_slice()));
    assert_eq!(the_hatch(&again).gradient_color.name, "SPHERICAL");
    // a gradient with an angle: no record (the layout of the angle is not shown by any sample)
    let mut turned = document_with_gradient("LINEAR");
    for entity in turned.entities_mut() {
        if let EntityType::Hatch(hatch) = entity {
            hatch.gradient_color.angle = 0.5;
        }
    }
    let out = write_and_read(&turned, DxfVersion::AC1015);
    assert!(gradient_chunk(&out).is_none());
}

use acadrust::entities::MText;

fn mtext_document(value: &str) -> CadDocument {
    let mut document = CadDocument::new();
    document.add_entity(EntityType::MText(Box::new(MText::with_value(value, Vector3::new(1.0, 2.0, 0.0))))).unwrap();
    document
}

fn the_mtext(document: &CadDocument) -> &MText {
    document
        .entities()
        .find_map(|entity| match entity {
            EntityType::MText(text) => Some(&**text),
            _ => None,
        })
        .expect("the MTEXT")
}

/// The checksum of AutoCAD's records: the sum of every UTF-16 unit times its 1-based position (sample_AC1015.dwg: the converted text
/// `this is a Mtext\Pwith multiple lines in it` states 85705).
fn checksum(text: &str) -> f64 {
    text.encode_utf16().enumerate().map(|(index, unit)| (index as f64 + 1.0) * f64::from(unit)).sum()
}

/// The `ACAD_MTEXT_RT` record of the MTEXT as the file held it: the checksum and the original in its chunks.
fn mtext_record(document: &CadDocument) -> Option<(f64, Vec<String>)> {
    let held = document.with_round_trip_records();
    let record = held.xrecord(the_mtext(&held).common.handle, "ACAD_MTEXT_RT")?.clone();
    let checksum = match record.entries.first()? {
        entry if entry.code == 40 => entry.value.as_double()?,
        _ => return None,
    };
    let chunks = record.entries[1..].iter().map(|entry| {
        assert_eq!(entry.code, 1);
        entry.value.as_string().unwrap().to_string()
    });
    Some((checksum, chunks.collect()))
}

/// The text ACadSharp reads of the MTEXT (what the file states).
fn viewed_value(document: &CadDocument) -> String {
    let dump = acadrust::compat::acadsharp::dump_document(document, "", "new");
    dump["objects"].as_array().unwrap().iter().find(|object| object["type"] == "MText").unwrap()["props"]["Value"].as_str().unwrap().to_string()
}

/// The MTEXT of R2004 and later holds a paragraph break as a line feed, the one of R14 and R2000 as `\P` (sample_AC1018.dwg `this is a
/// Mtext<LF>with multiple lines in it`, sample_AC1015.dwg `this is a Mtext\Pwith multiple lines in it`). AutoCAD's down-save keeps
/// the original in `ACAD_MTEXT_RT` (`40` the checksum of the converted text, `1` the original in chunks of 250 characters) and its
/// reader restores it while the checksum matches. The writer converts and states the record, the reader restores and takes it out
/// of the model.
#[test]
fn an_mtext_keeps_its_line_feeds_in_the_record_autocad_states_before_r2004() {
    let long: String = (0..7).map(|index| format!("line {index} of the long text {}\n", "x".repeat(90))).collect();
    for original in ["first line\nsecond line", long.as_str()] {
        let converted = original.replace('\n', "\\P");
        for version in [DxfVersion::AC1014, DxfVersion::AC1015] {
            let mut document = mtext_document(original);
            document.version = version;
            let bytes = DwgWriter::write_to_vec(&document).unwrap();
            let out = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
            assert_eq!(the_mtext(&out).value, original, "{version:?}: the original is back");
            assert_eq!(viewed_value(&out), converted, "{version:?}: the file states the break as \\P (the text ACadSharp reads)");
            assert!(out.xrecord(the_mtext(&out).common.handle, "ACAD_MTEXT_RT").is_none(), "{version:?}: the record is out of the model");
            let (sum, chunks) = mtext_record(&out).unwrap_or_else(|| panic!("{version:?}: the file held no record"));
            assert_eq!(sum, checksum(&converted), "{version:?}");
            assert_eq!(chunks.concat(), original, "{version:?}");
            assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 250), "{version:?}");
            assert_eq!(chunks.len(), original.chars().count().div_ceil(250), "{version:?}");
        }
        // R2004 holds the line feed itself
        let out = write_and_read(&mtext_document(original), DxfVersion::AC1018);
        assert_eq!(the_mtext(&out).value, original);
        assert!(mtext_record(&out).is_none());
    }
    // text without a break needs no record
    let out = write_and_read(&mtext_document("one line"), DxfVersion::AC1015);
    assert!(mtext_record(&out).is_none());
    assert_eq!(the_mtext(&out).value, "one line");
}

/// An edit after the read wins: the new text is converted and stated, a text without a break drops the record; a DXF of R2000 holds the
/// record as AutoCAD's twin does (sample_AC1015_ascii.dxf).
#[test]
fn an_mtext_record_follows_an_edit_and_the_dxf_of_r2000_holds_it_too() {
    let out = write_and_read(&mtext_document("first\nsecond"), DxfVersion::AC1015);
    let mut edited = out.clone();
    for entity in edited.entities_mut() {
        if let EntityType::MText(text) = entity {
            text.value = "other\nbreak\nhere".into();
        }
    }
    let again = write_and_read(&edited, DxfVersion::AC1015);
    assert_eq!(the_mtext(&again).value, "other\nbreak\nhere");
    assert_eq!(mtext_record(&again).unwrap().0, checksum("other\\Pbreak\\Phere"));
    let mut flat = out.clone();
    for entity in flat.entities_mut() {
        if let EntityType::MText(text) = entity {
            text.value = "no break".into();
        }
    }
    let again = write_and_read(&flat, DxfVersion::AC1015);
    assert_eq!(the_mtext(&again).value, "no break");
    assert!(mtext_record(&again).is_none());
    // the DXF of R2000: `\P` in the text and the record; R2004: the line feed
    let mut document = mtext_document("first\nsecond");
    document.version = DxfVersion::AC1015;
    let text = String::from_utf8_lossy(&DxfWriter::new(&document).write_to_vec().unwrap()).into_owned();
    assert!(text.contains("first\\Psecond") && text.contains("ACAD_MTEXT_RT"));
    let read = dxf_write_and_read(&mtext_document("first\nsecond"), DxfVersion::AC1015);
    assert_eq!(the_mtext(&read).value, "first\nsecond");
    assert!(read.xrecord(the_mtext(&read).common.handle, "ACAD_MTEXT_RT").is_none());
    assert_eq!(mtext_record(&read).unwrap().0, checksum("first\\Psecond"));
    document.version = DxfVersion::AC1018;
    let text = String::from_utf8_lossy(&DxfWriter::new(&document).write_to_vec().unwrap()).into_owned();
    assert!(!text.contains("ACAD_MTEXT_RT") && !text.contains("first\\Psecond"));
}

/// The record of an MTEXT whose conversion is not the line feed (sample_AC1015.dwg #662: `4'-0{\H1x;\S1#4;}"` is stated `4'-0\S1/4;"`)
/// is applied by its checksum all the same, as AutoCAD's up-save to R2004 does (sample_AC1018.dwg holds the original); the file's text
/// and the record are stated again while the text is unedited, and the text follows an edit.
#[test]
fn an_mtext_record_of_another_conversion_is_applied_by_its_checksum_and_stated_again() {
    let file = "4'-0\\S1/4;\"";
    let original = "4'-0{\\H1x;\\S1#4;}\"";
    let mut document = mtext_document(file);
    document.version = DxfVersion::AC1015;
    let owner = the_mtext(&document).common.handle;
    let (dictionary, record) = (document.allocate_handle(), document.allocate_handle());
    let mut entries = Dictionary::new();
    entries.handle = dictionary;
    entries.owner = owner;
    entries.hard_owner = true;
    entries.add_entry("ACAD_MTEXT_RT", record);
    document.objects.insert(dictionary, ObjectType::Dictionary(entries));
    let mut xrecord = acadrust::objects::XRecord::named("ACAD_MTEXT_RT");
    xrecord.handle = record;
    xrecord.owner = dictionary;
    xrecord.reactors = vec![dictionary];
    xrecord.add_double(40, checksum(file));
    xrecord.add_string(1, original);
    document.objects.insert(record, ObjectType::XRecord(xrecord));
    for entity in document.entities_mut() {
        if let EntityType::MText(text) = entity {
            text.common.xdictionary_handle = Some(dictionary);
        }
    }
    let out = write_and_read(&document, DxfVersion::AC1015);
    assert_eq!(the_mtext(&out).value, original, "the original is back");
    assert_eq!(viewed_value(&out), file, "ACadSharp reads the file's text");
    assert!(out.xrecord(the_mtext(&out).common.handle, "ACAD_MTEXT_RT").is_none());
    let (sum, chunks) = mtext_record(&out).expect("the file holds the record");
    assert_eq!((sum, chunks.concat()), (checksum(file), original.to_string()));
    // written again unedited: the same file text and record; edited: the text follows
    let again = write_and_read(&out, DxfVersion::AC1015);
    assert_eq!(the_mtext(&again).value, original);
    assert_eq!(viewed_value(&again), file);
    assert_eq!(mtext_record(&again).unwrap().0, checksum(file));
    let mut edited = out.clone();
    for entity in edited.entities_mut() {
        if let EntityType::MText(text) = entity {
            text.value = "plain".into();
        }
    }
    let again = write_and_read(&edited, DxfVersion::AC1015);
    assert_eq!(viewed_value(&again), "plain");
    assert!(mtext_record(&again).is_none(), "the record does not describe the new text");
    // R2004 holds the original and no record
    let up = write_and_read(&out, DxfVersion::AC1018);
    assert_eq!(the_mtext(&up).value, original);
    assert!(mtext_record(&up).is_none());
    assert!(up.with_round_trip_records().xrecord(the_mtext(&up).common.handle, "ACAD_MTEXT_RT").is_none());
}

use acadrust::tables::{AppId, BlockRecord, LineType, TextStyle};

fn names_document() -> CadDocument {
    let mut document = CadDocument::new();
    document.layers.add(Layer::new("Layer1")).unwrap();
    document.layers.add(Layer::new("0 @ 1")).unwrap();
    document.layers.add(Layer::new("ALREADY_UPPER")).unwrap();
    document.layers.add(Layer::new("Umlaut\u{c4}")).unwrap();
    document.line_types.add(LineType::new("My Dash")).unwrap();
    document.text_styles.add(TextStyle::new("my.style")).unwrap();
    document.app_ids.add(AppId::new("MyApp")).unwrap();
    document.block_records.add(BlockRecord::new("my block")).unwrap();
    document
}

/// The names of the table records as the ACadSharp view reads the file: `type -> names`.
fn viewed_names(document: &CadDocument, kind: &str) -> Vec<String> {
    let dump = acadrust::compat::acadsharp::dump_document(document, "", "new");
    let mut names: Vec<String> = dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|object| object["type"] == kind)
        .filter_map(|object| object["props"]["Name"].as_str().map(str::to_string))
        .collect();
    names.sort();
    names
}

fn names_of(document: &CadDocument) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
    let sorted = |mut names: Vec<String>| {
        names.sort();
        names
    };
    (
        sorted(document.layers.iter().map(|record| record.name.clone()).collect()),
        sorted(document.line_types.iter().map(|record| record.name.clone()).collect()),
        sorted(document.text_styles.iter().map(|record| record.name.clone()).collect()),
        sorted(document.app_ids.iter().map(|record| record.name.clone()).collect()),
        sorted(document.block_records.iter().map(|record| record.name.clone()).collect()),
    )
}

/// R14 states the name of a table record in upper case, a character it does not allow as `_` (sample_AC1014.dwg: `LAYER1`,
/// `0___1`, `1_5IN_SQUARE_-_MEDIUM_BLUE`), and keeps the name of the drawing in `ACAD_XREC_ROUNDTRIP {EXTNAMES 1 <R14 name> 2 <name>}` of
/// the record's extension dictionary, the first group of the record. The reader restores the names before anything refers to them and
/// takes the group out of the model; the view shows R14's names. A name with a character no sample shows stays as it is.
#[test]
fn an_r14_file_states_the_table_names_in_upper_case_and_keeps_the_drawings_names_in_a_record() {
    let source = names_document();
    let out = write_and_read(&source, DxfVersion::AC1014);
    let (layers, lines, styles, apps, blocks) = names_of(&out);
    for (names, wanted) in [
        (&layers, ["0", "0 @ 1", "ALREADY_UPPER", "Layer1", "Umlaut\u{c4}"]),
        (&lines, ["BYBLOCK", "BYLAYER", "CONTINUOUS", "My Dash", ""]),
    ] {
        for name in wanted.iter().filter(|name| !name.is_empty() && !["BYBLOCK", "BYLAYER", "CONTINUOUS"].contains(name)) {
            assert!(names.iter().any(|candidate| candidate == name), "{name} is restored: {names:?}");
        }
    }
    assert!(styles.iter().any(|name| name == "my.style"), "{styles:?}");
    assert!(apps.iter().any(|name| name == "MyApp"), "{apps:?}");
    assert!(blocks.iter().any(|name| name == "my block"), "{blocks:?}");
    // the model holds no record; the file held the group, first in the record
    assert!(out.xrecord(layer(&out, "Layer1").handle, "ACAD_XREC_ROUNDTRIP").is_none());
    let held = out.with_round_trip_records();
    let record = held.xrecord(layer(&out, "Layer1").handle, "ACAD_XREC_ROUNDTRIP").expect("the file held a record").clone();
    let texts: Vec<(i32, String)> = record.entries.iter().map(|entry| (entry.code, entry.value.as_string().unwrap().to_string())).collect();
    assert_eq!(texts, [(102, "EXTNAMES".to_string()), (1, "LAYER1".to_string()), (2, "Layer1".to_string())]);
    assert!(held.xrecord(layer(&out, "0").handle, "ACAD_XREC_ROUNDTRIP").is_none(), "a name R14 states as it is has no record");
    assert!(held.xrecord(layer(&out, "ALREADY_UPPER").handle, "ACAD_XREC_ROUNDTRIP").is_none());
    assert!(held.xrecord(layer(&out, "Umlaut\u{c4}").handle, "ACAD_XREC_ROUNDTRIP").is_none(), "a character no sample shows: the name is kept");
    let space = held.xrecord(layer(&out, "0 @ 1").handle, "ACAD_XREC_ROUNDTRIP").expect("record of the layer with the characters");
    assert_eq!(space.entries[1].value.as_string(), Some("0___1"));
    // ACadSharp reads R14's names
    let view = viewed_names(&out, "Layer");
    for name in ["LAYER1", "0___1", "ALREADY_UPPER", "UmlautÄ", "0"] {
        assert!(view.iter().any(|candidate| candidate == name), "{name} in {view:?}");
    }
    assert!(viewed_names(&out, "AppId").iter().any(|name| name == "MYAPP"));
    assert!(viewed_names(&out, "BlockRecord").iter().any(|name| name == "MY_BLOCK"));
    // R2000 and later state the names as they are and no record
    for version in [DxfVersion::AC1015, DxfVersion::AC1032] {
        let out = write_and_read(&source, version);
        assert!(out.with_round_trip_records().xrecord(layer(&out, "Layer1").handle, "ACAD_XREC_ROUNDTRIP").is_none(), "{version:?}");
        assert!(viewed_names(&out, "Layer").iter().any(|name| name == "Layer1"), "{version:?}");
    }
}

/// An edit after the read wins (the new name is stated in upper case with its record); a drawing read from R14 and written at R2000
/// carries the names and no record; a DXF carries the group as the file had it.
#[test]
fn an_r14_name_follows_an_edit_and_is_not_kept_by_other_versions() {
    let out = write_and_read(&names_document(), DxfVersion::AC1014);
    let mut edited = out.clone();
    edited.layers.rename("Layer1", "Other Name").unwrap();
    let again = write_and_read(&edited, DxfVersion::AC1014);
    assert!(again.layers.get("Other Name").is_some_and(|layer| layer.name == "Other Name"));
    assert!(viewed_names(&again, "Layer").iter().any(|name| name == "OTHER_NAME"));
    assert!(!viewed_names(&again, "Layer").iter().any(|name| name == "LAYER1"));
    let up = write_and_read(&out, DxfVersion::AC1015);
    assert!(up.layers.get("Layer1").is_some_and(|layer| layer.name == "Layer1"));
    assert!(up.with_round_trip_records().xrecord(layer(&up, "Layer1").handle, "ACAD_XREC_ROUNDTRIP").is_none());
    // a DXF carries the group the file had (the names in it are the drawing's already) and its reader takes it out of the model again
    let dxf = dxf_write_and_read(&out, DxfVersion::AC1015);
    assert!(dxf.layers.get("Layer1").is_some());
    assert!(dxf.xrecord(layer(&dxf, "Layer1").handle, "ACAD_XREC_ROUNDTRIP").is_none());
    assert!(dxf.with_round_trip_records().xrecord(layer(&dxf, "Layer1").handle, "ACAD_XREC_ROUNDTRIP").is_some());
    // unchanged and written again: the same names
    assert_eq!(names_of(&write_and_read(&out, DxfVersion::AC1014)), names_of(&out));
}

/// A down-saved dynamic block definition stated anonymous ("*U", flag 1) keeps its public name in extended data and is written under
/// the bare "*U": that is not a name R14 states, so no `EXTNAMES` group covers it (a group would name a record no reader finds, and
/// a second write would add one more object for every pass).
#[test]
fn a_dynamic_definition_stated_anonymous_has_no_extnames_group_at_r14() {
    use acadrust::tables::{AppId, BlockRecord};
    use acadrust::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};
    let mut document = CadDocument::with_version(DxfVersion::AC1014);
    document.app_ids.add(AppId::new("AcDbDynamicBlockTrueName")).unwrap();
    let mut record = BlockRecord::new("my-dynamic-block");
    record.handle = document.allocate_handle();
    record.block_entity_handle = document.allocate_handle();
    record.block_end_handle = document.allocate_handle();
    record.source_flags = 1;
    let handle = record.handle;
    document.block_records.add(record).unwrap();
    let mut data = ExtendedData::default();
    let mut name = ExtendedDataRecord::new("AcDbDynamicBlockTrueName");
    name.values = vec![XDataValue::String("my-dynamic-block".into())];
    data.add_record(name);
    document.object_xdata.insert(handle, data);
    let first = write_and_read(&document, DxfVersion::AC1014);
    let second = write_and_read(&first, DxfVersion::AC1014);
    assert_eq!(first.header.handle_seed, second.header.handle_seed, "a second write adds no object");
    for (label, read) in [("first", &first), ("second", &second)] {
        let record = read.block_records.get("my-dynamic-block").unwrap_or_else(|| panic!("{label}: the public name is gone"));
        assert!(read.with_round_trip_records().xrecord(record.handle, "ACAD_XREC_ROUNDTRIP").is_none(), "{label}: no group for it");
        // the marker states the name as it is, as a reader that names the record after its marker (ACadSharp) takes it
        let markers = viewed_names(read, "Block");
        assert!(markers.iter().any(|name| name == "my-dynamic-block"), "{label}: markers {markers:?}");
    }
}

/// The application `AcDbAttr` is stated as it is in a DWG of R14 (AutoCAD's own file), the other applications in upper case.
#[test]
fn the_application_autocad_adds_for_attributes_is_stated_as_it_is_at_r14() {
    use acadrust::tables::AppId;
    let mut document = CadDocument::with_version(DxfVersion::AC1014);
    document.app_ids.add(AppId::new("AcDbAttr")).unwrap();
    document.app_ids.add(AppId::new("MyApp")).unwrap();
    let out = write_and_read(&document, DxfVersion::AC1014);
    let held = out.with_round_trip_records();
    let app = out.app_ids.get("AcDbAttr").expect("the application keeps its name");
    assert!(held.xrecord(app.handle, "ACAD_XREC_ROUNDTRIP").is_none(), "no group for a name R14 states as it is");
    let view = viewed_names(&out, "AppId");
    assert!(view.iter().any(|name| name == "AcDbAttr"), "{view:?}");
    assert!(view.iter().any(|name| name == "MYAPP"), "{view:?}");
}

/// The record of a leader of R14 states the colour the leader uses (group 77); a leader that was not read from R14 holds that colour as
/// its override colour, which the record must state (AutoCAD's own R14 file, sample_AC1014.dwg, reads back ByLayer, where the writer
/// stated 0 and the leader came back ByBlock).
#[test]
fn a_leader_of_r14_states_its_override_colour() {
    use acadrust::entities::Leader;
    use acadrust::types::Color;
    for color in [Color::ByLayer, Color::Index(5), Color::ByBlock] {
        let mut leader = Leader::from_vertices(vec![Vector3::new(0.0, 0.0, 0.0), Vector3::new(5.0, 5.0, 0.0)]);
        leader.override_color = color;
        let mut document = CadDocument::new();
        document.add_entity(EntityType::Leader(leader)).unwrap();
        let out = write_and_read(&document, DxfVersion::AC1014);
        let read = out
            .entities()
            .find_map(|entity| match entity {
                EntityType::Leader(leader) => Some(leader.override_color),
                _ => None,
            })
            .expect("the leader");
        assert_eq!(read, color);
    }
}

/// The hookline flag is bit 8 of the arrowhead word of a leader of R14: the leader holds it as `hookline_enabled`, and the arrowhead
/// type is what is left (the writer stated the flag in the word, the reader left it there).
#[test]
fn the_hookline_flag_of_an_r14_leader_is_not_part_of_its_arrowhead_type() {
    use acadrust::entities::Leader;
    let mut leader = Leader::from_vertices(vec![Vector3::new(0.0, 0.0, 0.0), Vector3::new(5.0, 5.0, 0.0)]);
    leader.hookline_enabled = true;
    leader.arrowhead_type = 1;
    let mut document = CadDocument::new();
    document.add_entity(EntityType::Leader(leader)).unwrap();
    let out = write_and_read(&document, DxfVersion::AC1014);
    let read = out
        .entities()
        .find_map(|entity| match entity {
            EntityType::Leader(leader) => Some(leader.clone()),
            _ => None,
        })
        .expect("the leader");
    assert!(read.hookline_enabled);
    assert_eq!(read.arrowhead_type, 1);
    let again = write_and_read(&out, DxfVersion::AC1014);
    assert!(again.entities().any(|entity| matches!(entity, EntityType::Leader(leader) if leader.hookline_enabled && leader.arrowhead_type == 1)));
}

// ---------------------------------------------------------------------------------------------------------------------
// Review of R5b-11
// ---------------------------------------------------------------------------------------------------------------------

/// An R12 file names its continuous line type `CONTINUOUS` (sample_AC1009_ascii.dxf: the LTYPE record and the SOLID entities). The
/// continuous flag of an entity of R2000 and later reads back as `Continuous`, so an entity of such a record is stated by the record's
/// handle and reads back with the record's own name.
#[test]
fn an_entity_of_a_continuous_line_type_spelled_otherwise_keeps_the_records_name() {
    use acadrust::entities::Line;
    let mut document = CadDocument::new();
    document.line_types.rename("Continuous", "CONTINUOUS").unwrap();
    let mut line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 1.0, 0.0));
    line.common.linetype = "CONTINUOUS".to_string();
    document.add_entity(EntityType::Line(line)).unwrap();
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1032] {
        let out = write_and_read(&document, version);
        let read = out
            .entities()
            .find_map(|entity| match entity {
                EntityType::Line(line) => Some(line.common.linetype.clone()),
                _ => None,
            })
            .expect("the line");
        assert_eq!(read, "CONTINUOUS", "{version:?}");
    }
}

/// The UCS icon of a viewport of R2000 and later is in its status word, of a DWG and of a DXF alike; R14 states it in the icon word of
/// the MVIEW data (1 visible, 2 at the origin), and AutoCAD's own R14 file of the drawing states 1 or 3 where its R2000 file has the
/// status bits (sample_AC1014.dwg and sample_AC1015.dwg, the viewports #240 to #26B; group 74 is clear in both). The reader of R14
/// puts the icon word back into the status word, and the viewport's own group 74 is not the icon's.
#[test]
fn the_ucs_icon_of_a_viewport_survives_r14_from_a_dxf_and_from_a_dwg() {
    use acadrust::entities::{Viewport, ViewportStatusFlags};
    let viewport_of = |document: &CadDocument| {
        document
            .entities()
            .find_map(|entity| match entity {
                EntityType::Viewport(viewport) if viewport.width == 160.0 => Some(viewport.clone()),
                _ => None,
            })
            .expect("the viewport")
    };
    for (visible, at_origin) in [(true, true), (true, false), (false, false)] {
        let mut document = CadDocument::with_version(DxfVersion::AC1015);
        let mut viewport = Viewport::new();
        viewport.id = 2;
        viewport.width = 160.0;
        viewport.height = 100.0;
        viewport.status = ViewportStatusFlags::from_bits(0x8000 | if visible { 0x20 } else { 0 } | if at_origin { 0x40 } else { 0 });
        document.add_entity_to_layout(EntityType::Viewport(Box::new(viewport)), "Layout1").unwrap();
        let from_dxf = dxf_write_and_read(&document, DxfVersion::AC1015);
        let from_dwg = write_and_read(&document, DxfVersion::AC1015);
        for (label, source) in [("DXF", &from_dxf), ("DWG", &from_dwg)] {
            assert_eq!(viewport_of(source).status.ucs_icon_visible, visible, "{label}: the source");
            let out = write_and_read(source, DxfVersion::AC1014);
            let read = viewport_of(&out);
            assert_eq!(read.status.ucs_icon_visible, visible, "{label} {visible} {at_origin}: icon visible");
            assert_eq!(read.status.ucs_icon_at_origin, at_origin, "{label} {visible} {at_origin}: icon at the origin");
            assert!(!read.ucs_at_origin, "{label}: group 74 is not the icon's");
            let again = viewport_of(&write_and_read(&out, DxfVersion::AC1014));
            assert_eq!((again.status.ucs_icon_visible, again.status.ucs_icon_at_origin), (visible, at_origin), "{label}: R14 again");
        }
    }
}

/// Two names R14 states alike (`Layer 1` and `Layer_1` are both `LAYER_1` there): each record keeps its own name in its own
/// `EXTNAMES` group, so the fork's reader restores both. (How AutoCAD's down-save states such names is not in any sample.)
#[test]
fn two_names_r14_states_alike_are_both_restored() {
    let mut document = CadDocument::new();
    document.layers.add(Layer::new("Layer 1")).unwrap();
    document.layers.add(Layer::new("Layer_1")).unwrap();
    let out = write_and_read(&document, DxfVersion::AC1014);
    assert!(out.layers.iter().any(|layer| layer.name == "Layer 1"));
    assert!(out.layers.iter().any(|layer| layer.name == "Layer_1"));
    let again = write_and_read(&out, DxfVersion::AC1014);
    assert!(again.layers.iter().any(|layer| layer.name == "Layer 1"));
    assert!(again.layers.iter().any(|layer| layer.name == "Layer_1"));
}

/// A record whose checksum does not match the text the file holds (the text was edited after the down-save, by a program that kept
/// the record) is not applied: the text stays the file's, and the record stays in the model as the file had it.
#[test]
fn an_mtext_record_with_another_checksum_does_not_restore_its_stale_text() {
    let file = "edited later";
    let stale = "first\nsecond";
    let mut document = mtext_document(file);
    document.version = DxfVersion::AC1015;
    let owner = the_mtext(&document).common.handle;
    let (dictionary, record) = (document.allocate_handle(), document.allocate_handle());
    let mut entries = Dictionary::new();
    entries.handle = dictionary;
    entries.owner = owner;
    entries.hard_owner = true;
    entries.add_entry("ACAD_MTEXT_RT", record);
    document.objects.insert(dictionary, ObjectType::Dictionary(entries));
    let mut xrecord = acadrust::objects::XRecord::named("ACAD_MTEXT_RT");
    xrecord.handle = record;
    xrecord.owner = dictionary;
    xrecord.reactors = vec![dictionary];
    xrecord.add_double(40, checksum("first\\Psecond"));
    xrecord.add_string(1, stale);
    document.objects.insert(record, ObjectType::XRecord(xrecord));
    for entity in document.entities_mut() {
        if let EntityType::MText(text) = entity {
            text.common.xdictionary_handle = Some(dictionary);
        }
    }
    let out = write_and_read(&document, DxfVersion::AC1015);
    assert_eq!(the_mtext(&out).value, file, "the stale original is not restored");
    assert_eq!(viewed_value(&out), file);
    let kept = out.xrecord(the_mtext(&out).common.handle, "ACAD_MTEXT_RT").expect("the record stays in the model");
    assert_eq!(kept.entries.len(), 2);
}
