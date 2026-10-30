// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]
//! R5b-19 (gate G5-RC sample_V): the `DSTYLE` extended data of dimensions and leaders in the R14 form, and the `AnnotativeData` flag of
//! a text before R2010. Synthetic documents only; each test fails with its fix reverted. Evidence: AutoCAD's own files of one drawing
//! (sample_AC1014.dwg, entities #512 #515 #516 #527; sample_AC1015.dwg to sample_AC1032.dwg).

use std::io::Cursor;

use acadrust::compat::acadsharp::dump_reader_document;
use acadrust::entities::{Dimension, DimensionAligned, DimensionAngular3Pt, Leader, Text};
use acadrust::tables::{BlockRecord, DimStyle};
use acadrust::types::{DxfVersion, Handle, Vector3};
use acadrust::xdata::{ExtendedDataRecord, XDataValue as X};
use acadrust::{CadDocument, DwgReader, DwgWriter, EntityType};
use serde_json::Value;

fn write_and_read(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    let bytes = DwgWriter::write_to_vec(&document).unwrap_or_else(|error| panic!("{version:?}: DWG write failed: {error}"));
    DwgReader::from_stream(Cursor::new(bytes)).read().unwrap_or_else(|error| panic!("{version:?}: DWG read failed: {error}"))
}

fn dstyle(values: Vec<(i16, X)>) -> ExtendedDataRecord {
    let mut record = ExtendedDataRecord::new("ACAD");
    record.add_value(X::String("DSTYLE".into()));
    record.add_value(X::ControlString("{".into()));
    for (code, value) in values {
        record.add_value(X::Integer16(code));
        record.add_value(value);
    }
    record.add_value(X::ControlString("}".into()));
    record
}

struct Drawing {
    document: CadDocument,
    aligned: Handle,
    angular: Handle,
    leader: Handle,
    arrow: Handle,
}

/// The four entities of sample_AC1032.dwg that have an R14 form of their own, in the R2000 and later form (AutoCAD's R2000 file:
/// #527 states 276 1, 78 8, 277 4, 44 2.0, 46 2.0, 176 1, 343 <ArchTick>, 173 1, 344 <ArchTick>; #512, #515 and #516 state none), with the
/// style ISO-25 (DIMDEC 2, DIMADEC 0, DIMLUNIT 2, no DIMLDRBLK).
fn drawing() -> Drawing {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut tick = BlockRecord::new("_ArchTick");
    tick.handle = document.allocate_handle();
    let arrow = tick.handle;
    document.block_records.add(tick).unwrap();
    let mut style = DimStyle::new("ISO-25");
    style.handle = document.allocate_handle();
    style.dimdec = 2;
    style.dimadec = 0;
    style.dimlunit = 2;
    document.dim_styles.add(style).unwrap();

    let mut aligned = DimensionAligned::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
    aligned.base.style_name = "ISO-25".into();
    aligned.base.common.extended_data.add_record(dstyle(vec![
        (276, X::Integer16(1)),
        (78, X::Integer16(8)),
        (277, X::Integer16(4)),
        (44, X::Real(2.0)),
        (46, X::Real(2.0)),
        (176, X::Integer16(1)),
        (343, X::Handle(arrow)),
        (173, X::Integer16(1)),
        (344, X::Handle(arrow)),
    ]));
    let aligned = document.add_entity(EntityType::Dimension(Box::new(Dimension::Aligned(aligned)))).unwrap();

    let mut angular = DimensionAngular3Pt::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(5.0, 0.0, 0.0), Vector3::new(0.0, 5.0, 0.0));
    angular.base.style_name = "ISO-25".into();
    let angular = document.add_entity(EntityType::Dimension(Box::new(Dimension::Angular3Pt(angular)))).unwrap();

    let mut leader = Leader::new();
    leader.dimension_style = "ISO-25".into();
    leader.vertices = vec![Vector3::new(0.0, 0.0, 0.0), Vector3::new(5.0, 5.0, 0.0), Vector3::new(9.0, 5.0, 0.0)];
    let leader = document.add_entity(EntityType::Leader(leader)).unwrap();
    Drawing { document, aligned, angular, leader, arrow }
}

/// `(code, value)` pairs of the `DSTYLE` block of an entity's `ACAD` record in a model; `None` when it has none.
fn model_pairs(document: &CadDocument, handle: Handle) -> Option<Vec<(i16, X)>> {
    let data = &document.get_entity(handle)?.common().extended_data;
    let record = data.records().iter().find(|record| record.application_name.eq_ignore_ascii_case("ACAD"))?;
    let start = record.values.iter().position(|value| matches!(value, X::String(name) if name == "DSTYLE"))?;
    let inner = &record.values[start + 2..record.values.len() - 1];
    Some(inner.chunks(2).map(|pair| (if let X::Integer16(code) = pair[0] { code } else { panic!("code") }, pair[1].clone())).collect())
}

/// The `(code, value)` pairs of the `DSTYLE` block in the ACadSharp view of a document read from a file: what ACadSharp reads.
fn view_pairs(document: &CadDocument, handle: Handle) -> Option<Vec<(i64, Value)>> {
    let dump = dump_reader_document(document, "synthetic.dwg", "dwg");
    let object = dump["objects"].as_array()?.iter().find(|object| object["handle"] == format!("#{:X}", handle.value()))?;
    let records = object["props"]["ExtendedData"].as_array()?.first()?[1]["Records"].as_array()?.clone();
    let mut pairs = Vec::new();
    let mut iter = records.iter().skip(2);
    while let (Some(code), Some(value)) = (iter.next(), iter.next()) {
        if code["Code"] == 1002 {
            break;
        }
        pairs.push((code["RawValue"].as_i64()?, value["RawValue"].clone()));
    }
    Some(pairs)
}

fn ints(pairs: &[(i64, Value)]) -> Vec<i64> {
    pairs.iter().map(|(code, _)| *code).collect()
}

/// AutoCAD's R14 file of the drawing (sample_AC1014.dwg): #527 `270 4` first and `6 ARCHTICK`, `7 ARCHTICK` for the arrowhead handles;
/// #515 and #516 `271 0` (R14 has no DIMADEC: the dimension states its DIMDEC); #512 `5 ""` (R14 has no DIMLDRBLK: the leader states
/// its arrowhead as DIMBLK). The model of the read file holds the R2000 form again, and a file written from it at R14 or at R2000
/// and later carries no stale record.
#[test]
fn the_dstyle_data_of_dimensions_and_a_leader_is_stated_in_the_r14_form_and_restored_on_read() {
    let drawing = drawing();
    let r14 = write_and_read(&drawing.document, DxfVersion::AC1014);

    let aligned = view_pairs(&r14, drawing.aligned).expect("aligned dimension DSTYLE in the R14 view");
    assert_eq!(ints(&aligned), [270, 276, 78, 277, 44, 46, 176, 6, 173, 7]);
    assert_eq!(aligned[0].1, 4);
    assert_eq!((aligned[7].1.clone(), aligned[9].1.clone()), ("ARCHTICK".into(), "ARCHTICK".into()));
    assert_eq!(view_pairs(&r14, drawing.angular).expect("angular DSTYLE").as_slice(), [(271, Value::from(0))]);
    assert_eq!(view_pairs(&r14, drawing.leader).expect("leader DSTYLE").as_slice(), [(5, Value::from(""))]);

    // the model of the read file is the R2000 form
    assert_eq!(model_pairs(&r14, drawing.aligned), model_pairs(&drawing.document, drawing.aligned));
    assert!(model_pairs(&r14, drawing.angular).is_none() && model_pairs(&r14, drawing.leader).is_none());

    // R14 -> R14 is unchanged; R14 -> R2000 and later holds the R2000 form
    let again = write_and_read(&r14, DxfVersion::AC1014);
    assert_eq!(view_pairs(&again, drawing.aligned), Some(aligned));
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1032] {
        let up = write_and_read(&r14, version);
        assert_eq!(model_pairs(&up, drawing.aligned), model_pairs(&drawing.document, drawing.aligned), "{version:?}");
        assert!(model_pairs(&up, drawing.angular).is_none() && model_pairs(&up, drawing.leader).is_none(), "{version:?}");
        assert!(up.get_entity(drawing.aligned).is_some());
    }
    let _ = drawing.arrow;
}

/// A dimension or leader from a later version whose style makes the R14 data unnecessary gets none: DIMADEC equal to DIMDEC needs no
/// `271`, and DIMLUNIT equal to the style's needs no `270`. An edit of the style after the read changes what an R14 file states.
#[test]
fn the_r14_form_follows_the_style_and_an_edit_after_the_read_wins() {
    let mut drawing = drawing();
    drawing.document.dim_styles.get_mut("ISO-25").unwrap().dimadec = 2;
    let same = write_and_read(&drawing.document, DxfVersion::AC1014);
    assert!(view_pairs(&same, drawing.angular).is_none(), "DIMADEC = DIMDEC: no 271");

    // read an R14 file, change the style, write R14 again
    let drawing = self::drawing();
    let mut r14 = write_and_read(&drawing.document, DxfVersion::AC1014);
    assert_eq!(view_pairs(&r14, drawing.angular).unwrap().as_slice(), [(271, Value::from(0))]);
    r14.dim_styles.get_mut("ISO-25").unwrap().dimadec = 1;
    let edited = write_and_read(&r14, DxfVersion::AC1014);
    assert_eq!(view_pairs(&edited, drawing.angular).unwrap().as_slice(), [(271, Value::from(1))], "the edited style decides");
}

/// A block name no block record has, a `271` that is not the style's DIMADEC and a 270 that is not the derived unit are data the samples
/// do not explain: an R14 file read keeps them as they are (the model holds what the file stated; the view shows it unchanged).
#[test]
fn an_r14_record_the_samples_do_not_explain_stays_as_the_file_had_it() {
    let mut drawing = drawing();
    // an R14-shaped block written to an R14 file by hand: names for blocks that do not exist, 271 other than DIMADEC
    let aligned = drawing.document.get_entity_mut(drawing.aligned).unwrap().common_mut();
    aligned.extended_data = Default::default();
    aligned.extended_data.add_record(dstyle(vec![(6, X::String("NOSUCHARROW".into())), (173, X::Integer16(1))]));
    let angular = drawing.document.get_entity_mut(drawing.angular).unwrap().common_mut();
    angular.extended_data.add_record(dstyle(vec![(271, X::Integer16(5))]));
    let r14 = write_and_read(&drawing.document, DxfVersion::AC1014);
    assert_eq!(model_pairs(&r14, drawing.aligned), Some(vec![(6, X::String("NOSUCHARROW".into())), (173, X::Integer16(1))]));
    assert_eq!(model_pairs(&r14, drawing.angular), Some(vec![(271, X::Integer16(5))]));
    // ... and the view of the read file shows the same, with the 271 of the style not added on top
    assert_eq!(view_pairs(&r14, drawing.angular).unwrap().as_slice(), [(271, Value::from(5))]);
    // written to R14 again, the file states them as read (no 271 of the style, no conversion of the name)
    let again = write_and_read(&r14, DxfVersion::AC1014);
    for handle in [drawing.aligned, drawing.angular] {
        assert_eq!(model_pairs(&again, handle), model_pairs(&r14, handle));
        assert_eq!(view_pairs(&again, handle), view_pairs(&r14, handle));
    }
}

/// An edit after an R14 read wins: a block the reader left as the file had it gets the R14 form of what the caller put there, and a
/// dimension added after the read gets the R14 form like any other (R5b-19 review: both were written in the R2000 form).
#[test]
fn an_edit_after_an_r14_read_and_a_new_dimension_get_the_r14_form() {
    let mut drawing = drawing();
    let angular = drawing.document.get_entity_mut(drawing.angular).unwrap().common_mut();
    angular.extended_data.add_record(dstyle(vec![(271, X::Integer16(5))]));
    let mut r14 = write_and_read(&drawing.document, DxfVersion::AC1014);
    assert_eq!(model_pairs(&r14, drawing.angular), Some(vec![(271, X::Integer16(5))]), "left as the file had it");

    // the caller removes the override: the angular dimension states the style's DIMADEC as R14 does
    r14.get_entity_mut(drawing.angular).unwrap().common_mut().extended_data = Default::default();
    // ... and adds an aligned dimension with the arrowhead overrides in the model's (R2000) form
    let mut added = DimensionAligned::new(Vector3::new(0.0, 5.0, 0.0), Vector3::new(10.0, 5.0, 0.0));
    added.base.style_name = "ISO-25".into();
    added.base.common.extended_data.add_record(dstyle(vec![(343, X::Handle(drawing.arrow)), (344, X::Handle(drawing.arrow))]));
    let added = r14.add_entity(EntityType::Dimension(Box::new(Dimension::Aligned(added)))).unwrap();

    let edited = write_and_read(&r14, DxfVersion::AC1014);
    assert_eq!(view_pairs(&edited, drawing.angular).unwrap().as_slice(), [(271, Value::from(0))], "the edit, in the R14 form");
    assert_eq!(
        view_pairs(&edited, added).unwrap().as_slice(),
        [(6, Value::from("ARCHTICK")), (7, Value::from("ARCHTICK"))],
        "the new dimension names the arrowhead block"
    );
    assert_eq!(model_pairs(&edited, added), Some(vec![(343, X::Handle(drawing.arrow)), (344, X::Handle(drawing.arrow))]));
}

/// An arrowhead handle whose block record no longer exists has no R14 name: the block is written as the model holds it (no name is
/// invented) and an R14 read keeps it as it was written.
#[test]
fn an_arrowhead_handle_without_a_block_record_is_kept_as_the_model_holds_it() {
    let drawing = drawing();
    let mut r14 = write_and_read(&drawing.document, DxfVersion::AC1014);
    let gone = Handle::new(0x7FFF0);
    assert!(r14.block_records.iter().all(|block| block.handle != gone));
    let pairs = vec![(173, X::Integer16(1)), (343, X::Handle(gone))];
    let aligned = r14.get_entity_mut(drawing.aligned).unwrap().common_mut();
    aligned.extended_data = Default::default();
    aligned.extended_data.add_record(dstyle(pairs.clone()));
    let again = write_and_read(&r14, DxfVersion::AC1014);
    assert_eq!(model_pairs(&again, drawing.aligned), Some(pairs.clone()));
    let up = write_and_read(&again, DxfVersion::AC1015);
    assert_eq!(model_pairs(&up, drawing.aligned), Some(pairs));
}

// ── AnnotativeData of a text ───────────────────────────────────────────────────────────────────────────────────────────────

fn annotative_text(flag: i16) -> (CadDocument, Handle) {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut text = Text::new();
    text.value = "annotative".into();
    text.height = 1.0;
    let mut record = ExtendedDataRecord::new("AcadAnnotative");
    for value in [X::String("AnnotativeData".into()), X::ControlString("{".into()), X::Integer16(1), X::Integer16(flag), X::ControlString("}".into())] {
        record.add_value(value);
    }
    text.common.extended_data.add_record(record);
    let handle = document.add_entity(EntityType::Text(text)).unwrap();
    (document, handle)
}

fn flag(document: &CadDocument, handle: Handle) -> Option<i16> {
    let record = document.get_entity(handle)?.common().extended_data.records().iter().find(|record| record.application_name == "AcadAnnotative")?;
    match record.values.get(3) {
        Some(X::Integer16(flag)) => Some(*flag),
        _ => None,
    }
}

/// AutoCAD's R2010 and later files state the second flag 1 for two of the four annotative texts, every file before R2010 state 0
/// (sample_AC1015.dwg, sample_AC1018.dwg, sample_AC1021.dwg and the DXF twins). A down-save states 0, a file for R2010 or later keeps the
/// flag the text holds, and a text with 0 gets no 1 derived (nothing in the samples tells the texts apart).
#[test]
fn a_text_before_r2010_states_the_annotative_data_flag_0() {
    let (document, text) = annotative_text(1);
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1021] {
        assert_eq!(flag(&write_and_read(&document, version), text), Some(0), "{version:?}");
    }
    for version in [DxfVersion::AC1024, DxfVersion::AC1027, DxfVersion::AC1032] {
        assert_eq!(flag(&write_and_read(&document, version), text), Some(1), "{version:?}");
    }
    // the DXF of AutoCAD's files agrees (sample_AC1015_ascii.dxf: 0, sample_AC1024_ascii.dxf: 1 for these texts)
    let dxf_flag = |version: DxfVersion| {
        let mut document = document.clone();
        document.version = version;
        let text = String::from_utf8_lossy(&acadrust::DxfWriter::new(&document).write_to_vec().unwrap()).into_owned();
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        let at = lines.iter().rposition(|line| *line == "AnnotativeData").expect("AnnotativeData in the DXF");
        lines[at + 6].to_string()
    };
    assert_eq!(dxf_flag(DxfVersion::AC1015), "0");
    assert_eq!(dxf_flag(DxfVersion::AC1032), "1");
    let (document, text) = annotative_text(0);
    for version in [DxfVersion::AC1015, DxfVersion::AC1024, DxfVersion::AC1032] {
        assert_eq!(flag(&write_and_read(&document, version), text), Some(0), "{version:?} derives no 1");
    }
}

// ── ACAD_MTEXT_RT of an MTEXT with a character the code page lacks, and the stacked fraction ───────────────────────────────

fn mtext_drawing(text: &str) -> (CadDocument, Handle) {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut mtext = acadrust::entities::MText::new();
    mtext.value = text.into();
    mtext.height = 1.0;
    let handle = document.add_entity(EntityType::MText(Box::new(mtext))).unwrap();
    (document, handle)
}

fn mtext_value(document: &CadDocument, handle: Handle) -> String {
    match document.get_entity(handle) {
        Some(EntityType::MText(text)) => text.value.clone(),
        _ => panic!("no MTEXT"),
    }
}

/// The `(key, checksum, text)` of the round-trip record in the extension dictionary of the MTEXT in the ACadSharp view of a file read.
fn mtext_record(document: &CadDocument, handle: Handle) -> Option<(String, f64, String)> {
    let dump = dump_reader_document(document, "synthetic.dwg", "dwg");
    let objects = dump["objects"].as_array()?;
    let owner = format!("#{:X}", handle.value());
    let found = objects.iter().find(|object| object["owner"] == owner.as_str() && object["type"] == "CadDictionary")?;
    let key = found["props"]["EntryNames"][0].as_str()?.to_string();
    let record = objects.iter().find(|object| object["owner"] == found["handle"] && object["type"] == "XRecord")?;
    let entries = record["props"]["Entries"].as_array()?;
    Some((key, entries[0]["Value"].as_f64()?, entries[1]["Value"].as_str()?.to_string()))
}

/// AutoCAD's files of the drawing state the MTEXT #64B (`∅45,6`) as `\U+220545,6` and a record of the checksum of that text and that
/// text: `ACAD_MTEXT_RT` in R14 and R2000 (sample_AC1014.dwg, sample_AC1015.dwg), `ACAD_MTEXT_2008_RT` in R2004 (sample_AC1018.dwg); R2007
/// and later hold the character and no record. The model keeps the character; the checksum is the sum of every UTF-16 unit of the
/// stated text times its 1-based position (3427 for this text).
#[test]
fn an_mtext_with_a_character_the_code_page_lacks_states_the_round_trip_record_before_r2007() {
    let (document, text) = mtext_drawing("\u{2205}45,6");
    for (version, key) in [(DxfVersion::AC1014, "ACAD_MTEXT_RT"), (DxfVersion::AC1015, "ACAD_MTEXT_RT"), (DxfVersion::AC1018, "ACAD_MTEXT_2008_RT")] {
        let read = write_and_read(&document, version);
        assert_eq!(mtext_value(&read, text), "\u{2205}45,6", "{version:?}: the text stays");
        assert_eq!(mtext_record(&read, text), Some((key.to_string(), 3427.0, "\\U+220545,6".to_string())), "{version:?}");
        // the reader applied the record: it is no object of the model (the ACadSharp view above puts it back)
        assert!(
            !read.objects.values().any(|object| matches!(object, acadrust::objects::ObjectType::XRecord(record) if record.name.starts_with("ACAD_MTEXT"))),
            "{version:?}: the applied record is taken out of the model"
        );
        // written again at the same version, the record is the same (no new objects, no new record)
        let again = write_and_read(&read, version);
        assert_eq!(mtext_record(&again, text), mtext_record(&read, text), "{version:?} again");
    }
    for version in [DxfVersion::AC1021, DxfVersion::AC1024, DxfVersion::AC1032] {
        assert_eq!(mtext_record(&write_and_read(&document, version), text), None, "{version:?} holds the character itself");
    }
    // a text the code page has needs none
    let (plain, text) = mtext_drawing("caf\u{e9} one line");
    assert_eq!(mtext_record(&write_and_read(&plain, DxfVersion::AC1015), text), None);
}

/// A text with a paragraph break and a character the code page lacks: the file states `\P` and `\U+XXXX`, the record the checksum of
/// that spelling and the original; the reader gives the model the line feed and the character back and takes the record out (R5b-19
/// review: the record was left in the model and the text kept `\P`).
#[test]
fn an_mtext_with_a_paragraph_break_and_a_character_the_code_page_lacks_comes_back_on_read() {
    let original = "a\n\u{2205}5";
    let checksum: f64 = "a\\P\\U+22055".encode_utf16().enumerate().map(|(index, unit)| (index as f64 + 1.0) * f64::from(unit)).sum();
    let (document, text) = mtext_drawing(original);
    for version in [DxfVersion::AC1014, DxfVersion::AC1015] {
        let read = write_and_read(&document, version);
        assert_eq!(mtext_value(&read, text), original, "{version:?}");
        let (key, stated, _) = mtext_record(&read, text).expect("the record in the view");
        assert_eq!((key.as_str(), stated), ("ACAD_MTEXT_RT", checksum), "{version:?}");
        assert!(
            !read.objects.values().any(|object| matches!(object, acadrust::objects::ObjectType::XRecord(record) if record.name.starts_with("ACAD_MTEXT"))),
            "{version:?}: the applied record is taken out of the model"
        );
        let again = write_and_read(&read, version);
        assert_eq!(mtext_value(&again, text), original, "{version:?} again");
        assert_eq!(mtext_record(&again, text), mtext_record(&read, text), "{version:?} again");
    }
}

/// AutoCAD's R2000 file states the stacked fraction `{\H1x;\S1#4;}` of the MTEXT #662 as `\S1/4;` and the original in the record
/// (checksum 3566 of `4'-0\S1/4;"`); its R14 file has no such text and its R2004 file keeps the original. The original comes back on read.
#[test]
fn a_stacked_fraction_is_stated_with_the_slash_in_r2000_only() {
    let original = "4'-0{\\H1x;\\S1#4;}\"";
    let (document, text) = mtext_drawing(original);
    let r2000 = write_and_read(&document, DxfVersion::AC1015);
    assert_eq!(mtext_value(&r2000, text), original, "the reader restores the original");
    assert_eq!(mtext_record(&r2000, text), Some(("ACAD_MTEXT_RT".to_string(), 3566.0, original.to_string())));
    for version in [DxfVersion::AC1014, DxfVersion::AC1018, DxfVersion::AC1032] {
        let read = write_and_read(&document, version);
        assert_eq!(mtext_value(&read, text), original, "{version:?}");
        assert_eq!(mtext_record(&read, text), None, "{version:?} keeps the original and states no record");
    }
}

/// AutoCAD's R12 DXF (sample_AC1009_ascii.dxf) holds the R14 form of the `DSTYLE` data (`6 ARCHTICK`, `270`, `271`, `5`): a DXF of R14 or
/// earlier states it as a DWG of R14 does, a DXF of R2000 and later the R2000 form (handles).
#[test]
fn a_dxf_of_r14_or_earlier_states_the_r14_form_of_the_dstyle_data() {
    let drawing = drawing();
    let lines = |version: DxfVersion| {
        let mut document = drawing.document.clone();
        document.version = version;
        let text = String::from_utf8_lossy(&acadrust::DxfWriter::new(&document).write_to_vec().unwrap()).into_owned();
        text.lines().map(|line| line.trim().to_string()).collect::<Vec<_>>()
    };
    let r14 = lines(DxfVersion::AC1014);
    assert!(r14.iter().any(|line| line == "ARCHTICK"), "R14 names the arrowhead block");
    assert!(!r14.iter().any(|line| line == "343"), "no handle record in the R14 form");
    let r2000 = lines(DxfVersion::AC1015);
    assert!(!r2000.iter().any(|line| line == "ARCHTICK"));
    assert!(r2000.iter().any(|line| line == "343"), "R2000 states the arrowhead handle");
}
