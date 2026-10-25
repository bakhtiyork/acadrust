// SPDX-License-Identifier: MPL-2.0
//! R5b-9 (gate G5-RR, cross-version cells): the `ACAD_XREC_ROUNDTRIP` records AutoCAD keeps for the values an older version
//! cannot hold. Synthetic documents only; each test fails with its fix reverted. Evidence: AutoCAD's own files of one drawing
//! (sample_AC1014.dwg to sample_AC1032.dwg and their DXF twins; the native dumps of the fork).

use std::io::Cursor;

use acadrust::objects::{ClassObject, ClassObjectData, DictionaryCloningFlags, Layout, Material, ObjectType, SectionViewStyle, XRecord};
use acadrust::types::{DxfVersion, Handle};
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

/// The round-trip record the file held in the extension dictionary of `owner`. The reader takes a record it applied out of the
/// model (`with_round_trip_records` puts it back as the file held it, as the ACadSharp view and the writers do): the model has none.
fn record_of(document: &CadDocument, owner: Handle) -> Option<XRecord> {
    let record = document.with_round_trip_records().xrecord(owner, "ACAD_XREC_ROUNDTRIP").cloned();
    if record.is_some() {
        assert!(document.xrecord(owner, "ACAD_XREC_ROUNDTRIP").is_none(), "the reader took the record out of the model");
    }
    record
}

/// The group-102 names of a record, in order.
fn names(record: &XRecord) -> Vec<&str> {
    record.entries.iter().filter(|entry| entry.code == 102).filter_map(|entry| entry.value.as_string()).collect()
}

fn xrecord_count(document: &CadDocument) -> usize {
    document.objects.values().filter(|object| matches!(object, ObjectType::XRecord(_))).count()
}

// ---- view style names -----------------------------------------------------------------------------------------------

fn document_with_section_style() -> (CadDocument, Handle) {
    let mut document = CadDocument::new();
    let mut style = SectionViewStyle::default();
    style.base.description = "Imperial24".into();
    style.base.display_name = "Imperial24".into();
    style.base.flags = 5;
    let mut object = ClassObject::new(ClassObjectData::SectionViewStyle(style));
    object.handle = document.allocate_handle();
    object.owner = document.header.named_objects_dict_handle;
    let handle = object.handle;
    document.objects.insert(handle, ObjectType::ClassObject(object));
    (document, handle)
}

fn section_base(document: &CadDocument) -> (Handle, acadrust::objects::ModelDocViewStyle) {
    document
        .objects
        .iter()
        .find_map(|(handle, object)| match object {
            ObjectType::ClassObject(ClassObject { data: ClassObjectData::SectionViewStyle(style), .. }) => Some((*handle, style.base.clone())),
            _ => None,
        })
        .expect("the section view style")
}

/// AutoCAD's files before R2018 state the display name and the flags of a view style in `ACAD_XREC_ROUNDTRIP {DISPLAYNAME,
/// FLAGS}` of the style's extension dictionary (sample_AC1014.dwg to sample_AC1027.dwg and the DXF twins of R2000 to R2013:
/// 4 styles, `1 <name>`, `90 <flags>`); the object records state them from R2018 only. Before R5b-9 the name was lost.
#[test]
fn a_view_style_keeps_its_display_name_and_flags_before_r2018() {
    let (source, _) = document_with_section_style();
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024, DxfVersion::AC1027] {
        let out = write_and_read(&source, version);
        let (handle, base) = section_base(&out);
        assert_eq!(base.display_name, "Imperial24", "{version:?}");
        assert_eq!(base.flags, 5, "{version:?}");
        let record = record_of(&out, handle).unwrap_or_else(|| panic!("{version:?}: no record"));
        assert_eq!(names(&record), ["DISPLAYNAME", "FLAGS"], "{version:?}");
        // AutoCAD's records of R2000 and later keep existing objects when cloned (sample_AC1015.dwg); R14 states no flags
        let flags = if version >= DxfVersion::AC1015 { DictionaryCloningFlags::KeepExisting } else { DictionaryCloningFlags::NotApplicable };
        assert_eq!(record.cloning_flags, flags, "{version:?}");
        assert_eq!((record.entries[0].code, record.entries[1].value.as_string()), (102, Some("Imperial24")));
        assert_eq!(record.entries[2].code, 102);
        assert_eq!(record.entries[3].code, 90);
    }
    // R2018 states both in the object itself and has no record
    let out = write_and_read(&source, DxfVersion::AC1032);
    let (handle, base) = section_base(&out);
    assert_eq!((base.display_name.as_str(), base.flags), ("Imperial24", 5));
    assert!(record_of(&out, handle).is_none());
}

#[test]
fn a_view_style_record_is_edited_and_dropped_with_the_version() {
    let (source, _) = document_with_section_style();
    let mut down = write_and_read(&source, DxfVersion::AC1015);
    let (handle, _) = section_base(&down);
    // an edit after the read wins over the record the file had
    if let Some(ObjectType::ClassObject(ClassObject { data: ClassObjectData::SectionViewStyle(style), .. })) = down.objects.get_mut(&handle) {
        style.base.display_name = "Edited".into();
        style.base.flags = 9;
    }
    let again = write_and_read(&down, DxfVersion::AC1015);
    let (_, base) = section_base(&again);
    assert_eq!((base.display_name.as_str(), base.flags), ("Edited", 9));
    // an up-save drops the record it applied (AutoCAD's R2018 file has none) and keeps the values
    let up = write_and_read(&again, DxfVersion::AC1032);
    let (handle, base) = section_base(&up);
    assert_eq!((base.display_name.as_str(), base.flags), ("Edited", 9));
    assert!(record_of(&up, handle).is_none());
    assert!(xrecord_count(&up) <= xrecord_count(&again));
}

#[test]
fn a_dxf_of_r2000_to_r2013_holds_the_view_style_record_too() {
    let (source, _) = document_with_section_style();
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024, DxfVersion::AC1027] {
        let out = dxf_write_and_read(&source, version);
        let (handle, base) = section_base(&out);
        assert_eq!((base.display_name.as_str(), base.flags), ("Imperial24", 5), "{version:?}");
        assert!(record_of(&out, handle).is_some(), "{version:?}");
    }
    let out = dxf_write_and_read(&source, DxfVersion::AC1032);
    let (handle, base) = section_base(&out);
    assert_eq!(base.display_name, "Imperial24");
    assert!(record_of(&out, handle).is_none());
}

// ---- material ---------------------------------------------------------------------------------------------------------

fn named_material(document: &CadDocument, name: &str) -> (Handle, Material) {
    document
        .objects
        .iter()
        .find_map(|(handle, object)| match object {
            ObjectType::Material(material) if material.name == name => Some((*handle, material.clone())),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no material {name}"))
}

fn document_with_material() -> (CadDocument, Handle) {
    let mut document = CadDocument::new();
    let mut material = Material::new();
    material.name = "Brick".into();
    material.translucence = 0.5;
    material.self_illumination = 0.25;
    material.reflectivity = 0.15;
    material.illumination_model = 1;
    material.channel_flags = 17;
    material.mode = 0;
    material.diffuse_map.source = 1;
    material.bump_map.source = 1;
    material.specular_map.source = 0;
    material.handle = document.allocate_handle();
    material.owner = document.header.acad_material_dict_handle;
    let handle = material.handle;
    if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&document.header.acad_material_dict_handle) {
        entries.add_entry("Brick", handle);
    }
    document.objects.insert(handle, ObjectType::Material(material));
    (document, handle)
}

/// A drawing before R2007 has no reflectivity, translucence, self-illumination, illumination model, channel flags or mode in a
/// material; AutoCAD's R14, R2000 and R2004 files state them in `ACAD_XREC_ROUNDTRIP {MATERIAL 148 .. 149 .. 149 .. 93 .. 94
/// .. 282 .. 72 .. 77 .. 171 .. 175 .. 179 .. 273 ..}` (11 materials of sample_AC1014.dwg, sample_AC1015.dwg and
/// sample_AC1018.dwg; the DXF twins of R2000 and R2004).
#[test]
fn a_material_keeps_its_late_values_before_r2007() {
    let (source, _) = document_with_material();
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018] {
        let out = write_and_read(&source, version);
        let (handle, material) = named_material(&out, "Brick");
        assert_eq!(
            (material.translucence, material.self_illumination, material.reflectivity, material.illumination_model, material.channel_flags),
            (0.5, 0.25, 0.15, 1, 17),
            "{version:?}"
        );
        let record = record_of(&out, handle).unwrap_or_else(|| panic!("{version:?}: no record"));
        assert_eq!(names(&record), ["MATERIAL"]);
        let codes: Vec<i32> = record.entries.iter().skip(1).map(|entry| entry.code).collect();
        assert_eq!(codes, [148, 149, 149, 93, 94, 282, 72, 77, 171, 175, 179, 273]);
    }
    let out = write_and_read(&source, DxfVersion::AC1021);
    let (handle, material) = named_material(&out, "Brick");
    assert_eq!((material.reflectivity, material.channel_flags), (0.15, 17));
    assert!(record_of(&out, handle).is_none());
}

#[test]
fn a_material_record_is_edited_and_dropped_with_the_version() {
    let (source, _) = document_with_material();
    let mut down = write_and_read(&source, DxfVersion::AC1018);
    let (handle, _) = named_material(&down, "Brick");
    if let Some(ObjectType::Material(material)) = down.objects.get_mut(&handle) {
        material.reflectivity = 0.75;
        material.channel_flags = 3;
    }
    let again = write_and_read(&down, DxfVersion::AC1018);
    let (_, material) = named_material(&again, "Brick");
    assert_eq!((material.reflectivity, material.channel_flags), (0.75, 3));
    let up = write_and_read(&again, DxfVersion::AC1032);
    let (handle, material) = named_material(&up, "Brick");
    assert_eq!((material.reflectivity, material.channel_flags), (0.75, 3));
    assert!(record_of(&up, handle).is_none());
}

#[test]
fn a_dxf_of_r2000_and_r2004_holds_the_material_record_too() {
    let (source, _) = document_with_material();
    for version in [DxfVersion::AC1015, DxfVersion::AC1018] {
        let out = dxf_write_and_read(&source, version);
        let (handle, material) = named_material(&out, "Brick");
        assert_eq!((material.reflectivity, material.channel_flags, material.illumination_model), (0.15, 17, 1), "{version:?}");
        assert!(record_of(&out, handle).is_some(), "{version:?}");
    }
    let out = dxf_write_and_read(&source, DxfVersion::AC1021);
    let (handle, material) = named_material(&out, "Brick");
    assert_eq!(material.reflectivity, 0.15);
    assert!(record_of(&out, handle).is_none());
}

// ---- layout -----------------------------------------------------------------------------------------------------------

fn named_layout(document: &CadDocument, name: &str) -> (Handle, Layout) {
    document
        .objects
        .iter()
        .find_map(|(handle, object)| match object {
            ObjectType::Layout(layout) if layout.name == name => Some((*handle, layout.clone())),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no layout {name}"))
}

fn document_with_shade_plot() -> CadDocument {
    let mut document = CadDocument::new();
    let handle = named_layout(&document, "Model").0;
    if let Some(ObjectType::Layout(layout)) = document.objects.get_mut(&handle) {
        layout.shade_plot_mode = 1;
        layout.shade_plot_resolution = 3;
        layout.shade_plot_dpi = 600;
    }
    document
}

/// A layout of R14 and R2000 has no shade plot mode, resolution level or custom resolution; AutoCAD's files of those versions
/// state them in `ACAD_XREC_ROUNDTRIP {SHADEPLOT 70 0, SHADEPLOTRESLEVEL 70 2, SHADEPLOTCUSTOMDPI 70 300}` on each layout
/// (4 layouts of sample_AC1014.dwg and sample_AC1015.dwg; the DXF twin of R2000). R2004 holds them in the layout.
#[test]
fn a_layout_keeps_its_shade_plot_values_before_r2004() {
    let source = document_with_shade_plot();
    for version in [DxfVersion::AC1014, DxfVersion::AC1015] {
        let out = write_and_read(&source, version);
        let (handle, layout) = named_layout(&out, "Model");
        assert_eq!((layout.shade_plot_mode, layout.shade_plot_resolution, layout.shade_plot_dpi), (1, 3, 600), "{version:?}");
        let record = record_of(&out, handle).unwrap_or_else(|| panic!("{version:?}: no record"));
        assert_eq!(names(&record), ["SHADEPLOT", "SHADEPLOTRESLEVEL", "SHADEPLOTCUSTOMDPI"]);
        // a layout made in memory with the values a file without the record gives loses nothing: it gets none (a layout converted
        // from a file of another version gets one, as AutoCAD's down-saves state one for each layout)
        let (paper, layout) = named_layout(&out, "Layout1");
        assert_eq!((layout.shade_plot_mode, layout.shade_plot_resolution, layout.shade_plot_dpi), (0, 0, 300), "{version:?}");
        assert!(record_of(&out, paper).is_none(), "{version:?}: a default layout made in memory has no record");
    }
    let out = write_and_read(&source, DxfVersion::AC1018);
    let (handle, layout) = named_layout(&out, "Model");
    assert_eq!((layout.shade_plot_mode, layout.shade_plot_resolution, layout.shade_plot_dpi), (1, 3, 600));
    assert!(record_of(&out, handle).is_none());
}

#[test]
fn a_layout_record_is_edited_and_dropped_with_the_version() {
    let mut down = write_and_read(&document_with_shade_plot(), DxfVersion::AC1015);
    let (handle, _) = named_layout(&down, "Model");
    if let Some(ObjectType::Layout(layout)) = down.objects.get_mut(&handle) {
        layout.shade_plot_resolution = 5;
        layout.shade_plot_dpi = 1200;
    }
    let again = write_and_read(&down, DxfVersion::AC1015);
    let (_, layout) = named_layout(&again, "Model");
    assert_eq!((layout.shade_plot_mode, layout.shade_plot_resolution, layout.shade_plot_dpi), (1, 5, 1200));
    let up = write_and_read(&again, DxfVersion::AC1032);
    let (handle, layout) = named_layout(&up, "Model");
    assert_eq!((layout.shade_plot_mode, layout.shade_plot_resolution, layout.shade_plot_dpi), (1, 5, 1200));
    assert!(record_of(&up, handle).is_none());
}

#[test]
fn a_dxf_of_r2000_holds_the_layout_record_and_one_of_r2004_does_not() {
    let source = document_with_shade_plot();
    let out = dxf_write_and_read(&source, DxfVersion::AC1015);
    let (handle, layout) = named_layout(&out, "Model");
    assert_eq!((layout.shade_plot_mode, layout.shade_plot_resolution, layout.shade_plot_dpi), (1, 3, 600));
    assert!(record_of(&out, handle).is_some());
    let out = dxf_write_and_read(&source, DxfVersion::AC1018);
    let (handle, _) = named_layout(&out, "Model");
    assert!(record_of(&out, handle).is_none());
}

// ---- view, VPORT and VIEWPORT display settings -----------------------------------------------------------------------

/// The handle of a visual style of the document, which a new document does not hold: one is made.
fn a_visual_style(document: &mut CadDocument) -> Handle {
    let styles = match document.objects.get(&document.header.named_objects_dict_handle) {
        Some(ObjectType::Dictionary(root)) => root.get("ACAD_VISUALSTYLE").expect("the visual style dictionary"),
        _ => panic!("no root dictionary"),
    };
    let mut style = acadrust::objects::VisualStyle::new();
    style.handle = document.allocate_handle();
    style.owner = styles;
    let handle = style.handle;
    document.objects.insert(handle, ObjectType::VisualStyle(style));
    if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&styles) {
        entries.add_entry("MYSTYLE", handle);
    }
    handle
}

fn document_with_display() -> (CadDocument, Handle) {
    let mut document = CadDocument::new();
    let style = a_visual_style(&mut document);
    // a named view
    let mut view = acadrust::tables::View::new("V1");
    view.camera_plottable = true;
    view.visual_style_handle = style;
    document.views.add(view).unwrap();
    // the active VPORT
    let vport = document.vports.iter_mut().next().expect("the active VPORT");
    vport.visual_style_handle = style;
    vport.grid_flags = acadrust::entities::GridFlags::from_bits(7);
    vport.grid_major = 3;
    vport.brightness = 0.25;
    vport.contrast = 0.75;
    vport.default_lighting_type = 2;
    // a viewport on the first layout
    let mut viewport = acadrust::entities::Viewport::new();
    viewport.visual_style_handle = style;
    viewport.grid_flags = acadrust::entities::GridFlags::from_bits(3);
    viewport.grid_major = 4;
    viewport.brightness = 0.5;
    viewport.contrast = 0.125;
    viewport.shade_plot_mode = 2;
    let handle = document
        .add_entity_to_layout(acadrust::entities::EntityType::Viewport(Box::new(viewport)), "Layout1")
        .expect("a viewport on Layout1");
    (document, handle)
}

fn the_viewport(document: &CadDocument) -> acadrust::entities::Viewport {
    document
        .entities()
        .filter_map(|entity| match entity {
            acadrust::entities::EntityType::Viewport(viewport) if viewport.grid_major == 4 => Some((**viewport).clone()),
            _ => None,
        })
        .next()
        .expect("the viewport")
}

/// AutoCAD's R14, R2000 and R2004 files state the display settings of a viewport entity in `ACAD_XREC_ROUNDTRIP {VISUALSTYLE 340,
/// GRIDDISPLAY 70, GRIDMAJOR 70, DEFAULTLIGHTING 280, DEFAULTLIGHTINGTYPE 70, BRIGHTNESS 141, CONTRAST 142}` (6 viewports of
/// sample_AC1014.dwg, sample_AC1015.dwg and sample_AC1018.dwg) and, before R2004, `SHADEPLOT 70`; the view in `{CAMERAPLOTTABLE
/// 70, VISUALSTYLE 340}`; the VPORT table's control object in the same groups with the prefix `VTR_<extents>_`.
#[test]
fn a_viewport_a_view_and_a_vport_keep_their_display_settings_before_r2007() {
    let (source, _) = document_with_display();
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018] {
        let out = write_and_read(&source, version);
        let viewport = the_viewport(&out);
        assert_eq!((viewport.grid_flags.to_bits(), viewport.grid_major, viewport.brightness, viewport.contrast), (3, 4, 0.5, 0.125), "{version:?}");
        assert_ne!(viewport.visual_style_handle, Handle::NULL, "{version:?}");
        // R2004 holds the shade plot mode in the entity; before it, the record does
        assert_eq!(viewport.shade_plot_mode, 2, "{version:?}: shade plot mode");
        let record = record_of(&out, viewport.common.handle).unwrap_or_else(|| panic!("{version:?}: no viewport record"));
        // AutoCAD states the shade plot mode first (sample_AC1014.dwg, sample_AC1015.dwg)
        let shade = names(&record).first() == Some(&"SHADEPLOT");
        assert_eq!(shade, version < DxfVersion::AC1018, "{version:?}: {:?}", names(&record));
        assert!(names(&record).contains(&"GRIDDISPLAY") && names(&record).contains(&"VISUALSTYLE"), "{version:?}");
        let view = out.views.iter().find(|view| view.name.eq_ignore_ascii_case("V1")).expect("the view");
        assert!(view.camera_plottable && view.visual_style_handle != Handle::NULL, "{version:?}");
        let record = record_of(&out, view.handle).unwrap_or_else(|| panic!("{version:?}: no view record"));
        assert!(names(&record).contains(&"CAMERAPLOTTABLE") && names(&record).contains(&"VISUALSTYLE"), "{version:?}");
        let vport = out.vports.iter().next().expect("the VPORT");
        assert_eq!((vport.grid_flags.to_bits(), vport.grid_major, vport.brightness, vport.contrast, vport.default_lighting_type), (7, 3, 0.25, 0.75, 2), "{version:?}");
        assert_ne!(vport.visual_style_handle, Handle::NULL, "{version:?}");
        let record = record_of(&out, out.vports.handle()).unwrap_or_else(|| panic!("{version:?}: no VPORT record"));
        assert!(names(&record).iter().any(|name| name.starts_with("VTR_") && name.ends_with("_GRIDDISPLAY")), "{version:?}: {:?}", names(&record));
    }
    // R2007 holds them in the objects and states no record
    let out = write_and_read(&source, DxfVersion::AC1021);
    let viewport = the_viewport(&out);
    assert_eq!((viewport.grid_flags.to_bits(), viewport.shade_plot_mode), (3, 2));
    assert!(record_of(&out, viewport.common.handle).is_none());
    assert!(record_of(&out, out.vports.handle()).is_none());
}

#[test]
fn display_settings_are_edited_and_dropped_with_the_version() {
    let (source, _) = document_with_display();
    let mut down = write_and_read(&source, DxfVersion::AC1018);
    let handle = the_viewport(&down).common.handle;
    if let Some(acadrust::entities::EntityType::Viewport(viewport)) = down.get_entity_mut(handle) {
        viewport.brightness = 0.875;
        viewport.grid_flags = acadrust::entities::GridFlags::from_bits(1);
    }
    if let Some(vport) = down.vports.iter_mut().next() {
        vport.contrast = 0.5;
    }
    let again = write_and_read(&down, DxfVersion::AC1018);
    let viewport = the_viewport(&again);
    assert_eq!((viewport.brightness, viewport.grid_flags.to_bits()), (0.875, 1));
    assert_eq!(again.vports.iter().next().expect("the VPORT").contrast, 0.5);
    let up = write_and_read(&again, DxfVersion::AC1032);
    let viewport = the_viewport(&up);
    assert_eq!((viewport.brightness, viewport.grid_flags.to_bits()), (0.875, 1));
    assert!(record_of(&up, viewport.common.handle).is_none());
    assert!(record_of(&up, up.vports.handle()).is_none());
    assert_eq!(up.vports.iter().next().expect("the VPORT").contrast, 0.5);
}

#[test]
fn a_dxf_of_r2000_holds_the_shade_plot_record_of_a_viewport_only() {
    let (source, _) = document_with_display();
    let out = dxf_write_and_read(&source, DxfVersion::AC1015);
    let viewport = the_viewport(&out);
    assert_eq!(viewport.shade_plot_mode, 2, "restored from the record");
    let record = record_of(&out, viewport.common.handle).expect("AutoCAD's DXF of R2000 holds it");
    assert_eq!(names(&record), ["SHADEPLOT"]);
    let view = out.views.iter().find(|view| view.name.eq_ignore_ascii_case("V1")).expect("the view");
    assert!(record_of(&out, view.handle).is_none(), "AutoCAD's DXF holds no view record");
    assert!(record_of(&out, out.vports.handle()).is_none());
}

// ---- solids -----------------------------------------------------------------------------------------------------------

/// AutoCAD's R14, R2000 and R2004 files keep the handle of a solid's construction history in `ACAD_XREC_ROUNDTRIP
/// {ACAD_SOLID_HIST 360 <history>, ACAD_SOLID_HIST1 90 0, ACAD_SOLID_HIST2 90 <the solid's handle>}` (2 solids of sample_AC1014.dwg,
/// sample_AC1015.dwg, sample_AC1018.dwg); R2007 states it in the object.
#[test]
fn a_solid_keeps_the_handle_of_its_history_before_r2007() {
    let mut document = CadDocument::new();
    let history = document.allocate_handle();
    let mut solid = acadrust::entities::Solid3D::new();
    solid.history_handle = Some(history);
    let handle = document.add_entity(acadrust::entities::EntityType::Solid3D(solid)).unwrap();
    let history_of = |document: &CadDocument| -> (Handle, Option<Handle>) {
        document
            .entities()
            .find_map(|entity| match entity {
                acadrust::entities::EntityType::Solid3D(solid) => Some((solid.common.handle, solid.history_handle)),
                _ => None,
            })
            .expect("the solid")
    };
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018] {
        let out = write_and_read(&document, version);
        let (solid, found) = history_of(&out);
        assert!(found.is_some_and(|found| found != Handle::NULL), "{version:?}");
        let record = record_of(&out, solid).unwrap_or_else(|| panic!("{version:?}: no record"));
        assert_eq!(names(&record), ["ACAD_SOLID_HIST", "ACAD_SOLID_HIST1", "ACAD_SOLID_HIST2"]);
        assert_eq!(record.entries[1].code, 360);
        assert_eq!(record.entries.last().map(|entry| entry.code), Some(90));
        let _ = handle;
    }
    let out = write_and_read(&document, DxfVersion::AC1021);
    let (solid, found) = history_of(&out);
    assert!(found.is_some());
    assert!(record_of(&out, solid).is_none());
}

// ---- splines -----------------------------------------------------------------------------------------------------------

fn fit_spline_document(closed: bool) -> CadDocument {
    let points = vec![
        acadrust::types::Vector3::new(0.0, 0.0, 0.0),
        acadrust::types::Vector3::new(5.0, 10.0, 0.0),
        acadrust::types::Vector3::new(10.0, 0.0, 0.0),
        acadrust::types::Vector3::new(5.0, -10.0, 0.0),
    ];
    let mut spline = acadrust::entities::Spline::from_fit_points(points);
    spline.flags.closed = closed;
    spline.knot_parameterization = 2;
    let mut document = CadDocument::new();
    document.add_entity(acadrust::entities::EntityType::Spline(spline)).unwrap();
    document
}

fn the_spline(document: &CadDocument) -> acadrust::entities::Spline {
    document
        .entities()
        .find_map(|entity| match entity {
            acadrust::entities::EntityType::Spline(spline) => Some(spline.clone()),
            _ => None,
        })
        .expect("the spline")
}

/// AutoCAD's files before R2013 keep a fit-point spline's knot parameterization in the extended data `ACAD_SPLINE_DATA 1070 100
/// 1070 0 1070 101 1070 0 1070 102 <knot parameterization>` (the open spline #434 of sample_AC1014.dwg to sample_AC1024.dwg: 0, 0,
/// 2). The meaning of the first two values is not known (the samples state 0 for an open, non-periodic spline only): a closed spline
/// gets no record, and its flag is lost before R2013 as the fit-point form of the older record has none (`tests/issue114.rs`).
#[test]
fn an_open_fit_point_spline_keeps_its_knot_parameterization_before_r2013() {
    let source = fit_spline_document(false);
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024] {
        let out = write_and_read(&source, version);
        let spline = the_spline(&out);
        assert_eq!((spline.knot_parameterization, spline.flags.closed, spline.fit_points.len()), (2, false, 4), "{version:?}");
        // the record is consumed by the reader (the model holds the value), so a second write states it again
        assert!(spline.common.extended_data.get_record("ACAD_SPLINE_DATA").is_none(), "{version:?}");
        let again = write_and_read(&out, version);
        assert_eq!(the_spline(&again).knot_parameterization, 2, "{version:?}");
    }
    let out = write_and_read(&source, DxfVersion::AC1027);
    assert_eq!(the_spline(&out).knot_parameterization, 2);
    // a closed spline: no record whose first values would be a guess
    let closed = fit_spline_document(true);
    for version in [DxfVersion::AC1015, DxfVersion::AC1024] {
        let out = write_and_read(&closed, version);
        assert_ne!(the_spline(&out).knot_parameterization, 2, "{version:?}: no record states it");
        assert!(!the_spline(&out).flags.closed, "{version:?}");
    }
}

#[test]
fn a_spline_record_is_edited_with_the_model_and_a_dxf_has_none() {
    let mut down = write_and_read(&fit_spline_document(false), DxfVersion::AC1018);
    let handle = the_spline(&down).common.handle;
    if let Some(acadrust::entities::EntityType::Spline(spline)) = down.get_entity_mut(handle) {
        spline.knot_parameterization = 1;
    }
    let again = write_and_read(&down, DxfVersion::AC1018);
    assert_eq!(the_spline(&again).knot_parameterization, 1);
    // closed after the read: the record goes (no guessed value is written)
    let mut closed = again.clone();
    if let Some(acadrust::entities::EntityType::Spline(spline)) = closed.get_entity_mut(handle) {
        spline.flags.closed = true;
    }
    assert_ne!(the_spline(&write_and_read(&closed, DxfVersion::AC1018)).knot_parameterization, 1);
    // AutoCAD's DXF files of R2000 to R2013 hold no ACAD_SPLINE_DATA
    let out = dxf_write_and_read(&fit_spline_document(false), DxfVersion::AC1018);
    assert!(the_spline(&out).common.extended_data.get_record("ACAD_SPLINE_DATA").is_none());
}

// ---- the dimension linetypes of the header ------------------------------------------------------------------------------

fn document_with_dimension_linetypes() -> CadDocument {
    let mut document = CadDocument::new();
    for name in ["DASHED", "DOT", "HIDDEN"] {
        let mut line_type = acadrust::tables::LineType::new(name);
        line_type.handle = document.allocate_handle();
        document.line_types.add(line_type).unwrap();
    }
    document.header.dim_linetype_name = "DASHED".into();
    document.header.dim_linetype1_name = "DOT".into();
    document.header.dim_linetype2_name = "HIDDEN".into();
    // a document made in memory states the handles the DWG header of R2007 holds
    document.header.dim_linetype_handle = document.line_types.get("DASHED").expect("DASHED").handle;
    document.header.dim_linetype1_handle = document.line_types.get("DOT").expect("DOT").handle;
    document.header.dim_linetype2_handle = document.line_types.get("HIDDEN").expect("HIDDEN").handle;
    document
}

fn dimension_linetypes(document: &CadDocument) -> [String; 3] {
    [document.header.dim_linetype_name.clone(), document.header.dim_linetype1_name.clone(), document.header.dim_linetype2_name.clone()].map(|name| name.to_ascii_uppercase())
}

fn header_vars_dictionary(document: &CadDocument) -> Option<Handle> {
    match document.objects.get(&document.header.named_objects_dict_handle) {
        Some(ObjectType::Dictionary(root)) => root.get("ACAD_2007HEADERVARS_RT"),
        _ => None,
    }
}

/// $DIMLTYPE, $DIMLTEX1 and $DIMLTEX2 are R2007 variables. AutoCAD's R14, R2000 and R2004 files keep them in the dictionary
/// `ACAD_2007HEADERVARS_RT` of the root dictionary: the XRECORDs DIMLTYPE, DIMLTEX1 and DIMLTEX2, each `340 <the linetype>`
/// (sample_AC1014.dwg, sample_AC1015.dwg, sample_AC1018.dwg and the DXF twins of R2000 and R2004).
#[test]
fn the_dimension_linetypes_of_the_header_are_kept_before_r2007() {
    let source = document_with_dimension_linetypes();
    let want = ["DASHED".to_string(), "DOT".to_string(), "HIDDEN".to_string()];
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018] {
        let out = write_and_read(&source, version);
        assert_eq!(dimension_linetypes(&out), want, "{version:?}");
        assert!(header_vars_dictionary(&out).is_none(), "{version:?}: the reader took the chain out of the model");
        let file = out.with_round_trip_records();
        let dictionary = header_vars_dictionary(&file).unwrap_or_else(|| panic!("{version:?}: no dictionary"));
        let Some(ObjectType::Dictionary(entries)) = file.objects.get(&dictionary) else { panic!("not a dictionary") };
        // listed by name, as AutoCAD lists them; the entry is the first of the root dictionary (sample_AC1015.dwg)
        let keys: Vec<&str> = entries.entries.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(keys, ["DIMLTEX1", "DIMLTEX2", "DIMLTYPE"], "{version:?}");
        let Some(ObjectType::Dictionary(root)) = file.objects.get(&file.header.named_objects_dict_handle) else { panic!("no root") };
        assert_eq!(root.entries[0].0, "ACAD_2007HEADERVARS_RT", "{version:?}");
    }
    let out = write_and_read(&source, DxfVersion::AC1021);
    assert_eq!(dimension_linetypes(&out), want);
    assert!(header_vars_dictionary(&out).is_none());
    // a header that holds no other linetype than ByBlock states no record
    let plain = write_and_read(&CadDocument::new(), DxfVersion::AC1018);
    assert!(header_vars_dictionary(&plain).is_none());
}

#[test]
fn the_dimension_linetype_records_are_edited_and_dropped_with_the_version() {
    let mut down = write_and_read(&document_with_dimension_linetypes(), DxfVersion::AC1018);
    down.header.dim_linetype1_name = "HIDDEN".into();
    down.header.dim_linetype1_handle = Handle::NULL;
    let again = write_and_read(&down, DxfVersion::AC1018);
    assert_eq!(dimension_linetypes(&again), ["DASHED", "HIDDEN", "HIDDEN"].map(String::from));
    let up = write_and_read(&again, DxfVersion::AC1032);
    assert_eq!(dimension_linetypes(&up), ["DASHED", "HIDDEN", "HIDDEN"].map(String::from));
    assert!(header_vars_dictionary(&up).is_none());
    // AutoCAD's DXF of R2000 and R2004 holds the records, that of R2007 does not
    for (version, held) in [(DxfVersion::AC1015, true), (DxfVersion::AC1018, true), (DxfVersion::AC1021, false)] {
        let out = dxf_write_and_read(&document_with_dimension_linetypes(), version);
        assert_eq!(dimension_linetypes(&out), ["DASHED", "DOT", "HIDDEN"].map(String::from), "{version:?}");
        assert_eq!(header_vars_dictionary(&out.with_round_trip_records()).is_some(), held, "{version:?}");
        assert!(header_vars_dictionary(&out).is_none(), "{version:?}");
    }
}

// ---- blocks that scale uniformly ----------------------------------------------------------------------------------------

fn document_with_uniform_block() -> CadDocument {
    let mut document = CadDocument::new();
    let mut block = acadrust::tables::BlockRecord::new("B1");
    block.scale_uniformly = true;
    document.block_records.add(block).unwrap();
    let mut other = acadrust::tables::BlockRecord::new("B2");
    other.scale_uniformly = false;
    document.block_records.add(other).unwrap();
    document
}

/// The `Edit` record of the block `name` the file held (the reader takes the chain it applied out of the model).
fn block_chain(document: &CadDocument, name: &str) -> Option<Vec<i32>> {
    assert!(native_block_chain(document, name).is_none(), "the reader took the chain out of the model");
    native_block_chain(&document.with_round_trip_records(), name)
}

fn native_block_chain(document: &CadDocument, name: &str) -> Option<Vec<i32>> {
    let handle = document.block_records.iter().find(|block| block.name.eq_ignore_ascii_case(name))?.handle;
    let extension = document.extension_dictionary_handle(handle)?;
    let ObjectType::Dictionary(extension) = document.objects.get(&extension)? else { return None };
    let data = extension.get("AcDbBlockTableRecordData")?;
    let ObjectType::Dictionary(data) = document.objects.get(&data)? else { return None };
    let ObjectType::XRecord(edit) = document.objects.get(&data.get("Edit")?)? else { return None };
    Some(edit.entries.iter().map(|entry| entry.code).collect())
}

fn scales_uniformly(document: &CadDocument, name: &str) -> bool {
    document.block_records.iter().find(|block| block.name.eq_ignore_ascii_case(name)).expect("the block").scale_uniformly
}

/// R2007 added the flag that a block scales uniformly. AutoCAD's R14, R2000 and R2004 files keep it for the blocks that do in a
/// dictionary `AcDbBlockTableRecordData` (R14: upper case) of the block's extension dictionary, with the XRECORD `Edit {1070 0,
/// 1070 1, 1070 1}` (4 blocks of sample_AC1014.dwg, sample_AC1015.dwg and sample_AC1018.dwg); a block that does not has none.
#[test]
fn a_block_that_scales_uniformly_keeps_the_flag_before_r2007() {
    let source = document_with_uniform_block();
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018] {
        let out = write_and_read(&source, version);
        assert!(scales_uniformly(&out, "B1"), "{version:?}");
        assert!(!scales_uniformly(&out, "B2"), "{version:?}");
        if version >= DxfVersion::AC1015 {
            assert_eq!(block_chain(&out, "B1"), Some(vec![1070, 1070, 1070]), "{version:?}");
        }
        assert!(block_chain(&out, "B2").is_none(), "{version:?}");
    }
    let out = write_and_read(&source, DxfVersion::AC1021);
    assert!(scales_uniformly(&out, "B1") && !scales_uniformly(&out, "B2"));
    assert!(block_chain(&out, "B1").is_none());
}

#[test]
fn the_block_record_is_edited_and_dropped_with_the_version() {
    let mut down = write_and_read(&document_with_uniform_block(), DxfVersion::AC1018);
    for block in down.block_records.iter_mut() {
        if block.name.eq_ignore_ascii_case("B1") {
            block.scale_uniformly = false;
        }
        if block.name.eq_ignore_ascii_case("B2") {
            block.scale_uniformly = true;
        }
    }
    let again = write_and_read(&down, DxfVersion::AC1018);
    assert!(!scales_uniformly(&again, "B1") && scales_uniformly(&again, "B2"));
    assert!(block_chain(&again, "B1").is_none() && block_chain(&again, "B2").is_some());
    let up = write_and_read(&again, DxfVersion::AC1032);
    assert!(scales_uniformly(&up, "B2") && block_chain(&up, "B2").is_none());
    // AutoCAD's DXF of R2000 and R2004 holds the chain too (sample_AC1015_ascii.dxf, sample_AC1018_ascii.dxf); R2007 holds the flag
    for version in [DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1021] {
        let out = dxf_write_and_read(&document_with_uniform_block(), version);
        assert!(scales_uniformly(&out, "B1") && !scales_uniformly(&out, "B2"), "{version:?}");
        assert_eq!(block_chain(&out, "B1").is_some(), version < DxfVersion::AC1021, "{version:?}");
    }
}

// ---- multi-line attributes ----------------------------------------------------------------------------------------------

fn embedded_text() -> acadrust::entities::MText {
    let mut text = acadrust::entities::MText::with_value("first line\\Psecond line", acadrust::types::Vector3::new(12.0, 3.0, 0.0));
    text.height = 2.5;
    text.rectangle_width = 7.0;
    text.attachment_point = acadrust::entities::AttachmentPoint::MiddleCenter;
    text.drawing_direction = acadrust::entities::DrawingDirection::ByStyle;
    text.line_spacing_factor = 1.25;
    text.line_spacing_style = acadrust::entities::LineSpacingStyle::Exactly;
    text
}

fn document_with_multiline_attributes() -> CadDocument {
    let mut document = CadDocument::new();
    let mut definition = acadrust::entities::AttributeDefinition::new("MLTAG".into(), "prompt".into(), String::new());
    definition.mtext_flag = acadrust::entities::MTextFlag::ConstantMultiLine;
    definition.is_multiline = true;
    definition.embedded_mtext = Some(Box::new(embedded_text()));
    document.add_entity(acadrust::entities::EntityType::AttributeDefinition(definition)).unwrap();
    let mut attribute = acadrust::entities::AttributeEntity::new("MLTAG".into(), "first line\\Psecond line".into());
    attribute.mtext_flag = acadrust::entities::MTextFlag::MultiLine;
    attribute.is_multiline = true;
    attribute.embedded_mtext = Some(Box::new(embedded_text()));
    let mut insert = acadrust::entities::Insert::new("B1", acadrust::types::Vector3::ZERO);
    insert.attributes.push(attribute);
    document.add_entity(acadrust::entities::EntityType::Insert(insert)).unwrap();
    document
}

/// (the attribute definition, the attribute of the insert)
fn multiline_attributes(document: &CadDocument) -> (acadrust::entities::AttributeDefinition, acadrust::entities::AttributeEntity) {
    let definition = document
        .entities()
        .find_map(|entity| match entity {
            acadrust::entities::EntityType::AttributeDefinition(definition) => Some(definition.clone()),
            _ => None,
        })
        .expect("the attribute definition");
    let attribute = document
        .entities()
        .find_map(|entity| match entity {
            acadrust::entities::EntityType::Insert(insert) => insert.attributes.first().cloned(),
            _ => None,
        })
        .expect("the attribute");
    (definition, attribute)
}

/// Before R2018 AutoCAD writes a multi-line attribute as a single-line one and keeps the MTEXT in the record `ACAD_MLATT` of the
/// extension dictionary (`70 <flag> 70 0 70 0 40 1 2 <tag> 1 Embedded Object 10 <insertion> 40 <height> 41 <width> 46 <height> 71
/// <attachment> 72 <direction> 1 <text> 73 <spacing style> 44 <spacing factor>`) and its height in `ACAD_XREC_ROUNDTRIP
/// {ACAD_MATT_HEIGHT 140}` (the attributes #796 and #79D of sample_AC1014.dwg to sample_AC1027.dwg and the DXF twins of R2000 to
/// R2013).
#[test]
fn a_multiline_attribute_keeps_its_mtext_before_r2018() {
    let source = document_with_multiline_attributes();
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024, DxfVersion::AC1027] {
        let out = write_and_read(&source, version);
        let (definition, attribute) = multiline_attributes(&out);
        for (flag, multiline, text) in [
            (definition.mtext_flag, definition.is_multiline, definition.embedded_mtext.as_deref()),
            (attribute.mtext_flag, attribute.is_multiline, attribute.embedded_mtext.as_deref()),
        ] {
            let text = text.unwrap_or_else(|| panic!("{version:?}: no embedded MTEXT"));
            assert!(multiline, "{version:?}");
            assert_ne!(flag, acadrust::entities::MTextFlag::SingleLine, "{version:?}");
            assert_eq!(text.value, "first line\\Psecond line", "{version:?}");
            assert_eq!((text.height, text.rectangle_width, text.line_spacing_factor), (2.5, 7.0, 1.25), "{version:?}");
            assert_eq!(text.attachment_point, acadrust::entities::AttachmentPoint::MiddleCenter, "{version:?}");
            assert_eq!(text.line_spacing_style, acadrust::entities::LineSpacingStyle::Exactly, "{version:?}");
            assert_eq!(text.insertion_point, acadrust::types::Vector3::new(12.0, 3.0, 0.0), "{version:?}");
        }
        assert_eq!(definition.mtext_flag, acadrust::entities::MTextFlag::ConstantMultiLine, "{version:?}");
        assert_eq!(attribute.mtext_flag, acadrust::entities::MTextFlag::MultiLine, "{version:?}");
        // the reader took the chain out of the model; the extension dictionary stays, empty (as AutoCAD's files keep one)
        let kept = out.extension_dictionary_handle(definition.common.handle).unwrap_or_else(|| panic!("{version:?}: no extension dictionary"));
        assert!(matches!(out.objects.get(&kept), Some(ObjectType::Dictionary(entries)) if entries.entries.is_empty()), "{version:?}");
        let file = out.with_round_trip_records();
        let extension = file.extension_dictionary_handle(definition.common.handle).unwrap_or_else(|| panic!("{version:?}: no extension dictionary"));
        let Some(ObjectType::Dictionary(extension)) = file.objects.get(&extension) else { panic!("not a dictionary") };
        let keys: Vec<&str> = extension.entries.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(keys, ["ACAD_MLATT", "ACAD_XREC_ROUNDTRIP"], "{version:?}: listed by name, as AutoCAD lists them");
        let mlatt = extension.get("ACAD_MLATT").unwrap_or_else(|| panic!("{version:?}: no ACAD_MLATT"));
        let Some(ObjectType::XRecord(mlatt)) = file.objects.get(&mlatt) else { panic!("not a record") };
        let codes: Vec<i32> = mlatt.entries.iter().map(|entry| entry.code).collect();
        assert_eq!(codes, [70, 70, 70, 40, 2, 1, 10, 40, 41, 46, 71, 72, 1, 73, 44], "{version:?}");
        assert_eq!(mlatt.entries[5].value.as_string(), Some("Embedded Object"));
        let height = record_of(&out, definition.common.handle).unwrap_or_else(|| panic!("{version:?}: no height record"));
        assert_eq!(names(&height), ["ACAD_MATT_HEIGHT"], "{version:?}");
    }
    // R2018 states the MTEXT in the attribute records
    let out = write_and_read(&source, DxfVersion::AC1032);
    let (definition, attribute) = multiline_attributes(&out);
    assert!(definition.embedded_mtext.is_some() && attribute.embedded_mtext.is_some());
    assert!(out.extension_dictionary_handle(definition.common.handle).is_none_or(|extension| {
        !matches!(out.objects.get(&extension), Some(ObjectType::Dictionary(entries)) if entries.get("ACAD_MLATT").is_some())
    }));
}

#[test]
fn a_multiline_attribute_record_is_edited_and_dropped_with_the_version() {
    let mut down = write_and_read(&document_with_multiline_attributes(), DxfVersion::AC1018);
    let (definition, _) = multiline_attributes(&down);
    if let Some(acadrust::entities::EntityType::AttributeDefinition(definition)) = down.get_entity_mut(definition.common.handle) {
        if let Some(text) = definition.embedded_mtext.as_deref_mut() {
            text.value = "edited".into();
            text.height = 4.0;
        }
    }
    let again = write_and_read(&down, DxfVersion::AC1018);
    let text = multiline_attributes(&again).0.embedded_mtext.expect("the embedded MTEXT");
    assert_eq!((text.value.as_str(), text.height), ("edited", 4.0));
    let up = write_and_read(&again, DxfVersion::AC1032);
    let (definition, _) = multiline_attributes(&up);
    assert_eq!(definition.embedded_mtext.expect("the embedded MTEXT").value, "edited");
    // AutoCAD's DXF of R2000 to R2013 holds the records too
    let out = dxf_write_and_read(&document_with_multiline_attributes(), DxfVersion::AC1018);
    let (definition, _) = multiline_attributes(&out);
    assert!(definition.embedded_mtext.is_some());
}

// ---- tables -------------------------------------------------------------------------------------------------------------

fn document_with_typed_table() -> CadDocument {
    let mut table = acadrust::entities::Table::new(acadrust::types::Vector3::new(1.0, 2.0, 0.0), 3, 2);
    table.set_cell_text(0, 0, "Title");
    table.set_cell_text(1, 0, "Quantity");
    table.set_cell_text(1, 1, "0.500000");
    if let Some(content) = table.rows[1].cells[1].contents.first_mut() {
        content.value.value_type = acadrust::entities::CellValueType::Double;
        content.value.numeric_value = 0.5;
        content.value.text = "0.5".into();
        content.value.formatted_value = "0.500000".into();
    }
    let mut document = CadDocument::new();
    document.add_entity(acadrust::entities::EntityType::Table(Box::new(table))).unwrap();
    document
}

fn the_table(document: &CadDocument) -> acadrust::entities::Table {
    document
        .entities()
        .find_map(|entity| match entity {
            acadrust::entities::EntityType::Table(table) => Some((**table).clone()),
            _ => None,
        })
        .expect("the table")
}

fn table_objects(document: &CadDocument) -> (usize, usize) {
    let content = document.objects.values().filter(|object| matches!(object, ObjectType::TableContent(_))).count();
    let geometry = document
        .objects
        .values()
        .filter(|object| matches!(object, ObjectType::DataObject(data) if matches!(data.data, acadrust::objects::DataObjectData::TableGeometry(_))))
        .count();
    (content, geometry)
}

/// The entity record of a table before R2010 holds the legacy flat cells; AutoCAD keeps the typed cells in a TABLECONTENT object and
/// the cached geometry in a TABLEGEOMETRY object, linked by `ACAD_XREC_ROUNDTRIP {ACAD_ROUNDTRIP_2008_TABLE_ENTITY, 360 <content>, 70
/// 2, 90 <breaks>, .., 361 <geometry>}` of the table's extension dictionary (sample_AC1015.dwg and sample_AC1018.dwg, the DXF twins of
/// R2000 and R2004). A table made in memory had none, and its typed cells were lost at R2004 and older (blocks_and_tables_-_imperial.dwg:
/// 624 rows of the cells).
#[test]
fn a_modern_table_keeps_its_typed_cells_in_the_objects_autocad_keeps_them_in() {
    let source = document_with_typed_table();
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018] {
        let out = write_and_read(&source, version);
        let table = the_table(&out);
        let cell = &table.rows[1].cells[1];
        assert_eq!(cell.contents[0].value.value_type, acadrust::entities::CellValueType::Double, "{version:?}");
        assert_eq!(cell.contents[0].value.numeric_value, 0.5, "{version:?}");
        assert_eq!(table_objects(&out), (1, 1), "{version:?}: the objects stay in the document, as in a file of AutoCAD's");
        let record = out.xrecord(table.common.handle, "ACAD_XREC_ROUNDTRIP").unwrap_or_else(|| panic!("{version:?}: no record"));
        assert_eq!(record.entries[0].value.as_string(), Some("ACAD_ROUNDTRIP_2008_TABLE_ENTITY"));
        assert_eq!(record.entries[1].code, 360);
        assert_eq!(record.entries.last().map(|entry| entry.code), Some(361));
        // written again, the table keeps them (the objects it has are not made twice)
        let again = write_and_read(&out, version);
        assert_eq!(table_objects(&again), (1, 1), "{version:?}");
        assert_eq!(the_table(&again).rows[1].cells[1].contents[0].value.value_type, acadrust::entities::CellValueType::Double, "{version:?}");
    }
    // R2007 holds the value of every cell in the entity: no objects
    let out = write_and_read(&source, DxfVersion::AC1021);
    assert_eq!(table_objects(&out), (0, 0));
    assert_eq!(the_table(&out).rows[1].cells[1].contents[0].value.value_type, acadrust::entities::CellValueType::Double);
    // a DXF of R2000 and R2004 holds the objects too
    for version in [DxfVersion::AC1015, DxfVersion::AC1018] {
        let out = dxf_write_and_read(&source, version);
        assert_eq!(the_table(&out).rows[1].cells[1].contents[0].value.value_type, acadrust::entities::CellValueType::Double, "{version:?}");
        assert_eq!(table_objects(&out), (1, 1), "{version:?}");
    }
}

/// AutoCAD's own conversion of a fit-point spline is a control-point spline with the record (sample_AC1014.dwg to sample_AC1024.dwg,
/// #434): the model has no fit data for it, so the record is not taken in and stays extended data of the spline, as the file had it.
#[test]
fn the_record_of_a_control_point_spline_stays_as_the_file_had_it() {
    use acadrust::xdata::{ExtendedDataRecord, XDataValue};
    let mut document = CadDocument::new();
    let mut app = acadrust::tables::AppId::new("ACAD_SPLINE_DATA");
    app.handle = document.allocate_handle();
    document.app_ids.add(app).unwrap();
    let points = vec![
        acadrust::types::Vector3::new(0.0, 0.0, 0.0),
        acadrust::types::Vector3::new(5.0, 10.0, 0.0),
        acadrust::types::Vector3::new(10.0, 0.0, 0.0),
        acadrust::types::Vector3::new(15.0, 10.0, 0.0),
    ];
    let mut spline = acadrust::entities::Spline::from_control_points(3, points);
    let mut record = ExtendedDataRecord::new("ACAD_SPLINE_DATA");
    record.values = [100, 0, 101, 0, 102, 2].into_iter().map(XDataValue::Integer16).collect();
    spline.common.extended_data.add_record(record.clone());
    document.add_entity(acadrust::entities::EntityType::Spline(spline)).unwrap();
    for version in [DxfVersion::AC1018, DxfVersion::AC1024] {
        let out = write_and_read(&document, version);
        let spline = the_spline(&out);
        let held = spline.common.extended_data.get_record("ACAD_SPLINE_DATA").unwrap_or_else(|| panic!("{version:?}: the record is gone"));
        assert_eq!(held.values, record.values, "{version:?}");
        assert_ne!(spline.knot_parameterization, 2, "{version:?}: no fit data to give the parameterization to");
        // written again, the record is the file's (the writer does not make one from the control-point spline's values)
        let again = write_and_read(&out, version);
        assert_eq!(the_spline(&again).common.extended_data.get_record("ACAD_SPLINE_DATA").map(|r| r.values.clone()), Some(record.values.clone()), "{version:?}");
        // the record was not applied: an up-save is not its writer's to drop (what it states is lost otherwise)
        for up in [DxfVersion::AC1027, DxfVersion::AC1032] {
            let out = write_and_read(&out, up);
            assert_eq!(the_spline(&out).common.extended_data.get_record("ACAD_SPLINE_DATA").map(|r| r.values.clone()), Some(record.values.clone()), "{version:?} -> {up:?}");
        }
    }
}

/// The reader takes only the groups it applied out of a record: a group of another producer stays in the model, and a writer
/// keeps it.
#[test]
fn a_reader_takes_only_the_groups_it_applied_out_of_a_record() {
    use acadrust::objects::XRecordEntry;
    let (source, _) = document_with_material();
    let out = write_and_read(&source, DxfVersion::AC1015);
    let mut file = out.with_round_trip_records();
    let (handle, _) = named_material(&file, "Brick");
    let record = file.xrecord_mut(handle, "ACAD_XREC_ROUNDTRIP").expect("the record the file held");
    record.entries.push(XRecordEntry::string(102, "OTHERAPP"));
    record.entries.push(XRecordEntry::new(70, acadrust::objects::XRecordValue::Int16(7)));
    let again = write_and_read(&file, DxfVersion::AC1015);
    let (handle, material) = named_material(&again, "Brick");
    assert_eq!((material.reflectivity, material.channel_flags), (0.15, 17));
    let kept = again.xrecord(handle, "ACAD_XREC_ROUNDTRIP").expect("the group the reader did not apply stays");
    assert_eq!(names(kept), ["OTHERAPP"]);
    let file = again.with_round_trip_records();
    assert_eq!(names(file.xrecord(handle, "ACAD_XREC_ROUNDTRIP").expect("the file's record")), ["MATERIAL", "OTHERAPP"]);
}

/// A record whose owner was deleted after the read is not written again.
#[test]
fn a_record_of_a_deleted_owner_is_not_written_again() {
    let (source, _) = document_with_material();
    let mut out = write_and_read(&source, DxfVersion::AC1015);
    let (handle, _) = named_material(&out, "Brick");
    // the material goes with its extension dictionary
    let extension = out.extension_dictionary_handle(handle).expect("the extension dictionary the record was in");
    out.objects.remove(&extension);
    out.objects.remove(&handle);
    for object in out.objects.values_mut() {
        if let ObjectType::Dictionary(dictionary) = object {
            dictionary.entries.retain(|(_, target)| *target != handle);
        }
    }
    let file = out.with_round_trip_records();
    assert!(file.objects.values().all(|object| !matches!(object, ObjectType::XRecord(record) if record.owner == extension)));
    let again = write_and_read(&out, DxfVersion::AC1015);
    assert!(again.objects.values().all(|object| !matches!(object, ObjectType::Material(material) if material.name == "Brick")));
}
