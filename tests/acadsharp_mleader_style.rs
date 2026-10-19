// SPDX-License-Identifier: MPL-2.0
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::{dump_document, new_document};
use acadrust::objects::{MultiLeaderPathType, MultiLeaderStyle, ObjectType, TextAttachmentType};
use acadrust::types::{Color, DxfVersion, Handle};
use acadrust::{CadDocument, DxfReader, DxfWriter};
use serde_json::Value;
use std::io::Cursor;

fn read_style(codes: &str) -> CadDocument {
    let text = format!(
        "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1032\n0\nENDSEC\n0\nSECTION\n2\nOBJECTS\n0\nMLEADERSTYLE\n5\n2C\n330\n29\n100\nAcDbMLeaderStyle\n{codes}0\nENDSEC\n0\nEOF\n"
    );
    DxfReader::from_reader(Cursor::new(text.into_bytes()))
        .expect("DXF reader init")
        .read()
        .expect("DXF read")
}

fn style(doc: &CadDocument) -> &MultiLeaderStyle {
    let styles = || {
        doc.objects.values().filter_map(|object| match object {
            ObjectType::MultiLeaderStyle(value) => Some(value),
            _ => None,
        })
    };
    styles()
        .find(|value| value.dxf_present_codes.is_some())
        .or_else(|| styles().next())
        .expect("MLEADERSTYLE object")
}

fn style_props(doc: &CadDocument, reader: &str) -> Value {
    dump_document(doc, "", reader)["objects"]
        .as_array()
        .expect("objects")
        .iter()
        .find(|object| {
            object["type"] == "MultiLeaderStyle"
                && object["handle"] == format!("#{:X}", style(doc).handle.value())
        })
        .expect("MultiLeaderStyle dump")["props"]
        .clone()
}

#[test]
fn dxf_reader_keeps_style_fields_colors_rotation_and_references() {
    let doc = read_style(
        "3\nDescription\n300\nDefault contents\n170\n1\n171\n1\n172\n1\n90\n7\n40\n0.125\n41\n0.25\n173\n2\n91\n-1023440896\n93\n-1040154560\n94\n-1039526882\n340\nABC\n341\nDEF\n342\n123\n343\n456\n46\n0.75\n47\n2.0\n49\n3.0\n140\n4.0\n141\n90.0\n143\n0.5\n177\n1\n293\n1\n294\n1\n295\n1\n296\n1\n297\n1\n298\n1\n174\n9\n178\n10\n272\n8\n273\n7\n"
    );
    let value = style(&doc);

    assert_eq!(value.description, "Description");
    assert_eq!(value.default_text, "Default contents");
    assert_eq!(value.multileader_draw_order as i16, 1);
    assert_eq!(value.leader_draw_order as i16, 1);
    assert_eq!(value.max_leader_points, 7);
    assert!((value.first_segment_angle - 0.125).abs() < 1e-12);
    assert!((value.second_segment_angle - 0.25).abs() < 1e-12);
    assert_eq!(value.path_type, MultiLeaderPathType::Spline);
    assert_eq!(
        value.line_color,
        Color::Rgb {
            r: 255,
            g: 136,
            b: 0
        }
    );
    assert_eq!(
        value.text_color,
        Color::Rgb {
            r: 0,
            g: 128,
            b: 64
        }
    );
    assert_eq!(
        value.block_content_color,
        Color::Rgb {
            r: 10,
            g: 20,
            b: 30
        }
    );
    assert_eq!(value.line_type_handle, Some(Handle::new(0xABC)));
    assert_eq!(value.arrowhead_handle, Some(Handle::new(0xDEF)));
    assert_eq!(value.text_style_handle, Some(Handle::new(0x123)));
    assert_eq!(value.block_content_handle, Some(Handle::new(0x456)));
    assert!((value.align_space - 0.75).abs() < 1e-12);
    assert_eq!(value.block_content_scale_x, 2.0);
    assert_eq!(value.block_content_scale_y, 3.0);
    assert_eq!(value.block_content_scale_z, 4.0);
    assert_eq!(value.block_content_rotation, 90.0);
    assert_eq!(value.text_left_attachment, TextAttachmentType::CenterOfText);
    assert_eq!(
        value.text_right_attachment,
        TextAttachmentType::CenterOfTextOverline
    );
    assert_eq!(value.text_bottom_attachment as i16, 8);
    assert_eq!(value.text_top_attachment as i16, 7);
    assert!(value.enable_block_scale && value.enable_block_rotation);
    assert!(value.property_changed && value.is_annotative && value.text_always_left);
    assert!(value.unknown_flag_298);
    assert!(value.dxf_code_present(298));
    assert!(!value.dxf_code_present(299));
}

#[test]
fn omitted_dxf_values_project_reference_defaults_but_explicit_values_survive() {
    let omitted = read_style("3\nOnly description\n");
    let properties = style_props(&omitted, "dxf");
    assert_eq!(properties["PathType"], 0);
    assert_eq!(properties["TextLeftAttachment"], 0);
    assert_eq!(properties["TextRightAttachment"], 0);
    assert_eq!(properties["TextTopAttachment"], 0);
    assert_eq!(properties["TextBottomAttachment"], 0);
    assert_eq!(properties["AlignSpace"], 0.0);

    let explicit = read_style("3\nExplicit\n173\n2\n174\n9\n178\n10\n272\n8\n273\n7\n46\n0.75\n");
    let properties = style_props(&explicit, "dxf");
    assert_eq!(properties["PathType"], 2);
    assert_eq!(properties["TextLeftAttachment"], 9);
    assert_eq!(properties["TextRightAttachment"], 10);
    assert_eq!(properties["TextTopAttachment"], 7);
    assert_eq!(properties["TextBottomAttachment"], 8);
    assert_eq!(properties["AlignSpace"], 0.75);
}

#[test]
fn style_equality_ignores_dxf_presence_and_dump_uses_dictionary_name() {
    let parsed = read_style("3\nSource description\n173\n2\n");
    let mut without_source = style(&parsed).clone();
    without_source.dxf_present_codes = None;
    assert_eq!(style(&parsed), &without_source);

    let mut doc = CadDocument::new();
    let handle = doc
        .objects
        .iter()
        .find_map(|(handle, object)| {
            matches!(object, ObjectType::MultiLeaderStyle(_)).then_some(*handle)
        })
        .expect("native Standard style");
    if let Some(ObjectType::MultiLeaderStyle(value)) = doc.objects.get_mut(&handle) {
        value.name = "Native wire name".into();
        value.description = "Stored description".into();
        value.path_type = MultiLeaderPathType::Spline;
        value.text_left_attachment = TextAttachmentType::CenterOfText;
        value.text_right_attachment = TextAttachmentType::CenterOfTextOverline;
        value.text_top_attachment = TextAttachmentType::CenterOfText;
        value.text_bottom_attachment = TextAttachmentType::CenterOfTextOverline;
    }
    let properties = style_props(&doc, "new");
    assert_eq!(properties["Name"], "Standard");
    assert_eq!(properties["Description"], "Stored description");
    assert_eq!(properties["PathType"], 2);
    assert_eq!(properties["TextLeftAttachment"], 9);
    assert_eq!(properties["TextRightAttachment"], 10);
    assert_eq!(properties["TextTopAttachment"], 9);
    assert_eq!(properties["TextBottomAttachment"], 10);
}

#[test]
fn compat_new_document_uses_reference_defaults_without_mutating_native_defaults() {
    let compat = style_props(&new_document(), "new");
    assert_eq!(compat["PathType"], 0);
    assert_eq!(compat["TextLeftAttachment"], 0);
    assert_eq!(compat["TextRightAttachment"], 0);
    assert_eq!(compat["TextTopAttachment"], 0);
    assert_eq!(compat["TextBottomAttachment"], 0);

    let native = style_props(&CadDocument::new(), "new");
    assert_eq!(native["PathType"], 1);
    assert_eq!(native["TextLeftAttachment"], 1);
    assert_eq!(native["TextRightAttachment"], 1);
    assert_eq!(native["TextTopAttachment"], 0);
    assert_eq!(native["TextBottomAttachment"], 4);
}

#[test]
fn native_dxf_roundtrip_retains_description_and_default_text() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let handle = doc
        .objects
        .iter()
        .find_map(|(handle, object)| {
            matches!(object, ObjectType::MultiLeaderStyle(_)).then_some(*handle)
        })
        .expect("native Standard style");
    if let Some(ObjectType::MultiLeaderStyle(value)) = doc.objects.get_mut(&handle) {
        value.name = "Native wire name".into();
        value.description = "Round-tripped description".into();
        value.default_text = "Default MText contents".into();
        value.text_top_attachment = TextAttachmentType::CenterOfText;
        value.text_bottom_attachment = TextAttachmentType::CenterOfTextOverline;
        value.text_attachment_direction = acadrust::objects::TextAttachmentDirectionType::Vertical;
        value.unknown_flag_298 = true;
    }
    let bytes = DxfWriter::new(&doc).write_to_vec().expect("DXF write");
    let wire = String::from_utf8(bytes.clone()).expect("UTF-8 DXF");
    assert!(wire
        .lines()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|lines| lines[0].trim() == "3" && lines[1] == "Round-tripped description"));
    let round_trip = DxfReader::from_reader(Cursor::new(bytes))
        .expect("DXF reader init")
        .read()
        .expect("DXF read");
    let value = style(&round_trip);
    assert_eq!(value.description, "Round-tripped description");
    assert_eq!(value.name, "Round-tripped description");
    assert_eq!(value.default_text, "Default MText contents");
    assert_eq!(value.text_top_attachment as i16, 9);
    assert_eq!(value.text_bottom_attachment as i16, 10);
    assert_eq!(value.text_attachment_direction as i16, 1);
    assert!(value.unknown_flag_298);
}

#[test]
fn source_specific_style_projection_preserves_native_rotation_links_and_attachments() {
    let doc = read_style(
        "141\n90\n341\nABC\n343\nDEF\n91\n-1023440896\n93\n-1040154560\n94\n-1039526882\n",
    );
    let p = style_props(&doc, "dxf");
    assert_eq!(p["BlockContentRotation"], std::f64::consts::FRAC_PI_2);
    assert_eq!(p["Arrowhead"], Value::Null);
    assert_eq!(p["BlockContent"], Value::Null);
    for color in ["LineColor", "TextColor", "BlockContentColor"] {
        assert_eq!(p[color]["Index"], 0);
    }
    assert_eq!(style(&doc).block_content_rotation, 90.0);
    assert_eq!(style(&doc).arrowhead_handle, Some(Handle::new(0xABC)));
    let mut doc = CadDocument::new();
    let h = style(&doc).handle;
    if let Some(ObjectType::MultiLeaderStyle(v)) = doc.objects.get_mut(&h) {
        v.text_top_attachment = TextAttachmentType::CenterOfText;
        v.text_bottom_attachment = TextAttachmentType::CenterOfTextOverline;
    }
    doc.dwg_source_version = Some(DxfVersion::AC1032);
    let p = style_props(&doc, "dwg");
    assert_eq!(p["TextTopAttachment"], 10);
    assert_eq!(p["TextBottomAttachment"], 9);
    assert_eq!(style(&doc).text_top_attachment as i16, 9);
}
