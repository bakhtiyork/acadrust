// SPDX-License-Identifier: MPL-2.0
//! Default registration policy corresponding to ACadSharp CadDocument.CreateDefaults.
//!
//! Ordering is significant: registering a style also registers its referenced
//! table entries; registering a layout registers its block, markers, then entities.
//! No identities are remapped after construction and no fixture handles are used.

use crate::entities::{Block, BlockEnd, EntityType, Viewport};
use crate::objects::{
    Dictionary, DictionaryVariable, Layout, MLineStyle, Material, MultiLeaderStyle, ObjectType,
    Scale, TableStyle,
};
use crate::tables::{AppId, BlockRecord, DimStyle, Layer, LineType, TextStyle, VPort};
use crate::types::{Handle, Vector3};
use crate::CadDocument;

/// Create ACadSharp's default document graph using native, editable objects.
///
/// This is an explicit compatibility policy, separate from [`CadDocument::new`],
/// whose AutoCAD/writer-oriented defaults and reserved handles are unchanged.
/// In particular, source classes start empty, and the first registered object
/// receives the header's initial handle seed (1).
pub fn new_document() -> CadDocument {
    let mut doc = CadDocument::new_without_defaults();
    create_defaults(&mut doc);
    // Native DWG writers require class numbers for Material/TableStyle/etc.
    // These are a writer registry, separate from the empty source class list
    // exposed by the new-document compatibility view.
    doc.classes.update_defaults();
    doc.classes.begin_source();
    doc
}

fn create_defaults(doc: &mut CadDocument) {
    // CadDocument.CreateDefaults: table constructors register immediately.
    macro_rules! table {
        ($table:ident, $header:ident) => {{
            let handle = doc.allocate_handle();
            doc.$table.set_handle(handle);
            doc.header.$header = handle;
        }};
    }
    table!(block_records, block_control_handle);
    table!(layers, layer_control_handle);
    table!(dim_styles, dimstyle_control_handle);
    table!(text_styles, style_control_handle);
    table!(line_types, linetype_control_handle);
    table!(views, view_control_handle);
    table!(ucss, ucs_control_handle);
    table!(vports, vport_control_handle);
    table!(app_ids, appid_control_handle);

    // CadDictionary.CreateRoot/CreateDefaultEntries, in insertion order.
    let mut root = Dictionary::new();
    root.duplicate_cloning = 0;
    root.handle = doc.allocate_handle();
    let root_handle = root.handle;
    doc.header.named_objects_dict_handle = root_handle;
    doc.objects
        .insert(root_handle, ObjectType::Dictionary(root));
    for name in [
        "ACAD_COLOR",
        "ACAD_GROUP",
        "ACAD_LAYOUT",
        "ACAD_MATERIAL",
        "ACAD_SORTENTS",
        "ACAD_MLEADERSTYLE",
        "ACAD_MLINESTYLE",
        "ACAD_TABLESTYLE",
        "ACAD_PLOTSETTINGS",
        "AcDbVariableDictionary",
        "ACAD_SCALELIST",
        "ACAD_VISUALSTYLE",
        "ACAD_IMAGE_DICT",
        "ACAD_FIELDLIST",
    ] {
        add_dictionary(doc, root_handle, name);
    }
    doc.header.acad_group_dict_handle = dictionary(doc, "ACAD_GROUP");
    doc.header.acad_layout_dict_handle = dictionary(doc, "ACAD_LAYOUT");
    doc.header.acad_mlinestyle_dict_handle = dictionary(doc, "ACAD_MLINESTYLE");
    doc.header.acad_plotsettings_dict_handle = dictionary(doc, "ACAD_PLOTSETTINGS");
    doc.header.acad_material_dict_handle = dictionary(doc, "ACAD_MATERIAL");
    doc.header.acad_color_dict_handle = dictionary(doc, "ACAD_COLOR");
    doc.header.acad_visualstyle_dict_handle = dictionary(doc, "ACAD_VISUALSTYLE");

    // UpdateCollections(true, true): scales, MLine, MLeader, TableStyle,
    // PDF definitions, variables, then materials. Other collections are empty.
    for drawing in [1, 2, 4, 5, 8, 10, 16, 20, 30, 40, 50, 100] {
        add_scale(doc, 1, drawing);
    }
    for paper in [2, 4, 8, 10, 100] {
        add_scale(doc, paper, 1);
    }

    let mline = MLineStyle::standard();
    let mline_handle = add_object(
        doc,
        "ACAD_MLINESTYLE",
        "Standard",
        ObjectType::MLineStyle(mline),
    );
    doc.header.current_multiline_style_handle = mline_handle;
    // MLineStyle.Element.AssignDocument canonicalizes each ByLayer reference.
    let by_layer = ensure_line_type(doc, LineType::by_layer());

    let mleader_handle = add_object(
        doc,
        "ACAD_MLEADERSTYLE",
        "Standard",
        ObjectType::MultiLeaderStyle(MultiLeaderStyle::new("Standard")),
    );
    // MultiLeaderStyle.AssignDocument registers its text style before linetype.
    let text_style = ensure_text_style(doc);
    if let Some(ObjectType::MultiLeaderStyle(style)) = doc.objects.get_mut(&mleader_handle) {
        style.text_style_handle = Some(text_style);
        style.line_type_handle = Some(by_layer);
    }
    add_object(
        doc,
        "ACAD_TABLESTYLE",
        "Standard",
        ObjectType::TableStyle(TableStyle::standard()),
    );
    add_dictionary(doc, root_handle, "ACAD_PDFDEFINITIONS");
    for (name, value) in [
        ("CMLEADERSTYLE", "Standard"),
        ("CANNOSCALE", "1:1"),
        ("CTABLESTYLE", "Standard"),
        ("WIPEOUTFRAME", "1"),
        ("CVIEWDETAILSTYLE", "Metric50"),
        ("CVIEWSECTIONSTYLE", "Metric50"),
    ] {
        add_object(
            doc,
            "AcDbVariableDictionary",
            name,
            ObjectType::DictionaryVariable(DictionaryVariable::new(name, value)),
        );
    }
    for name in ["Global", "ByLayer", "ByBlock"] {
        let mut material = Material::new();
        material.name = name.into();
        add_object(doc, "ACAD_MATERIAL", name, ObjectType::Material(material));
    }

    // Table.CreateDefaultEntries skips dependencies that were already registered.
    let mut app = AppId::acad();
    app.handle = doc.allocate_handle();
    doc.app_ids.add(app).expect("unique default app ID");
    ensure_line_type(doc, LineType::by_layer());
    doc.header.byblock_linetype_handle = ensure_line_type(doc, LineType::by_block());
    let continuous = ensure_line_type(doc, LineType::continuous());
    doc.header.continuous_linetype_handle = continuous;
    doc.header.bylayer_linetype_handle = by_layer;
    doc.header.current_linetype_handle = by_layer;
    let mut layer = Layer::layer_0();
    layer.handle = doc.allocate_handle();
    doc.header.current_layer_handle = layer.handle;
    doc.layers.add(layer).expect("unique default layer");
    ensure_text_style(doc);
    doc.header.current_text_style_handle = text_style;
    doc.header.dim_text_style_handle = text_style;
    let mut dimension = DimStyle::standard();
    dimension.handle = doc.allocate_handle();
    dimension.dimtxsty_handle = text_style;
    doc.header.current_dimstyle_handle = dimension.handle;
    doc.dim_styles
        .add(dimension)
        .expect("unique default dimension style");
    let mut vport = VPort::active();
    vport.handle = doc.allocate_handle();
    doc.vports
        .add(vport)
        .expect("unique default viewport configuration");

    add_layout(doc, "Model", "*Model_Space", false);
    add_layout(doc, "Layout1", "*Paper_Space", true);
}

fn dictionary(doc: &CadDocument, name: &str) -> Handle {
    let ObjectType::Dictionary(root) = &doc.objects[&doc.header.named_objects_dict_handle] else {
        unreachable!("root dictionary")
    };
    root.get(name).expect("default dictionary exists")
}

fn add_dictionary(doc: &mut CadDocument, owner: Handle, name: &str) -> Handle {
    let mut value = Dictionary::new();
    value.duplicate_cloning = 0;
    value.owner = owner;
    value.handle = doc.allocate_handle();
    let handle = value.handle;
    doc.objects.insert(handle, ObjectType::Dictionary(value));
    append_entry(doc, owner, name, handle);
    handle
}

fn append_entry(doc: &mut CadDocument, owner: Handle, name: &str, handle: Handle) {
    let Some(ObjectType::Dictionary(parent)) = doc.objects.get_mut(&owner) else {
        unreachable!("owning default dictionary")
    };
    parent.add_entry(name, handle);
}

fn add_object(doc: &mut CadDocument, parent: &str, name: &str, mut value: ObjectType) -> Handle {
    let owner = dictionary(doc, parent);
    match &mut value {
        ObjectType::Scale(v) => v.owner_handle = owner,
        ObjectType::MLineStyle(v) => v.owner = owner,
        ObjectType::MultiLeaderStyle(v) => v.owner_handle = owner,
        ObjectType::TableStyle(v) => v.owner_handle = owner,
        ObjectType::DictionaryVariable(v) => v.owner_handle = owner,
        ObjectType::Material(v) => v.owner = owner,
        ObjectType::Layout(v) => v.owner = owner,
        _ => unreachable!("default dictionary entry type"),
    }
    let handle = doc.allocate_handle();
    value.set_handle(handle);
    doc.objects.insert(handle, value);
    append_entry(doc, owner, name, handle);
    handle
}

fn add_scale(doc: &mut CadDocument, paper: i32, drawing: i32) {
    let name = format!("{paper}:{drawing}");
    add_object(
        doc,
        "ACAD_SCALELIST",
        &name,
        ObjectType::Scale(Scale::from_ratio(&name, paper, drawing)),
    );
}

fn ensure_line_type(doc: &mut CadDocument, mut value: LineType) -> Handle {
    if let Some(existing) = doc.line_types.get(&value.name) {
        return existing.handle;
    }
    value.handle = doc.allocate_handle();
    let handle = value.handle;
    doc.line_types.add(value).expect("unique default linetype");
    handle
}

fn ensure_text_style(doc: &mut CadDocument) -> Handle {
    if let Some(existing) = doc.text_styles.get("Standard") {
        return existing.handle;
    }
    let mut value = TextStyle::standard();
    value.handle = doc.allocate_handle();
    let handle = value.handle;
    doc.text_styles
        .add(value)
        .expect("unique default text style");
    handle
}

fn add_layout(doc: &mut CadDocument, name: &str, block_name: &str, paper: bool) {
    let mut layout = Layout::new(name);
    layout.tab_order = i16::from(paper);
    layout.paper_width = 297.0;
    layout.paper_height = 210.0;
    let layout_handle = add_object(doc, "ACAD_LAYOUT", name, ObjectType::Layout(layout));

    // Layout.AssignDocument -> AddCadObject(BlockRecord) -> markers -> entities.
    let mut block = BlockRecord::new(block_name);
    block.handle = doc.allocate_handle();
    block.layout = layout_handle;
    block.block_entity_handle = doc.allocate_handle();
    block.block_end_handle = doc.allocate_handle();
    let owner = block.handle;
    let mut begin = Block::new(block_name, Vector3::ZERO);
    begin.common.handle = block.block_entity_handle;
    begin.common.owner_handle = owner;
    let mut end = BlockEnd::new();
    end.common.handle = block.block_end_handle;
    end.common.owner_handle = owner;
    doc.block_records.add(block).expect("unique default block");
    if paper {
        doc.header.paper_space_block_handle = owner;
    } else {
        doc.header.model_space_block_handle = owner;
    }
    doc.add_entity(EntityType::Block(begin))
        .expect("default block begin");
    doc.add_entity(EntityType::BlockEnd(end))
        .expect("default block end");

    let viewport = if paper {
        let mut viewport = Viewport::new();
        viewport.id = 1;
        viewport.common.owner_handle = owner;
        viewport.common.entity_mode = Some(1);
        viewport.center = Vector3::new(viewport.width / 2.0, viewport.height / 2.0, 0.0);
        Some(
            doc.add_entity(EntityType::Viewport(Box::new(viewport)))
                .expect("default paper viewport"),
        )
    } else {
        None
    };
    let Some(ObjectType::Layout(layout)) = doc.objects.get_mut(&layout_handle) else {
        unreachable!("registered default layout")
    };
    layout.block_record = owner;
    layout.viewports.extend(viewport);
    // LastActiveViewport is a separate, initially absent reference.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_uses_the_allocator_instead_of_fixed_identities() {
        let mut doc = CadDocument::new_without_defaults();
        doc.header.handle_seed = 1000;
        create_defaults(&mut doc);
        assert_eq!(doc.block_records.handle().value(), 1000);
        assert_eq!(doc.header.handle_seed, 1071);
        assert_eq!(doc.line_types.get("ByLayer").unwrap().handle.value(), 1042);
        assert_eq!(
            doc.text_styles.get("Standard").unwrap().handle.value(),
            1044
        );
        let dump = super::super::dump_document(&doc, "", "new");
        assert_eq!(dump["objects"].as_array().unwrap().len(), 71);
        assert_eq!(doc.allocate_handle().value(), 1071);
    }
}
