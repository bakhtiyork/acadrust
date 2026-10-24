// SPDX-License-Identifier: MPL-2.0
//! R5b-7 (gate G4/G5): a DXF down-save carries what AutoCAD's own DXF of that version holds. AutoCAD's R2000 and R2004 DXF
//! files hold none of its round-trip records of the DWG (`RTTcAl`, `RTLwPs`, `RTMaterial`: sample_AC1015_ascii.dxf,
//! sample_AC1018_ascii.dxf), while the R2000 file states `MTEXTBEGIN` in all 36 MTEXT and every DXF from R2000 to R2013 states
//! the stamp of the visual styles. Synthetic documents only.

use std::io::Cursor;

use acadrust::entities::{Circle, EntityType, Line, MText, Point};
use acadrust::objects::{Material, ObjectType, VisualStyle};
use acadrust::types::{Color, DxfVersion, Handle, LineWeight, Vector3};
use acadrust::{CadDocument, DxfReader, DxfWriter};

fn dxf(document: &CadDocument, version: DxfVersion, binary: bool) -> Vec<u8> {
    let mut document = document.clone();
    document.version = version;
    let writer = if binary { DxfWriter::new_binary(&document) } else { DxfWriter::new(&document) };
    writer.write_to_vec().unwrap_or_else(|error| panic!("{version:?}: DXF write failed: {error}"))
}

fn contains(bytes: &[u8], text: &str) -> bool {
    bytes.windows(text.len()).any(|window| window == text.as_bytes())
}

struct Drawing {
    document: CadDocument,
    line: Handle,
    circle: Handle,
}

/// An R2018 drawing with the values the older versions hold in the round-trip records: a true colour and a line weight, a
/// material, and an MTEXT with a background fill.
fn drawing() -> Drawing {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    document.dwg_source_version = Some(DxfVersion::AC1032);
    let materials = document.header.acad_material_dict_handle;
    let mut material = Material::new();
    material.name = "Aluminum Cast".into();
    material.handle = document.allocate_handle();
    material.owner = materials;
    let material_handle = material.handle;
    document.objects.insert(material_handle, ObjectType::Material(material));
    if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&materials) {
        entries.add_entry("Aluminum Cast", material_handle);
    }

    let mut line = Line::from_points(Vector3::ZERO, Vector3::UNIT_X);
    line.common.color = Color::from_rgb(245, 10, 147);
    line.common.line_weight = LineWeight::Value(13);
    let line = document.add_entity(EntityType::Line(line)).unwrap();

    let mut circle = Circle::new();
    circle.common.material_flags = 0b11;
    circle.common.material_handle = Some(material_handle);
    let circle = document.add_entity(EntityType::Circle(circle)).unwrap();

    let mut point = Point::new();
    point.common.material_flags = 0b01;
    document.add_entity(EntityType::Point(point)).unwrap();

    let mut text = MText::new();
    text.value = "filled".into();
    text.background_fill_flags = 1;
    text.background_scale = 1.25;
    document.add_entity(EntityType::MText(Box::new(text))).unwrap();
    Drawing { document, line, circle }
}

/// AutoCAD's DXF of R2000 and R2004 hold no `RT*` record (sample_AC1015_ascii.dxf: the point #29A states colour 230 and
/// nothing else, its DWG `RTTcAl`), in either the ASCII or the binary form; the MTEXT blocks of R2000 stay (36 `MTEXTBEGIN`
/// in sample_AC1015_ascii.dxf, none in sample_AC1018_ascii.dxf).
#[test]
fn a_dxf_down_save_states_no_round_trip_record() {
    let drawing = drawing();
    for binary in [false, true] {
        for (version, mtext_blocks) in [
            (DxfVersion::AC1014, true),
            (DxfVersion::AC1015, true),
            (DxfVersion::AC1018, false),
            (DxfVersion::AC1021, false),
        ] {
            let bytes = dxf(&drawing.document, version, binary);
            for name in ["RTTcAl", "RTLwPs", "RTMaterial"] {
                assert!(!contains(&bytes, name), "{version:?} binary {binary}: {name}");
            }
            assert_eq!(contains(&bytes, "MTEXTBEGIN"), mtext_blocks, "{version:?} binary {binary}: MTEXTBEGIN");
            assert_eq!(contains(&bytes, "ACAD_MTEXT_BBRT"), mtext_blocks, "{version:?} binary {binary}: ACAD_MTEXT_BBRT");
        }
    }
}

/// The values the records carried read back as the version holds them: the nearest index for the true colour of an R2000 DXF
/// (the entity record of R2004 holds the colour natively), no line weight before R2000 and no material before R2007.
#[test]
fn a_dxf_down_save_reads_back_as_the_version_holds_the_values() {
    let drawing = drawing();
    for (version, color, line_weight, material) in [
        (DxfVersion::AC1014, Color::Index(230), LineWeight::ByLayer, None),
        (DxfVersion::AC1015, Color::Index(230), LineWeight::Value(13), None),
        (DxfVersion::AC1018, Color::from_rgb(245, 10, 147), LineWeight::Value(13), None),
    ] {
        let read = DxfReader::from_reader(Cursor::new(dxf(&drawing.document, version, false))).unwrap().read().unwrap();
        let line = read.get_entity(drawing.line).unwrap().common();
        assert_eq!((line.color, line.line_weight), (color, line_weight), "{version:?}");
        let circle = read.get_entity(drawing.circle).unwrap().common();
        assert_eq!((circle.material_flags, circle.material_handle), (0, material), "{version:?}");
        // the values are not data of a record: nothing is left in the extended data
        assert!(line.extended_data.get_record("ACAD").is_none(), "{version:?}");
        assert!(circle.extended_data.get_record("ACAD").is_none(), "{version:?}");
    }
}

/// Every DXF of AutoCAD from R2000 to R2013 states `ACAD 1000 AcDbSavedByObjectVersion 1070 0` in each of its 24 visual styles
/// (sample_AC1015_ascii.dxf to sample_AC1027_ascii.dxf), the R2018 one none.
#[test]
fn a_dxf_down_save_states_the_visual_style_stamp_as_autocad_does() {
    let mut document = CadDocument::with_version(DxfVersion::AC1032);
    document.dwg_source_version = Some(DxfVersion::AC1032);
    let mut style = VisualStyle::new();
    style.handle = document.allocate_handle();
    document.objects.insert(style.handle, ObjectType::VisualStyle(style));
    for (version, stamp) in [
        (DxfVersion::AC1015, true),
        (DxfVersion::AC1018, true),
        (DxfVersion::AC1021, true),
        (DxfVersion::AC1024, true),
        (DxfVersion::AC1027, true),
        (DxfVersion::AC1032, false),
    ] {
        for binary in [false, true] {
            let bytes = dxf(&document, version, binary);
            assert_eq!(contains(&bytes, "AcDbSavedByObjectVersion"), stamp, "{version:?} binary {binary}");
        }
    }
}
