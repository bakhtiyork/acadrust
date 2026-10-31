// SPDX-License-Identifier: MPL-2.0
//! Source-aware MText projection. No font metrics or layout engine is involved.

use super::*;
use crate::entities::{MText, MTextColumnData};

fn background_color(c: Color) -> Value {
    let mut result = color(c);
    if let Color::Rgb { r, g, b } = c {
        // ACadSharp Color.TrueColor packs bytes R,G,B in little-endian order.
        result["TrueColor"] = json!(i32::from(r) | (i32::from(g) << 8) | (i32::from(b) << 16));
    }
    result
}

fn background_transparency(raw: i32, dxf: bool, present: bool) -> Value {
    let value = if !present {
        0
    } else if dxf {
        // DxfProperty applies Transparency.FromAlphaValue, including its reserved 100.
        let alpha = (100.0 - (raw as u8 as f64 / 255.0) * 100.0) as i16;
        if alpha == 100 {
            100
        } else {
            alpha.clamp(0, 90)
        }
    } else {
        raw as i16
    };
    props(
        "Transparency",
        [
            ("Value", json!(value)),
            ("IsByBlock", json!(value == 100)),
            ("IsByLayer", json!(value == -1)),
        ],
    )
}

pub(super) fn properties(doc: &CadDocument, text: &MText) -> Vec<(&'static str, Value)> {
    let dwg = doc.dwg_source_version;
    let dxf = text.dxf_source.as_ref();
    let from_file = dwg.is_some() || dxf.is_some();
    let direction = dxf
        .map(|s| s.alignment_point)
        .or(text.dwg_x_direction)
        .unwrap_or_else(|| Vector3::new(text.rotation.cos(), text.rotation.sin(), 0.0));
    let rotation = direction.y.atan2(direction.x);
    let spacing = if let Some(source) = dxf {
        source.line_spacing_style.unwrap_or(0)
    } else if dwg.is_some_and(|v| v < DxfVersion::AC1015) {
        0
    } else {
        // ACadSharp keeps the word of the record as it is, also when it is no style (0).
        text.source_line_spacing_style.0.unwrap_or(text.line_spacing_style as i16)
    };
    let recovered = text.background_from_xdata;
    let fill_present = !recovered
        && dxf.map(|s| s.has_background_color).unwrap_or_else(|| {
            dwg.is_none()
                || text.background_fill_flags & 1 != 0
                || (dwg.is_some_and(|v| v >= DxfVersion::AC1032)
                    && text.background_fill_flags & 16 != 0)
        });
    let default_columns = MTextColumnData::default();
    // The oracle reads the embedded object, but ignores the older 75/76/... layout.
    // A DWG before R2018 states the columns in the block `ACAD_MTEXT_COLUMN_INFO` of the extended data, which the oracle keeps as
    // data and reads no columns from (the native model holds the columns the reader took out of the block).
    let columns = if dxf.is_some_and(|s| !s.has_embedded_columns) || dwg.is_some_and(|v| v < DxfVersion::AC1032) {
        &default_columns
    } else {
        &text.column_data
    };
    let column_data = props(
        "MText.TextColumnData",
        [
            ("AutoHeight", json!(columns.auto_height)),
            (
                "ColumnCount",
                json!(if dwg.is_some() {
                    0
                } else {
                    columns.column_count
                }),
            ),
            ("ColumnType", json!(columns.column_type)),
            ("FlowReversed", json!(columns.flow_reversed)),
            ("Gutter", number(columns.gutter)),
            (
                "Heights",
                Value::Array(columns.heights.iter().copied().map(number).collect()),
            ),
            ("Width", number(columns.width)),
        ],
    );
    let value = super::view::mtext_value(doc, text);
    vec![
        ("AlignmentPoint", vec3(direction)),
        ("AttachmentPoint", json!(text.attachment_point as i16)),
        (
            "BackgroundColor",
            background_color(if fill_present {
                text.background_color
            } else {
                Color::ByBlock
            }),
        ),
        (
            "BackgroundFillFlags",
            json!(if recovered {
                0
            } else {
                text.background_fill_flags
            }),
        ),
        (
            "BackgroundScale",
            number(if recovered {
                1.5
            } else {
                text.background_scale
            }),
        ),
        (
            "BackgroundTransparency",
            background_transparency(
                text.background_transparency,
                dxf.is_some(),
                !recovered
                    && dxf
                        .map(|s| s.has_background_transparency)
                        .unwrap_or(fill_present),
            ),
        ),
        ("ColumnData", column_data),
        ("DrawingDirection", json!(text.drawing_direction as i16)),
        ("HasColumns", json!(columns.column_type != 0)),
        ("Height", number(text.height)),
        (
            "HorizontalWidth",
            number(if from_file { 0.9 } else { text.extents_width }),
        ),
        ("InsertPoint", vec3(text.insertion_point)),
        ("IsAnnotative", json!(text.is_annotative)),
        ("LineSpacing", number(text.line_spacing_factor)),
        ("LineSpacingStyle", json!(spacing)),
        ("Normal", vec3(text.normal)),
        (
            "PlainText",
            json!(super::mtext_plain_text::plain_text(&value)),
        ),
        (
            "RectangleHeight",
            number(text.rectangle_height.unwrap_or(0.0)),
        ),
        ("RectangleWidth", number(text.rectangle_width)),
        ("Rotation", number(rotation)),
        (
            "Style",
            optional_handle(doc.text_styles.get(&text.style).map(|style| style.handle)),
        ),
        ("Value", json!(value)),
        (
            "VerticalHeight",
            number(if from_file { 0.2 } else { text.extents_height }),
        ),
    ]
}
