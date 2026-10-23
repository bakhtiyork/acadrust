// SPDX-License-Identifier: MPL-2.0
//! R5b-4 (gate G5): the down-saves keep what AutoCAD's own down-saves keep, at every version. Synthetic documents only;
//! each test fails with its fix reverted. Evidence: AutoCAD's sample_AC1014.dwg, sample_AC1015.dwg, sample_AC1018.dwg
//! (the C# dumps under Tests/CadCoreTests/Fixtures/dumps).

use std::io::Cursor;

use acadrust::objects::ObjectType;
use acadrust::types::DxfVersion;
use acadrust::{CadDocument, DwgReader, DwgWriter};

const DOWN_SAVES: [DxfVersion; 4] = [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024];

fn write_and_read(document: &CadDocument, version: DxfVersion) -> CadDocument {
    let mut document = document.clone();
    document.version = version;
    let bytes = DwgWriter::write_to_vec(&document).unwrap_or_else(|error| panic!("{version:?}: DWG write failed: {error}"));
    DwgReader::from_stream(Cursor::new(bytes))
        .read()
        .unwrap_or_else(|error| panic!("{version:?}: DWG read failed: {error}"))
}

fn count(document: &CadDocument, wanted: fn(&ObjectType) -> bool) -> usize {
    document.objects.values().filter(|object| wanted(object)).count()
}

fn root_names(document: &CadDocument) -> Vec<String> {
    match document.objects.get(&document.header.named_objects_dict_handle) {
        Some(ObjectType::Dictionary(root)) => root.entries.iter().map(|(name, _)| name.clone()).collect(),
        _ => panic!("no root dictionary"),
    }
}

fn entry_of(document: &CadDocument, dictionary: &str) -> acadrust::types::Handle {
    match document.objects.get(&document.header.named_objects_dict_handle) {
        Some(ObjectType::Dictionary(root)) => root.get(dictionary).unwrap_or_else(|| panic!("no {dictionary} in the root dictionary")),
        _ => panic!("no root dictionary"),
    }
}

/// A drawing with a material and a visual style, as an R2013 DWG holds them, read back from that DWG.
fn dwg_source_with_materials() -> CadDocument {
    let mut document = CadDocument::new();
    let materials = entry_of(&document, "ACAD_MATERIAL");
    let styles = entry_of(&document, "ACAD_VISUALSTYLE");
    let mut material = acadrust::objects::Material::new();
    material.name = "TILES".into();
    material.handle = document.allocate_handle();
    material.owner = materials;
    let material_handle = material.handle;
    document.objects.insert(material_handle, ObjectType::Material(material));
    let mut style = acadrust::objects::VisualStyle::new();
    style.handle = document.allocate_handle();
    style.owner = styles;
    let style_handle = style.handle;
    document.objects.insert(style_handle, ObjectType::VisualStyle(style));
    for (dictionary, name, handle) in [(materials, "TILES", material_handle), (styles, "MYSTYLE", style_handle)] {
        if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&dictionary) {
            entries.add_entry(name, handle);
        }
    }
    write_and_read(&document, DxfVersion::AC1032)
}

/// AutoCAD's own R14, R2000 and R2004 files hold the table style, multileader style, material and visual style
/// dictionaries with their objects, and the layout and plot settings dictionaries, as its R2010 and later ones do
/// (sample_AC1014.dwg, sample_AC1015.dwg, sample_AC1018.dwg: ACAD_MATERIAL, ACAD_VISUALSTYLE, ACAD_TABLESTYLE,
/// ACAD_MLEADERSTYLE and ACAD_LAYOUT in the root dictionary, 11 materials, 24 visual styles, 4 layouts): a down-save of a
/// drawing from another version keeps them at every version. Before R5b-4 the writer kept them for a same-version source only.
#[test]
fn a_down_save_keeps_the_style_material_and_visual_style_objects() {
    let source = dwg_source_with_materials();
    for version in DOWN_SAVES {
        let out = write_and_read(&source, version);
        let names = root_names(&out);
        for wanted in ["ACAD_MATERIAL", "ACAD_VISUALSTYLE", "ACAD_TABLESTYLE", "ACAD_MLEADERSTYLE", "ACAD_LAYOUT", "ACAD_PLOTSETTINGS"] {
            assert!(names.iter().any(|name| name == wanted), "{version:?}: {wanted} is in the root dictionary ({names:?})");
        }
        assert!(out.objects.values().any(|o| matches!(o, ObjectType::Material(m) if m.name == "TILES")), "{version:?}: the material");
        // ... and its dictionaries list them
        for (dictionary, entry) in [("ACAD_MATERIAL", "TILES"), ("ACAD_VISUALSTYLE", "MYSTYLE")] {
            let handle = entry_of(&out, dictionary);
            match out.objects.get(&handle) {
                Some(ObjectType::Dictionary(entries)) => {
                    assert!(entries.get(entry).is_some(), "{version:?}: {dictionary} lists {entry}")
                }
                _ => panic!("{version:?}: no {dictionary}"),
            }
        }
        assert_eq!(count(&out, |o| matches!(o, ObjectType::VisualStyle(_))), 1, "{version:?}: the visual style");
        assert_eq!(count(&out, |o| matches!(o, ObjectType::TableStyle(_))), 1, "{version:?}: the table style");
        assert_eq!(count(&out, |o| matches!(o, ObjectType::MultiLeaderStyle(_))), 1, "{version:?}: the multileader style");
        assert_eq!(count(&out, |o| matches!(o, ObjectType::Layout(_))), 2, "{version:?}: Model and Layout1");
    }
}

fn class_names(document: &CadDocument) -> Vec<String> {
    document.classes.iter().map(|class| class.dxf_name.clone()).collect()
}

/// AutoCAD's own saves of one drawing keep the class table of the source whole, in its order and numbering (the 50 classes
/// 500..549 are the same list in sample_AC1014.dwg to sample_AC1032.dwg), and differ only by the version's profile: R14 adds
/// DATATABLE, LWPOLYLINE, HATCH and XRECORD (550..553), R2000 and R2004 add DATATABLE (550), later versions add none. The
/// writer shrank the table to a short list of classes and renumbered it (the shifted numbers were 33 rows per sample_V cell).
#[test]
fn a_down_save_keeps_the_class_table_of_its_dwg_source() {
    let source = dwg_source_with_materials();
    let table = class_names(&source);
    assert!(table.len() > 40, "the source table is the long one: {}", table.len());
    for (version, profile) in [
        (DxfVersion::AC1014, vec!["DATATABLE", "LWPOLYLINE", "HATCH", "XRECORD"]),
        (DxfVersion::AC1015, vec!["DATATABLE"]),
        (DxfVersion::AC1018, vec!["DATATABLE"]),
        (DxfVersion::AC1024, vec![]),
        (DxfVersion::AC1027, vec![]),
    ] {
        let out = write_and_read(&source, version);
        // A class the version's profile does not declare goes; one it declares and the table lacks follows the table.
        let mut want = table.clone();
        want.retain(|name| !["DATATABLE", "LWPOLYLINE", "HATCH", "XRECORD"].contains(&name.as_str()) || profile.contains(&name.as_str()));
        for name in &profile {
            if !want.iter().any(|have| have == name) {
                want.push(name.to_string());
            }
        }
        let got = class_names(&out);
        let missing: Vec<_> = want.iter().filter(|n| !got.contains(n)).collect();
        let extra: Vec<_> = got.iter().filter(|n| !want.contains(n)).collect();
        assert!(missing.is_empty() && extra.is_empty() && got == want, "{version:?}: missing {missing:?}, extra {extra:?}");
        for (index, class) in out.classes.iter().enumerate() {
            assert_eq!(class.class_number, 500 + index as i16, "{version:?}: {} is numbered by position", class.dxf_name);
        }
    }
}

/// A VPORT record before R2007 has no grid behaviour field, and the drawing AutoCAD converts from R2004 to R2010 states 3 for it
/// (sample_AC1018.dwg to sample_AC1024.dwg); the fork wrote the 2 its reader leaves.
#[test]
fn a_vport_of_an_old_dwg_gets_the_grid_behaviour_autocad_states_when_it_converts() {
    let mut document = CadDocument::new();
    document.version = DxfVersion::AC1015;
    let old = write_and_read(&document, DxfVersion::AC1015);
    let converted = write_and_read(&old, DxfVersion::AC1024);
    let flags: Vec<i16> = converted.vports.iter().map(|vport| vport.grid_flags.to_bits()).collect();
    assert!(!flags.is_empty());
    assert!(flags.iter().all(|bits| *bits == 3), "{flags:?}");
}
