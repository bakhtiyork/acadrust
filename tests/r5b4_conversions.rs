// SPDX-License-Identifier: MPL-2.0
//! R5b-4 (gate G5): conversion defects found by comparing the fork's down- and up-saves with AutoCAD's own files of one drawing
//! (sample_AC1014.dwg to sample_AC1032.dwg, their C# dumps under Tests/CadCoreTests/Fixtures/dumps). Synthetic documents only;
//! each test fails with its fix reverted.

use std::io::Cursor;

use acadrust::entities::{EntityType, Spline, Table, ViewportStatusFlags};
use acadrust::objects::ObjectType;
use acadrust::types::{DxfVersion, Vector3};
use acadrust::{CadDocument, DwgReader, DwgWriter};

fn write_and_read(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    let bytes = DwgWriter::write_to_vec(&document).unwrap_or_else(|error| panic!("{version:?}: DWG write failed: {error}"));
    DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .unwrap_or_else(|error| panic!("{version:?}: DWG read failed: {error}"))
}

/// The document as a DWG of `version` that was read back, so that it has a DWG source of that version.
fn source_at(document: CadDocument, version: DxfVersion) -> CadDocument {
    write_and_read(&document, version)
}

/// The channels the maps of a material use, as its R2010 record states them: one bit per map that is the image file (diffuse 1,
/// specular 2, reflection 4, opacity 8, bump 16), 63 when all five are; AutoCAD's own R2010 file of the drawing (sample_AC1024.dwg)
/// states 63, 17, 1 and 0 for its built-in, diffuse-and-bump, diffuse-only and map-less materials, where its R2004 file states none.
#[test]
fn a_material_of_an_older_dwg_states_the_channels_of_its_maps_in_r2010() {
    let mut document = CadDocument::with_version(DxfVersion::AC1018);
    let root = document.header.named_objects_dict_handle;
    let cases: [(&str, [u8; 5], i32); 4] = [
        ("ALL", [1, 1, 1, 1, 1], 63),
        ("DIFFUSE_BUMP", [1, 0, 0, 0, 1], 17),
        ("DIFFUSE", [1, 0, 0, 0, 0], 1),
        ("NONE", [0, 0, 0, 0, 0], 0),
    ];
    for (name, sources, _) in cases {
        let mut material = acadrust::objects::Material::new();
        material.name = name.into();
        material.handle = document.allocate_handle();
        material.owner = root;
        material.diffuse_map.source = sources[0];
        material.specular_map.source = sources[1];
        material.reflection_map.source = sources[2];
        material.opacity_map.source = sources[3];
        material.bump_map.source = sources[4];
        let handle = material.handle;
        document.objects.insert(handle, ObjectType::Material(material));
        if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&root) {
            entries.add_entry(&format!("M_{name}"), handle);
        }
    }
    let source = source_at(document, DxfVersion::AC1018);
    let converted = write_and_read(&source, DxfVersion::AC1024);
    let flags_of = |document: &CadDocument, name: &str| {
        document.objects.values().find_map(|o| match o {
            ObjectType::Material(m) if m.name == name => Some(m.channel_flags),
            _ => None,
        })
    };
    for (name, _, flags) in cases {
        assert_eq!(flags_of(&converted, name), Some(flags), "{name}");
    }
    // A value an R2007+ record states is kept, also where it differs from the maps (review of R5b-4)
    let mut stated = CadDocument::with_version(DxfVersion::AC1024);
    let mut material = acadrust::objects::Material::new();
    material.name = "STATED".into();
    material.handle = stated.allocate_handle();
    material.owner = stated.header.named_objects_dict_handle;
    material.channel_flags = 5;
    let handle = material.handle;
    stated.objects.insert(handle, ObjectType::Material(material));
    let root = stated.header.named_objects_dict_handle;
    if let Some(ObjectType::Dictionary(entries)) = stated.objects.get_mut(&root) {
        entries.add_entry("M_STATED", handle);
    }
    let stated = source_at(stated, DxfVersion::AC1024);
    assert_eq!(flags_of(&write_and_read(&stated, DxfVersion::AC1032), "STATED"), Some(5));
}

/// AutoCAD's own R14, R2000, R2004 and R2010 files keep a visual style's properties beyond the layout in an ACAD_XREC_ROUNDTRIP
/// record of its extension dictionary (252 entries before R2010, 126 in R2010: sample_AC1014.dwg ... sample_AC1024.dwg); the R2013 file has none.
#[test]
fn a_visual_style_keeps_its_round_trip_record_at_every_version_before_r2013() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let root = document.header.named_objects_dict_handle;
    let mut style = acadrust::objects::VisualStyle::new();
    style.handle = document.allocate_handle();
    style.owner = root;
    let handle = style.handle;
    document.objects.insert(handle, ObjectType::VisualStyle(style));
    if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&root) {
        entries.add_entry("MYSTYLE", handle);
    }
    let source = source_at(document, DxfVersion::AC1032);
    let record_of = |document: &CadDocument| {
        // the record as the file held it: a reader takes the record it applied out of the model (R5b-10)
        let document = &document.with_round_trip_records();
        let style = document.objects.values().find_map(|o| match o {
            ObjectType::VisualStyle(style) => Some(style.clone()),
            _ => None,
        });
        let dictionary = style.and_then(|style| style.xdictionary_handle).and_then(|h| document.objects.get(&h));
        match dictionary {
            Some(ObjectType::Dictionary(entries)) => entries.get("ACAD_XREC_ROUNDTRIP").is_some(),
            _ => false,
        }
    };
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018] {
        assert!(record_of(&write_and_read(&source, version)), "{version:?}");
    }
    assert!(!record_of(&write_and_read(&source, DxfVersion::AC1027)), "AC1027: the R2013 layout states all properties");
    // An R2004 source (with the record) written to R2010 keeps it.
    let r2004 = write_and_read(&source, DxfVersion::AC1018);
    assert!(record_of(&write_and_read(&r2004, DxfVersion::AC1024)), "AC1018 -> AC1024");
}

/// AutoCAD's own R2013 file states "custom" (15) as the knot parameterization of the control-point splines of a drawing it
/// converts from R2010 or earlier (sample_AC1024.dwg to sample_AC1027.dwg).
#[test]
fn a_control_point_spline_of_an_older_dwg_states_custom_knot_parameterization_in_r2013() {
    let mut document = CadDocument::with_version(DxfVersion::AC1024);
    let mut spline = Spline::new();
    spline.degree = 3;
    spline.control_points = vec![Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 2.0, 0.0), Vector3::new(2.0, 2.0, 0.0), Vector3::new(3.0, 0.0, 0.0)];
    spline.knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    document.add_entity(EntityType::Spline(spline)).unwrap();
    let source = source_at(document, DxfVersion::AC1024);
    let converted = write_and_read(&source, DxfVersion::AC1027);
    let found = converted.entities().find_map(|e| match e {
        EntityType::Spline(spline) => Some(spline.knot_parameterization),
        _ => None,
    });
    assert_eq!(found, Some(15));
}

/// An R14 DIMSTYLE states DIMFIT; the record of an R2000 or later DWG states DIMATFIT instead (ACadSharp's DimensionFit of such
/// a file is the DIMATFIT), and AutoCAD's own R14 file of a drawing from them states its value (3 in sample_AC1014.dwg).
#[test]
fn an_r14_dimension_style_of_a_newer_drawing_states_dimfit_from_dimatfit() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    for style in document.dim_styles.iter_mut() {
        style.dimatfit = 2;
    }
    let source = source_at(document, DxfVersion::AC1032);
    let converted = write_and_read(&source, DxfVersion::AC1014);
    let fits: Vec<i16> = converted.dim_styles.iter().map(|style| style.dimfit).collect();
    assert!(!fits.is_empty());
    assert!(fits.iter().all(|fit| *fit == 2), "{fits:?}");
}

/// The R14 header states OSMODE, which an R2000 or later header does not (it reads as 0): AutoCAD's default 4133 goes into the
/// R14 header of a drawing converted down (ACadSharp reads 4133 from every later version's file, sample_AC1015.dwg).
#[test]
fn an_r14_header_of_a_newer_drawing_states_the_default_snap_mode() {
    let source = source_at(CadDocument::with_version(DxfVersion::AC1032), DxfVersion::AC1032);
    assert_eq!(source.header.object_snap_mode, 0);
    let converted = write_and_read(&source, DxfVersion::AC1014);
    assert_eq!(converted.header.object_snap_mode, 4133);
}

/// The MVIEW data of an R14 viewport states fast zoom (always 1 since R2000) and the UCS icon from the status word of an R2000+
/// viewport (sample_AC1014.dwg from sample_AC1015.dwg: 1 for the fast zoom, 3 for an icon at the origin).
#[test]
fn an_r14_viewport_of_a_newer_drawing_states_fast_zoom_and_the_status_icon() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut viewport = acadrust::entities::Viewport::new();
    viewport.id = 2;
    viewport.width = 160.0;
    viewport.height = 100.0;
    viewport.status = ViewportStatusFlags::from_bits(0x8000 | 0x20 | 0x40);
    viewport.ucs_icon_visible = false;
    viewport.ucs_at_origin = false;
    document.add_entity_to_layout(EntityType::Viewport(Box::new(viewport)), "Layout1").unwrap();
    let source = source_at(document, DxfVersion::AC1032);
    let converted = write_and_read(&source, DxfVersion::AC1014);
    let viewport = converted
        .entities()
        .find_map(|e| match e {
            EntityType::Viewport(v) if v.width == 160.0 => Some(v.clone()),
            _ => None,
        })
        .expect("viewport");
    assert!(viewport.status.fast_zoom, "fast zoom");
    assert!(viewport.ucs_icon_visible, "icon visible");
    assert!(viewport.ucs_at_origin, "icon at the origin");
}

/// The classes of a DWG source keep the zombie flag their file stated: none of AutoCAD's own classes is a zombie in any of its six
/// files of the drawing, including the classes without a record (sample_AC1014.dwg ... sample_AC1032.dwg).
#[test]
fn the_classes_of_a_dwg_source_are_not_made_zombies_by_a_conversion() {
    let mut source = source_at(CadDocument::with_version(DxfVersion::AC1032), DxfVersion::AC1032);
    // The drawing as AutoCAD's own file states its classes: none a zombie (this writer flags the unused ones of a new document).
    let names: Vec<String> = source.classes.iter().map(|class| class.dxf_name.clone()).collect();
    for name in &names {
        source.classes.get_mut_by_name(name).unwrap().was_zombie = false;
    }
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024] {
        let converted = write_and_read(&source, version);
        let zombies: Vec<_> = converted.classes.iter().filter(|class| class.was_zombie).map(|class| class.dxf_name.clone()).collect();
        assert!(zombies.is_empty(), "{version:?}: {zombies:?}");
    }
}

fn table_document() -> CadDocument {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    let mut table = Table::new(Vector3::ZERO, 1, 1);
    table.set_cell_text(0, 0, "Text");
    {
        let cell = table.cell_mut(0, 0).unwrap();
        // The content overrides the text height and the colour: AutoCAD's legacy cell states them as overrides (0x20, 0x08).
        let content = &mut cell.contents[0];
        content.text_height = 4.0;
        content.format_override_flags |= 128 | 32;
        content.color = acadrust::types::Color::from_index(5);
    }
    document.add_entity(EntityType::Table(Box::new(table))).unwrap();
    document
}

/// A cell of the R2010 layout written before R2010 states what its content format overrides in the legacy cell's override word
/// (sample_AC1018.dwg from sample_AC1032.dwg: text height 4 in the cells whose content overrides it).
#[test]
fn a_modern_table_cell_written_before_r2010_states_its_content_overrides() {
    let source = source_at(table_document(), DxfVersion::AC1032);
    for version in [DxfVersion::AC1015, DxfVersion::AC1018] {
        let converted = write_and_read(&source, version);
        let style = converted
            .entities()
            .find_map(|e| match e {
                EntityType::Table(table) => table.rows[0].cells[0].style.clone(),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{version:?}: the cell has a style"));
        assert_eq!(style.text_height, 4.0, "{version:?}");
        assert_eq!(style.content_color, acadrust::types::Color::from_index(5), "{version:?}");
        assert_eq!(style.override_flags & 0x28, 0x28, "{version:?}: text height and content colour overrides");
    }
}

/// A legacy cell states a text height equal to the height of its row where the user set none (the legacy table style's row
/// heights); AutoCAD's own R2010 file (sample_AC1024.dwg from sample_AC1018.dwg) keeps the style's text height there and overrides
/// it only for another value, and states the text of a text item as its formatted value.
#[test]
fn a_legacy_cell_in_the_r2010_layout_overrides_only_a_real_text_height() {
    use acadrust::entities::{CellContent, CellStyle, TableCell};
    let build = |height: f64| {
        let mut cell = TableCell::new();
        let mut style = CellStyle::new();
        style.legacy_override_bits = true;
        style.override_flags = 0x20;
        style.property_flags = acadrust::entities::CellStylePropertyFlags::TEXT_HEIGHT;
        style.text_height = height;
        cell.style = Some(style);
        cell.contents = vec![CellContent::text("Table")];
        cell.contents[0].value.formatted_value.clear();
        cell
    };
    let row_heights = [0.14249252057197062, 0.10259461481181885];
    let derived = build(0.10259461481181885);
    let laid = derived.binary_layout_with(&row_heights);
    assert_eq!(laid.contents[0].format_override_flags & 128, 0, "the row height is no text height override");
    assert_eq!(laid.contents[0].value.formatted_value, "Table", "the text is the formatted value");
    let explicit = build(4.0);
    let laid = explicit.binary_layout_with(&row_heights);
    assert_eq!(laid.contents[0].format_override_flags & 128, 128, "another value is an override");
    assert_eq!(laid.contents[0].text_height, 4.0);
}

/// AutoCAD's own R2010 file states no content for the cells a merge of one row covers (sample_AC1024.dwg: the title row); a legacy
/// cell states an empty text there.
#[test]
fn the_cells_a_merge_covers_state_no_content_in_the_r2010_layout() {
    let mut document = CadDocument::with_version(DxfVersion::AC1018);
    let mut table = Table::new(Vector3::ZERO, 1, 3);
    table.set_cell_text(0, 0, "Title");
    // a legacy cell states an empty text for the cells the merge covers
    for col in 1..3 {
        table.cell_mut(0, col).unwrap().contents = vec![acadrust::entities::CellContent::text("")];
    }
    table.cell_mut(0, 0).unwrap().merge_width = 3;
    document.add_entity(EntityType::Table(Box::new(table))).unwrap();
    let source = source_at(document, DxfVersion::AC1018);
    let converted = write_and_read(&source, DxfVersion::AC1024);
    let table = converted
        .entities()
        .find_map(|e| match e {
            EntityType::Table(table) => Some(table.clone()),
            _ => None,
        })
        .expect("table");
    assert!(!table.rows[0].cells[0].contents.is_empty());
    assert!(table.rows[0].cells[1].contents.is_empty(), "covered cell 1");
    assert!(table.rows[0].cells[2].contents.is_empty(), "covered cell 2");
}
