// SPDX-License-Identifier: MPL-2.0
//! ACadSharp-view fixes found by the DXF round-trip gate: values the reference reader derives differently when
//! a DXF omits groups its own writer leaves out, or carries a name with several `$`.
use super::dump_reader_document;
use crate::objects::{BookColor, ImageDefinition, Layout, ObjectType};
use crate::types::Handle;
use crate::CadDocument;
use serde_json::Value;

fn find<'a>(dump: &'a Value, h: u64) -> &'a Value {
    dump["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["handle"] == format!("#{h:X}"))
        .unwrap_or_else(|| panic!("missing object #{h:X}"))
}

#[test]
fn image_definition_default_size_is_the_reference_constructor_value_when_a_dxf_omits_it() {
    let mut doc = CadDocument::new();
    let mut image = ImageDefinition::new("image.jpg");
    image.handle = Handle::new(0x7500);
    doc.objects.insert(image.handle, ObjectType::ImageDefinition(image));

    let dxf = dump_reader_document(&doc, "x.dxf", "dxf");
    assert_eq!(find(&dxf, 0x7500)["props"]["DefaultSize"], serde_json::json!([1.0, 1.0]));
    let dwg = dump_reader_document(&doc, "x.dwg", "dwg");
    assert_eq!(find(&dwg, 0x7500)["props"]["DefaultSize"], serde_json::json!([0.0, 0.0]));
}

#[test]
fn book_color_name_with_several_dollars_keeps_the_first_and_last_segments() {
    let mut doc = CadDocument::new();
    let mut color = BookColor::default();
    color.handle = Handle::new(0x7501);
    color.book_name = "RAL".to_string();
    color.color_name = "1006$RAL".to_string();
    doc.objects.insert(color.handle, ObjectType::BookColor(color));

    let dump = dump_reader_document(&doc, "x.dxf", "dxf");
    let props = &find(&dump, 0x7501)["props"];
    assert_eq!(props["ColorName"], "RAL");
    assert_eq!(props["BookName"], "RAL");
    assert_eq!(props["Name"], "RAL$RAL");
}

#[test]
fn layout_flags_fall_back_to_the_plot_settings_word_when_the_layout_subclass_has_no_group_70() {
    let mut doc = CadDocument::new();
    let mut layout = Layout::new("Layout1");
    layout.handle = Handle::new(0x7502);
    layout.raw_plot_settings_codes = Some(vec![(70, "688".to_string())]);
    layout.flags = 0;
    doc.objects.insert(layout.handle, ObjectType::Layout(layout.clone()));
    layout.handle = Handle::new(0x7503);
    layout.flags = 1;
    layout.dxf_layout_flags_present = true;
    doc.objects.insert(layout.handle, ObjectType::Layout(layout));

    let dump = dump_reader_document(&doc, "x.dxf", "dxf");
    // One property for the reference reader: ACadSharp's own writer omits the layout's group 70, so the
    // PlotSettings value is the last one assigned.
    assert_eq!(find(&dump, 0x7502)["props"]["LayoutFlags"], 688);
    assert_eq!(find(&dump, 0x7503)["props"]["LayoutFlags"], 1);
}

#[test]
fn dxf_dimension_style_flags_and_layout_shade_plot_mode_follow_the_reference_reader() {
    use crate::tables::DimStyle;
    let mut doc = CadDocument::new();
    let mut style = DimStyle::dxf_defaults("FROMDXF");
    style.dxf_present_codes = Some([0; 6]);
    style.xref_reference = true;
    style.handle = Handle::new(0x7504);
    doc.dim_styles.add(style).unwrap();
    let mut layout = Layout::new("Layout1");
    layout.handle = Handle::new(0x7505);
    layout.shade_plot_mode = 1;
    doc.objects.insert(layout.handle, ObjectType::Layout(layout));

    let dxf = dump_reader_document(&doc, "x.dxf", "dxf");
    assert_eq!(find(&dxf, 0x7504)["props"]["Flags"], 0);
    assert_eq!(find(&dxf, 0x7505)["props"]["ShadePlotMode"], 0);
    let dwg = dump_reader_document(&doc, "x.dwg", "dwg");
    assert_eq!(find(&dwg, 0x7505)["props"]["ShadePlotMode"], 1);
}
