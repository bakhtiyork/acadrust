// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]
//! Job R5a-6 (gate G4, DXF written at the version of the source): an R12 file is written as an R12 file (text and
//! binary) and reads back to the same model, and binary DXF is written at the version of the document (R13, R14,
//! R2000, R2004, ...) instead of as R2010. Every document is synthetic: R12 sources are written out here, the others
//! are built in memory. The models are compared through `native_dump` (the fork's own model, one JSON record per
//! object).
use std::io::Cursor;

use acadrust::entities::{EntityType, LwPolyline, Text};
use acadrust::types::{Vector2, Vector3};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfVersion, DxfWriter};

fn dump(document: &CadDocument) -> String {
    let mut out = Vec::new();
    acadrust::native_dump::write_native_dump(document, &mut out).expect("native dump");
    String::from_utf8(out).expect("UTF-8 dump")
}

fn read(bytes: Vec<u8>) -> CadDocument {
    DxfReader::from_reader(Cursor::new(bytes)).expect("DXF reader").read().expect("DXF read")
}

/// The dump without the padding of ASCII integers: a text DXF states a number in a fixed-width field (`     0`) and a
/// binary one as a number, and the model keeps the raw group values of layouts and table styles as text.
fn unpadded(dump: &str) -> String {
    let mut out = String::with_capacity(dump.len());
    let mut chars = dump.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '"' {
            let mut spaces = 0;
            while chars.peek() == Some(&' ') {
                chars.next();
                spaces += 1;
            }
            if !chars.peek().is_some_and(|next| next.is_ascii_digit() || *next == '-') {
                out.extend(std::iter::repeat(' ').take(spaces));
            }
        }
    }
    out
}

/// (code, value) pairs of an ASCII DXF.
fn pairs(text: &str) -> Vec<(i32, String)> {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .chunks_exact(2)
        .map(|pair| (pair[0].trim().parse().expect("group code"), pair[1].trim().to_string()))
        .collect()
}

fn dxf(groups: &[(i32, &str)]) -> String {
    groups.iter().map(|(code, value)| format!("{code}\n{value}\n")).collect()
}

/// An R12 file as AutoCAD writes it: table records without handles, BLOCK without a handle and ENDBLK with one,
/// entity handles when `handles`, the VIEWPORT as centre, size, status and number with the view in its MVIEW
/// record. `$VIEWCTR` and `$SNAPMODE` are the pre-R13 view state, which the model keeps apart.
fn r12_source(handles: bool) -> String {
    let h = |value: &'static str| -> Vec<(i32, &'static str)> { if handles { vec![(5, value)] } else { vec![] } };
    let mut g: Vec<(i32, &str)> = vec![
        (0, "SECTION"),
        (2, "HEADER"),
        (9, "$ACADVER"),
        (1, "AC1009"),
        (9, "$INSBASE"),
        (10, "0.0"),
        (20, "0.0"),
        (30, "0.0"),
        (9, "$LTSCALE"),
        (40, "2.0"),
        (9, "$CLAYER"),
        (8, "LAYER1"),
        (9, "$HANDLING"),
        (70, if handles { "1" } else { "0" }),
        (9, "$HANDSEED"),
        (5, "100"),
        (9, "$UNITMODE"),
        (70, "1"),
        (9, "$VIEWCTR"),
        (10, "5.0"),
        (20, "6.0"),
        (30, "0.0"),
        (9, "$SNAPMODE"),
        (70, "1"),
        (0, "ENDSEC"),
        (0, "SECTION"),
        (2, "TABLES"),
        // VPORT
        (0, "TABLE"),
        (2, "VPORT"),
        (70, "1"),
        (0, "VPORT"),
        (2, "*ACTIVE"),
        (70, "0"),
        (10, "0.0"),
        (20, "0.0"),
        (11, "1.0"),
        (21, "1.0"),
        (12, "5.0"),
        (22, "5.0"),
        (13, "0.0"),
        (23, "0.0"),
        (14, "0.5"),
        (24, "0.5"),
        (15, "0.5"),
        (25, "0.5"),
        (16, "0.0"),
        (26, "0.0"),
        (36, "1.0"),
        (17, "0.0"),
        (27, "0.0"),
        (37, "0.0"),
        (40, "10.0"),
        (41, "1.5"),
        (42, "50.0"),
        (43, "0.0"),
        (44, "0.0"),
        (50, "0.0"),
        (51, "0.0"),
        (71, "0"),
        (72, "100"),
        (73, "1"),
        (74, "3"),
        (75, "0"),
        (76, "0"),
        (77, "0"),
        (78, "0"),
        (0, "ENDTAB"),
        // LTYPE
        (0, "TABLE"),
        (2, "LTYPE"),
        (70, "2"),
        (0, "LTYPE"),
        (2, "CONTINUOUS"),
        (70, "0"),
        (3, "Solid line"),
        (72, "65"),
        (73, "0"),
        (40, "0.0"),
        (0, "LTYPE"),
        (2, "DASHED"),
        (70, "0"),
        (3, "Dashed"),
        (72, "65"),
        (73, "2"),
        (40, "0.75"),
        (49, "0.5"),
        (49, "-0.25"),
        (0, "ENDTAB"),
        // LAYER
        (0, "TABLE"),
        (2, "LAYER"),
        (70, "2"),
        (0, "LAYER"),
        (2, "0"),
        (70, "0"),
        (62, "7"),
        (6, "CONTINUOUS"),
        (0, "LAYER"),
        (2, "LAYER1"),
        (70, "0"),
        (62, "3"),
        (6, "DASHED"),
        (0, "ENDTAB"),
        // STYLE
        (0, "TABLE"),
        (2, "STYLE"),
        (70, "1"),
        (0, "STYLE"),
        (2, "STANDARD"),
        (70, "0"),
        (40, "0.0"),
        (41, "1.0"),
        (50, "0.0"),
        (71, "0"),
        (42, "0.2"),
        (3, "txt"),
        (4, ""),
        (0, "ENDTAB"),
        // VIEW, UCS: empty
        (0, "TABLE"),
        (2, "VIEW"),
        (70, "0"),
        (0, "ENDTAB"),
        (0, "TABLE"),
        (2, "UCS"),
        (70, "0"),
        (0, "ENDTAB"),
        // APPID
        (0, "TABLE"),
        (2, "APPID"),
        (70, "2"),
        (0, "APPID"),
        (2, "ACAD"),
        (70, "0"),
        (0, "APPID"),
        (2, "MY_APP"),
        (70, "0"),
        (0, "ENDTAB"),
        // DIMSTYLE
        (0, "TABLE"),
        (2, "DIMSTYLE"),
        (70, "1"),
        (0, "DIMSTYLE"),
        (2, "STANDARD"),
        (70, "0"),
        (3, ""),
        (4, ""),
        (5, ""),
        (6, ""),
        (7, ""),
        (40, "1.0"),
        (41, "0.18"),
        (42, "0.0625"),
        (43, "0.38"),
        (44, "0.18"),
        (45, "0.0"),
        (46, "0.0"),
        (47, "0.0"),
        (48, "0.0"),
        (140, "0.18"),
        (141, "0.09"),
        (142, "0.0"),
        (143, "25.4"),
        (144, "1.0"),
        (145, "0.0"),
        (146, "1.0"),
        (147, "0.09"),
        (71, "0"),
        (72, "0"),
        (73, "1"),
        (74, "1"),
        (75, "0"),
        (76, "0"),
        (77, "0"),
        (78, "0"),
        (170, "0"),
        (171, "2"),
        (172, "0"),
        (173, "0"),
        (174, "0"),
        (175, "0"),
        (176, "0"),
        (177, "0"),
        (178, "0"),
        (0, "ENDTAB"),
        (0, "ENDSEC"),
        // BLOCKS
        (0, "SECTION"),
        (2, "BLOCKS"),
        (0, "BLOCK"),
        (8, "0"),
        (2, "$MODEL_SPACE"),
        (70, "2"),
        (10, "0.0"),
        (20, "0.0"),
        (30, "0.0"),
        (3, "$MODEL_SPACE"),
        (1, ""),
        (0, "ENDBLK"),
    ];
    g.extend(h("21"));
    g.extend([
        (8, "0"),
        (0, "BLOCK"),
        (67, "1"),
        (8, "0"),
        (2, "$PAPER_SPACE"),
        (70, "0"),
        (10, "0.0"),
        (20, "0.0"),
        (30, "0.0"),
        (3, "$PAPER_SPACE"),
        (1, ""),
        (0, "ENDBLK"),
    ]);
    g.extend(h("22"));
    g.extend([
        (67, "1"),
        (8, "0"),
        (0, "BLOCK"),
        (8, "0"),
        (2, "PART"),
        (70, "0"),
        (10, "1.0"),
        (20, "1.0"),
        (30, "0.0"),
        (3, "PART"),
        (1, ""),
        (0, "LINE"),
    ]);
    g.extend(h("30"));
    g.extend([(8, "0"), (10, "0.0"), (20, "0.0"), (30, "0.0"), (11, "1.0"), (21, "1.0"), (31, "0.0"), (0, "ENDBLK")]);
    g.extend(h("31"));
    g.extend([
        (8, "0"),
        (0, "ENDSEC"),
        // ENTITIES
        (0, "SECTION"),
        (2, "ENTITIES"),
        (0, "LINE"),
    ]);
    g.extend(h("40"));
    g.extend([
        (8, "LAYER1"),
        (6, "DASHED"),
        (62, "1"),
        (10, "0.0"),
        (20, "0.0"),
        (30, "0.0"),
        (11, "4.0"),
        (21, "3.0"),
        (31, "0.0"),
        (1001, "MY_APP"),
        (1000, "tag"),
        // A POLYLINE with default widths: the first VERTEX has none, the second states zero widths.
        (0, "POLYLINE"),
    ]);
    g.extend(h("41"));
    g.extend([
        (8, "0"),
        (66, "1"),
        (10, "0.0"),
        (20, "0.0"),
        (30, "0.0"),
        (40, "0.2"),
        (41, "0.1"),
        (70, "1"),
        (0, "VERTEX"),
    ]);
    g.extend(h("42"));
    g.extend([(8, "0"), (10, "0.0"), (20, "0.0"), (30, "0.0"), (0, "VERTEX")]);
    g.extend(h("43"));
    g.extend([
        (8, "0"),
        (10, "1.0"),
        (20, "0.0"),
        (30, "0.0"),
        (40, "0.0"),
        (41, "0.0"),
        (42, "0.5"),
        (0, "VERTEX"),
    ]);
    g.extend(h("44"));
    g.extend([(8, "0"), (10, "1.0"), (20, "1.0"), (30, "0.0"), (40, "0.3"), (41, "0.4"), (0, "SEQEND")]);
    g.extend(h("45"));
    g.extend([(8, "0"), (0, "INSERT")]);
    g.extend(h("46"));
    g.extend([(8, "0"), (2, "PART"), (10, "3.0"), (20, "3.0"), (30, "0.0"), (0, "TEXT")]);
    g.extend(h("47"));
    g.extend([(8, "0"), (10, "1.0"), (20, "1.0"), (30, "0.0"), (40, "1.0"), (1, "Hello"), (0, "VIEWPORT")]);
    g.extend(h("48"));
    g.extend([
        (67, "1"),
        (8, "0"),
        (10, "5.0"),
        (20, "5.0"),
        (30, "0.0"),
        (40, "4.0"),
        (41, "3.0"),
        (68, "1"),
        (69, "1"),
        (1001, "ACAD"),
        (1000, "MVIEW"),
        (1002, "{"),
        (1070, "16"),
        (1010, "0.0"),
        (1020, "0.0"),
        (1030, "0.0"),
        (1010, "0.0"),
        (1020, "0.0"),
        (1030, "1.0"),
        (1040, "0.0"),
        (1040, "3.0"),
        (1040, "5.0"),
        (1040, "5.0"),
        (1040, "50.0"),
        (1040, "0.0"),
        (1040, "0.0"),
        (1070, "0"),
        (1070, "1000"),
        (1070, "1"),
        (1070, "1"),
        (1070, "0"),
        (1070, "0"),
        (1070, "0"),
        (1070, "0"),
        (1040, "0.0"),
        (1040, "0.0"),
        (1040, "0.0"),
        (1040, "0.5"),
        (1040, "0.5"),
        (1040, "0.5"),
        (1040, "0.5"),
        (1070, "0"),
        (1002, "{"),
        (1002, "}"),
        (1002, "}"),
        (0, "ENDSEC"),
        (0, "EOF"),
    ]);
    dxf(&g)
}

fn r12_document(handles: bool) -> CadDocument {
    let document = read(r12_source(handles).into_bytes());
    assert_eq!(document.version, DxfVersion::AC1009);
    document
}

/// The records of the section `name` as (type, groups).
fn section_records(pairs: &[(i32, String)], name: &str) -> Vec<(String, Vec<(i32, String)>)> {
    let mut inside = false;
    let mut records: Vec<(String, Vec<(i32, String)>)> = Vec::new();
    for (index, (code, value)) in pairs.iter().enumerate() {
        if *code == 2 && index > 0 && pairs[index - 1] == (0, "SECTION".to_string()) {
            inside = value == name;
            continue;
        }
        if !inside {
            continue;
        }
        if *code == 0 {
            if value == "ENDSEC" {
                inside = false;
            } else {
                records.push((value.clone(), Vec::new()));
            }
        } else if let Some(record) = records.last_mut() {
            record.1.push((*code, value.clone()));
        }
    }
    records
}

fn assert_r12_structure(text: &str, handles: bool) {
    let pairs = pairs(text);
    for (code, value) in &pairs {
        assert!(![100, 102, 105, 330, 360].contains(code), "group {code} {value} does not exist in R12");
    }
    let version = pairs.iter().position(|pair| pair == &(9, "$ACADVER".to_string())).expect("$ACADVER");
    assert_eq!(pairs[version + 1].1, "AC1009");
    let sections: Vec<&String> =
        pairs.windows(2).filter(|pair| pair[0] == (0, "SECTION".to_string())).map(|pair| &pair[1].1).collect();
    assert_eq!(sections, ["HEADER", "TABLES", "BLOCKS", "ENTITIES"]);
    assert!(!pairs.contains(&(2, "BLOCK_RECORD".into())));
    // Table records are not handled objects.
    for (kind, groups) in section_records(&pairs, "TABLES") {
        // (group 5 of a DIMSTYLE is the name of the DIMBLK arrow block)
        if matches!(kind.as_str(), "LAYER" | "LTYPE" | "STYLE" | "APPID" | "VPORT") {
            assert!(groups.iter().all(|(code, _)| *code != 5), "{kind} record with a handle");
        }
    }
    // BLOCK has no handle, ENDBLK has the one the source stated.
    let blocks = section_records(&pairs, "BLOCKS");
    for (kind, groups) in &blocks {
        match kind.as_str() {
            "BLOCK" => assert!(groups.iter().all(|(code, _)| *code != 5)),
            "ENDBLK" => assert_eq!(groups.iter().any(|(code, _)| *code == 5), handles),
            _ => {}
        }
    }
    let names: Vec<&str> = blocks
        .iter()
        .filter(|(kind, _)| kind == "BLOCK")
        .flat_map(|(_, groups)| groups.iter().filter(|(code, _)| *code == 2).map(|(_, name)| name.as_str()))
        .collect();
    assert_eq!(names, ["$MODEL_SPACE", "$PAPER_SPACE", "PART"]);
    // The VIEWPORT is centre, size, status and number; the rest of the view is its MVIEW record.
    let entities = section_records(&pairs, "ENTITIES");
    let (_, viewport) = entities.iter().find(|(kind, _)| kind == "VIEWPORT").expect("VIEWPORT");
    let codes: Vec<i32> = viewport.iter().map(|(code, _)| *code).filter(|code| *code < 1000).collect();
    let expected: Vec<i32> = if handles { vec![5, 67, 8, 10, 20, 30, 40, 41, 68, 69] } else { vec![67, 8, 10, 20, 30, 40, 41, 68, 69] };
    assert_eq!(codes, expected);
    assert!(viewport.contains(&(1000, "MVIEW".to_string())));
    // Handles of the entities only when the source had them.
    let handled = entities.iter().filter(|(_, groups)| groups.iter().any(|(code, _)| *code == 5)).count();
    assert_eq!(handled == entities.len(), handles);
    assert_eq!(handled == 0, !handles);
    let handling = pairs.iter().position(|pair| pair == &(9, "$HANDLING".to_string())).expect("$HANDLING");
    assert_eq!(pairs[handling + 1].1, if handles { "1" } else { "0" });
}

#[test]
fn r12_ascii_is_written_as_r12_and_reads_back_to_the_same_model() {
    for handles in [true, false] {
        let document = r12_document(handles);
        let bytes = DxfWriter::new(&document).write_to_vec().expect("R12 write");
        let text = String::from_utf8(bytes.clone()).expect("ASCII");
        assert_r12_structure(&text, handles);
        let again = read(bytes);
        assert_eq!(again.version, DxfVersion::AC1009);
        assert_eq!(dump(&again), dump(&document), "handles={handles}");
        // Writing the re-read document states the same file.
        assert_eq!(DxfWriter::new(&again).write_to_vec().unwrap(), text.as_bytes(), "handles={handles}");
    }
}

#[test]
fn r12_binary_is_written_with_one_byte_group_codes_and_reads_back_to_the_same_model() {
    for handles in [true, false] {
        let document = r12_document(handles);
        let bytes = DxfWriter::new_binary(&document).write_to_vec().expect("binary R12 write");
        assert!(bytes.starts_with(b"AutoCAD Binary DXF\r\n\x1a\0"));
        // After the sentinel: code 0 as one byte, then the string SECTION.
        assert_eq!(&bytes[22..30], b"\0SECTION");
        let again = read(bytes.clone());
        assert_eq!(again.version, DxfVersion::AC1009);
        assert_eq!(dump(&again), dump(&document), "handles={handles}");
        // The binary file states what the text file states.
        let text = DxfWriter::new(&document).write_to_vec().unwrap();
        assert_eq!(dump(&read(text)), dump(&again), "handles={handles}");
        assert_eq!(DxfWriter::new_binary(&again).write_to_vec().unwrap(), bytes, "handles={handles}");
    }
}

#[test]
fn r12_polyline_vertex_widths_survive_the_default_width_of_the_record() {
    let document = r12_document(true);
    let widths = |document: &CadDocument| -> Vec<(f64, f64)> {
        document
            .entities()
            .find_map(|entity| match entity {
                EntityType::Polyline2D(polyline) => {
                    Some(polyline.vertices.iter().map(|vertex| (vertex.start_width, vertex.end_width)).collect())
                }
                _ => None,
            })
            .expect("a 2D polyline")
    };
    // The first vertex has no widths (the record's 0.2 / 0.1), the second states zero, the third its own.
    assert_eq!(widths(&document), [(0.2, 0.1), (0.0, 0.0), (0.3, 0.4)]);
    let text = String::from_utf8(DxfWriter::new(&document).write_to_vec().unwrap()).unwrap();
    let again = read(text.into_bytes());
    assert_eq!(widths(&again), widths(&document));
}

#[test]
fn r12_file_keeps_the_pre_r13_view_state_of_the_header() {
    let document = r12_document(true);
    let state = document.header.legacy.clone().expect("the legacy header state");
    assert_eq!(state.view_center, Vector3::new(5.0, 6.0, 0.0));
    assert_eq!(state.snap_mode, 1);
    assert_eq!(state.unit_mode, 1);
    let text = String::from_utf8(DxfWriter::new(&document).write_to_vec().unwrap()).unwrap();
    let again = read(text.clone().into_bytes());
    assert_eq!(again.header.legacy, Some(state));
    // Only what differs from the default state is stated, and nothing that is not an R12 header variable.
    let header = pairs(&text);
    let variables: Vec<&str> =
        header.iter().filter(|(code, _)| *code == 9).map(|(_, name)| name.as_str()).collect();
    assert!(variables.contains(&"$VIEWCTR") && variables.contains(&"$SNAPMODE") && variables.contains(&"$UNITMODE"));
    assert!(!variables.contains(&"$GRIDMODE"));
    assert!(variables.iter().all(|name| !matches!(*name, "$CMLSTYLE" | "$DIMDSEP" | "$PSTYLEMODE" | "$ACADMAINTVER")));
}

#[test]
fn an_ac1009_document_with_a_newer_entity_is_converted_to_r12_entities() {
    let mut document = r12_document(true);
    let mut polyline = LwPolyline::from_points(vec![Vector2::new(0.0, 0.0), Vector2::new(2.0, 0.0), Vector2::new(2.0, 2.0)]);
    polyline.vertices[0].bulge = 0.5;
    document.add_entity(EntityType::LwPolyline(polyline)).unwrap();
    let mtext = acadrust::entities::MText::with_value("Note", Vector3::new(1.0, 1.0, 0.0));
    document.add_entity(EntityType::MText(Box::new(mtext))).unwrap();
    for binary in [false, true] {
        let writer = if binary { DxfWriter::new_binary(&document) } else { DxfWriter::new(&document) };
        let bytes = writer.write_to_vec().expect("R12 write");
        let again = read(bytes);
        assert_eq!(again.version, DxfVersion::AC1009, "binary={binary}");
        assert!(
            again.entities().all(|entity| !matches!(entity, EntityType::LwPolyline(_) | EntityType::MText(_))),
            "binary={binary}"
        );
        assert!(
            again.entities().any(|entity| matches!(entity, EntityType::Text(text) if text.value == "Note")),
            "the MTEXT becomes a TEXT, binary={binary}"
        );
        assert_eq!(
            again.entities().filter(|entity| matches!(entity, EntityType::Polyline2D(_))).count(),
            2,
            "binary={binary}"
        );
        // Only the newer entity is converted: the R12 records of the rest keep their handles and extended data.
        let line = again.get_entity(acadrust::types::Handle::new(0x40)).expect("the LINE keeps its handle");
        assert!(matches!(line, EntityType::Line(_)), "binary={binary}");
        assert!(line.common().extended_data.get_record("MY_APP").is_some(), "binary={binary}");
        for entity in document.entities().filter(|entity| !matches!(entity, EntityType::LwPolyline(_) | EntityType::MText(_))) {
            assert!(again.get_entity(entity.common().handle).is_some(), "binary={binary}");
        }
    }
}

/// An entity added to a document read from an R12 file states its handle while the file has handles (`$HANDLING`
/// 1), so it keeps it; a file without handles states none.
#[test]
fn an_entity_added_to_an_r12_document_keeps_its_handle() {
    for handles in [true, false] {
        let mut document = r12_document(handles);
        let added = document
            .add_entity(EntityType::Line(acadrust::entities::Line::from_points(Vector3::ZERO, Vector3::new(5.0, 5.0, 0.0))))
            .unwrap();
        for binary in [false, true] {
            let writer = if binary { DxfWriter::new_binary(&document) } else { DxfWriter::new(&document) };
            let bytes = writer.write_to_vec().expect("R12 write");
            if !binary {
                let text = String::from_utf8(bytes.clone()).unwrap();
                let pairs = pairs(&text);
                let handled = section_records(&pairs, "ENTITIES")
                    .iter()
                    .filter(|(_, groups)| groups.iter().any(|(code, _)| *code == 5))
                    .count();
                assert_eq!(handled > 0, handles, "handles={handles}");
                let seed = pairs.iter().position(|pair| pair == &(9, "$HANDSEED".to_string())).unwrap();
                let seed = u64::from_str_radix(&pairs[seed + 1].1, 16).unwrap();
                assert!(!handles || seed > added.value(), "$HANDSEED above the added handle");
            }
            let again = read(bytes);
            assert_eq!(again.entities().count(), document.entities().count(), "handles={handles} binary={binary}");
            if handles {
                let line = again.get_entity(added).expect("the added LINE keeps its handle");
                assert!(matches!(line, EntityType::Line(_)), "binary={binary}");
            }
        }
    }
}

/// Binary R13, R14, R2000 and R2004 DXF: declared at the version of the document, strings in the declared code page,
/// and the model it reads back to is the one the text form reads back to.
#[test]
fn binary_dxf_is_written_at_the_version_of_the_document() {
    for version in [DxfVersion::AC1012, DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018] {
        let mut document = CadDocument::with_version(version);
        document.header.code_page = "ANSI_1252".into();
        document.layers.add(acadrust::tables::Layer::new("Walls")).unwrap();
        let mut text = Text::with_value("caf\u{E9}", Vector3::new(1.0, 2.0, 0.0));
        text.common.layer = "Walls".into();
        document.add_entity(EntityType::Text(text)).unwrap();
        document
            .add_entity(EntityType::Line(acadrust::entities::Line::from_points(Vector3::ZERO, Vector3::new(3.0, 4.0, 0.0))))
            .unwrap();

        let binary = DxfWriter::new_binary(&document).write_to_vec().expect("binary write");
        assert!(binary.starts_with(b"AutoCAD Binary DXF\r\n\x1a\0"), "{version:?}");
        // Two-byte group codes from R13 on: the first group is 0 as 16 bits, then SECTION.
        assert_eq!(&binary[22..31], b"\0\0SECTION", "{version:?}");
        // $ACADVER is the document's version, not AC1024.
        let declared = format!("{}\0", version.to_dxf_string());
        let at = binary.windows(8).position(|window| window == b"$ACADVER").expect("$ACADVER");
        assert_eq!(&binary[at + 9 + 2..at + 9 + 2 + declared.len()], declared.as_bytes(), "{version:?}");
        // The text is in the declared code page (the byte E9), not UTF-8.
        assert!(binary.windows(4).any(|window| window == b"caf\xE9"), "{version:?}");

        let from_binary = read(binary);
        assert_eq!(from_binary.version, version);
        let from_text = read(DxfWriter::new(&document).write_to_vec().expect("text write"));
        assert_eq!(unpadded(&dump(&from_binary)), unpadded(&dump(&from_text)), "{version:?}");
        let value = from_binary
            .entities()
            .find_map(|entity| match entity {
                EntityType::Text(text) => Some(text.value.clone()),
                _ => None,
            })
            .expect("text");
        assert_eq!(value, "caf\u{E9}", "{version:?}");
    }
}

/// A document read from an R12 DWG has no DXF source: the entities state their handles, the table records none,
/// and the file reads back with the same entities.
#[test]
fn an_r12_dwg_is_written_as_an_r12_dxf_that_states_the_handles_of_its_entities() {
    let mut document = CadDocument::with_version(DxfVersion::AC1009);
    document
        .add_entity(EntityType::Line(acadrust::entities::Line::from_points(Vector3::ZERO, Vector3::new(3.0, 4.0, 0.0))))
        .unwrap();
    document
        .add_entity(EntityType::Circle(acadrust::entities::Circle::from_center_radius(Vector3::new(1.0, 1.0, 0.0), 2.0)))
        .unwrap();
    document.add_entity(EntityType::Text(Text::with_value("R12", Vector3::new(1.0, 2.0, 0.0)))).unwrap();
    let dwg = DwgReader::from_stream(Cursor::new(DwgWriter::write_to_vec(&document).expect("DWG write")))
        .read()
        .expect("DWG read");
    assert_eq!(dwg.version, DxfVersion::AC1009);
    let entities = dwg.entities().count();
    assert_eq!(entities, 3);

    for binary in [false, true] {
        let writer = if binary { DxfWriter::new_binary(&dwg) } else { DxfWriter::new(&dwg) };
        let bytes = writer.write_to_vec().expect("R12 write");
        if !binary {
            assert_r12_structure_of_a_handled_document(&String::from_utf8(bytes.clone()).unwrap());
        }
        let again = read(bytes);
        assert_eq!(again.version, DxfVersion::AC1009, "binary={binary}");
        assert_eq!(again.entities().count(), entities, "binary={binary}");
        for entity in dwg.entities() {
            let handle = entity.common().handle;
            assert!(again.get_entity(handle).is_some(), "entity {handle:?} keeps its handle, binary={binary}");
        }
    }
}

fn assert_r12_structure_of_a_handled_document(text: &str) {
    let pairs = pairs(text);
    assert!(pairs.iter().all(|(code, _)| ![100, 102, 105, 330, 360].contains(code)));
    let handling = pairs.iter().position(|pair| pair == &(9, "$HANDLING".to_string())).expect("$HANDLING");
    assert_eq!(pairs[handling + 1].1, "1");
    for (kind, groups) in section_records(&pairs, "ENTITIES") {
        assert!(groups.iter().any(|(code, _)| *code == 5), "{kind} states its handle");
    }
    for (kind, groups) in section_records(&pairs, "TABLES") {
        if matches!(kind.as_str(), "LAYER" | "LTYPE" | "STYLE" | "APPID" | "VPORT") {
            assert!(groups.iter().all(|(code, _)| *code != 5), "{kind} record with a handle");
        }
    }
}

/// A string of a binary DXF that is not UTF-8, in a file with no declared code page, reads as the text reader reads
/// it (byte per byte as Latin-1) and keeps its bytes for the ACadSharp view (which decodes them as UTF-8 and loses
/// them); it used to read as U+FFFD.
#[test]
fn binary_dxf_text_without_a_code_page_reads_like_ascii_dxf_text() {
    let document = r12_document(true);
    let patch = |bytes: Vec<u8>| -> Vec<u8> {
        let at = bytes.windows(6).position(|window| window == b"Hello\0" || window == b"Hello\r").expect("the TEXT value");
        let mut bytes = bytes;
        bytes[at] = 0xD8;
        bytes
    };
    let text_of = |document: &CadDocument| -> (String, Option<Vec<u8>>) {
        document
            .entities()
            .find_map(|entity| match entity {
                EntityType::Text(text) => {
                    Some((text.value.clone(), text.source.as_ref().and_then(|source| source.undecoded_bytes.clone())))
                }
                _ => None,
            })
            .expect("a TEXT")
    };
    let ascii = read(patch(DxfWriter::new(&document).write_to_vec().unwrap()));
    let binary = read(patch(DxfWriter::new_binary(&document).write_to_vec().unwrap()));
    let (value, bytes) = text_of(&ascii);
    assert_eq!(value, "\u{D8}ello");
    assert_eq!(bytes.as_deref(), Some(&b"\xD8ello"[..]));
    assert_eq!(text_of(&binary), (value, bytes));
}
