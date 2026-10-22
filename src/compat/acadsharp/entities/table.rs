// SPDX-License-Identifier: MPL-2.0
//! INSERT-derived tables and their stored nested values. Reader quirks live
//! here, separately from native table layout and effective-style calculation.

use super::super::objects::{table_component_default, table_component_styles, table_content_color};
use super::*;
use crate::entities::table::{
    CellBorder, CellContent, CellStyle, CellValue, CellValueType, Table, TableCell, TableCustomData,
};
use crate::objects::ObjectType;

#[derive(Clone, Copy)]
struct Source {
    modern: bool,
    dxf: bool,
    value_metadata: bool,
}

impl Source {
    fn new(doc: &CadDocument, table: &Table) -> Self {
        let dxf = table.dxf_insert_scale.is_some();
        Self {
            modern: !dxf && doc.version >= DxfVersion::AC1024,
            dxf,
            value_metadata: doc.version >= DxfVersion::AC1021,
        }
    }
}

pub(super) fn collect(doc: &CadDocument, table: &Table, out: &mut Vec<Value>) {
    let source = Source::new(doc, table);
    let mut fields = content_fields(doc, table);
    let scale = table.dxf_insert_scale.unwrap_or(table.insert_scale);
    let (columns, rows, column_spacing, row_spacing) =
        table.dxf_insert_array.unwrap_or((1, 1, 0.0, 0.0));
    fields.extend([
        ("Attributes", json!([])),
        ("HasAttributes", json!(false)),
        (
            "Block",
            optional_handle(
                doc.block_records
                    .get(&table.block_name)
                    .map(|b| b.handle)
                    .or_else(|| if source.dxf { None } else { table.block_record_handle }),
            ),
        ),
        // These are INSERT array dimensions, not the table grid dimensions.
        ("ColumnCount", integer(columns)),
        ("RowCount", integer(rows)),
        ("ColumnSpacing", number(column_spacing)),
        ("RowSpacing", number(row_spacing)),
        ("IsMultiple", json!(columns > 1 || rows > 1)),
        ("InsertPoint", vec3(table.insertion_point)),
        ("Normal", vec3(table.normal)),
        ("HorizontalDirection", vec3(table.horizontal_direction)),
        (
            "Rotation",
            number(table.dxf_insert_rotation.unwrap_or(table.insert_rotation)),
        ),
        ("XScale", number(scale.x)),
        ("YScale", number(scale.y)),
        ("ZScale", number(scale.z)),
        ("SpatialFilter", spatial_filter(doc, table)),
        (
            "Version",
            integer(if source.dxf {
                table.data_version as i64
            } else {
                0
            }),
        ),
        ("ValueFlag", integer(table.value_flags as i64)),
        // DWG consumes the legacy override blocks without assigning these
        // DXF-facing booleans.
        ("OverrideFlag", json!(source.dxf && table.override_flag)),
        (
            "OverrideBorderColor",
            json!(source.dxf && table.override_border_color),
        ),
        (
            "OverrideBorderLineWeight",
            json!(source.dxf && table.override_border_line_weight),
        ),
        (
            "OverrideBorderVisibility",
            json!(source.dxf && table.override_border_visibility),
        ),
        ("HasBreadData", json!(table.has_break_data)),
        (
            "BreakData",
            props(
                "TableEntity.TableBreakData",
                [
                    ("Flags", integer(table.break_options.bits() as i64)),
                    (
                        "FlowDirection",
                        integer(if table.has_break_data {
                            table
                                .source_break_flow_direction
                                .unwrap_or(table.break_flow_direction as i32)
                                as i64
                        } else {
                            0
                        }),
                    ),
                    ("BreakSpacing", number(table.break_spacing)),
                    (
                        "Heights",
                        array(table.break_data.iter().map(|b| {
                            props(
                                "TableEntity.TableBreakData.BreakHeight",
                                [("Height", number(b.height)), ("Position", vec3(b.position))],
                            )
                        })),
                    ),
                ],
            ),
        ),
    ]);
    super::field_common(
        doc,
        &table.common,
        "TableEntity",
        "ACAD_TABLE",
        -999,
        "AcDbTable",
        true,
        fields,
        out,
    );
}

fn spatial_filter(doc: &CadDocument, table: &Table) -> Value {
    let h = table
        .common
        .xdictionary_handle
        .or_else(|| doc.extension_dictionary_handle(table.common.handle));
    let dictionary = |h| match doc.objects.get(&h) {
        Some(ObjectType::Dictionary(d)) => Some(d),
        _ => None,
    };
    let entry = |h, name: &str| {
        dictionary(h)?
            .entries
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, h)| *h)
    };
    let result = h
        .and_then(|h| entry(h, "ACAD_FILTER"))
        .and_then(|h| entry(h, "SPATIAL"))
        .filter(|h| matches!(doc.objects.get(h), Some(ObjectType::SpatialFilter(_))));
    optional_handle(result)
}

pub(in super::super) fn content_fields(
    doc: &CadDocument,
    table: &Table,
) -> Vec<(&'static str, Value)> {
    let source = Source::new(doc, table);
    let styles = table_component_styles(doc, table);
    let inherited = |id| {
        let key = match id {
            1 => "TitleCellStyle",
            2 => "HeaderCellStyle",
            3 => "DataCellStyle",
            _ => return Value::Null,
        };
        styles.as_ref().map_or(Value::Null, |s| s[key].clone())
    };
    // A table a DXF reader completed from its TABLECONTENT object is projected as the oracle's DXF reader sees it:
    // from the legacy entity only.
    let legacy = table.dxf_legacy.0.as_deref().filter(|_| source.dxf);
    let base_style = legacy.map_or(table.base_style.as_ref(), |l| l.base_style.as_ref());
    let base_style_type = legacy.map_or(table.base_style_type, |l| l.base_style_type);
    let table_columns = legacy.map_or(&table.columns, |l| &l.columns);
    let table_rows = legacy.map_or(&table.rows, |l| &l.rows);
    vec![
        (
            "Style",
            handle(table.table_style_handle.filter(|h| matches!(
                doc.objects.get(h), Some(ObjectType::TableStyle(_))
            )).unwrap_or(Handle::NULL)),
        ),
        (
            "CellStyleOverride",
            style(base_style, base_style_type, source, true),
        ),
        (
            "Columns",
            array(table_columns.iter().map(|c| {
                props(
                    "TableEntity.Column",
                    [
                        ("Name", json!(c.name)),
                        ("Width", number(c.width)),
                        ("CustomData", integer(c.custom_data)),
                        (
                            "CustomDataCollection",
                            custom_data(&c.custom_data_items, source),
                        ),
                        (
                            "CellStyle",
                            if source.modern {
                                inherited(c.style_id)
                            } else {
                                Value::Null
                            },
                        ),
                        (
                            "CellStyleOverride",
                            style(c.style.as_ref(), c.source_style_type, source, true),
                        ),
                    ],
                )
            })),
        ),
        (
            "Rows",
            array(table_rows.iter().map(|r| {
                props(
                    "TableEntity.Row",
                    [
                        ("Height", number(r.height)),
                        ("CustomData", integer(r.custom_data)),
                        (
                            "CustomDataCollection",
                            custom_data(&r.custom_data_items, source),
                        ),
                        (
                            "CellStyle",
                            if source.modern {
                                inherited(r.style_id)
                            } else {
                                Value::Null
                            },
                        ),
                        (
                            "CellStyleOverride",
                            style(r.style.as_ref(), r.source_style_type, source, true),
                        ),
                        ("Cells", array(r.cells.iter().map(|c| cell(c, source)))),
                    ],
                )
            })),
        ),
        (
            "MergedCellRanges",
            if source.modern {
                array(table.merged_ranges.iter().map(|r| {
                    props(
                        "TableEntity.CellRange",
                        [
                            ("TopRowIndex", integer(r.top_row as i64)),
                            ("BottomRowIndex", integer(r.bottom_row as i64)),
                            ("LeftColumnIndex", integer(r.left_col as i64)),
                            ("RightColumnIndex", integer(r.right_col as i64)),
                        ],
                    )
                }))
            } else {
                json!([])
            },
        ),
    ]
}

fn custom_data(values: &[TableCustomData], source: Source) -> Value {
    array(values.iter().map(|v| {
        props(
            "TableEntity.CustomDataEntry",
            [
                ("Name", json!(v.name)),
                (
                    "Value",
                    cad_value(&v.value, source.value_metadata, source.dxf),
                ),
            ],
        )
    }))
}

fn cell(c: &TableCell, source: Source) -> Value {
    props(
        "TableEntity.Cell",
        [
            ("AutoFit", json!(c.auto_fit)),
            (
                "BlockScale",
                number(
                    if !source.modern
                        && !source.dxf
                        && c.cell_type == crate::entities::CellType::Block
                    {
                        c.block_scale
                    } else {
                        0.0
                    },
                ),
            ),
            // The native merged-range cache populates these for modern tables;
            // ACadSharp only assigns them in the legacy cell reader.
            (
                "BorderHeight",
                integer(if source.modern {
                    0
                } else {
                    c.merge_height as i64
                }),
            ),
            (
                "BorderWidth",
                integer(if source.modern {
                    0
                } else {
                    c.merge_width as i64
                }),
            ),
            (
                "Contents",
                array(
                    c.contents
                        .iter()
                        .filter(|v| {
                            source.modern
                                || v.content_type != crate::entities::TableCellContentType::Block
                        })
                        .map(|v| content(v, source)),
                ),
            ),
            ("CustomData", integer(c.custom_data)),
            (
                "CustomDataCollection",
                custom_data(&c.custom_data_items, source),
            ),
            ("EdgeFlags", integer(c.edge_flags)),
            (
                "Geometry",
                c.geometry.as_ref().map_or(Value::Null, |g| {
                    props(
                        "TableEntity.CellContentGeometry",
                        [
                            // Native width/height are the inner dimensions; the reference
                            // names the first serialized pair ContentWidth/ContentHeight.
                            ("ContentWidth", number(g.width)),
                            ("ContentHeight", number(g.height)),
                            ("Width", number(g.outer_width)),
                            ("Height", number(g.outer_height)),
                            ("DistanceCenter", vec3(g.distance_to_center)),
                            ("DistanceTopLeft", vec3(g.distance_to_top_left)),
                            ("Flags", integer(g.flags)),
                        ],
                    )
                }),
            ),
            ("HasLinkedData", json!(c.has_linked_data)),
            (
                "MergedValue",
                integer(if source.modern { 0 } else { c.merged as i64 }),
            ),
            ("Rotation", number(c.rotation)),
            ("StateFlags", integer(c.state.bits() as i64)),
            ("Style", Value::Null),
            (
                "StyleOverride",
                style(c.style.as_ref(), c.source_style_type, source, false),
            ),
            (
                "ToolTip",
                if source.modern {
                    json!(c.tooltip)
                } else {
                    Value::Null
                },
            ),
            (
                "Type",
                integer(if source.modern { 0 } else { c.cell_type as i64 }),
            ),
            ("VirtualEdgeFlag", integer(c.virtual_edge)),
        ],
    )
}

fn content(c: &CellContent, source: Source) -> Value {
    let typed = source.modern;
    let read_value = !typed || c.content_type == crate::entities::TableCellContentType::Value;
    let has_format = typed && c.format_has_data;
    let mut format = props(
        "TableStyle.ContentFormat",
        [
            ("Alignment", integer(0)),
            ("Color", color(crate::types::Color::ByBlock)),
            ("HasData", json!(has_format)),
            ("PropertyFlags", integer(0)),
            ("PropertyOverrideFlags", integer(0)),
            ("Rotation", number(0.0)),
            ("Scale", number(0.0)),
            ("TextHeight", number(0.0)),
            ("TextStyle", handle(Handle::NULL)),
            ("ValueDataType", integer(0)),
            ("ValueFormatString", Value::Null),
            ("ValueUnitType", integer(0)),
        ],
    );
    if has_format {
        for (k, v) in [
            ("Alignment", integer(c.alignment)),
            ("Color", table_content_color(c.color)),
            ("PropertyFlags", integer(c.format_property_flags)),
            ("PropertyOverrideFlags", integer(c.format_override_flags)),
            ("Rotation", number(c.rotation)),
            ("Scale", number(c.scale)),
            ("TextHeight", number(c.text_height)),
            ("ValueDataType", integer(c.format_value_data_type)),
            ("ValueUnitType", integer(c.format_value_unit_type)),
            ("ValueFormatString", json!(c.value_format)),
        ] {
            format[k] = v;
        }
        // The DWG content-format template is never attached to its parent;
        // its TextStyle remains the unattached default (#0).
    }
    props(
        "TableEntity.CellContent",
        [
            (
                "CadValue",
                if read_value {
                    cad_value(&c.value, source.value_metadata, source.dxf)
                } else {
                    cad_value(&CellValue::new(), false, false)
                },
            ),
            (
                "ContentType",
                integer(if typed { c.content_type as i64 } else { 0 }),
            ),
            ("Format", format),
        ],
    )
}

fn cad_value(v: &CellValue, metadata: bool, dxf: bool) -> Value {
    let value = if let Some(long) = v.dxf_long.filter(|_| dxf && v.value_type == CellValueType::Unknown) {
        // readCadValue stores group 91 whatever the 93 flags say.
        integer(long as i64)
    } else if v.flags & 1 != 0 {
        Value::Null
    } else {
        match v.value_type {
            CellValueType::Long => integer(v.numeric_value as i64),
            CellValueType::Double => number(v.numeric_value),
            CellValueType::String | CellValueType::General => json!(v.text),
            CellValueType::Point2D => json!([v.point_value.x, v.point_value.y]),
            CellValueType::Point3D => vec3(v.point_value),
            CellValueType::Date if !dxf => date_value(&v.binary_value),
            // Neither table-cell builder wires the returned value template.
            CellValueType::Handle | CellValueType::Date => Value::Null,
            CellValueType::Unknown if metadata => integer(v.numeric_value as i64),
            _ => Value::Null,
        }
    };
    props(
        "CadValue",
        [
            ("Flags", integer(v.flags)),
            ("ValueType", integer(v.raw_type_code)),
            ("Units", integer(v.raw_unit_type_code)),
            (
                "Format",
                if metadata {
                    json!(v.format)
                } else {
                    Value::Null
                },
            ),
            (
                "FormattedValue",
                if metadata {
                    json!(v.formatted_value)
                } else {
                    Value::Null
                },
            ),
            ("Value", value),
        ],
    )
}

pub(in crate::compat::acadsharp) fn date_value(bytes: &[u8]) -> Value {
    if !matches!(bytes.len(), 14 | 16) {
        return Value::Null;
    }
    let word = |i: usize| i32::from(i16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]));
    let (year, month) = (word(0), word(1));
    let i = if bytes.len() == 16 { 3 } else { 2 };
    let day = word(i);
    if !(1..=9999).contains(&year) || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Value::Null;
    }
    let a = (14 - month) / 12;
    let y = year + 4800 - a;
    let m = month + 12 * a - 3;
    let julian = day + (153 * m + 2) / 5 + 365 * y + y / 4 - y / 100 + y / 400 - 32045;
    let millis = ((word(i + 1) * 60 + word(i + 2)) * 60 + word(i + 3)) * 1000 + word(i + 4);
    number(julian as f64 + millis as f64 / 86_400_000.0)
}

fn style(
    v: Option<&CellStyle>,
    source_type: Option<i32>,
    source: Source,
    resolve_text: bool,
) -> Value {
    let mut result = table_component_default();
    if source.modern {
        result["Type"] =
            integer(source_type.unwrap_or_else(|| v.map_or(0, |s| s.style_type as i32)));
    }
    let Some(v) = v else {
        return result;
    };
    if source.modern {
        for (key, value) in [
            ("HasData", json!(true)),
            ("Alignment", integer(v.alignment)),
            ("BackgroundColor", table_content_color(v.background_color)),
            ("Color", table_content_color(v.content_color)),
            ("ContentLayoutFlags", integer(v.layout_flags.bits() as i64)),
            ("MarginOverrideFlags", integer(v.margin_override_flags)),
            ("PropertyFlags", integer(v.content_property_flags)),
            (
                "PropertyOverrideFlags",
                integer(v.content_format_override_flags),
            ),
            ("Rotation", number(v.rotation)),
            ("Scale", number(v.scale)),
            (
                "TableCellStylePropertyFlags",
                integer(v.property_flags.bits() as i64),
            ),
            ("TextHeight", number(v.text_height)),
            ("ValueDataType", integer(v.value_data_type)),
            ("ValueFormatString", json!(v.value_format)),
            ("ValueUnitType", integer(v.value_unit_type)),
        ] {
            result[key] = value;
        }
        if resolve_text {
            result["TextStyle"] = handle(v.text_style_handle.unwrap_or(Handle::NULL));
        }
        if v.margin_override_flags & 1 != 0 {
            for (key, value) in [
                ("VerticalMargin", v.margin_top),
                ("HorizontalMargin", v.margin_left),
                ("BottomMargin", v.margin_bottom),
                ("RightMargin", v.margin_right),
                ("MarginHorizontalSpacing", v.horizontal_spacing),
                ("MarginVerticalSpacing", v.vertical_spacing),
            ] {
                result[key] = number(value);
            }
        }
        for (key, mask, border) in [
            ("TopBorder", 1, &v.top_border),
            ("RightBorder", 2, &v.right_border),
            ("BottomBorder", 4, &v.bottom_border),
            ("LeftBorder", 8, &v.left_border),
        ] {
            if v.applied_border_edges.bits() & mask != 0 {
                modern_border(&mut result[key], border);
            }
        }
        for (mask, border) in &v.additional_borders {
            let key = match mask {
                16 => "VerticalInsideBorder",
                32 => "HorizontalInsideBorder",
                _ => continue,
            };
            modern_border(&mut result[key], border);
        }
    } else {
        let flags = v.override_flags;
        if !source.dxf && flags & 1 != 0 {
            result["CellAlignment"] = integer(v.alignment);
        }
        if flags & 2 != 0 {
            result["IsFillColorOn"] = json!(!v.fill_enabled);
        }
        if flags & 4 != 0 {
            result["BackgroundColor"] = table_content_color(v.background_color);
        }
        if flags & 8 != 0 {
            result["ContentColor"] = table_content_color(v.content_color);
        }
        if !source.dxf && flags & 0x20 != 0 {
            result["TextHeight"] = number(v.text_height);
        }
        for (key, color_bit, weight_bit, border) in [
            ("TopBorder", 0x40, 0x400, &v.top_border),
            ("RightBorder", 0x80, 0x800, &v.right_border),
            ("BottomBorder", 0x100, 0x1000, &v.bottom_border),
            ("LeftBorder", 0x200, 0x2000, &v.left_border),
        ] {
            if flags & color_bit != 0 {
                result[key]["Color"] = table_content_color(border.color);
            }
            if flags & weight_bit != 0 {
                result[key]["LineWeight"] = integer(border.line_weight.value());
                if !source.dxf {
                    result[key]["IsInvisible"] = json!(border.invisible);
                }
            }
        }
    }
    result
}

fn modern_border(result: &mut Value, b: &CellBorder) {
    for (k, v) in [
        ("ApplyBorder", json!(true)),
        ("Color", table_content_color(b.color)),
        ("DoubleLineSpacing", number(b.double_spacing)),
        ("IsInvisible", json!(b.invisible)),
        ("LineWeight", integer(b.line_weight.value())),
        (
            "PropertyOverrideFlags",
            integer(b.override_flags.bits() as i64),
        ),
        ("Type", integer(b.border_type as i64)),
    ] {
        result[k] = v;
    }
}

#[cfg(test)]
mod r4_c1_tests {
    use super::*;

    #[test]
    fn dxf_group_91_is_the_value_of_an_unknown_typed_cell_whatever_the_flags() {
        let mut value = CellValue::new();
        value.flags = 3;
        value.dxf_long = Some(0);
        assert_eq!(cad_value(&value, false, true)["Value"], 0);
        value.dxf_long = Some(7);
        assert_eq!(cad_value(&value, false, true)["Value"], 7);
        // Without a group 91 the reference never sets a value.
        value.dxf_long = None;
        assert_eq!(cad_value(&value, false, true)["Value"], Value::Null);
        // A flagged DWG value stays empty.
        value.dxf_long = Some(7);
        assert_eq!(cad_value(&value, true, false)["Value"], Value::Null);
    }
}
