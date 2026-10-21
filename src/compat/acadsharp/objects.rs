//! ACadSharp-compatible dumps for the non-graphical objects in a document.
//!
//! The reader intentionally keeps a small, typed subset of ACadSharp's very
//! large object model.  This module maps that subset field by field and lets
//! the schema writer fill all fields that acadrust cannot represent with
//! `!unmapped`.  Keeping the mapping here (rather than adding dump-specific
//! serde derives to the model) also keeps the normal crate API unchanged.

use crate::objects::{
    BlockEvalValue, Dictionary, DictionaryWithDefault, DynamicBlockData, DynamicBlockObject,
    Group, ImageDefinition, ImageDefinitionReactor, Layout, MLineStyle, Material,
    MultiLeaderPathType, MultiLeaderStyle, ObjectContextData, ObjectType, PlotSettings, Scale,
    SortEntitiesTable, TableStyle, TextAttachmentType, VisualStyle, XRecord,
};
use crate::tables::TableEntry;
use crate::types::{Color, DxfVersion, Handle, Matrix4, Vector2, Vector3};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

use super::{color, handle, number, optional_handle, props, record, vec2, vec3};

#[path = "geo_spatial.rs"]
mod geo_spatial;
#[path = "dimension_association.rs"]
mod dimension_association;
mod dynamic_actions;
#[cfg(test)]
mod dynamic_tests;

const UNLISTED: i64 = -999;

/// Borrowed object recipes and a single layout index shared by both passes.
pub(super) struct Records<'a> {
    doc: &'a crate::CadDocument,
    reader: &'a str,
    layout_viewports: HashMap<Handle, Vec<Handle>>,
    sources: Vec<Source<'a>>,
}

enum Source<'a> {
    Object(Handle, &'a ObjectType),
    Visibility(Handle, &'a crate::objects::BlockVisibilityParameter),
    Field(Handle, &'a crate::document::FieldDef),
}

impl<'a> Records<'a> {
    pub(super) fn new(doc: &'a crate::CadDocument, reader: &'a str) -> Self {
        let layout_viewports: HashMap<_, Vec<_>> = doc
            .block_records
            .iter()
            .map(|block| {
                let viewports = super::tables::view_entity_handles(doc, block)
                    .iter()
                    .copied()
                    .filter(|h| {
                        matches!(
                            doc.get_entity(*h),
                            Some(crate::entities::EntityType::Viewport(_))
                        )
                    })
                    .collect();
                (block.handle, viewports)
            })
            .collect();
        let mut sources = Vec::new();
        let mut emitted = HashSet::new();
        let mut handles: Vec<_> = doc.objects.keys().copied().collect();
        handles.sort_unstable();
        for h in handles {
            sources.push(Source::Object(h, &doc.objects[&h]));
            emitted.insert(h);
        }
        let mut handles: Vec<_> = doc.block_visibility_params.keys().copied().collect();
        handles.sort_unstable();
        for h in handles {
            let value = &doc.block_visibility_params[&h];
            let h = if value.handle.is_null() {
                h
            } else {
                value.handle
            };
            if emitted.insert(h) {
                sources.push(Source::Visibility(h, value));
            }
        }
        let mut handles: Vec<_> = doc.fields.keys().copied().collect();
        handles.sort_unstable();
        for h in handles {
            let value = &doc.fields[&h];
            let h = if value.handle.is_null() {
                h
            } else {
                value.handle
            };
            if emitted.insert(h) {
                sources.push(Source::Field(h, value));
            }
        }
        Self {
            doc,
            reader,
            layout_viewports,
            sources,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.sources.len()
    }

    pub(super) fn render(&self, index: usize) -> Value {
        match self.sources[index] {
            Source::Object(h, object) => enrich_object_record(
                self.doc,
                h,
                Some(object),
                dump_object(self.doc, self.reader, h, object, &self.layout_viewports)
                    .expect("every native object has a canonical representation"),
            ),
            Source::Visibility(h, value) => enrich_object_record(
                self.doc,
                h,
                None,
                dump_visibility_parameter(self.doc, self.reader, h, value),
            ),
            Source::Field(h, value) => {
                enrich_object_record(self.doc, h, None, dump_field_definition(h, value))
            }
        }
    }
}

pub(super) fn collect(doc: &crate::CadDocument, reader: &str, out: &mut Vec<Value>) {
    let records = Records::new(doc, reader);
    out.extend((0..records.len()).map(|index| records.render(index)));
}

fn dump_object(
    _doc: &crate::CadDocument,
    reader: &str,
    map_handle: Handle,
    object: &ObjectType,
    layout_viewports: &HashMap<Handle, Vec<Handle>>,
) -> Option<Value> {
    Some(match object {
        ObjectType::Dictionary(value) => dump_dictionary(map_handle, value),
        ObjectType::DictionaryWithDefault(value) => dump_dictionary_with_default(map_handle, value),
        ObjectType::XRecord(value) => dump_xrecord(_doc, map_handle, value),
        ObjectType::Layout(value) => {
            let associated_viewports = layout_viewports
                .get(&value.block_record)
                .map(Vec::as_slice);
            dump_layout(
                _doc,
                reader,
                map_handle,
                value,
                associated_viewports,
            )
        }
        ObjectType::Group(value) => dump_group(map_handle, value),
        ObjectType::MLineStyle(value) => dump_mline_style(_doc, reader, map_handle, value),
        ObjectType::MultiLeaderStyle(value) => dump_mleader_style(_doc, map_handle, value),
        ObjectType::TableStyle(value) => dump_table_style(_doc, reader, map_handle, value),
        ObjectType::Scale(value) => dump_scale(map_handle, value),
        ObjectType::VisualStyle(value) => dump_visual_style(map_handle, value, reader),
        ObjectType::Material(value) => dump_material(_doc, reader, map_handle, value),
        ObjectType::ImageDefinition(value) => dump_image_definition(map_handle, value, reader),
        ObjectType::ImageDefinitionReactor(value) => {
            dump_image_definition_reactor(map_handle, value)
        }
        ObjectType::GeoData(value) => geo_spatial::dump_geo_data(_doc, map_handle, value, reader),
        ObjectType::SpatialFilter(value) => geo_spatial::dump_spatial_filter(map_handle, value, reader),
        ObjectType::RasterVariables(value) => dump_raster_variables(map_handle, value),
        ObjectType::BookColor(value) => dump_book_color(_doc, map_handle, value),
        ObjectType::PlaceHolder(value) => dump_placeholder(map_handle, value),
        ObjectType::DictionaryVariable(value) => dump_dictionary_variable(map_handle, value),
        ObjectType::SortEntitiesTable(value) => dump_sort_entities(map_handle, value),
        ObjectType::PlotSettings(value) => dump_plot_settings(_doc, reader, map_handle, value),
        ObjectType::WipeoutVariables(value) => dump_wipeout_variables(map_handle, value),
        ObjectType::Field(value) => dump_field(_doc, reader, map_handle, value),
        ObjectType::FieldList(value) => dump_field_list(map_handle, value),
        ObjectType::BlockVisibilityParameter(value) => dump_visibility_parameter(_doc, reader, map_handle, value),
        ObjectType::DynamicBlock(value) => dump_dynamic_block(_doc, reader, map_handle, value),
        ObjectType::ObjectContextData(value) => dump_object_context_data(map_handle, value),
        ObjectType::TableContent(value) => dump_table_content(_doc, map_handle, value),
        ObjectType::ProxyObject(value) => dump_proxy_object(_doc, map_handle, value),
        ObjectType::UnderlayDefinition(value) => dump_underlay_definition(map_handle, value),
        ObjectType::Associative(value) => match &value.data {
            crate::objects::AssociativeData::DimensionAssociation(data) => {
                dimension_association::dump_object(_doc, map_handle, value, data, reader)
            }
            _ => dump_opaque_named(
                map_handle,
                value.owner,
                &value.dxf_name,
                &value.cpp_class_name,
                &value.reactors,
                value.xdictionary_handle,
            ),
        },
        ObjectType::ClassObject(value) => dump_opaque_named(
            map_handle,
            value.owner,
            value.dxf_name(),
            value.dxf_name(),
            &value.reactors,
            value.xdictionary_handle,
        ),
        ObjectType::DataObject(value) => dump_data_object(map_handle, value),
        ObjectType::RegisteredClass(value) => dump_registered_class(map_handle, value),
        ObjectType::DgnLineStyle(value) => dump_dgn_line_style(map_handle, value),
        ObjectType::Unknown {
            type_name, owner, ..
        } => super::aec::object(_doc, map_handle, object)
            .unwrap_or_else(|| dump_unknown(map_handle, *owner, type_name)),
    })
}

fn dump_unknown(map_handle: Handle, owner: Handle, type_name: &str) -> Value {
    let class = class_name(type_name);
    typed_record(
        map_handle,
        &class,
        owner,
        common(type_name, UNLISTED, "", None, None, None),
    )
}

/// Use a stable proxy schema for open-ended application classes that have no
/// corresponding entry in the generated ACadSharp checklist.
fn typed_record(
    handle_value: Handle,
    class: &str,
    owner: Handle,
    mapped: Vec<(&'static str, Value)>,
) -> Value {
    let output_class = if super::schema().contains_key(class) {
        class
    } else {
        "ProxyObject"
    };
    record(
        handle_value,
        output_class,
        owner,
        props(output_class, mapped),
    )
}

fn dump_opaque_named(
    map_handle: Handle,
    owner: Handle,
    dxf_name: &str,
    cpp_name: &str,
    reactors: &[Handle],
    xdictionary_handle: Option<Handle>,
) -> Value {
    let class = class_name(cpp_name).to_string();
    let object_name = if dxf_name.is_empty() {
        class.as_str()
    } else {
        dxf_name
    };
    typed_record(
        map_handle,
        &class,
        owner,
        common(
            object_name,
            UNLISTED,
            cpp_name,
            None,
            Some(reactors),
            xdictionary_handle,
        ),
    )
}

fn common(
    object_name: &str,
    object_type: i64,
    subclass: &str,
    name: Option<&str>,
    reactors: Option<&[Handle]>,
    xdictionary_handle: Option<Handle>,
) -> Vec<(&'static str, Value)> {
    let mut values = vec![
        ("HasDynamicSubclass", Value::Bool(false)),
        ("ObjectName", Value::String(object_name.to_string())),
        ("ObjectType", int(object_type)),
        ("SubclassMarker", Value::String(subclass.to_string())),
    ];
    if let Some(name) = name {
        values.push(("Name", Value::String(name.to_string())));
    }
    if let Some(reactors) = reactors {
        values.push(("Reactors", handles(reactors.iter().copied())));
    }
    if xdictionary_handle.is_some() {
        values.push(("XDictionary", optional_handle(xdictionary_handle)));
    }
    values
}

fn int(value: i64) -> Value {
    Value::Number(value.into())
}

fn bool_value(value: bool) -> Value {
    Value::Bool(value)
}

fn text(value: impl Into<String>) -> Value {
    Value::String(value.into())
}

fn handles<I>(values: I) -> Value
where
    I: IntoIterator<Item = Handle>,
{
    Value::Array(values.into_iter().map(handle).collect())
}

fn numeric_handles<I>(values: I) -> Value
where
    I: IntoIterator<Item = Handle>,
{
    Value::Array(
        values
            .into_iter()
            .map(|value| Value::Number(value.value().into()))
            .collect(),
    )
}

fn strings<I>(values: I) -> Value
where
    I: IntoIterator,
    I::Item: Into<String>,
{
    Value::Array(values.into_iter().map(|value| text(value.into())).collect())
}

fn numbers<I>(values: I) -> Value
where
    I: IntoIterator<Item = f64>,
{
    Value::Array(values.into_iter().map(number).collect())
}

fn tuple2(value: (f64, f64)) -> Value {
    vec2(Vector2::new(value.0, value.1))
}

fn tuple3(value: (f64, f64, f64)) -> Value {
    vec3(Vector3::new(value.0, value.1, value.2))
}

fn class_name(cpp_name: &str) -> String {
    if let Some(name) = super::type_names::canonical(cpp_name) {
        return name.to_owned();
    }
    let mut value = cpp_name
        .strip_prefix("AcDb")
        .unwrap_or(cpp_name)
        .to_string();
    // These are the names used by ACadSharp's public classes, while the DWG
    // C++ class names retain Autodesk's historical spellings.
    for (from, to) in [
        ("EvalGraph", "EvaluationGraph"),
        ("Basepoint", "BasePoint"),
        ("BlockRotateAction", "BlockRotationAction"),
        ("BlockParamDependencyBody", "BlockParameterDependencyBody"),
        ("BlockGripExpr", "BlockGripLocationComponent"),
        ("Mline", "MLine"),
        ("Mleader", "MultiLeader"),
        ("DictionaryVar", "DictionaryVariable"),
        ("RasterImageDefReactor", "ImageDefinitionReactor"),
        ("RasterImageDef", "ImageDefinition"),
        ("ProxyObject", "ProxyObject"),
    ] {
        if value == from {
            value = to.to_string();
        }
    }
    value
}

fn dump_dictionary(handle_value: Handle, value: &Dictionary) -> Value {
    let mut p = common(
        "DICTIONARY",
        0x2A,
        "AcDbDictionary",
        None,
        Some(&value.reactors),
        value.xdictionary_handle,
    );
    p.extend([
        // A DXF dictionary without group 281 keeps ACadSharp's constructor
        // default (not applicable) rather than the native cloning default.
        (
            "ClonningFlags",
            int(if value.cloning_absent.0 {
                0
            } else {
                value.duplicate_cloning as i64
            }),
        ),
        (
            "EntryHandles",
            numeric_handles(value.entries.iter().map(|(_, h)| *h)),
        ),
        (
            "EntryNames",
            strings(value.entries.iter().map(|(name, _)| name)),
        ),
        ("HardOwnerFlag", bool_value(value.hard_owner)),
    ]);
    record(
        handle_value,
        "CadDictionary",
        value.owner,
        props("CadDictionary", p),
    )
}

fn dump_dictionary_with_default(handle_value: Handle, value: &DictionaryWithDefault) -> Value {
    let mut p = common(
        "ACDBDICTIONARYWDFLT",
        UNLISTED,
        "AcDbDictionaryWithDefault",
        None,
        None,
        None,
    );
    p.extend([
        ("ClonningFlags", int(value.duplicate_cloning as i64)),
        ("DefaultEntry", handle(value.default_handle)),
        (
            "EntryHandles",
            numeric_handles(value.entries.iter().map(|(_, h)| *h)),
        ),
        (
            "EntryNames",
            strings(value.entries.iter().map(|(name, _)| name)),
        ),
        ("HardOwnerFlag", bool_value(value.hard_owner)),
    ]);
    record(
        handle_value,
        "CadDictionaryWithDefault",
        value.owner,
        props("CadDictionaryWithDefault", p),
    )
}

fn dump_xrecord(doc: &crate::CadDocument, handle_value: Handle, value: &XRecord) -> Value {
    let mut p = common(
        "XRECORD",
        0x4F,
        "AcDbXrecord",
        Some(&value.name),
        Some(&value.reactors),
        value.xdictionary_handle,
    );
    p.push(("CloningFlags", int(value.cloning_flags.to_value() as i64)));
    p.push((
        "Entries",
        Value::Array(
            value
                .entries
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    let mut projected = xrecord_entry(doc, entry);
                    if let Some(source) = value.source_value(index) {
                        // The reference's XRecord reader exposes lexical
                        // strings; native decoded text remains editable.
                        projected["Value"] = text(source);
                    }
                    projected
                })
                .collect(),
        ),
    ));
    record(handle_value, "XRecord", value.owner, props("XRecord", p))
}

fn xrecord_entry(doc: &crate::CadDocument, entry: &crate::objects::XRecordEntry) -> Value {
    let value = match &entry.value {
        crate::objects::XRecordValue::String(value) => text(value),
        crate::objects::XRecordValue::Double(value) => number(*value),
        crate::objects::XRecordValue::Int16(value) => int(*value as i64),
        crate::objects::XRecordValue::Int32(value) => int(*value as i64),
        crate::objects::XRecordValue::Int64(value) => int(*value),
        crate::objects::XRecordValue::Byte(value) => int(*value as i64),
        crate::objects::XRecordValue::Bool(value) => bool_value(*value),
        crate::objects::XRecordValue::Handle(value) => xrecord_handle(doc, *value),
        crate::objects::XRecordValue::Point3D(x, y, z) => vec3(Vector3::new(*x, *y, *z)),
        crate::objects::XRecordValue::Chunk(value) => Value::String(hex_bytes(value)),
    };
    json!({"Code": entry.code, "Value": value})
}

fn xrecord_handle(doc: &crate::CadDocument, value: Handle) -> Value {
    if resolves_handle(doc, value) {
        handle(value)
    } else {
        json!(value.value())
    }
}

fn resolves_handle(doc: &crate::CadDocument, value: Handle) -> bool {
    if value.is_null() {
        return false;
    }
    if doc.objects.contains_key(&value)
        || doc.get_entity(value).is_some()
        || doc.block_visibility_params.contains_key(&value)
        || doc.fields.contains_key(&value)
        || doc.dgn_ls_definitions.contains_key(&value)
        || doc.dgn_ls_components.contains_key(&value)
    {
        return true;
    }

    value == doc.layers.handle()
        || value == doc.line_types.handle()
        || value == doc.text_styles.handle()
        || value == doc.block_records.handle()
        || value == doc.dim_styles.handle()
        || value == doc.app_ids.handle()
        || value == doc.views.handle()
        || value == doc.vports.handle()
        || value == doc.ucss.handle()
        || value == doc.vx_table.handle()
        || doc.layers.iter().any(|entry| entry.handle() == value)
        || doc.line_types.iter().any(|entry| entry.handle() == value)
        || doc.text_styles.iter().any(|entry| entry.handle() == value)
        || doc
            .block_records
            .iter()
            .any(|entry| entry.handle() == value)
        || doc.dim_styles.iter().any(|entry| entry.handle() == value)
        || doc.app_ids.iter().any(|entry| entry.handle() == value)
        || doc.views.iter().any(|entry| entry.handle() == value)
        || doc.vports.iter().any(|entry| entry.handle() == value)
        || doc.ucss.iter().any(|entry| entry.handle() == value)
        || doc.vx_table.iter().any(|entry| entry.handle() == value)
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(&mut value, "{byte:02x}");
    }
    value
}

fn valid_block_record(doc: &crate::CadDocument, value: Handle) -> Option<Handle> {
    (!value.is_null() && doc.block_records.iter().any(|block| block.handle == value))
        .then_some(value)
}

fn valid_ucs(doc: &crate::CadDocument, value: Handle) -> Option<Handle> {
    (!value.is_null() && doc.ucss.iter().any(|ucs| ucs.handle() == value)).then_some(value)
}

fn valid_viewport(doc: &crate::CadDocument, value: Handle) -> Option<Handle> {
    (!value.is_null()
        && matches!(
            doc.get_entity(value),
            Some(crate::entities::EntityType::Viewport(_))
        ))
    .then_some(value)
}

fn source_dxf(doc: &crate::CadDocument) -> bool {
    doc.dwg_source_version.is_none() && doc.header.dxf_source_variables.is_some()
}

fn raw_code_present(codes: &Option<Vec<(i32, String)>>, code: i32) -> bool {
    codes
        .as_ref()
        .is_some_and(|codes| codes.iter().any(|(value, _)| *value == code))
}

fn dxf_plot_code_present(value: &PlotSettings, code: i32) -> bool {
    value.dxf_code_present(code)
}

fn plot_string(value: &str, null_when_empty: bool) -> Value {
    if value.is_empty() && null_when_empty {
        Value::Null
    } else {
        text(value)
    }
}

fn dump_layout(
    doc: &crate::CadDocument,
    reader: &str,
    handle_value: Handle,
    value: &Layout,
    associated_viewports: Option<&[Handle]>,
) -> Value {
    let is_new = reader == "new";
    let dxf_source = doc.dwg_source_version.is_none()
        && (reader == "dxf" || source_dxf(doc) || value.raw_plot_settings_codes.is_some());
    let dwg_source = doc
        .dwg_source_version
        .or_else(|| (reader == "dwg").then_some(doc.version));
    let raw_plot_code = |code| raw_code_present(&value.raw_plot_settings_codes, code);
    let min_limits = if is_new && value.min_limits == (0.0, 0.0) {
        (-20.0, -7.5)
    } else {
        value.min_limits
    };
    let max_limits = if is_new && value.max_limits == (0.0, 0.0) {
        (277.0, 202.5)
    } else {
        value.max_limits
    };
    let min_extents = if is_new && value.min_extents == (0.0, 0.0, 0.0) {
        (25.7, 19.5, 0.0)
    } else {
        value.min_extents
    };
    let max_extents = if is_new && value.max_extents == (12.0, 9.0, 0.0) {
        (231.3, 175.5, 0.0)
    } else {
        value.max_extents
    };
    // DxfObjectsSectionReader.readLayout tries the Layout map before the
    // PlotSettings map, regardless of subclass. Codes 1 and 70 are consumed
    // as Name/LayoutFlags, leaving the inherited properties at their defaults.
    let flags = if is_new || dxf_source {
        688
    } else {
        value.plot_flags.to_bits() as i64
    };
    // Code 70 is one property to the reference reader, so a DXF whose AcDbLayout subclass has no group 70
    // (ACadSharp's own writer omits it) leaves the PlotSettings value as the layout flags.
    let layout_flags = if is_new && value.flags == 0 {
        1
    } else if dxf_source && !value.dxf_layout_flags_present {
        value
            .raw_plot_settings_codes
            .iter()
            .flatten()
            .rev()
            .find(|(code, _)| *code == 70)
            .and_then(|(_, text)| text.trim().parse::<i16>().ok())
            .map_or(value.flags as i64, i64::from)
    } else {
        value.flags as i64
    };
    let page_name = if is_new || dxf_source {
        "none_device"
    } else {
        value.plot_page_name.as_str()
    };
    let paper_size = if (is_new || (dxf_source && !raw_plot_code(4))) && value.paper_size.is_empty()
    {
        "ISO_A4_(210.00_x_297.00_MM)"
    } else {
        value.paper_size.as_str()
    };
    let paper_units =
        if (is_new || (dxf_source && !raw_plot_code(72))) && value.plot_paper_units == 0 {
            1
        } else {
            value.plot_paper_units
        };
    let plot_type = if (is_new || (dxf_source && !raw_plot_code(74))) && value.plot_type == 5 {
        1
    } else {
        value.plot_type
    };
    let plot_view_name_is_null = is_new
        || dwg_source.is_some_and(|version| version >= DxfVersion::AC1018)
        || (dxf_source && !raw_plot_code(6));
    let style_sheet_is_null = is_new || (dxf_source && !raw_plot_code(7));
    let printer_name_is_null = is_new || (dxf_source && !raw_plot_code(2));
    // These are three independent C# stored properties, not alias getters:
    // DWG assigns only the vector; DXF assigns only the two scalar properties.
    let (paper_image_origin, paper_image_origin_x, paper_image_origin_y) = if dxf_source {
        (
            Vector2::ZERO,
            value.paper_image_origin_x,
            value.paper_image_origin_y,
        )
    } else if dwg_source.is_some() {
        (
            Vector2::new(value.paper_image_origin_x, value.paper_image_origin_y),
            0.0,
            0.0,
        )
    } else {
        (
            Vector2::new(value.paper_image_origin_x, value.paper_image_origin_y),
            value.paper_image_origin_x,
            value.paper_image_origin_y,
        )
    };
    let paper_width = if (is_new || (dxf_source && !raw_plot_code(44))) && value.paper_width == 0.0
    {
        297.0
    } else {
        value.paper_width
    };
    let paper_height =
        if (is_new || (dxf_source && !raw_plot_code(45))) && value.paper_height == 0.0 {
            210.0
        } else {
            value.paper_height
        };
    let shade_plot_resolution = if is_new || (dxf_source && !raw_plot_code(77)) {
        0
    } else {
        value.shade_plot_resolution as i64
    };
    let shade_plot_dpi = if value.shade_plot_dpi <= 0
        && (is_new
            || (dxf_source && !raw_plot_code(78))
            || dwg_source.is_some_and(|version| version < DxfVersion::AC1018))
    {
        300
    } else {
        value.shade_plot_dpi as i64
    };
    let is_model = value.name.eq_ignore_ascii_case("Model");
    let paper_viewport = associated_viewports.and_then(|viewports| viewports.first().copied());

    let mut p = common(
        "LAYOUT",
        0x52,
        "AcDbLayout",
        Some(&value.name),
        Some(&value.reactors),
        value.xdictionary_handle,
    );
    p.extend([
        (
            "AssociatedBlock",
            optional_handle(valid_block_record(doc, value.block_record)),
        ),
        ("BaseUCS", optional_handle(valid_ucs(doc, value.base_ucs))),
        ("Elevation", number(value.elevation)),
        ("Flags", int(flags)),
        ("InsertionBasePoint", tuple3(value.insertion_base)),
        ("IsPaperSpace", bool_value(!is_model)),
        (
            "LastActiveViewport",
            // DXF fills LasActiveViewportHandle, but CadLayoutTemplate builds
            // only ActiveViewportHandle (the field populated by DWG).
            optional_handle(if dxf_source {
                None
            } else {
                valid_viewport(doc, value.viewport)
            }),
        ),
        ("LayoutFlags", int(layout_flags)),
        ("MaxExtents", tuple3(max_extents)),
        ("MaxLimits", tuple2(max_limits)),
        ("MinExtents", tuple3(min_extents)),
        ("MinLimits", tuple2(min_limits)),
        ("Origin", tuple3(value.ucs_origin)),
        ("PageName", text(page_name)),
        ("PaperHeight", number(paper_height)),
        ("PaperImageOrigin", vec2(paper_image_origin)),
        ("PaperImageOriginX", number(paper_image_origin_x)),
        ("PaperImageOriginY", number(paper_image_origin_y)),
        ("PaperRotation", int(value.plot_rotation as i64)),
        ("PaperSize", text(paper_size)),
        ("PaperUnits", int(paper_units as i64)),
        ("PaperWidth", number(paper_width)),
        ("PlotOriginX", number(value.plot_origin_x)),
        ("PlotOriginY", number(value.plot_origin_y)),
        ("PlotType", int(plot_type as i64)),
        (
            "PlotViewName",
            if dwg_source.is_some_and(|version| version >= DxfVersion::AC1018) {
                Value::Null
            } else {
                plot_string(&value.plot_view_name, plot_view_name_is_null)
            },
        ),
        ("ScaledFit", int(value.plot_scale_type as i64)),
        ("ShadePlotDPI", int(shade_plot_dpi)),
        ("ShadePlotIDHandle", int(0)),
        // The reference DXF reader hands group 76 to the Layout's own UCS orthographic type before the
        // PlotSettings member is tried, so a DXF never assigns the shade plot mode.
        (
            "ShadePlotMode",
            int(if dxf_source { 0 } else { value.shade_plot_mode as i64 }),
        ),
        ("ShadePlotResolutionMode", int(shade_plot_resolution)),
        ("StandardScale", number(value.plot_scale_factor)),
        (
            "StyleSheet",
            plot_string(&value.plot_style_sheet, style_sheet_is_null),
        ),
        (
            "SystemPrinterName",
            plot_string(&value.plot_printer_name, printer_name_is_null),
        ),
        ("TabOrder", int(value.tab_order as i64)),
        ("UCS", optional_handle(valid_ucs(doc, value.named_ucs))),
        ("UcsOrthographicType", int(value.ucs_ortho_type as i64)),
        (
            "UnprintableMargin",
            props(
                "PaperMargin",
                [
                    ("Bottom", number(value.plot_margin_bottom)),
                    ("Left", number(value.plot_margin_left)),
                    ("Right", number(value.plot_margin_right)),
                    ("Top", number(value.plot_margin_top)),
                ],
            ),
        ),
        ("WindowLowerLeftX", number(value.plot_window_min_x)),
        ("WindowLowerLeftY", number(value.plot_window_min_y)),
        ("WindowUpperLeftX", number(value.plot_window_max_x)),
        ("WindowUpperLeftY", number(value.plot_window_max_y)),
        ("XAxis", tuple3(value.ucs_x_axis)),
        ("YAxis", tuple3(value.ucs_y_axis)),
    ]);
    p.push(("NumeratorScale", number(value.plot_scale_numerator)));
    p.push(("DenominatorScale", number(value.plot_scale_denominator)));
    p.push((
        "PrintScale",
        number(value.plot_scale_numerator / value.plot_scale_denominator),
    ));
    p.push((
        "Viewports",
        associated_viewports
            .map(|viewports| handles(viewports.iter().copied()))
            .unwrap_or(Value::Null),
    ));
    p.push((
        "PaperViewport",
        if associated_viewports.is_some() {
            optional_handle(paper_viewport)
        } else {
            // Enumerable.FirstOrDefault receives null when AssociatedBlock is absent.
            text("!throws:ArgumentNullException")
        },
    ));
    record(handle_value, "Layout", value.owner, props("Layout", p))
}

fn dump_group(handle_value: Handle, value: &Group) -> Value {
    // DXF groups carry their public Name in the owning dictionary key rather
    // than in the GROUP body.  An empty model name must therefore remain
    // unmapped so `enrich_object_record` can project that key.
    let name = (!value.name.is_empty()).then_some(value.name.as_str());
    let mut p = common("GROUP", 0x48, "AcDbGroup", name, None, None);
    p.extend([
        ("Description", text(&value.description)),
        ("Entities", handles(value.entities.iter().copied())),
        ("IsUnnamed", bool_value(value.unnamed)),
        ("Selectable", bool_value(value.selectable)),
    ]);
    record(handle_value, "Group", value.owner, props("Group", p))
}

fn dump_mline_style(
    doc: &crate::CadDocument,
    reader: &str,
    handle_value: Handle,
    value: &MLineStyle,
) -> Value {
    let mut p = common(
        "MLINESTYLE",
        0x49,
        "AcDbMlineStyle",
        Some(&value.name),
        None,
        None,
    );
    p.extend([
        (
            "Description",
            if (reader == "new" || value.dxf_description_present == Some(false))
                && value.description.is_empty()
            {
                Value::Null
            } else {
                text(&value.description)
            },
        ),
        (
            "Elements",
            Value::Array(
                value
                    .elements
                    .iter()
                    .map(|element| {
                        props(
                            "MLineStyle.Element",
                            [
                                ("Color", super::common::appearance_color(element.color)),
                                (
                                    "LineType",
                                    optional_handle(
                                        doc.line_types
                                            .get(&element.linetype)
                                            .map(|line| line.handle),
                                    ),
                                ),
                                ("Offset", number(element.offset)),
                            ],
                        )
                    })
                    .collect(),
            ),
        ),
        ("EndAngle", number(value.end_angle)),
        ("FillColor", super::common::appearance_color(value.fill_color)),
        ("Flags", int(value.flags.to_bits() as i64)),
        ("StartAngle", number(value.start_angle)),
    ]);
    record(
        handle_value,
        "MLineStyle",
        value.owner,
        props("MLineStyle", p),
    )
}

fn dump_mleader_style(
    doc: &crate::CadDocument,
    handle_value: Handle,
    value: &MultiLeaderStyle,
) -> Value {
    let source_dwg = doc.dwg_source_version.is_some();
    let source_dxf = value.dxf_present_codes.is_some();
    let omitted = |code| source_dxf && !value.dxf_code_present(code);
    let style_name = doc
        .objects
        .get(&value.owner_handle)
        .and_then(|object| match object {
            ObjectType::Dictionary(dictionary) => {
                dictionary.entries.iter().find_map(|(name, handle)| {
                    (*handle == value.handle || *handle == handle_value).then(|| name.clone())
                })
            }
            _ => None,
        })
        .or_else(|| (!value.name.is_empty()).then(|| value.name.clone()))
        .unwrap_or_else(|| MultiLeaderStyle::STANDARD.to_string());
    // An unresolved linetype reference leaves the style at its ByLayer default.
    let line_type = value.line_type_handle.filter(|handle| {
        !handle.is_null() && doc.line_types.iter().any(|line| line.handle == *handle)
    }).or_else(|| {
        doc.line_types
            .get("ByLayer")
            .or_else(|| doc.line_types.get("BYLAYER"))
            .map(|line| line.handle)
    });
    let text_style = value
        .text_style_handle
        .or_else(|| doc.text_styles.get("Standard").map(|style| style.handle));
    let line_color = if source_dxf {
        Color::ByBlock
    } else {
        value.line_color
    };
    let text_color = if source_dxf {
        Color::ByBlock
    } else {
        value.text_color
    };
    let block_content_color = if source_dxf {
        Color::ByBlock
    } else {
        value.block_content_color
    };
    let top = if source_dwg {
        value.text_bottom_attachment
    } else {
        value.text_top_attachment
    };
    let bottom = if source_dwg {
        value.text_top_attachment
    } else {
        value.text_bottom_attachment
    };
    let angle = if source_dxf {
        value.block_content_rotation.to_radians()
    } else {
        value.block_content_rotation
    };
    let mut p = common(
        "MLEADERSTYLE",
        UNLISTED,
        "AcDbMLeaderStyle",
        Some(&style_name),
        None,
        None,
    );
    p.extend([
        (
            "AlignSpace",
            number(if omitted(46) { 0.0 } else { value.align_space }),
        ),
        (
            "Arrowhead",
            if source_dxf {
                Value::Null
            } else {
                optional_handle(value.arrowhead_handle)
            },
        ),
        ("ArrowheadSize", number(value.arrowhead_size)),
        (
            "BlockContent",
            if source_dxf {
                Value::Null
            } else {
                optional_handle(value.block_content_handle)
            },
        ),
        ("BlockContentColor", mleader_color(block_content_color)),
        (
            "BlockContentConnection",
            int(value.block_content_connection as i64),
        ),
        ("BlockContentRotation", number(angle)),
        (
            "BlockContentScale",
            vec3(Vector3::new(
                value.block_content_scale_x,
                value.block_content_scale_y,
                value.block_content_scale_z,
            )),
        ),
        ("BlockContentScaleX", number(value.block_content_scale_x)),
        ("BlockContentScaleY", number(value.block_content_scale_y)),
        ("BlockContentScaleZ", number(value.block_content_scale_z)),
        ("BreakGapSize", number(value.break_gap_size)),
        ("ContentType", int(value.content_type as i64)),
        (
            "DefaultTextContents",
            text(if omitted(300) {
                ""
            } else {
                &value.default_text
            }),
        ),
        ("Description", text(&value.description)),
        (
            "EnableBlockContentRotation",
            bool_value(value.enable_block_rotation),
        ),
        (
            "EnableBlockContentScale",
            bool_value(value.enable_block_scale),
        ),
        ("EnableDogleg", bool_value(value.enable_dogleg)),
        ("EnableLanding", bool_value(value.enable_landing)),
        (
            "FirstSegmentAngleConstraint",
            number(value.first_segment_angle),
        ),
        ("IsAnnotative", bool_value(value.is_annotative)),
        ("LandingDistance", number(value.landing_distance)),
        ("LandingGap", number(value.landing_gap)),
        ("LeaderDrawOrder", int(value.leader_draw_order as i64)),
        ("LeaderLineType", optional_handle(line_type)),
        ("LeaderLineWeight", int(value.line_weight.value() as i64)),
        ("LineColor", mleader_color(line_color)),
        (
            "MaxLeaderSegmentsPoints",
            int(value.max_leader_points as i64),
        ),
        (
            "MultiLeaderDrawOrder",
            int(value.multileader_draw_order as i64),
        ),
        ("OverwritePropertyValue", bool_value(value.property_changed)),
        (
            "PathType",
            int(if omitted(173) {
                MultiLeaderPathType::Invisible as i64
            } else {
                value.path_type as i64
            }),
        ),
        ("ScaleFactor", number(value.scale_factor)),
        (
            "SecondSegmentAngleConstraint",
            number(value.second_segment_angle),
        ),
        ("TextAlignAlwaysLeft", bool_value(value.text_always_left)),
        ("TextAlignment", int(value.text_alignment as i64)),
        ("TextAngle", int(value.text_angle_type as i64)),
        (
            "TextAttachmentDirection",
            int(value.text_attachment_direction as i64),
        ),
        (
            "TextBottomAttachment",
            int(if omitted(272) {
                TextAttachmentType::TopOfTopLine as i64
            } else {
                bottom as i64
            }),
        ),
        ("TextColor", mleader_color(text_color)),
        ("TextFrame", bool_value(value.text_frame)),
        ("TextHeight", number(value.text_height)),
        (
            "TextLeftAttachment",
            int(if omitted(174) {
                TextAttachmentType::TopOfTopLine as i64
            } else {
                value.text_left_attachment as i64
            }),
        ),
        (
            "TextRightAttachment",
            int(if omitted(178) {
                TextAttachmentType::TopOfTopLine as i64
            } else {
                value.text_right_attachment as i64
            }),
        ),
        ("TextStyle", optional_handle(text_style)),
        (
            "TextTopAttachment",
            int(if omitted(273) {
                TextAttachmentType::TopOfTopLine as i64
            } else {
                top as i64
            }),
        ),
        ("UnknownFlag298", bool_value(value.unknown_flag_298)),
    ]);
    record(
        handle_value,
        "MultiLeaderStyle",
        value.owner_handle,
        props("MultiLeaderStyle", p),
    )
}

fn dump_scale(handle_value: Handle, value: &Scale) -> Value {
    let mut p = common(
        "SCALE",
        UNLISTED,
        "AcDbScale",
        Some(&value.name),
        None,
        None,
    );
    p.extend([
        ("DrawingUnits", number(value.drawing_units)),
        ("IsUnitScale", bool_value(value.is_unit_scale)),
        ("PaperUnits", number(value.paper_units)),
        ("ScaleFactor", number(value.factor())),
    ]);
    record(handle_value, "Scale", value.owner_handle, props("Scale", p))
}

fn dump_image_definition(handle_value: Handle, value: &ImageDefinition, reader: &str) -> Value {
    let mut p = common("IMAGEDEF", UNLISTED, "AcDbRasterImageDef", None, None, None);
    // A DXF without groups 11/21 (ACadSharp's writer omits them) leaves the reference constructor's (1, 1);
    // the native pixel size of such an object is the zero it was created with.
    let default_size = if reader == "dxf" && value.pixel_size == (0.0, 0.0) {
        (1.0, 1.0)
    } else {
        value.pixel_size
    };
    p.extend([
        ("ClassVersion", int(value.class_version as i64)),
        ("DefaultSize", tuple2(default_size)),
        ("FileName", text(&value.file_name)),
        ("IsLoaded", bool_value(value.is_loaded)),
        (
            "Size",
            tuple2((value.size_in_pixels.0 as f64, value.size_in_pixels.1 as f64)),
        ),
        ("Units", int(value.resolution_unit.to_code() as i64)),
    ]);
    record(
        handle_value,
        "ImageDefinition",
        value.owner,
        props("ImageDefinition", p),
    )
}

fn dump_image_definition_reactor(handle_value: Handle, value: &ImageDefinitionReactor) -> Value {
    let mut p = common(
        "IMAGEDEF_REACTOR",
        UNLISTED,
        "AcDbRasterImageDefReactor",
        None,
        None,
        None,
    );
    p.push(("Image", handle(value.image_handle)));
    record(
        handle_value,
        "ImageDefinitionReactor",
        value.owner,
        props("ImageDefinitionReactor", p),
    )
}

fn dump_visual_style(handle_value: Handle, value: &VisualStyle, reader: &str) -> Value {
    let mut p = common(
        "VISUALSTYLE",
        UNLISTED,
        "AcDbVisualStyle",
        Some(&value.description),
        Some(&value.reactors),
        value.xdictionary_handle,
    );
    // ACadSharp DwgObjectReader.readVisualStyle reads only Name and Type.
    // Project its constructor defaults here; the native property bag remains
    // intact. The caller supplies provenance, independently of the filename.
    let defaults = VisualStyle::new();
    let data = if reader == "dwg" { &defaults } else { value };
    p.extend([
        ("Brightness", number(property_double(data, 26, 0.0))),
        ("Color", property_color(data, 6)),
        (
            "Description",
            if reader == "dwg" {
                Value::Null
            } else {
                text(&value.description)
            },
        ),
        ("DisplaySettings", int(property_int(data, 25, 0))),
        (
            "EdgeApplyStyleFlag",
            int(if data.properties.len() == 24 {
                match &data.properties[18].value {
                    crate::objects::VisualStylePropertyValue::Short(v) => *v as i64,
                    crate::objects::VisualStylePropertyValue::Long(v) => *v as i64,
                    _ => 0,
                }
            } else {
                0
            }),
        ),
        (
            "EdgeColor",
            match property_value(data, 15) {
                Some(crate::objects::VisualStylePropertyValue::Color(c)) => c
                    .index()
                    .map(|index| int(index as i64))
                    .unwrap_or_else(|| text(super::UNMAPPED)),
                _ => int(0),
            },
        ),
        ("EdgeCreaseAngle", number(property_double(data, 13, 0.0))),
        ("EdgeIntersectionColor", property_color(data, 9)),
        ("EdgeIntersectionLineType", int(property_int(data, 12, 0))),
        ("EdgeIsolineCount", int(property_int(data, 23, 0))),
        ("EdgeJitter", int(property_int(data, 19, 0))),
        ("EdgeModifiers", int(property_int(data, 14, 0))),
        ("EdgeObscuredColor", property_color(data, 10)),
        ("EdgeObscuredLineType", int(property_int(data, 11, 0))),
        ("EdgeOverhang", int(property_int(data, 18, 0))),
        ("EdgeSilhouetteColor", property_color(data, 20)),
        ("EdgeSilhouetteWidth", int(property_int(data, 21, 0))),
        ("EdgeStyle", int(data.edge_style as i64)),
        ("EdgeStyleModel", int(data.edge_model as i64)),
        ("EdgeWidth", int(property_int(data, 17, 0))),
        ("FaceColorMode", int(data.face_color_mode as i64)),
        ("FaceLightingModel", int(data.face_lighting_model as i64)),
        (
            "FaceLightingQuality",
            int(data.face_lighting_quality as i64),
        ),
        ("FaceModifiers", int(data.face_modifier as i64)),
        ("FaceOpacityLevel", number(property_double(data, 4, 0.0))),
        ("FaceSpecularLevel", number(property_double(data, 5, 0.0))),
        ("FaceStyleMonoColor", property_color(data, 6)),
        ("HaloGap", int(property_int(data, 22, 0))),
        ("InternalFlag", bool_value(data.internal_use_only)),
        ("OpacityLevel", number(property_double(data, 16, 0.0))),
        ("PrecisionFlag", bool_value(property_bool(data, 24, false))),
        ("RasterFile", property_text(data, 54)),
        ("ShadowType", int(property_int(data, 27, 0))),
        ("Type", int(value.style_type as i64)),
    ]);
    record(
        handle_value,
        "VisualStyle",
        value.owner,
        props("VisualStyle", p),
    )
}

fn property_value(
    value: &VisualStyle,
    index: usize,
) -> Option<&crate::objects::VisualStylePropertyValue> {
    // Legacy records interleave the scalar fields and have a 24-value bag.
    // Use the modern slot numbers at call sites without converting/cloning
    // the native bag (conversion for writing also changes signed magnitudes).
    let index = if value.properties.len() == 24 {
        match index {
            4 => 0,
            5 => 1,
            6 => 2,
            9 => 3,
            10 => 4,
            11 => 5,
            12 => 19,
            13 => 6,
            14 => 7,
            15 => 8,
            16 => 9,
            17 => 10,
            18 => 11,
            19 => 12,
            20 => 13,
            21 => 14,
            22 => 15,
            23 => 16,
            24 => 17,
            25 => 20,
            26 => 21,
            27 => 22,
            _ => return None,
        }
    } else {
        index
    };
    value.properties.get(index).map(|property| &property.value)
}

fn property_double(value: &VisualStyle, index: usize, fallback: f64) -> f64 {
    match property_value(value, index) {
        Some(crate::objects::VisualStylePropertyValue::Double(value)) => *value,
        Some(crate::objects::VisualStylePropertyValue::Short(value)) => *value as f64,
        Some(crate::objects::VisualStylePropertyValue::Long(value)) => *value as f64,
        _ => fallback,
    }
}

fn property_int(value: &VisualStyle, index: usize, fallback: i64) -> i64 {
    match property_value(value, index) {
        Some(crate::objects::VisualStylePropertyValue::Short(value)) => *value as i64,
        Some(crate::objects::VisualStylePropertyValue::Long(value)) => *value as i64,
        Some(crate::objects::VisualStylePropertyValue::Double(value)) => *value as i64,
        Some(crate::objects::VisualStylePropertyValue::Bool(value)) => *value as i64,
        _ => fallback,
    }
}

fn property_bool(value: &VisualStyle, index: usize, fallback: bool) -> bool {
    match property_value(value, index) {
        Some(crate::objects::VisualStylePropertyValue::Bool(value)) => *value,
        Some(crate::objects::VisualStylePropertyValue::Short(value)) => *value != 0,
        Some(crate::objects::VisualStylePropertyValue::Long(value)) => *value != 0,
        _ => fallback,
    }
}

fn property_text(value: &VisualStyle, index: usize) -> Value {
    match property_value(value, index) {
        Some(crate::objects::VisualStylePropertyValue::Text(value)) => text(value),
        _ => Value::Null,
    }
}

fn property_color(value: &VisualStyle, index: usize) -> Value {
    match property_value(value, index) {
        Some(crate::objects::VisualStylePropertyValue::Color(value)) => color(*value),
        _ => color(Color::ByBlock),
    }
}

fn dump_material(
    doc: &crate::CadDocument,
    reader: &str,
    handle_value: Handle,
    value: &Material,
) -> Value {
    let dxf = value.dxf_source.as_deref();
    let absent = |code| dxf.is_some_and(|source| !source.contains(code));
    let advanced = |code| dxf.is_some_and(|source| source.contains_advanced(code));
    let scalar = |code, native: i64, default| int(if absent(code) { default } else { native });
    let string = |code, native: &str| {
        if (absent(code) || reader == "new") && native.is_empty() {
            Value::Null
        } else {
            text(native)
        }
    };
    let filename = |code, map: &crate::objects::MaterialMap| {
        if reader == "dwg" && map.source != 1 && map.file_name.is_empty() {
            Value::Null
        } else {
            string(code, &map.file_name)
        }
    };
    // The DXF mapper handles code 90 only; 91/92 leave Color at ByBlock.
    // DWG reads an RGB only for the Override color method.
    let ambient_rgb = dxf.map_or(value.ambient_color.rgb, |source| source.last_color_90);
    let diffuse_rgb = if dxf.is_some() {
        None
    } else {
        value.diffuse_color.rgb
    };
    let specular_rgb = if dxf.is_some() {
        None
    } else {
        value.specular_color.rgb
    };
    let mut p = common(
        "MATERIAL",
        UNLISTED,
        "AcDbMaterial",
        Some(&value.name),
        Some(&value.reactors),
        value.xdictionary_handle,
    );
    p.extend([
        ("AmbientColor", material_color(ambient_rgb)),
        ("AmbientColorFactor", number(value.ambient_color.factor)),
        ("AmbientColorMethod", int(value.ambient_color.flag as i64)),
        (
            "BumpAutoTransform",
            int(value.bump_map.auto_transform as i64),
        ),
        ("BumpMapBlendFactor", number(value.bump_map.blend_factor)),
        ("BumpMapFileName", filename(8, &value.bump_map)),
        (
            "BumpMapSource",
            scalar(179, value.bump_map.source as i64, 1),
        ),
        ("BumpMatrix", matrix(value.bump_map.transform)),
        (
            "BumpProjectionMethod",
            scalar(270, value.bump_map.projection as i64, 1),
        ),
        ("BumpTilingMethod", int(value.bump_map.tiling as i64)),
        (
            "ChannelFlags",
            int(
                if absent(94) || (reader == "dwg" && doc.version < crate::types::DxfVersion::AC1021)
                {
                    1
                } else {
                    value.channel_flags as i64
                },
            ),
        ),
        ("Description", string(2, &value.description)),
        (
            "DiffuseAutoTransform",
            int(value.diffuse_map.auto_transform as i64),
        ),
        ("DiffuseColor", material_color(diffuse_rgb)),
        ("DiffuseColorFactor", number(value.diffuse_color.factor)),
        ("DiffuseColorMethod", int(value.diffuse_color.flag as i64)),
        (
            "DiffuseMapBlendFactor",
            number(value.diffuse_map.blend_factor),
        ),
        ("DiffuseMapFileName", filename(3, &value.diffuse_map)),
        (
            "DiffuseMapSource",
            scalar(72, value.diffuse_map.source as i64, 1),
        ),
        ("DiffuseMatrix", matrix(value.diffuse_map.transform)),
        (
            "DiffuseProjectionMethod",
            scalar(73, value.diffuse_map.projection as i64, 1),
        ),
        ("DiffuseTilingMethod", int(value.diffuse_map.tiling as i64)),
        ("IlluminationModel", int(value.illumination_model as i64)),
        ("Mode", int(value.mode as i64)),
        ("Opacity", number(value.opacity_percent)),
        (
            "OpacityAutoTransform",
            int(value.opacity_map.auto_transform as i64),
        ),
        (
            "OpacityMapBlendFactor",
            number(value.opacity_map.blend_factor),
        ),
        ("OpacityMapFileName", filename(7, &value.opacity_map)),
        (
            "OpacityMapSource",
            scalar(175, value.opacity_map.source as i64, 1),
        ),
        ("OpacityMatrix", matrix(value.opacity_map.transform)),
        (
            "OpacityProjectionMethod",
            scalar(176, value.opacity_map.projection as i64, 1),
        ),
        ("OpacityTilingMethod", int(value.opacity_map.tiling as i64)),
        (
            "ReflectionAutoTransform",
            int(value.reflection_map.auto_transform as i64),
        ),
        (
            "ReflectionMapBlendFactor",
            number(value.reflection_map.blend_factor),
        ),
        ("ReflectionMapFileName", filename(6, &value.reflection_map)),
        (
            "ReflectionMapSource",
            scalar(171, value.reflection_map.source as i64, 1),
        ),
        ("ReflectionMatrix", matrix(value.reflection_map.transform)),
        (
            "ReflectionProjectionMethod",
            scalar(172, value.reflection_map.projection as i64, 1),
        ),
        (
            "ReflectionTilingMethod",
            int(value.reflection_map.tiling as i64),
        ),
        ("Reflectivity", number(value.reflectivity)),
        (
            "RefractionAutoTransform",
            int(value.refraction_map.auto_transform as i64),
        ),
        ("RefractionIndex", number(value.refraction_index)),
        (
            "RefractionMapBlendFactor",
            number(value.refraction_map.blend_factor),
        ),
        ("RefractionMapFileName", filename(9, &value.refraction_map)),
        (
            "RefractionMapSource",
            scalar(273, value.refraction_map.source as i64, 1),
        ),
        ("RefractionMatrix", matrix(value.refraction_map.transform)),
        (
            "RefractionProjectionMethod",
            scalar(274, value.refraction_map.projection as i64, 1),
        ),
        (
            "RefractionTilingMethod",
            int(value.refraction_map.tiling as i64),
        ),
        (
            "SelfIllumination",
            number(if absent(149) {
                0.0
            } else {
                value.self_illumination
            }),
        ),
        (
            "SpecularAutoTransform",
            int(value.specular_map.auto_transform as i64),
        ),
        ("SpecularColor", material_color(specular_rgb)),
        ("SpecularColorFactor", number(value.specular_color.factor)),
        ("SpecularColorMethod", int(value.specular_color.flag as i64)),
        ("SpecularGlossFactor", number(value.specular_gloss_factor)),
        (
            "SpecularMapBlendFactor",
            number(value.specular_map.blend_factor),
        ),
        ("SpecularMapFileName", filename(4, &value.specular_map)),
        (
            "SpecularMapSource",
            scalar(77, value.specular_map.source as i64, 1),
        ),
        ("SpecularMatrix", matrix(value.specular_map.transform)),
        (
            "SpecularProjectionMethod",
            scalar(78, value.specular_map.projection as i64, 1),
        ),
        (
            "SpecularTilingMethod",
            int(value.specular_map.tiling as i64),
        ),
        ("Translucence", number(value.translucence)),
    ]);
    // The reference DXF mapper does not distinguish the advanced tail. Reused
    // codes overwrite its earlier fields; the native model keeps both maps.
    for (code, name, projected) in [
        (
            42,
            "DiffuseMapBlendFactor",
            number(value.normal_map.blend_factor),
        ),
        (72, "DiffuseMapSource", int(value.normal_map.source as i64)),
        (3, "DiffuseMapFileName", text(&value.normal_map.file_name)),
        (
            73,
            "DiffuseProjectionMethod",
            int(value.normal_map.projection as i64),
        ),
        (
            74,
            "DiffuseTilingMethod",
            int(value.normal_map.tiling as i64),
        ),
        (
            75,
            "DiffuseAutoTransform",
            int(value.normal_map.auto_transform as i64),
        ),
        (43, "DiffuseMatrix", matrix(value.normal_map.transform)),
        (
            270,
            "BumpProjectionMethod",
            int(value.luminance_mode as i64),
        ),
        (271, "BumpTilingMethod", int(value.normal_map_method as i64)),
        (
            272,
            "BumpAutoTransform",
            int(value.global_illumination as i64),
        ),
        (273, "RefractionMapSource", int(value.final_gather as i64)),
    ] {
        if advanced(code) {
            p.push((name, projected));
        }
    }
    record(handle_value, "Material", value.owner, props("Material", p))
}

fn material_color(rgb: Option<i32>) -> Value {
    let Some(rgb) = rgb else {
        return color(Color::ByBlock);
    };
    let mut result = color(Color::from_true_color_value(rgb));
    // ACadSharp exposes RGB bytes packed little-endian, independently of the
    // DWG/DXF integer's BGR packing.
    result["TrueColor"] = int(((rgb & 0xff) << 16 | (rgb & 0xff00) | ((rgb >> 16) & 0xff)) as i64);
    result
}

fn matrix(value: [f64; 16]) -> Value {
    props(
        "Matrix4",
        [
            ("M00", number(value[0])),
            ("M01", number(value[1])),
            ("M02", number(value[2])),
            ("M03", number(value[3])),
            ("M10", number(value[4])),
            ("M11", number(value[5])),
            ("M12", number(value[6])),
            ("M13", number(value[7])),
            ("M20", number(value[8])),
            ("M21", number(value[9])),
            ("M22", number(value[10])),
            ("M23", number(value[11])),
            ("M30", number(value[12])),
            ("M31", number(value[13])),
            ("M32", number(value[14])),
            ("M33", number(value[15])),
        ],
    )
}

fn matrix4(value: &Matrix4) -> Value {
    props(
        "Matrix4",
        [
            ("M00", number(value.m[0][0])),
            ("M01", number(value.m[0][1])),
            ("M02", number(value.m[0][2])),
            ("M03", number(value.m[0][3])),
            ("M10", number(value.m[1][0])),
            ("M11", number(value.m[1][1])),
            ("M12", number(value.m[1][2])),
            ("M13", number(value.m[1][3])),
            ("M20", number(value.m[2][0])),
            ("M21", number(value.m[2][1])),
            ("M22", number(value.m[2][2])),
            ("M23", number(value.m[2][3])),
            ("M30", number(value.m[3][0])),
            ("M31", number(value.m[3][1])),
            ("M32", number(value.m[3][2])),
            ("M33", number(value.m[3][3])),
        ],
    )
}

fn dump_raster_variables(handle_value: Handle, value: &crate::objects::RasterVariables) -> Value {
    let mut p = common(
        "RASTERVARIABLES",
        UNLISTED,
        "AcDbRasterVariables",
        None,
        None,
        None,
    );
    p.extend([
        ("ClassVersion", int(value.class_version as i64)),
        ("DisplayQuality", int(value.image_quality as i64)),
        (
            "IsDisplayFrameShown",
            bool_value(value.display_image_frame != 0),
        ),
        ("Units", int(value.units as i64)),
    ]);
    record(
        handle_value,
        "RasterVariables",
        value.owner,
        props("RasterVariables", p),
    )
}

fn dump_book_color(
    doc: &crate::CadDocument,
    handle_value: Handle,
    value: &crate::objects::BookColor,
) -> Value {
    // ACadSharp computes Name from the two color-book components.  Keep this
    // local to the compatibility projection: the native model intentionally
    // stores the components independently.
    let dictionary_name = owned_dictionary_name(doc, handle_value, value.owner);
    let dictionary_parts = dictionary_name
        .as_deref()
        .and_then(|name| name.split_once('$'));
    let book_name = if value.book_name.is_empty() {
        dictionary_parts.map_or("", |(book, _)| book)
    } else {
        value.book_name.as_str()
    };
    let color_name = if value.color_name.is_empty() {
        dictionary_parts.map_or("", |(_, color)| color)
    } else {
        value.color_name.as_str()
    };
    // Its Name setter splits a DXF group 430 on every `$`: the first segment becomes the book and the last the
    // color (the native model keeps everything after the first `$` as the color name).
    let color_name = if doc.dwg_source_version.is_none() {
        color_name.rsplit('$').next().unwrap_or(color_name)
    } else {
        color_name
    };
    let name = if color_name.is_empty() {
        String::new()
    } else {
        format!("{}${}", book_name, color_name)
    };
    // The reference's default Color is ByBlock; native Color remains ByLayer.
    let source_color = if value.dxf_color_present == Some(false) {
        crate::types::Color::ByBlock
    } else {
        value.color
    };
    let mut compat_color = color(source_color);
    if let (Some((r, g, b)), Some(color_props)) =
        (source_color.true_color_rgb(), compat_color.as_object_mut())
    {
        // ACadSharp's Color packs RGB bytes in host little-endian order for
        // the exposed TrueColor property (R | G<<8 | B<<16).
        color_props.insert(
            "TrueColor".to_string(),
            json!(i32::from(r) | (i32::from(g) << 8) | (i32::from(b) << 16)),
        );
    }
    let name_value = if color_name.is_empty() {
        // Older DXF files omit group 430 and keep the full name only as the
        // owning dictionary key; let the object enrichment pass restore it.
        text(super::UNMAPPED)
    } else {
        text(name.as_str())
    };
    let mut p = common("DBCOLOR", UNLISTED, "AcDbColor", None, None, None);
    p.push(("Name", name_value));
    p.extend([
        ("BookName", text(book_name)),
        ("Color", compat_color),
        ("ColorName", text(color_name)),
    ]);
    record(
        handle_value,
        "BookColor",
        value.owner,
        props("BookColor", p),
    )
}

fn dump_placeholder(handle_value: Handle, value: &crate::objects::PlaceHolder) -> Value {
    let p = common("ACDBPLACEHOLDER", 0x50, "AcDbPlaceHolder", None, None, None);
    record(
        handle_value,
        "AcdbPlaceHolder",
        value.owner,
        props("AcdbPlaceHolder", p),
    )
}

fn dump_dictionary_variable(
    handle_value: Handle,
    value: &crate::objects::DictionaryVariable,
) -> Value {
    let mut p = common(
        "DICTIONARYVAR",
        UNLISTED,
        "DictionaryVariables",
        Some(&value.name),
        None,
        None,
    );
    p.extend([
        ("ObjectSchemaNumber", int(value.schema_number as i64)),
        ("Value", text(&value.value)),
    ]);
    record(
        handle_value,
        "DictionaryVariable",
        value.owner_handle,
        props("DictionaryVariable", p),
    )
}

fn dump_sort_entities(handle_value: Handle, value: &SortEntitiesTable) -> Value {
    let mut p = common(
        "SORTENTSTABLE",
        UNLISTED,
        "AcDbSortentsTable",
        None,
        None,
        None,
    );
    p.push(("BlockOwner", handle(value.block_owner_handle)));
    record(
        handle_value,
        "SortEntitiesTable",
        value.owner_handle,
        props("SortEntitiesTable", p),
    )
}

fn paper_margin(value: &crate::objects::PaperMargin) -> Value {
    props(
        "PaperMargin",
        [
            ("Bottom", number(value.bottom)),
            ("Left", number(value.left)),
            ("Right", number(value.right)),
            ("Top", number(value.top)),
        ],
    )
}

fn dump_plot_settings(
    doc: &crate::CadDocument,
    reader: &str,
    handle_value: Handle,
    value: &PlotSettings,
) -> Value {
    let is_new = reader == "new";
    let dxf_source = doc.dwg_source_version.is_none()
        && (reader == "dxf" || source_dxf(doc) || value.dxf_present_codes.is_some());
    let dwg_source = doc
        .dwg_source_version
        .or_else(|| (reader == "dwg").then_some(doc.version));
    let dxf_code = |code| dxf_plot_code_present(value, code);
    let flags = if is_new || (dxf_source && !dxf_code(70) && value.flags.to_bits() == 0) {
        688
    } else {
        value.flags.to_bits() as i64
    };
    let page_name = if (is_new || (dxf_source && !dxf_code(1))) && value.page_name.is_empty() {
        "none_device"
    } else {
        value.page_name.as_str()
    };
    let plot_view_name_is_null = is_new
        || dwg_source.is_some_and(|version| version >= DxfVersion::AC1018)
        || (dxf_source && !dxf_code(6));
    // These are three independent C# stored properties, not alias getters:
    // DWG assigns only the vector; DXF assigns only the two scalar properties.
    let (paper_image_origin, paper_image_origin_x, paper_image_origin_y) = if dxf_source {
        (
            Vector2::ZERO,
            value.paper_image_origin_x,
            value.paper_image_origin_y,
        )
    } else if dwg_source.is_some() {
        (
            Vector2::new(value.paper_image_origin_x, value.paper_image_origin_y),
            0.0,
            0.0,
        )
    } else {
        (
            Vector2::new(value.paper_image_origin_x, value.paper_image_origin_y),
            value.paper_image_origin_x,
            value.paper_image_origin_y,
        )
    };
    let paper_size = if (is_new || (dxf_source && !dxf_code(4))) && value.paper_size.is_empty() {
        "ISO_A4_(210.00_x_297.00_MM)"
    } else {
        value.paper_size.as_str()
    };
    let paper_width = if (is_new || (dxf_source && !dxf_code(44))) && value.paper_width == 0.0 {
        297.0
    } else {
        value.paper_width
    };
    let paper_height = if (is_new || (dxf_source && !dxf_code(45))) && value.paper_height == 0.0 {
        210.0
    } else {
        value.paper_height
    };
    let paper_units =
        if (is_new || (dxf_source && !dxf_code(72))) && value.paper_units.to_code() == 0 {
            1
        } else {
            value.paper_units.to_code()
        };
    let plot_type = if (is_new || (dxf_source && !dxf_code(74))) && value.plot_type.to_code() == 4 {
        1
    } else {
        value.plot_type.to_code()
    };
    let shade_plot_resolution = if is_new || (dxf_source && !dxf_code(77)) {
        0
    } else {
        value.shade_plot_resolution.to_code()
    };
    let shade_plot_dpi = if value.shade_plot_dpi <= 0
        && (is_new || dwg_source.is_some_and(|version| version < DxfVersion::AC1018))
    {
        300
    } else {
        value.shade_plot_dpi as i64
    };
    let mut p = common(
        "PLOTSETTINGS",
        UNLISTED,
        "AcDbPlotSettings",
        Some(&value.page_name),
        Some(&value.reactors),
        value.xdictionary_handle,
    );
    p.extend([
        ("DenominatorScale", number(value.scale_denominator)),
        ("Flags", int(flags)),
        ("NumeratorScale", number(value.scale_numerator)),
        ("PageName", text(page_name)),
        ("PaperHeight", number(paper_height)),
        ("PaperImageOrigin", vec2(paper_image_origin)),
        ("PaperImageOriginX", number(paper_image_origin_x)),
        ("PaperImageOriginY", number(paper_image_origin_y)),
        ("PaperRotation", int(value.rotation.to_code() as i64)),
        ("PaperSize", text(paper_size)),
        ("PaperUnits", int(paper_units as i64)),
        ("PaperWidth", number(paper_width)),
        ("PlotOriginX", number(value.origin_x)),
        ("PlotOriginY", number(value.origin_y)),
        ("PlotType", int(plot_type as i64)),
        (
            "PlotViewName",
            if dwg_source.is_some_and(|version| version >= DxfVersion::AC1018) {
                Value::Null
            } else {
                plot_string(&value.plot_view_name, plot_view_name_is_null)
            },
        ),
        (
            "PrintScale",
            number(value.scale_numerator / value.scale_denominator),
        ),
        ("ScaledFit", int(value.scale_type.to_code() as i64)),
        ("ShadePlotDPI", int(shade_plot_dpi)),
        // ACadSharp marks code 333 as an ignored reference and never assigns
        // it while reading either DXF or DWG.
        ("ShadePlotIDHandle", int(0)),
        ("ShadePlotMode", int(value.shade_plot_mode.to_code() as i64)),
        ("ShadePlotResolutionMode", int(shade_plot_resolution as i64)),
        ("StandardScale", number(value.standard_scale_factor)),
        (
            "StyleSheet",
            plot_string(
                &value.current_style_sheet,
                is_new || (dxf_source && !dxf_code(7)),
            ),
        ),
        (
            "SystemPrinterName",
            plot_string(&value.printer_name, is_new || (dxf_source && !dxf_code(2))),
        ),
        ("UnprintableMargin", paper_margin(&value.margins)),
        ("WindowLowerLeftX", number(value.plot_window.lower_left_x)),
        ("WindowLowerLeftY", number(value.plot_window.lower_left_y)),
        ("WindowUpperLeftX", number(value.plot_window.upper_right_x)),
        ("WindowUpperLeftY", number(value.plot_window.upper_right_y)),
    ]);
    record(
        handle_value,
        "PlotSettings",
        value.owner,
        props("PlotSettings", p),
    )
}

fn dump_wipeout_variables(handle_value: Handle, value: &crate::objects::WipeoutVariables) -> Value {
    let mut p = common(
        "WIPEOUTVARIABLES",
        UNLISTED,
        "AcDbWipeoutVariables",
        None,
        None,
        None,
    );
    p.push(("DisplayImageFrame", bool_value(value.display_frame != 0)));
    record(
        handle_value,
        "WipeoutVariables",
        value.owner,
        props("WipeoutVariables", p),
    )
}

fn eval_value(value: &BlockEvalValue) -> Value {
    match value {
        BlockEvalValue::None => Value::Null,
        BlockEvalValue::Real(value) => number(*value),
        BlockEvalValue::Point(value) => vec2(Vector2::new(value[0], value[1])),
        BlockEvalValue::Text(value) => text(value),
        BlockEvalValue::Long(value) => int(*value as i64),
        BlockEvalValue::Handle(value) => handle(*value),
        BlockEvalValue::Short(value) => int(*value as i64),
    }
}

fn eval_pair(value: &crate::objects::BlockEvalExpression) -> Value {
    eval_pair_value(value.value_code, &value.value)
}

fn eval_pair_value(code: i16, value: &BlockEvalValue) -> Value {
    // ACadSharp constructs a value pair only for positive evaluation codes.
    if code <= 0 {
        return Value::Null;
    }
    props(
        "DxfValuePair",
        [("Code", int(code as i64)), ("Value", eval_value(value))],
    )
}

fn element_props(element: &crate::objects::BlockElement, p: &mut Vec<(&'static str, Value)>) {
    p.push(("ElementName", text(&element.name)));
    p.push(("EvaluatedValue", eval_pair(&element.eval)));
    p.push(("Value1071", int(element.eed_1071 as i64)));
    p.push(("Value98", int(element.major as i64)));
    p.push(("Value99", int(element.minor as i64)));
}

fn one_point_parameter_props(
    value: &crate::objects::BlockOnePointParameter,
    p: &mut Vec<(&'static str, Value)>,
) {
    element_props(&value.parameter.element, p);
    p.push(("ChainActions", bool_value(value.parameter.chain_actions)));
    p.push((
        "ShowProperties",
        bool_value(value.parameter.show_properties),
    ));
    p.push(("Id", int(value.parameter.element.eval.node_id as i64)));
    p.push(("Location", vec3(value.definition_point)));
    p.push(("GripId", int(value.property_count as i64)));
    p.push(("DisplacementX", parameter_property(&value.properties[0])));
    p.push(("DisplacementY", parameter_property(&value.properties[1])));
}

fn two_point_parameter_props(
    value: &crate::objects::BlockTwoPointParameter,
    p: &mut Vec<(&'static str, Value)>,
) {
    element_props(&value.parameter.element, p);
    p.push(("ChainActions", bool_value(value.parameter.chain_actions)));
    p.push((
        "ShowProperties",
        bool_value(value.parameter.show_properties),
    ));
    p.push(("Id", int(value.parameter.element.eval.node_id as i64)));
    p.push(("FirstPoint", vec3(value.definition_base_point)));
    p.push(("SecondPoint", vec3(value.definition_end_point)));
    p.push(("BaseLocation", int(value.parameter_base_location as i64)));
    p.push(("GripIds", json!(value.property_states)));
    for (name, property) in [
        "FirstPointDisplacementX",
        "FirstPointDisplacementY",
        "SecondPointDisplacementX",
        "SecondPointDisplacementY",
    ]
    .into_iter()
    .zip(&value.properties)
    {
        p.push((name, parameter_property(property)));
    }
}

fn eval_connection(id: i32, name: &str) -> Value {
    props(
        "EvalConnection",
        [("Id", int(id as i64)), ("Name", text(name))],
    )
}

fn parameter_property(value: &crate::objects::BlockParameterProperty) -> Value {
    props(
        "EvalParameterProperty",
        [(
            "Connections",
            Value::Array(
                value
                    .connections
                    .iter()
                    .map(|connection| eval_connection(connection.code, &connection.name))
                    .collect(),
            ),
        )],
    )
}

fn grip_props(value: &crate::objects::BlockGrip, p: &mut Vec<(&'static str, Value)>) {
    element_props(&value.element, p);
    p.extend([
        ("Cycling", bool_value(value.insert_cycling)),
        ("Id", int(value.element.eval.node_id as i64)),
        ("Location", vec3(value.location)),
        ("ExpressionId1", int(value.flags_91 as i64)),
        ("ExpressionId2", int(value.flags_92 as i64)),
        ("Value93", int(value.insert_cycling_weight as i64)),
    ]);
}

fn action_props(value: &crate::objects::BlockAction, p: &mut Vec<(&'static str, Value)>) {
    element_props(&value.element, p);
    p.extend([
        ("Id", int(value.element.eval.node_id as i64)),
        ("LabelPosition", vec3(value.display_location)),
        (
            "ParametersIds",
            Value::Array(value.action_ids.iter().map(|v| int(*v as i64)).collect()),
        ),
        ("Entities", handles(value.dependencies.iter().copied())),
    ]);
}

fn parameter_value_set(value: &crate::objects::BlockParameterValueSet) -> Value {
    props(
        "ParameterValueSet",
        [
            ("AllowedValues", numbers(value.values.iter().copied())),
            ("Increment", number(value.increment)),
            ("Maximum", number(value.maximum)),
            ("Minimum", number(value.minimum)),
            ("Type", int(value.flags as i64)),
        ],
    )
}

fn visibility_parameter_props(
    doc: &crate::CadDocument,
    reader: &str,
    value: &crate::objects::BlockVisibilityParameter,
    p: &mut Vec<(&'static str, Value)>,
) {
    p.extend([
        // The C# DWG reader assigns the visibility initialization bit to
        // ChainActions, overwriting the inherited bit, and leaves Value281 false.
        (
            "ChainActions",
            bool_value(if reader == "dwg" {
                value.is_initialized
            } else {
                value.chain_actions
            }),
        ),
        ("Description", text(&value.description)),
        ("ElementName", text(&value.element_name)),
        (
            "EvaluatedValue",
            eval_pair_value(value.eval_value_code, &value.eval_value),
        ),
        // The C# DXF callback tries the visibility map first: its entity-count
        // code 93 consumes the inherited grip-ID code without assigning it.
        (
            "GripId",
            int(if reader == "dxf" {
                0
            } else {
                value.property_info_count as i64
            }),
        ),
        ("Id", int(value.eval_node_id as i64)),
        ("Label", text(&value.name)),
        ("Location", vec3(value.def_point)),
        ("ShowProperties", bool_value(value.show_properties)),
        ("Value1071", int(value.element_eed_1071 as i64)),
        (
            "Value281",
            bool_value(reader != "dwg" && value.is_initialized),
        ),
        ("Value91", bool_value(value.unknown_bool)),
        ("Value98", int(value.element_major as i64)),
        ("Value99", int(value.element_minor as i64)),
        (
            "Entities",
            handles(
                value
                    .all_blocks
                    .iter()
                    .copied()
                    .filter(|h| doc.get_entity(*h).is_some()),
            ),
        ),
        (
            "States",
            Value::Array(
                value
                    .states
                    .iter()
                    .map(|state| {
                        // State templates use insertion-ordered sets; unresolved
                        // handles are omitted when the template is built.
                        let mut entities = HashSet::new();
                        let mut expressions = HashSet::new();
                        let state_value = props(
                            "BlockVisibilityParameter.State",
                            [
                                (
                                    "Entities",
                                    handles(state.visible_blocks.iter().copied().filter(|h| {
                                        entities.insert(*h) && doc.get_entity(*h).is_some()
                                    })),
                                ),
                                (
                                    "Expressions",
                                    handles(state.visible_params.iter().copied().filter(|h| {
                                        expressions.insert(*h) && is_evaluation_expression(doc, *h)
                                    })),
                                ),
                                ("Name", text(&state.name)),
                            ],
                        );
                        json!([state.name, state_value])
                    })
                    .collect(),
            ),
        ),
    ]);
    for (name, property) in ["DisplacementX", "DisplacementY"]
        .into_iter()
        .zip(&value.property_info)
    {
        p.push((
            name,
            props(
                "EvalParameterProperty",
                [(
                    "Connections",
                    Value::Array(
                        property
                            .connections
                            .iter()
                            .map(|connection| eval_connection(connection.code, &connection.name))
                            .collect(),
                    ),
                )],
            ),
        ));
    }
}

fn is_evaluation_expression(doc: &crate::CadDocument, h: Handle) -> bool {
    match doc.objects.get(&h) {
        Some(ObjectType::BlockVisibilityParameter(_)) => true,
        Some(ObjectType::DynamicBlock(object)) => super::schema()
            .get(dynamic_class_name(object).as_str())
            .is_some_and(|members| members.contains(&"EvaluatedValue")),
        _ => doc.block_visibility_params.contains_key(&h),
    }
}

fn dump_visibility_parameter(
    doc: &crate::CadDocument,
    reader: &str,
    handle_value: Handle,
    value: &crate::objects::BlockVisibilityParameter,
) -> Value {
    let mut p = common(
        "BLOCKVISIBILITYPARAMETER",
        UNLISTED,
        "AcDbBlockVisibilityParameter",
        Some(&dictionary_name(doc, handle_value, &handle(value.owner)).unwrap_or_default()),
        None,
        None,
    );
    visibility_parameter_props(doc, reader, value, &mut p);
    record(
        handle_value,
        "BlockVisibilityParameter",
        value.owner,
        props("BlockVisibilityParameter", p),
    )
}

fn dump_dynamic_block(
    doc: &crate::CadDocument,
    reader: &str,
    handle_value: Handle,
    value: &DynamicBlockObject,
) -> Value {
    let class = dynamic_class_name(value);
    let subclass = match class.as_str() {
        "BlockLookupGrip" => "AcDbBlockLookUpGrip",
        "BlockLookupParameter" => "AcDbBlockLookUpParameter",
        "BlockRotationAction" => "AcDbBlockRotationAction",
        _ => &value.cpp_class_name,
    };
    let mut p = common(
        &value.dxf_name,
        UNLISTED,
        subclass,
        Some(&dictionary_name(doc, handle_value, &handle(value.owner)).unwrap_or_default()),
        Some(&value.reactors),
        value.xdictionary_handle,
    );
    dynamic_data_props(&value.data, &mut p, doc, reader);
    typed_record(handle_value, &class, value.owner, p)
}

fn dynamic_class_name(value: &DynamicBlockObject) -> String {
    if value.dxf_name.eq_ignore_ascii_case("ACAD_EVALUATION_GRAPH") {
        "EvaluationGraph".to_string()
    } else if value
        .dxf_name
        .eq_ignore_ascii_case("ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION")
    {
        "DynamicBlockPurgePreventer".to_string()
    } else {
        class_name(&value.cpp_class_name)
    }
}

fn dynamic_data_props(
    value: &DynamicBlockData,
    p: &mut Vec<(&'static str, Value)>,
    doc: &crate::CadDocument,
    reader: &str,
) {
    match value {
        DynamicBlockData::Representation(value) => {
            p.extend([
                ("Block", handle(value.block)),
                ("Version", int(value.flags as i64)),
            ]);
        }
        DynamicBlockData::GripLocationComponent(value) => {
            p.extend([
                (
                    "Connection",
                    eval_connection(value.grip_type, &value.expression),
                ),
                ("Id", int(value.eval.node_id as i64)),
                ("Value98", int(value.eval.major as i64)),
                ("Value99", int(value.eval.minor as i64)),
                ("EvaluatedValue", eval_pair(&value.eval)),
            ]);
        }
        DynamicBlockData::AlignmentGrip(value) | DynamicBlockData::LinearGrip(value) => {
            grip_props(&value.grip, p);
            p.push(("AlignmentX", number(value.orientation.x)));
            p.push(("AlignmentY", number(value.orientation.y)));
            p.push(("AlignmentZ", number(value.orientation.z)));
            p.push(("DistanceX", number(value.orientation.x)));
            p.push(("DistanceY", number(value.orientation.y)));
            p.push(("DistanceZ", number(value.orientation.z)));
        }
        DynamicBlockData::FlipGrip(value) => {
            grip_props(&value.grip, p);
            p.extend([
                ("DirectionX", number(value.orientation.x)),
                ("DirectionY", number(value.orientation.y)),
                ("DirectionZ", number(value.orientation.z)),
                ("FlipExpressionId", int(value.combined_state as i64)),
            ]);
        }
        DynamicBlockData::LookupGrip(value)
        | DynamicBlockData::PolarGrip(value)
        | DynamicBlockData::RotationGrip(value)
        | DynamicBlockData::VisibilityGrip(value)
        | DynamicBlockData::XYGrip(value)
        | DynamicBlockData::PropertiesTableGrip(value) => grip_props(value, p),
        DynamicBlockData::AlignmentParameter(value) => {
            two_point_parameter_props(&value.parameter, p);
            p.push(("IsPerpendicular", bool_value(value.align_perpendicular)));
        }
        DynamicBlockData::BasePointParameter(value) => {
            one_point_parameter_props(&value.parameter, p);
            p.extend([
                ("Point1011", vec3(value.point)),
                ("Point1012", vec3(value.base_point)),
            ]);
        }
        DynamicBlockData::FlipParameter(value) => {
            two_point_parameter_props(&value.parameter, p);
            p.extend([
                ("BaseStateName", text(&value.base_state_label)),
                ("Description", text(&value.flip_label_description)),
                ("FlippedStateName", text(&value.flipped_state_label)),
                ("Label", text(&value.flip_label)),
                ("LabelPosition", vec3(value.definition_label_point)),
                (
                    "UpdatedFlipConnection",
                    eval_connection(value.flags_96, &value.tooltip),
                ),
            ]);
        }
        DynamicBlockData::LinearParameter(value) => {
            two_point_parameter_props(&value.parameter, p);
            p.extend([
                ("Description", text(&value.distance_description)),
                ("Label", text(&value.distance_name)),
                ("LabelOffset", number(value.distance)),
                ("ValueSet", parameter_value_set(&value.value_set)),
            ]);
        }
        DynamicBlockData::LookupParameter(value) => {
            one_point_parameter_props(&value.parameter, p);
            p.extend([
                ("ActionId", int(value.index as i64)),
                ("Description", text(&value.lookup_description)),
                ("Label", text(&value.lookup_name)),
            ]);
        }
        DynamicBlockData::PointParameter(value) => {
            one_point_parameter_props(&value.parameter, p);
            p.extend([
                ("Description", text(&value.position_description)),
                ("Label", text(&value.position_name)),
                ("LabelPosition", vec3(value.definition_label_point)),
            ]);
        }
        DynamicBlockData::PolarParameter(value) => {
            two_point_parameter_props(&value.parameter, p);
            p.extend([
                ("AngleDescription", text(&value.angle_description)),
                ("AngleName", text(&value.angle_name)),
                ("AngleValueSet", parameter_value_set(&value.angle_value_set)),
                (
                    "DistanceValueSet",
                    parameter_value_set(&value.distance_value_set),
                ),
                ("Description", text(&value.distance_description)),
                ("Label", text(&value.distance_name)),
                ("LabelOffset", number(value.offset)),
            ]);
        }
        DynamicBlockData::RotationParameter(value) => {
            two_point_parameter_props(&value.parameter, p);
            p.extend([
                ("Label", text(&value.angle_name)),
                ("Description", text(&value.angle_description)),
                ("LabelOffset", number(value.angle)),
                ("Point", vec3(value.definition_base_angle_point)),
                ("ValueSet", parameter_value_set(&value.value_set)),
            ]);
        }
        DynamicBlockData::VisibilityParameter(value) => {
            visibility_parameter_props(doc, reader, value, p)
        }
        DynamicBlockData::XYParameter(value) => {
            two_point_parameter_props(&value.parameter, p);
            p.extend([
                ("DescriptionX", text(&value.x_label_description)),
                ("DescriptionY", text(&value.y_label_description)),
                ("LabelX", text(&value.x_label)),
                ("LabelY", text(&value.y_label)),
                ("LabelOffsetX", number(value.x_value)),
                ("LabelOffsetY", number(value.y_value)),
                ("ValueSetX", parameter_value_set(&value.x_value_set)),
                ("ValueSetY", parameter_value_set(&value.y_value_set)),
            ]);
        }
        DynamicBlockData::MoveAction(value) => {
            action_props(&value.action, p);
            p.extend([
                ("AngleOffset", number(value.offsets.angle_offset)),
                (
                    "DistanceMultiplier",
                    number(value.offsets.distance_multiplier),
                ),
                ("UnknownFlag", int(value.offsets.flags as i64)),
            ]);
        }
        DynamicBlockData::FlipAction(value) => action_props(&value.action, p),
        DynamicBlockData::RotateAction(value) | DynamicBlockData::ScaleAction(value) => {
            action_props(&value.action.action, p)
        }
        DynamicBlockData::ArrayAction(value) => {
            action_props(&value.action, p);
            p.extend([
                ("ColumnOffset", number(value.column_offset)),
                ("RowOffset", number(value.row_offset)),
            ]);
        }
        DynamicBlockData::LookupAction(value) => {
            action_props(&value.action, p);
            p.push(("UnknownFlag", bool_value(value.flag_280)));
            p.push((
                "Columns",
                Value::Array(
                    value
                        .columns
                        .iter()
                        .map(|column| {
                            props(
                                "BlockLookupAction.ColumnData",
                                [
                                    ("ConnectionName", text(&column.connection_name)),
                                    ("IsLookupProperty", bool_value(column.lookup_property)),
                                    ("IsReadOnly", bool_value(column.writable)),
                                    ("NodeId", int(column.node_id as i64)),
                                    ("Type", int(column.property_type as i64)),
                                    ("UnmatchedName", text(&column.unmatched_name)),
                                    ("ValueType", int(column.value_type as i64)),
                                ],
                            )
                        })
                        .collect(),
                ),
            ));
        }
        DynamicBlockData::StretchAction(value) => {
            action_props(&value.action, p);
            p.extend([
                ("AngleOffset", number(value.offsets.angle_offset)),
                (
                    "DistanceMultiplier",
                    number(value.offsets.distance_multiplier),
                ),
                (
                    "Boundary",
                    Value::Array(value.points.iter().copied().map(vec2).collect()),
                ),
                ("UnknownFlag", int(value.offsets.flags as i64)),
            ]);
        }
        DynamicBlockData::PolarStretchAction(value) => {
            action_props(&value.action, p);
            p.extend([
                ("AngleOffset", number(value.angle_offset)),
                ("DistanceMultiplier", number(value.distance_multiplier)),
                (
                    "Boundary",
                    Value::Array(value.points.iter().copied().map(vec2).collect()),
                ),
                ("RotateBindings", handles(value.handles.iter().copied())),
            ]);
        }
        DynamicBlockData::AngularConstraintParameter(value) => {
            two_point_parameter_props(&value.constraint.parameter, p);
            p.extend([
                ("CenterPoint", vec3(value.center_point)),
                ("EndPoint", vec3(value.end_point)),
                ("ExpressionName", text(&value.expression_name)),
                ("ExpressionDescription", text(&value.expression_description)),
                ("ValueSet", parameter_value_set(&value.value_set)),
            ]);
        }
        DynamicBlockData::DiametricConstraintParameter(value)
        | DynamicBlockData::RadialConstraintParameter(value) => {
            two_point_parameter_props(&value.constraint.parameter, p);
            p.extend([
                ("ExpressionName", text(&value.expression_name)),
                ("ExpressionDescription", text(&value.expression_description)),
                ("ValueSet", parameter_value_set(&value.value_set)),
            ]);
        }
        DynamicBlockData::AlignedConstraintParameter(value)
        | DynamicBlockData::LinearConstraintParameter(value)
        | DynamicBlockData::HorizontalConstraintParameter(value)
        | DynamicBlockData::VerticalConstraintParameter(value) => {
            two_point_parameter_props(&value.constraint.parameter, p);
            p.extend([
                ("ExpressionName", text(&value.expression_name)),
                ("ExpressionDescription", text(&value.expression_description)),
                ("ValueSet", parameter_value_set(&value.value_set)),
            ]);
        }
        DynamicBlockData::EvaluationGraph(value) => {
            p.extend([
                ("Value96", int(value.first_node_id as i64)),
                ("Value97", int(value.first_node_id_copy as i64)),
                (
                    "Nodes",
                    Value::Array(
                        value
                            .nodes
                            .iter()
                            .map(|node| {
                                json!({
                                    "Data1": node.node_data[0],
                                    "Data2": node.node_data[1],
                                    "Data3": node.node_data[2],
                                    "Data4": node.node_data[3],
                                    "Expression": handle(node.expression),
                                    "Flags": node.edge_flags,
                                    "Id": node.next_id,
                                    "Index": node.id,
                                })
                            })
                            .collect(),
                    ),
                ),
                (
                    "Edges",
                    Value::Array(
                        value
                            .edges
                            .iter()
                            .map(|edge| {
                                json!({
                                    "Data1": edge.outgoing_edges[0],
                                    "Data2": edge.outgoing_edges[1],
                                    "Data3": edge.outgoing_edges[2],
                                    "Data4": edge.outgoing_edges[3],
                                    "Data5": edge.outgoing_edges[4],
                                    "Flags": edge.next_id,
                                    "FromNodeIndex": edge.source_node,
                                    "Index": edge.id,
                                    "ToNodeIndex": edge.destination_node,
                                    "TrackedCount": edge.incoming_edge,
                                })
                            })
                            .collect(),
                    ),
                ),
            ]);
        }
        DynamicBlockData::ParameterDependencyBody(value) => {
            p.extend([
                ("Version", int(value.class_version as i64)),
                ("Name", text(&value.name)),
            ]);
        }
        DynamicBlockData::SolidHistory(_) | DynamicBlockData::SolidHistoryNode(_) => {}
        DynamicBlockData::ProxyNode(_)
        | DynamicBlockData::UserParameter(_)
        | DynamicBlockData::AngularConstraintParameterEntity(_)
        | DynamicBlockData::AlignmentParameterEntity
        | DynamicBlockData::BasePointParameterEntity
        | DynamicBlockData::FlipParameterEntity
        | DynamicBlockData::LinearParameterEntity
        | DynamicBlockData::PointParameterEntity
        | DynamicBlockData::RotationParameterEntity
        | DynamicBlockData::VisibilityParameterEntity
        | DynamicBlockData::XYParameterEntity
        | DynamicBlockData::FlipGripEntity
        | DynamicBlockData::LinearGripEntity
        | DynamicBlockData::PolarGripEntity
        | DynamicBlockData::RotationGripEntity
        | DynamicBlockData::VisibilityGripEntity
        | DynamicBlockData::XYGripEntity
        | DynamicBlockData::Unknown => {}
    }
    dynamic_actions::extend(value, p, doc);
}

// The following objects have deliberately small public models in acadrust.
// Keep their scalar and handle members explicit here; properties that have no
// equivalent model are left for `props` to mark as `!unmapped`.

/// The `CadValue` a FIELD holds (`readCadValue`; the field template never
/// builds its value templates, so a handle value stays unset).
fn field_cad_value(
    doc: &crate::CadDocument,
    reader: &str,
    value: &crate::entities::CellValue,
) -> Value {
    use crate::entities::CellValueType as Kind;
    let dxf = reader == "dxf";
    let r2007 = doc.version >= DxfVersion::AC1021;
    let body = if r2007 { value.flags & 1 == 0 } else { true };
    let shown = if !body {
        Value::Null
    } else {
        match value.value_type {
            Kind::Unknown | Kind::Long => int(value.numeric_value as i64),
            Kind::Double => number(value.numeric_value),
            // Before R2007 the reference decodes the stored bytes including
            // the terminating NUL.
            Kind::String | Kind::General if !dxf && !r2007 => text(format!("{}\0", value.text)),
            Kind::String | Kind::General => text(&value.text),
            Kind::Date if !dxf => super::entities::table::date_value(&value.binary_value),
            Kind::Point2D => vec2(Vector2::new(value.point_value.x, value.point_value.y)),
            Kind::Point3D => vec3(value.point_value),
            Kind::Handle | Kind::Date | Kind::Buffer | Kind::ResultBuffer => Value::Null,
        }
    };
    let strings = |s: &str| if r2007 { text(s) } else { Value::Null };
    props(
        "CadValue",
        [
            ("Flags", int(value.flags as i64)),
            ("Format", strings(&value.format)),
            ("FormattedValue", strings(&value.formatted_value)),
            ("Units", int(value.raw_unit_type_code as i64)),
            ("Value", shown),
            ("ValueType", int(value.raw_type_code as i64)),
        ],
    )
}

fn dump_field(
    doc: &crate::CadDocument,
    reader: &str,
    handle_value: Handle,
    value: &crate::objects::Field,
) -> Value {
    let mut p = common("FIELD", UNLISTED, "AcDbField", None, None, None);
    p.extend([
        // `CadObjects` holds the 331 references only; child fields go to
        // `Children`.
        ("CadObjects", handles(value.referenced_objects.iter().copied())),
        ("Children", handles(value.child_fields.iter().copied())),
        (
            "EvaluationErrorCode",
            int(value.evaluation_error_code as i64),
        ),
        (
            "EvaluationErrorMessage",
            text(&value.evaluation_error_message),
        ),
        ("EvaluationOptionFlags", int(value.evaluation_option as i64)),
        ("EvaluationStatusFlags", int(value.evaluation_status as i64)),
        ("EvaluatorId", text(&value.evaluator_id)),
        ("FieldCode", text(&value.code)),
        ("FieldStateFlags", int(value.state as i64)),
        ("FilingOptionFlags", int(value.filing_option as i64)),
        // Both readers assign the value string (DWG: the last text of the
        // record, after the older format string; DXF: group 301).
        // (DXF keeps the group's source caret escapes.)
        (
            "FormatString",
            text(match &value.value_string_source.0 {
                Some(source) if reader == "dxf" => source,
                _ => &value.value_string,
            }),
        ),
        ("ObjectName", text("FIELD")),
        ("Value", field_cad_value(doc, reader, &value.value)),
        (
            "Values",
            Value::Array(
                value
                    .child_values
                    .iter()
                    .map(|child| json!([child.key, field_cad_value(doc, reader, &child.value)]))
                    .collect(),
            ),
        ),
    ]);
    record(handle_value, "Field", value.owner, props("Field", p))
}

fn dump_field_list(handle_value: Handle, value: &crate::objects::FieldList) -> Value {
    let mut p = common("FIELDLIST", UNLISTED, "AcDbFieldList", None, None, None);
    p.extend([
        ("Fields", handles(value.fields.iter().copied())),
        ("ObjectName", text("FIELDLIST")),
    ]);
    record(
        handle_value,
        "FieldList",
        value.owner,
        props("FieldList", p),
    )
}

fn context_base(
    value: &ObjectContextData,
    object_name: &str,
    class: &str,
    props_type: &str,
) -> Vec<(&'static str, Value)> {
    let mut p = common(
        object_name,
        UNLISTED,
        "AcDbObjectContextData",
        None,
        Some(&value.reactors),
        value.xdictionary_handle,
    );
    p.extend([
        ("Default", bool_value(value.is_default)),
        (
            "HasFileToExtensionDictionary",
            bool_value(value.xdictionary_handle.is_some()),
        ),
        ("Scale", handle(value.scale)),
        ("Version", int(value.class_version as i64)),
    ]);
    // `class` and `props_type` are kept as arguments to make each concrete
    // branch self-documenting; the marker is also useful to consumers that
    // inspect a dump without consulting the type tag.
    let _ = (class, props_type);
    p
}

fn dump_object_context_data(handle_value: Handle, value: &ObjectContextData) -> Value {
    match &value.kind {
        crate::objects::ObjectContextKind::BlkRef {
            rotation,
            insertion,
            scale_factor,
        } => {
            let mut p = context_base(
                value,
                "ACDB_BLKREFOBJECTCONTEXTDATA_CLASS",
                "AcDbBlkRefObjectContextData",
                "BlockReferenceObjectContextData",
            );
            p.extend([
                ("InsertionPoint", vec3(*insertion)),
                ("ObjectName", text("ACDB_BLKREFOBJECTCONTEXTDATA_CLASS")),
                ("SubclassMarker", text("AcDbAnnotScaleObjectContextData")),
                // The reader never assigns this public property, so it keeps
                // its constructor default; it is unrelated to the common
                // extension-dictionary reference.
                ("HasFileToExtensionDictionary", bool_value(true)),
                ("Rotation", number(*rotation)),
                ("XScale", number(scale_factor.x)),
                ("YScale", number(scale_factor.y)),
                ("ZScale", number(scale_factor.z)),
            ]);
            record(
                handle_value,
                "BlockReferenceObjectContextData",
                value.owner_handle,
                props("BlockReferenceObjectContextData", p),
            )
        }
        crate::objects::ObjectContextKind::MTextAttribute(context) => {
            let mut p = context_base(
                value,
                "ACDB_MTEXTATTRIBUTEOBJECTCONTEXTDATA_CLASS",
                "AcDbMTextAttributeObjectContextData",
                "MTextAttributeObjectContextData",
            );
            p.extend([
                (
                    "AlignmentPoint",
                    tuple2((context.alignment.x, context.alignment.y)),
                ),
                ("AttachmentPoint", int(context.horizontal_mode as i64)),
                (
                    "InsertPoint",
                    tuple2((context.insertion.x, context.insertion.y)),
                ),
                ("ObjectName", text("MTEXTATTRIBUTEOBJECTCONTEXTDATA")),
                ("Rotation", number(context.rotation)),
                ("Value290", bool_value(context.enable_context)),
            ]);
            if let Some(embedded) = &context.context {
                p.push(("Default", bool_value(embedded.is_default)));
                p.push(("Scale", handle(embedded.scale)));
                p.push(("Version", int(embedded.class_version as i64)));
            }
            record(
                handle_value,
                "MTextAttributeObjectContextData",
                value.owner_handle,
                props("MTextAttributeObjectContextData", p),
            )
        }
        crate::objects::ObjectContextKind::MLeader(context) => {
            dump_mleader_context(handle_value, value, context)
        }
        // Preserve unsupported context leaves through the unknown-object
        // schema rather than assigning another context class's identity.
        _ => {
            let mut p = context_base(
                value,
                "ACDB_OBJECTCONTEXTDATA_CLASS",
                "AcDbObjectContextData",
                "BlockReferenceObjectContextData",
            );
            p.push(("ObjectName", text(value.class_name())));
            typed_record(
                handle_value,
                &class_name(value.class_name()),
                value.owner_handle,
                p,
            )
        }
    }
}

fn mleader_color(value: crate::types::Color) -> Value {
    let mut result = color(value);
    if let crate::types::Color::Rgb { r, g, b } = value {
        result["TrueColor"] =
            serde_json::json!(i32::from(r) | (i32::from(g) << 8) | (i32::from(b) << 16));
    }
    result
}

fn dump_mleader_context(
    handle_value: Handle,
    value: &ObjectContextData,
    context: &crate::entities::multileader::MultiLeaderAnnotContext,
) -> Value {
    let mut p = context_base(
        value,
        "ACDB_MLEADEROBJECTCONTEXTDATA_CLASS",
        "AcDbMLeaderObjectContextData",
        "MultiLeaderObjectContextData",
    );
    p.extend([
        ("ArrowheadSize", number(context.arrowhead_size)),
        ("BackgroundFillColor", mleader_color(context.background_fill_color)),
        (
            "BackgroundFillEnabled",
            bool_value(context.background_fill_enabled),
        ),
        (
            "BackgroundMaskFillOn",
            bool_value(context.background_mask_fill_on),
        ),
        (
            "BackgroundScaleFactor",
            number(context.background_scale_factor),
        ),
        (
            "BackgroundTransparency",
            int(context.background_transparency as i64),
        ),
        ("BaseDirection", vec3(context.base_direction)),
        ("BasePoint", vec3(context.base_point)),
        ("BaseVertical", vec3(context.base_vertical)),
        (
            "BlockContent",
            optional_handle(context.block_content_handle),
        ),
        ("BlockContentColor", mleader_color(context.block_content_color)),
        (
            "BlockContentConnection",
            int(context.block_connection_type as i64),
        ),
        ("BlockContentLocation", vec3(context.block_content_location)),
        ("BlockContentNormal", vec3(context.block_content_normal)),
        ("BlockContentRotation", number(context.block_rotation)),
        ("BlockContentScale", vec3(context.block_content_scale)),
        ("BoundaryHeight", number(context.text_boundary_height)),
        ("BoundaryWidth", number(context.text_width)),
        (
            "ColumnFlowReversed",
            bool_value(context.column_flow_reversed),
        ),
        ("ColumnGutter", number(context.column_gutter)),
        ("ColumnSizes", numbers(context.column_sizes.iter().copied())),
        ("ColumnType", int(context.column_type as i64)),
        ("ColumnWidth", number(context.column_width)),
        ("ContentBasePoint", vec3(context.content_base_point)),
        ("Direction", vec3(context.text_direction)),
        ("FlowDirection", int(context.text_flow_direction as i64)),
        ("HasContentsBlock", bool_value(context.has_block_contents)),
        ("HasTextContents", bool_value(context.has_text_contents)),
        ("LandingGap", number(context.landing_gap)),
        ("LineSpacing", int(context.line_spacing_style as i64)),
        ("LineSpacingFactor", number(context.line_spacing_factor)),
        ("NormalReversed", bool_value(context.normal_reversed)),
        ("ObjectName", text("MLEADEROBJECTCONTEXTDATA")),
        ("ScaleFactor", number(context.scale_factor)),
        ("TextAlignment", int(context.text_alignment as i64)),
        (
            "TextAttachmentPoint",
            int(context.text_attachment_point as i64),
        ),
        (
            "TextBottomAttachment",
            int(context.text_bottom_attachment as i64),
        ),
        ("TextColor", mleader_color(context.text_color)),
        ("TextHeight", number(context.text_height)),
        (
            "TextHeightAutomatic",
            bool_value(context.text_height_automatic),
        ),
        ("TextLabel", text(&context.text_string)),
        (
            "TextLeftAttachment",
            int(context.text_left_attachment as i64),
        ),
        ("TextLocation", vec3(context.text_location)),
        ("TextNormal", vec3(context.text_normal)),
        (
            "TextRightAttachment",
            int(context.text_right_attachment as i64),
        ),
        ("TextRotation", number(context.text_rotation)),
        ("TextStyle", optional_handle(context.text_style_handle)),
        ("TextTopAttachment", int(context.text_top_attachment as i64)),
        (
            "TransformationMatrix",
            matrix4(&Matrix4 {
                m: [
                    [
                        context.transform_matrix[0],
                        context.transform_matrix[1],
                        context.transform_matrix[2],
                        context.transform_matrix[3],
                    ],
                    [
                        context.transform_matrix[4],
                        context.transform_matrix[5],
                        context.transform_matrix[6],
                        context.transform_matrix[7],
                    ],
                    [
                        context.transform_matrix[8],
                        context.transform_matrix[9],
                        context.transform_matrix[10],
                        context.transform_matrix[11],
                    ],
                    [
                        context.transform_matrix[12],
                        context.transform_matrix[13],
                        context.transform_matrix[14],
                        context.transform_matrix[15],
                    ],
                ],
            }),
        ),
        ("WordBreak", bool_value(context.word_break)),
    ]);
    p.push((
        "LeaderRoots",
        Value::Array(context.leader_roots.iter().map(mleader_root).collect()),
    ));
    record(
        handle_value,
        "MultiLeaderObjectContextData",
        value.owner_handle,
        props("MultiLeaderObjectContextData", p),
    )
}

fn mleader_root(value: &crate::entities::multileader::LeaderRoot) -> Value {
    props(
        "MultiLeaderObjectContextData.LeaderRoot",
        [
            (
                "BreakStartEndPointsPairs",
                Value::Array(value.break_points.iter().map(start_end_pair).collect()),
            ),
            ("ConnectionPoint", vec3(value.connection_point)),
            ("ContentValid", bool_value(value.content_valid)),
            ("Direction", vec3(value.direction)),
            ("LandingDistance", number(value.landing_distance)),
            ("LeaderIndex", int(value.leader_index as i64)),
            (
                "Lines",
                Value::Array(value.lines.iter().map(mleader_line).collect()),
            ),
            (
                "TextAttachmentDirection",
                int(value.text_attachment_direction as i64),
            ),
            ("Unknown", bool_value(value.unknown)),
        ],
    )
}

fn mleader_line(value: &crate::entities::multileader::LeaderLine) -> Value {
    props(
        "MultiLeaderObjectContextData.LeaderLine",
        [
            ("Arrowhead", optional_handle(value.arrowhead_handle)),
            ("ArrowheadSize", number(value.arrowhead_size)),
            (
                "BreakInfoEntries",
                Value::Array(
                    value
                        .break_infos
                        .iter()
                        .map(|break_info| {
                            props(
                                "MultiLeaderObjectContextData.BreakInfo",
                                [
                                    ("SegmentIndex", int(break_info.segment_index as i64)),
                                    (
                                        "StartEndPoints",
                                        Value::Array(
                                            break_info
                                                .break_points
                                                .iter()
                                                .map(start_end_pair)
                                                .collect(),
                                        ),
                                    ),
                                ],
                            )
                        })
                        .collect(),
                ),
            ),
            ("Index", int(value.index as i64)),
            ("LineColor", mleader_color(value.line_color)),
            ("LineWeight", int(value.line_weight.value() as i64)),
            ("OverrideFlags", int(value.override_flags.bits() as i64)),
            ("PathType", int(value.path_type as i64)),
            (
                "Points",
                Value::Array(value.points.iter().copied().map(vec3).collect()),
            ),
        ],
    )
}

fn start_end_pair(value: &crate::entities::multileader::StartEndPointPair) -> Value {
    props(
        "MultiLeaderObjectContextData.StartEndPointPair",
        [
            ("EndPoint", vec3(value.end_point)),
            ("StartPoint", vec3(value.start_point)),
        ],
    )
}

fn dump_table_style(
    doc: &crate::CadDocument,
    reader: &str,
    handle_value: Handle,
    value: &TableStyle,
) -> Value {
    let dxf = value.raw_dxf_codes.as_deref();
    let is_dxf = dxf.is_some() || reader == "dxf";
    let modern = !is_dxf && value.modern_style.is_some();
    let source_modern = reader == "dwg" && modern;
    let raw = |code| {
        dxf.and_then(|codes| {
            codes
                .iter()
                .rev()
                .find(|(c, _)| *c == code)
                .map(|(_, v)| v.as_str())
        })
    };
    let source_flag = |code, native: bool| {
        raw(code)
            .and_then(|v| v.parse::<i16>().ok())
            .map_or(if dxf.is_some() { false } else { native }, |v| v != 0)
    };
    let row = |native: &crate::objects::RowCellStyle,
               default: crate::objects::RowCellStyle,
               name,
               class| {
        if is_dxf || (reader == "new" && *native == default && !modern) {
            // DXF creates unnamed cell templates. The builder only installs
            // named built-ins, so those parsed rows remain unexposed.
            default_cell_style(name, class)
        } else if modern {
            modern_row_style(value, name, class)
        } else {
            legacy_row_style(native, name, class, doc.version)
        }
    };
    let name = owned_dictionary_name(doc, handle_value, value.owner_handle)
        .unwrap_or_else(|| value.name.clone());
    let mut p = common(
        "TABLESTYLE",
        UNLISTED,
        "AcDbTableStyle",
        Some(&name),
        None,
        None,
    );
    let description = if (reader == "new" || (dxf.is_some() && raw(3).is_none()))
        && value.description.is_empty()
    {
        Value::Null
    } else {
        text(&value.description)
    };
    let flags = if source_modern {
        0
    } else if let Some(codes) = dxf {
        codes
            .iter()
            .rev()
            .find(|(c, _)| *c == 71)
            .and_then(|(_, v)| v.parse::<i16>().ok())
            .unwrap_or(0)
    } else {
        value
            .legacy_source
            .map_or(value.flags.bits(), |source| source.0)
    };
    p.extend([
        (
            "CellStyles",
            if reader == "dwg" || is_dxf {
                json!([])
            } else {
                Value::Array(
                    value
                        .modern_overrides
                        .iter()
                        .filter(|(_, style)| {
                            !["_DATA", "_TITLE", "_HEADER"]
                                .iter()
                                .any(|n| style.name.eq_ignore_ascii_case(n))
                        })
                        .map(|(_, style)| named_cell_style(style))
                        .collect(),
                )
            },
        ),
        (
            "DataCellStyle",
            row(
                &value.data_row_style,
                crate::objects::RowCellStyle::data_row(),
                "_DATA",
                2,
            ),
        ),
        (
            "TitleCellStyle",
            row(
                &value.title_row_style,
                crate::objects::RowCellStyle::title_row(),
                "_TITLE",
                1,
            ),
        ),
        (
            "HeaderCellStyle",
            row(
                &value.header_row_style,
                crate::objects::RowCellStyle::header_row(),
                "_HEADER",
                1,
            ),
        ),
        (
            "TableCellStyle",
            if modern {
                named_cell_style(value.modern_style.as_ref().unwrap())
            } else {
                default_cell_style("Table", 2)
            },
        ),
        ("Description", description),
        ("Flags", int(flags as i64)),
        (
            "FlowDirection",
            int(if source_modern {
                0
            } else {
                value.flow_direction as i64
            }),
        ),
        (
            "HorizontalCellMargin",
            number(if source_modern {
                0.06
            } else {
                value.horizontal_margin
            }),
        ),
        (
            "VerticalCellMargin",
            number(if source_modern {
                0.06
            } else {
                value.vertical_margin
            }),
        ),
        (
            "SuppressHeaderRow",
            bool_value(
                !source_modern
                    && source_flag(
                        281,
                        value.legacy_source.map_or(value.header_suppressed, |s| s.2),
                    ),
            ),
        ),
        (
            "SuppressTitle",
            bool_value(
                !source_modern
                    && source_flag(
                        280,
                        value.legacy_source.map_or(value.title_suppressed, |s| s.1),
                    ),
            ),
        ),
    ]);
    record(
        handle_value,
        "TableStyle",
        value.owner_handle,
        props("TableStyle", p),
    )
}

fn default_cell_style(name: &str, style_class: i32) -> Value {
    let border = |edge| default_border(edge);
    props(
        "TableStyle.CellStyle",
        [
            ("Alignment", int(0)),
            ("BackgroundColor", color(Color::ByBlock)),
            ("BottomBorder", border(4)),
            ("BottomMargin", number(0.0)),
            ("CellAlignment", int(0)),
            ("Color", color(Color::ByBlock)),
            ("ContentColor", color(Color::ByBlock)),
            ("ContentLayoutFlags", int(0)),
            ("HasData", bool_value(true)),
            ("HorizontalInsideBorder", border(32)),
            ("HorizontalMargin", number(0.06)),
            ("IsFillColorOn", bool_value(false)),
            ("LeftBorder", border(8)),
            ("MarginHorizontalSpacing", number(0.0)),
            ("MarginOverrideFlags", int(0)),
            ("MarginVerticalSpacing", number(0.0)),
            ("Name", text(name)),
            ("PropertyFlags", int(0)),
            ("PropertyOverrideFlags", int(0)),
            ("RightBorder", border(2)),
            ("RightMargin", number(0.0)),
            ("Rotation", number(0.0)),
            ("Scale", number(0.0)),
            ("StyleClass", int(style_class as i64)),
            ("TableCellStylePropertyFlags", int(0)),
            ("TextColor", color(Color::ByBlock)),
            ("TextHeight", number(0.0)),
            ("TextStyle", handle(Handle::NULL)),
            ("TopBorder", border(2)),
            ("Type", int(0)),
            ("ValueDataType", int(0)),
            ("ValueFormatString", Value::Null),
            ("ValueUnitType", int(0)),
            ("VerticalInsideBorder", border(16)),
            ("VerticalMargin", number(0.06)),
        ],
    )
}

/// Table components have the same stored style shape as named table styles,
/// but their constructor leaves the name null and HasData false.
pub(super) fn table_component_default() -> Value {
    let mut value = default_cell_style("", 0);
    value["Name"] = Value::Null;
    value["HasData"] = bool_value(false);
    value
}

pub(super) fn table_component_styles(
    doc: &crate::CadDocument,
    table: &crate::entities::Table,
) -> Option<Value> {
    let h = table.table_style_handle?;
    let ObjectType::TableStyle(style) = doc.objects.get(&h)? else {
        return None;
    };
    let reader = if doc.dwg_source_version.is_some() { "dwg" } else { "dxf" };
    Some(dump_table_style(doc, reader, h, style)["props"].take())
}

pub(super) fn table_content_color(value: Color) -> Value {
    style_color(value)
}

fn default_border(edge_flags: i32) -> Value {
    props(
        "TableStyle.CellBorder",
        [
            ("ApplyBorder", bool_value(false)),
            ("Color", color(Color::ByBlock)),
            ("DoubleLineSpacing", number(0.0)),
            ("EdgeFlags", int(edge_flags as i64)),
            ("IsInvisible", bool_value(false)),
            (
                "LineWeight",
                int(crate::types::LineWeight::ByBlock.value() as i64),
            ),
            ("PropertyOverrideFlags", int(0)),
            ("Type", int(1)),
        ],
    )
}

fn style_color(value: Color) -> Value {
    // C# treats the CMC "none" marker as RGB black, and exposes true color
    // in little-endian RGB order. Keep the native color/method unchanged.
    let value = if value == Color::None {
        Color::from_rgb(0, 0, 0)
    } else {
        value
    };
    if let Some(rgb) = value.to_true_color_value() {
        material_color(Some(rgb))
    } else {
        color(value)
    }
}

fn legacy_row_style(
    value: &crate::objects::RowCellStyle,
    name: &str,
    style_class: i32,
    version: DxfVersion,
) -> Value {
    let mut result = default_cell_style(name, style_class);
    for (key, field) in [
        ("BackgroundColor", style_color(value.fill_color)),
        ("CellAlignment", int(value.alignment as i64)),
        ("IsFillColorOn", bool_value(value.fill_enabled)),
        ("TextColor", style_color(value.text_color)),
        ("TextHeight", number(value.text_height)),
        (
            "TextStyle",
            handle(value.text_style_handle.unwrap_or(Handle::NULL)),
        ),
    ] {
        result[key] = field;
    }
    for (key, edge, border) in [
        ("TopBorder", 2, &value.top_border),
        (
            "HorizontalInsideBorder",
            32,
            &value.horizontal_inside_border,
        ),
        ("BottomBorder", 4, &value.bottom_border),
        ("LeftBorder", 8, &value.left_border),
        ("VerticalInsideBorder", 16, &value.vertical_inside_border),
        ("RightBorder", 2, &value.right_border),
    ] {
        let mut projected = default_border(edge);
        projected["Color"] = style_color(border.color);
        projected["LineWeight"] = int(border.line_weight.value() as i64);
        projected["IsInvisible"] = bool_value(border.is_invisible);
        result[key] = projected;
    }
    if version >= DxfVersion::AC1021 {
        result["ValueDataType"] = int(value.data_type as i64);
        result["ValueUnitType"] = int(value.unit_type as i64);
        result["ValueFormatString"] = text(&value.format_string);
    }
    result
}

fn modern_row_style(value: &TableStyle, name: &str, style_class: i32) -> Value {
    value
        .modern_overrides
        .iter()
        .find(|(_, style)| style.name.eq_ignore_ascii_case(name))
        .map(|(_, style)| named_cell_style(style))
        .unwrap_or_else(|| default_cell_style(name, style_class))
}

fn named_cell_style(value: &crate::objects::NamedTableCellStyle) -> Value {
    let data = &value.cell_style;
    let format = &data.content_format;
    let mut result = default_cell_style(&value.name, value.style_type);
    result["Type"] = int(data.style_type as i64);
    result["HasData"] = bool_value(data.data_flags != 0);
    if data.data_flags == 0 {
        return result;
    }
    // The content-format assignment overwrites the first override flag slot;
    // Color, ContentColor and TextColor are distinct stored C# properties.
    for (key, field) in [
        ("Alignment", int(format.cell_alignment as i64)),
        ("BackgroundColor", style_color(data.background_color)),
        ("Color", style_color(format.content_color)),
        ("ContentLayoutFlags", int(data.content_layout as i64)),
        (
            "MarginOverrideFlags",
            int(data.margin_override_flags as i64),
        ),
        ("PropertyFlags", int(format.property_flags as i64)),
        (
            "PropertyOverrideFlags",
            int(format.property_override_flags as i64),
        ),
        ("Rotation", number(format.rotation)),
        ("Scale", number(format.block_scale)),
        ("TableCellStylePropertyFlags", int(data.merge_flags as i64)),
        ("TextHeight", number(format.text_height)),
        ("TextStyle", handle(format.text_style)),
        ("ValueDataType", int(format.value_data_type as i64)),
        ("ValueFormatString", text(&format.value_format_string)),
        ("ValueUnitType", int(format.value_unit_type as i64)),
    ] {
        result[key] = field;
    }
    if data.margin_override_flags & 1 != 0 {
        for (key, field) in [
            ("HorizontalMargin", data.horizontal_margin),
            ("VerticalMargin", data.vertical_margin),
            ("BottomMargin", data.bottom_margin),
            ("RightMargin", data.right_margin),
            ("MarginHorizontalSpacing", data.horizontal_spacing),
            ("MarginVerticalSpacing", data.vertical_spacing),
        ] {
            result[key] = number(field);
        }
    }
    for grid in &data.borders {
        let (key, edge) = match grid.index_mask {
            // The reference declares its top border with Right's immutable
            // EdgeFlags, even when the raw source mask is Top.
            1 => ("TopBorder", 2),
            2 => ("RightBorder", 2),
            4 => ("BottomBorder", 4),
            8 => ("LeftBorder", 8),
            16 => ("VerticalInsideBorder", 16),
            32 => ("HorizontalInsideBorder", 32),
            _ => continue,
        };
        let border = &grid.border;
        let mut projected = default_border(edge);
        for (key, field) in [
            ("ApplyBorder", bool_value(true)),
            ("Color", style_color(border.color)),
            ("DoubleLineSpacing", number(border.double_line_spacing)),
            ("IsInvisible", bool_value(!border.is_invisible)),
            ("LineWeight", int(border.line_weight.value() as i64)),
            (
                "PropertyOverrideFlags",
                int(border.property_flags.bits() as i64),
            ),
            ("Type", int(border.border_type as i64)),
        ] {
            projected[key] = field;
        }
        result[key] = projected;
    }
    result
}

fn dump_table_content(
    doc: &crate::CadDocument,
    handle_value: Handle,
    value: &crate::entities::Table,
) -> Value {
    let mut p = common(
        "TABLECONTENT", UNLISTED, "AcDbTableContent", Some(&value.name),
        Some(&value.common.reactors), value.common.xdictionary_handle,
    );
    p.extend(super::entities::table::content_fields(doc, value));
    p.extend([
        ("Description", text(&value.description)),
        ("StyleOverride", Value::Null),
    ]);
    record(handle_value, "TableContent", value.common.owner_handle, props("TableContent", p))
}

fn dump_proxy_object(
    doc: &crate::CadDocument,
    handle_value: Handle,
    value: &crate::objects::ProxyObject,
) -> Value {
    let mut p = common(
        "ACAD_PROXY_OBJECT",
        UNLISTED,
        "AcDbProxyObject",
        None,
        Some(&value.reactors),
        value.xdictionary_handle,
    );
    let dwg = doc.classes.source_classes().is_none();
    p.extend(super::proxy::members(
        doc,
        value.class_id,
        value.dwg_version,
        value.maintenance_version,
        value.from_dxf,
        value.text_payload.bit_count > 0 || value.payload.bit_count > 0,
    ));
    p.extend([
        // The DWG reader never assigns BinaryData (code 310 only exists in DXF).
        (
            "BinaryData",
            if !dwg && value.payload.bit_count > 0 {
                json!({})
            } else {
                Value::Null
            },
        ),
        ("ObjectName", text("ACAD_PROXY_OBJECT")),
        ("ProxyClassId", int(value.proxy_id as i64)),
    ]);
    record(
        handle_value,
        "ProxyObject",
        value.owner,
        props("ProxyObject", p),
    )
}

fn dump_underlay_definition(
    handle_value: Handle,
    value: &crate::objects::UnderlayDefinition,
) -> Value {
    // Only PDF has a concrete ACadSharp schema in this checklist. Other
    // underlay definitions retain their name through the unknown schema.
    let name = (!value.name.is_empty()).then_some(value.name.as_str());
    let mut p = common(
        value.underlay_type.definition_name(),
        UNLISTED,
        "AcDbUnderlayDefinition",
        name,
        Some(&value.reactors),
        None,
    );
    p.extend([
        ("File", text(&value.file_path)),
        (
            "Name",
            if value.name.is_empty() {
                text(super::UNMAPPED)
            } else {
                text(&value.name)
            },
        ),
        ("Page", text(&value.page_name)),
    ]);
    typed_record(
        handle_value,
        super::type_names::canonical(value.underlay_type.definition_name())
            .unwrap_or("UnknownNonGraphicalObject"),
        value.owner_handle,
        p,
    )
}

fn dump_data_object(handle_value: Handle, value: &crate::objects::DataObject) -> Value {
    // DataObject has no public ACadSharp class in the checklist.  ProxyObject
    // is the stable opaque shape used for records whose payload is not part
    // of that public model.
    let mut p = common(
        value.dxf_name(),
        UNLISTED,
        "AcDbDataObject",
        None,
        Some(&value.reactors),
        value.xdictionary_handle,
    );
    p.push(("ObjectName", text(value.dxf_name())));
    record(
        handle_value,
        "ProxyObject",
        value.owner,
        props("ProxyObject", p),
    )
}

fn dump_registered_class(
    handle_value: Handle,
    value: &crate::objects::RegisteredClassObject,
) -> Value {
    let mut p = common(
        &value.dxf_name,
        UNLISTED,
        &value.cpp_class_name,
        None,
        Some(&value.reactors),
        value.xdictionary_handle,
    );
    p.extend([("ObjectName", text(&value.dxf_name))]);
    record(
        handle_value,
        "ProxyObject",
        value.owner,
        props("ProxyObject", p),
    )
}

fn dump_dgn_line_style(handle_value: Handle, value: &crate::objects::DgnLineStyleObject) -> Value {
    let mut p = common(
        value.dxf_name(),
        UNLISTED,
        "AcDbLSDefinition",
        None,
        Some(&value.reactors),
        value.xdictionary_handle,
    );
    p.push(("ObjectName", text(value.dxf_name())));
    match &value.data {
        crate::objects::DgnLineStyleData::Definition {
            description,
            version,
            style_number,
            root_component,
            is_continuous,
            unit_definition,
            unit_scale,
            units_type,
            is_element,
            is_physical,
            is_scale_independent,
            is_snappable,
            ..
        } => p.extend([
            ("Description", text(description)),
            ("Version", int(*version as i64)),
            ("StyleNumber", int(*style_number as i64)),
            ("RootComponent", handle(*root_component)),
            ("IsContinuous", bool_value(*is_continuous)),
            ("UnitDefinition", number(*unit_definition)),
            ("UnitScale", number(*unit_scale)),
            ("UnitsType", int(*units_type as i64)),
            ("IsElement", bool_value(*is_element)),
            ("IsPhysical", bool_value(*is_physical)),
            ("IsScaleIndependent", bool_value(*is_scale_independent)),
            ("IsSnappable", bool_value(*is_snappable)),
        ]),
        crate::objects::DgnLineStyleData::Component {
            description,
            version,
            scale,
            property_flags,
            kind,
            ..
        } => p.extend([
            ("Description", text(description)),
            ("Version", int(*version as i64)),
            ("Scale", number(*scale)),
            ("PropertyFlags", int(*property_flags as i64)),
            ("ComponentType", int(kind.code() as i64)),
        ]),
        crate::objects::DgnLineStyleData::Registered { .. } => {}
    }
    record(
        handle_value,
        "ProxyObject",
        value.owner,
        props("ProxyObject", p),
    )
}

fn dump_field_definition(handle_value: Handle, value: &crate::document::FieldDef) -> Value {
    let mut p = common("FIELD", UNLISTED, "AcDbField", None, None, None);
    p.extend([
        ("CadObjects", handles(value.objects.iter().copied())),
        ("EvaluatorId", text(&value.evaluator)),
        ("FieldCode", text(&value.code)),
        ("ObjectName", text("FIELD")),
    ]);
    record(handle_value, "Field", value.owner, props("Field", p))
}

fn is_unmapped(value: Option<&Value>) -> bool {
    value.is_none_or(|value| value.as_str() == Some(super::UNMAPPED))
}

fn owned_dictionary_name(
    doc: &crate::CadDocument,
    object_handle: Handle,
    owner: Handle,
) -> Option<String> {
    let entries = match doc.objects.get(&owner)? {
        ObjectType::Dictionary(dictionary) => &dictionary.entries,
        ObjectType::DictionaryWithDefault(dictionary) => &dictionary.entries,
        _ => return None,
    };
    entries
        .iter()
        .find(|(_, h)| *h == object_handle)
        .map(|(name, _)| name.clone())
}

fn dictionary_name(
    doc: &crate::CadDocument,
    object_handle: Handle,
    owner: &Value,
) -> Option<String> {
    let owner = Handle::new(u64::from_str_radix(owner.as_str()?.strip_prefix('#')?, 16).ok()?);
    owned_dictionary_name(doc, object_handle, owner)
}

fn unknown_non_graphical_record(value: &mut Value) {
    if value.get("type").and_then(Value::as_str) != Some("ProxyObject") {
        return;
    }
    let Some(props_value) = value.get("props").and_then(Value::as_object) else {
        return;
    };
    let keys = [
        "DxfClass",
        "ExtendedData",
        "HasDynamicSubclass",
        "Name",
        "ObjectName",
        "ObjectType",
        "Reactors",
        "SubclassMarker",
        "XDictionary",
    ];
    let mapped: Vec<(&'static str, Value)> = keys
        .iter()
        .filter_map(|key| {
            let item = props_value.get(*key)?;
            (!is_unmapped(Some(item))).then(|| (*key, item.clone()))
        })
        .collect();
    let class = ["ObjectName", "SubclassMarker"]
        .iter()
        .filter_map(|key| props_value.get(*key).and_then(Value::as_str))
        .find_map(super::type_names::canonical)
        .unwrap_or("UnknownNonGraphicalObject");
    value["type"] = Value::String(class.to_string());
    value["props"] = props(class, mapped);
}

fn enrich_object_record(
    doc: &crate::CadDocument,
    object_handle: Handle,
    object: Option<&ObjectType>,
    mut value: Value,
) -> Value {
    // A proxy object has its own public ACadSharp shape. Every other record
    // that reached the opaque fallback is an unknown non-graphical object.
    if !matches!(object, Some(ObjectType::ProxyObject(_))) {
        unknown_non_graphical_record(&mut value);
    }

    let is_root_dictionary = object_handle == doc.header.named_objects_dict_handle;
    if is_root_dictionary {
        value["owner"] = handle(Handle::NULL);
    }
    let owned_name = dictionary_name(doc, object_handle, &value["owner"]);
    let unnamed_aec = matches!(
        value["type"].as_str(),
        Some("AecCleanupGroup" | "AecWallStyle")
    );

    let Some(props_value) = value.get_mut("props").and_then(Value::as_object_mut) else {
        return value;
    };

    if is_root_dictionary {
        if props_value.contains_key("Name") {
            props_value.insert("Name".to_string(), text("ROOT"));
        }
    } else if is_unmapped(props_value.get("Name")) {
        if let Some(name) = owned_name {
            props_value.insert("Name".to_string(), text(name));
        } else if unnamed_aec || matches!(
            object,
            Some(
                ObjectType::Dictionary(_)
                    | ObjectType::DictionaryWithDefault(_)
                    | ObjectType::Field(_)
                    | ObjectType::FieldList(_)
            )
        ) {
            // NonGraphicalObject.Name starts empty. An extension dictionary
            // or a child field has no dictionary key to assign it a name.
            props_value.insert("Name".to_string(), text(""));
        }
    }

    if let Some(reactors) = doc.reactors_by_handle.get(&object_handle) {
        if props_value.contains_key("Reactors") {
            props_value.insert("Reactors".to_string(), handles(reactors.iter().copied()));
        }
    } else if is_unmapped(props_value.get("Reactors")) {
        props_value.insert("Reactors".into(), json!([]));
    }
    if let Some(xdictionary_handle) = doc.xdic_by_handle.get(&object_handle) {
        if props_value.contains_key("XDictionary") {
            props_value.insert(
                "XDictionary".into(),
                optional_handle(Some(*xdictionary_handle)),
            );
        }
    } else if is_unmapped(props_value.get("XDictionary")) {
        props_value.insert("XDictionary".into(), Value::Null);
    }
    if props_value.contains_key("ExtendedData") {
        props_value.insert(
            "ExtendedData".into(),
            super::xdata::object_extended_data(doc, object_handle),
        );
    }
    value
}
