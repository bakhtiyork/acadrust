// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]
//! R5b-13 (gate G5-RR, the R12 sources): what the fork's DWG of an R12 DXF loses or states differently from its native model.
//! Synthetic R12 documents only; each test fails with its fix reverted. Evidence: AutoCAD's own files of one drawing
//! (sample_AC1014.dwg to sample_AC1032.dwg; the C# dumps under Tests/CadCoreTests/Fixtures/dumps) and the R12 sample
//! (sample_AC1009_ascii.dxf).

use std::io::Cursor;

use acadrust::entities::{EntityType, Viewport};
use acadrust::types::DxfVersion;
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfWriter};

/// An R12 file: the layer 0, the text styles STANDARD and two unnamed shape styles, the block `_BOXBLANK`, a dimension style `STD`
/// that names its arrowhead `BOXBLANK` (group 5), a polyline with default widths 0.2 / 0.1 and three vertices (own widths 0.3 / 0.3,
/// none, an explicit zero), and a viewport in the paper space.
fn r12_text(arrow: &str) -> String {
    let pairs: Vec<(&str, String)> = vec![
        ("0", "SECTION".into()), ("2", "HEADER".into()), ("9", "$ACADVER".into()), ("1", "AC1009".into()), ("9", "$HANDSEED".into()),
        ("5", "1".into()), ("0", "ENDSEC".into()),
        ("0", "SECTION".into()), ("2", "TABLES".into()),
        ("0", "TABLE".into()), ("2", "VPORT".into()), ("70", "0".into()), ("0", "ENDTAB".into()),
        ("0", "TABLE".into()), ("2", "LAYER".into()), ("70", "1".into()), ("0", "LAYER".into()), ("2", "0".into()), ("70", "0".into()),
        ("62", "7".into()), ("6", "CONTINUOUS".into()), ("0", "ENDTAB".into()),
        ("0", "TABLE".into()), ("2", "STYLE".into()), ("70", "3".into()),
        ("0", "STYLE".into()), ("2", "STANDARD".into()), ("70", "0".into()), ("40", "0.0".into()), ("41", "1.0".into()), ("50", "0.0".into()),
        ("71", "0".into()), ("42", "0.5".into()), ("3", "txt".into()), ("4", "".into()),
        ("0", "STYLE".into()), ("2", "".into()), ("70", "1".into()), ("40", "0.0".into()), ("41", "1.0".into()), ("50", "0.0".into()),
        ("71", "0".into()), ("42", "0.2".into()), ("3", "one.shx".into()), ("4", "".into()),
        ("0", "STYLE".into()), ("2", "".into()), ("70", "1".into()), ("40", "0.0".into()), ("41", "1.0".into()), ("50", "0.0".into()),
        ("71", "0".into()), ("42", "7.5".into()), ("3", "two.shx".into()), ("4", "".into()),
        ("0", "ENDTAB".into()),
        ("0", "TABLE".into()), ("2", "DIMSTYLE".into()), ("70", "1".into()),
        ("0", "DIMSTYLE".into()), ("2", "STD".into()), ("70", "0".into()), ("3", "".into()), ("4", "".into()), ("5", arrow.into()),
        ("0", "ENDTAB".into()),
        ("0", "ENDSEC".into()),
        ("0", "SECTION".into()), ("2", "BLOCKS".into()),
        ("0", "BLOCK".into()), ("8", "0".into()), ("2", "_BOXBLANK".into()), ("70", "0".into()), ("10", "0".into()), ("20", "0".into()),
        ("30", "0".into()), ("3", "_BOXBLANK".into()), ("0", "ENDBLK".into()), ("8", "0".into()),
        ("0", "ENDSEC".into()),
        ("0", "SECTION".into()), ("2", "ENTITIES".into()),
        ("0", "POLYLINE".into()), ("8", "0".into()), ("66", "1".into()), ("10", "0".into()), ("20", "0".into()), ("30", "0".into()),
        ("40", "0.2".into()), ("41", "0.1".into()), ("70", "0".into()),
        ("0", "VERTEX".into()), ("8", "0".into()), ("10", "0".into()), ("20", "0".into()), ("30", "0".into()), ("40", "0.3".into()),
        ("41", "0.3".into()), ("70", "0".into()),
        ("0", "VERTEX".into()), ("8", "0".into()), ("10", "5".into()), ("20", "0".into()), ("30", "0".into()), ("70", "0".into()),
        ("0", "VERTEX".into()), ("8", "0".into()), ("10", "5".into()), ("20", "5".into()), ("30", "0".into()), ("40", "0".into()),
        ("41", "0".into()), ("70", "0".into()),
        ("0", "SEQEND".into()), ("8", "0".into()),
        ("0", "VIEWPORT".into()), ("8", "0".into()), ("67", "1".into()), ("10", "5".into()), ("20", "5".into()), ("30", "0".into()),
        ("40", "10".into()), ("41", "5".into()), ("68", "1".into()), ("69", "2".into()),
        ("0", "ENDSEC".into()),
        ("0", "EOF".into()),
    ];
    pairs.iter().map(|(code, value)| format!("{code}\n{value}\n")).collect()
}

fn r12(arrow: &str) -> CadDocument {
    DxfReader::from_reader(Cursor::new(r12_text(arrow).into_bytes())).unwrap().read().unwrap()
}

fn write_and_read(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    let bytes = DwgWriter::write_to_vec(&document).unwrap_or_else(|error| panic!("{version:?}: DWG write failed: {error}"));
    DwgReader::from_stream(Cursor::new(bytes)).read().unwrap_or_else(|error| panic!("{version:?}: DWG read failed: {error}"))
}

fn dxf_round_trip(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let text = DxfWriter::new(document)
        .with_version(version)
        .write_to_vec()
        .unwrap_or_else(|error| panic!("{version:?}: DXF write failed: {error}"));
    DxfReader::from_reader(Cursor::new(text)).unwrap().read().unwrap()
}

const DWG_VERSIONS: [DxfVersion; 4] = [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1024, DxfVersion::AC1032];

fn polyline(document: &CadDocument) -> (f64, f64, Vec<(f64, f64)>) {
    for entity in document.entities() {
        if let EntityType::Polyline2D(polyline) = entity {
            let vertices = polyline.vertices.iter().map(|vertex| (vertex.start_width, vertex.end_width)).collect();
            return (polyline.start_width, polyline.end_width, vertices);
        }
    }
    panic!("no 2D polyline in {:?}", document.entities().map(|entity| entity.as_entity().entity_type()).collect::<Vec<_>>());
}

// ── R12 polyline default widths ──────────────────────────────────────────────────────────────────────────────────────────────

#[test]
fn the_default_widths_of_an_r12_polyline_are_the_polylines_own_with_widths_on_the_vertices() {
    let document = r12("BOXBLANK");
    // The vertices keep their own widths (an explicit zero too) and the one without takes the default; the polyline keeps the default.
    let expected = (0.2, 0.1, vec![(0.3, 0.3), (0.2, 0.1), (0.0, 0.0)]);
    assert_eq!(polyline(&document), expected);
    // The defaults are consumed into the polyline's own widths: no second copy stays in the model.
    for entity in document.entities() {
        if let EntityType::Polyline2D(polyline) = entity {
            assert_eq!(polyline.source_widths, None, "the read keeps the defaults in start_width / end_width only");
        }
    }
    for version in DWG_VERSIONS {
        assert_eq!(polyline(&write_and_read(&document, version)), expected, "{version:?} DWG");
    }
    // The R12 DXF states the polyline's defaults (groups 40 / 41) and only the widths that differ on the vertices; a DXF of R2000 and
    // later writes the heavy polyline as an LWPOLYLINE (widths per vertex), which has no such defaults.
    assert_eq!(polyline(&dxf_round_trip(&document, DxfVersion::AC1009)), expected, "R12 DXF");
}

#[test]
fn an_edit_of_the_default_widths_after_the_read_is_written() {
    let mut document = r12("BOXBLANK");
    for entity in document.entities_mut() {
        if let EntityType::Polyline2D(polyline) = entity {
            polyline.start_width = 0.5;
            polyline.end_width = 0.6;
        }
    }
    for version in DWG_VERSIONS {
        let (start, end, vertices) = polyline(&write_and_read(&document, version));
        assert_eq!((start, end), (0.5, 0.6), "{version:?} DWG");
        assert_eq!(vertices, vec![(0.3, 0.3), (0.2, 0.1), (0.0, 0.0)], "{version:?} DWG: the vertices keep their widths");
    }
    assert_eq!(polyline(&dxf_round_trip(&document, DxfVersion::AC1009)).0, 0.5);

    // Defaults cleared after the read stay cleared: the file's 0.2 / 0.1 do not come back in a DXF or a DWG.
    for entity in document.entities_mut() {
        if let EntityType::Polyline2D(polyline) = entity {
            polyline.start_width = 0.0;
            polyline.end_width = 0.0;
        }
    }
    let (start, end, _) = polyline(&dxf_round_trip(&document, DxfVersion::AC1009));
    assert_eq!((start, end), (0.0, 0.0), "R12 DXF");
    for version in DWG_VERSIONS {
        let (start, end, _) = polyline(&write_and_read(&document, version));
        assert_eq!((start, end), (0.0, 0.0), "{version:?} DWG");
    }
}

// ── Table style, layouts, dimension style, text styles, viewports, classes ───────────────────────────────────────────────────

fn standard_table_style(document: &CadDocument) -> &acadrust::objects::TableStyle {
    document
        .objects
        .values()
        .find_map(|object| match object {
            acadrust::objects::ObjectType::TableStyle(style) if style.name == "Standard" => Some(style),
            _ => None,
        })
        .expect("the Standard table style")
}

#[test]
fn the_default_table_style_of_an_r12_file_names_the_files_text_style() {
    let document = r12("BOXBLANK");
    let standard = document.text_styles.get("STANDARD").expect("STANDARD").handle;
    let style = standard_table_style(&document);
    for row in [&style.data_row_style, &style.header_row_style, &style.title_row_style] {
        assert_eq!(row.text_style_handle, Some(standard), "a row names the record the file spells STANDARD, not the default's handle");
    }
    // The R2010 record states the same style in its cell styles, with AutoCAD's margins and spacings (sample_AC1018.dwg against
    // sample_AC1024.dwg: margin flag 1, the style's margins on four sides, spacings 0.18).
    let read = write_and_read(&document, DxfVersion::AC1032);
    let style = standard_table_style(&read);
    assert_eq!(style.modern_overrides.len(), 3);
    for (_, named) in &style.modern_overrides {
        let cell = &named.cell_style;
        assert_eq!(cell.margin_override_flags, 1);
        assert_eq!((cell.vertical_margin, cell.horizontal_margin, cell.bottom_margin, cell.right_margin), (0.06, 0.06, 0.06, 0.06));
        assert_eq!((cell.vertical_spacing, cell.horizontal_spacing), (0.18, 0.18));
        assert_eq!(cell.content_format.text_style, read.text_styles.get("STANDARD").unwrap().handle);
    }
}

#[test]
fn the_layouts_of_an_r12_file_list_the_viewports_and_the_model_layout_is_of_model_type() {
    let document = r12("BOXBLANK");
    let layouts: Vec<_> = document
        .objects
        .values()
        .filter_map(|object| match object {
            acadrust::objects::ObjectType::Layout(layout) => Some(layout),
            _ => None,
        })
        .collect();
    let model = layouts.iter().find(|layout| layout.name == "Model").unwrap();
    let paper = layouts.iter().find(|layout| layout.name == "Layout1").unwrap();
    assert!(model.plot_flags.model_type);
    assert!(!paper.plot_flags.model_type);
    assert_eq!(paper.viewports.len(), 1, "the paper-space viewport of the file is listed on its layout");
    assert!(matches!(document.get_entity(paper.viewports[0]), Some(EntityType::Viewport(_))));
    assert!(model.viewports.is_empty());
    // A document made in memory has the same Model layout.
    let fresh = CadDocument::new();
    assert!(fresh.objects.values().any(|object| matches!(object, acadrust::objects::ObjectType::Layout(layout) if layout.name == "Model" && layout.plot_flags.model_type)));
    // And the file written states it for the layout and the list for the re-read.
    let read = write_and_read(&document, DxfVersion::AC1032);
    assert!(read.objects.values().any(|object| matches!(object, acadrust::objects::ObjectType::Layout(layout) if layout.name == "Model" && layout.plot_flags.model_type)));
    assert!(read.objects.values().any(|object| matches!(object, acadrust::objects::ObjectType::Layout(layout) if layout.name == "Layout1" && layout.viewports.len() == 1)));
}

#[test]
fn a_dimension_style_that_names_its_arrowhead_states_the_blocks_handle_in_a_dwg_of_r2000_and_later() {
    let document = r12("BOXBLANK");
    let style = document.dim_styles.get("STD").expect("STD");
    assert_eq!(style.dimblk_name, "BOXBLANK");
    assert!(style.dimblk.is_null(), "an R12 style names the block and has no handle");
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1032] {
        let read = write_and_read(&document, version);
        let block = read.block_records.get("_BOXBLANK").expect("the arrowhead block").handle;
        let style = read.dim_styles.get("STD").expect("STD");
        assert_eq!(style.dimblk, block, "{version:?}: the arrowhead is the block `_BOXBLANK`");
        assert_eq!(style.dimblk_name, "", "{version:?}: a handle replaces the name");
    }
    // R14 states the name.
    assert_eq!(write_and_read(&document, DxfVersion::AC1014).dim_styles.get("STD").unwrap().dimblk_name, "BOXBLANK");
    // A name no block answers to states no handle.
    let unknown = r12("NOSUCH");
    assert!(write_and_read(&unknown, DxfVersion::AC1032).dim_styles.get("STD").unwrap().dimblk.is_null());
}

#[test]
fn two_unnamed_shape_file_text_styles_both_survive_a_dwg() {
    let document = r12("BOXBLANK");
    let shapes = |document: &CadDocument| {
        let mut shapes: Vec<(String, u64)> = document
            .text_styles
            .iter()
            .filter(|style| style.name.is_empty())
            .map(|style| (style.font_file.clone(), style.handle.value()))
            .collect();
        shapes.sort();
        shapes
    };
    let before = shapes(&document);
    assert_eq!(before.iter().map(|(file, _)| file.as_str()).collect::<Vec<_>>(), ["one.shx", "two.shx"]);
    assert!(before.iter().all(|(_, handle)| *handle != 0), "each record has a handle of its own: {before:?}");
    assert_ne!(before[0].1, before[1].1);
    for version in DWG_VERSIONS {
        let after = shapes(&write_and_read(&document, version));
        assert_eq!(after.iter().map(|(file, _)| file.as_str()).collect::<Vec<_>>(), ["one.shx", "two.shx"], "{version:?}");
    }
}

#[test]
fn the_scale_of_a_viewport_is_the_paper_height_over_the_view_height_in_every_reader() {
    let mut document = CadDocument::new();
    let mut viewport = Viewport::new();
    viewport.width = 10.0;
    viewport.height = 5.0;
    viewport.view_height = 20.0;
    document.add_entity(EntityType::Viewport(Box::new(viewport))).unwrap();
    let scale = |document: &CadDocument| {
        document
            .entities()
            .find_map(|entity| match entity {
                EntityType::Viewport(viewport) if viewport.width == 10.0 => Some(viewport.custom_scale),
                _ => None,
            })
            .expect("the viewport")
    };
    for version in [DxfVersion::AC1015, DxfVersion::AC1024, DxfVersion::AC1032] {
        assert_eq!(scale(&write_and_read(&document, version)), 0.25, "{version:?} DWG");
        assert_eq!(scale(&dxf_round_trip(&document, version)), 0.25, "{version:?} DXF");
    }
    // The R12 sample's viewport keeps its scale through a DWG of R2000 and later (it read 1 before).
    let r12 = r12("BOXBLANK");
    let own = r12.entities().find_map(|entity| match entity {
        EntityType::Viewport(viewport) => Some(viewport.custom_scale),
        _ => None,
    });
    assert_eq!(scale_of_first(&write_and_read(&r12, DxfVersion::AC1032)), own);
}

fn scale_of_first(document: &CadDocument) -> Option<f64> {
    document.entities().find_map(|entity| match entity {
        EntityType::Viewport(viewport) => Some(viewport.custom_scale),
        _ => None,
    })
}

#[test]
fn a_dwg_of_an_r12_file_declares_the_classes_its_records_need() {
    let document = r12("BOXBLANK");
    for version in DWG_VERSIONS {
        let read = write_and_read(&document, version);
        let mut names: Vec<_> = read.classes.iter().map(|class| class.dxf_name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["ACDBDICTIONARYWDFLT", "ACDBPLACEHOLDER", "LAYOUT", "MLEADERSTYLE", "TABLESTYLE"], "{version:?}");
        // AutoCAD's own definitions (sample_AC1014.dwg to sample_AC1032.dwg).
        let class = |name: &str| read.classes.get_by_name(name).unwrap();
        assert_eq!(class("TABLESTYLE").proxy_flags.0, 4095);
        assert_eq!(class("MLEADERSTYLE").proxy_flags.0, 4095);
        assert_eq!(class("MLEADERSTYLE").application_name, "ACDB_MLEADERSTYLE_CLASS");
        assert_eq!(class("LAYOUT").proxy_flags.0, 0);
    }
    // A document made in memory keeps the writer's whole list of classes.
    let fresh = write_and_read(&CadDocument::new(), DxfVersion::AC1032);
    assert!(fresh.classes.len() > 100, "{} classes", fresh.classes.len());
    assert_eq!(fresh.classes.get_by_name("TABLESTYLE").unwrap().proxy_flags.0, 4095);
}

/// The TABLESTYLE record states the style's description (ACadSharp's writer: `Description TV 3`); the name is the key of the
/// style's ACAD_TABLESTYLE entry. The default Standard style of an R12 file has no description and keeps none in a DWG or a DXF,
/// and a description of its own is written instead of the name.
#[test]
fn a_table_style_states_its_description_and_is_named_by_its_dictionary_entry() {
    let description = |document: &CadDocument| {
        let style = standard_table_style(document);
        (style.name.clone(), style.description.clone())
    };
    let document = r12("BOXBLANK");
    assert_eq!(description(&document), ("Standard".to_string(), String::new()));
    for version in DWG_VERSIONS {
        assert_eq!(description(&write_and_read(&document, version)), ("Standard".to_string(), String::new()), "{version:?} DWG");
    }
    let mut described = document.clone();
    for object in described.objects.values_mut() {
        if let acadrust::objects::ObjectType::TableStyle(style) = object {
            style.description = "Plain grid".to_string();
        }
    }
    for version in DWG_VERSIONS {
        assert_eq!(description(&write_and_read(&described, version)), ("Standard".to_string(), "Plain grid".to_string()), "{version:?} DWG");
    }
    for version in [DxfVersion::AC1015, DxfVersion::AC1032] {
        assert_eq!(description(&dxf_round_trip(&described, version)), ("Standard".to_string(), "Plain grid".to_string()), "{version:?} DXF");
    }
}

/// The class table of an R12 document is made when it is written: an object added after the read whose record needs a class gets
/// it (the classes are those of the finished document, not of the file read).
#[test]
fn an_entity_added_to_an_r12_document_has_its_class_in_the_dwg() {
    let mut document = r12("BOXBLANK");
    document.add_entity(EntityType::MultiLeader(Box::new(acadrust::entities::MultiLeader::new()))).unwrap();
    for version in [DxfVersion::AC1018, DxfVersion::AC1032] {
        let read = write_and_read(&document, version);
        assert!(read.classes.get_by_name("MULTILEADER").is_some(), "{version:?}: the class of the added entity is declared");
        assert!(read.entities().any(|entity| matches!(entity, EntityType::MultiLeader(_))), "{version:?}: the entity is written");
    }
}
