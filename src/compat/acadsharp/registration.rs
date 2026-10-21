//! The default ACadSharp readers register reachable objects, not every decoded
//! record. Keep the native document intact and construct that graph once here.
use super::*;
use std::collections::{HashMap, HashSet};

pub(super) fn source_document(doc: &CadDocument) -> bool {
    doc.header.source_handle_seed.is_some() || doc.header.dxf_source_variables.is_some()
}

/// The dictionary ACadSharp adopts as its root: the first dictionary with owner
/// zero that the object reader meets. A DWG reader starts from the header's
/// dictionary handles in file order (group, mlinestyle, named objects, then the
/// R2000+ layout/plot/material/colour/visual-style dictionaries), so a damaged
/// file whose ACAD_GROUP also has owner zero makes that dictionary the root and
/// leaves the real named-objects dictionary unregistered.
pub(super) fn root_dictionary(doc: &CadDocument, reader: &str) -> u64 {
    let named = doc.header.named_objects_dict_handle;
    if reader != "dwg" {
        return named.value();
    }
    let header = &doc.header;
    [
        header.acad_group_dict_handle,
        header.acad_mlinestyle_dict_handle,
        named,
        header.acad_layout_dict_handle,
        header.acad_plotsettings_dict_handle,
        header.acad_plotstylename_dict_handle,
        header.acad_material_dict_handle,
        header.acad_color_dict_handle,
        header.acad_visualstyle_dict_handle,
    ]
    .into_iter()
    .find(|candidate| {
        !candidate.is_null()
            && matches!(
                doc.objects.get(candidate),
                Some(crate::objects::ObjectType::Dictionary(dictionary)) if dictionary.owner.is_null()
            )
    })
    .unwrap_or(named)
    .value()
}

/// Entities that carry a `DimensionStyle` defaulting to "Standard".
pub(super) fn is_dimension_entity(record: &Value) -> bool {
    record["type"].as_str().is_some_and(|ty| {
        matches!(ty, "Leader" | "Tolerance")
            || (ty.starts_with("Dimension")
                && !matches!(
                    ty,
                    "DimensionStyle" | "DimensionStylesTable" | "DimensionAssociation"
                ))
    })
}

pub(super) fn has_dimension_entity(records: &[Value]) -> bool {
    records.iter().any(is_dimension_entity)
}

fn id(value: &Value) -> Option<u64> {
    u64::from_str_radix(value.as_str()?.strip_prefix('#')?, 16).ok()
}

pub(super) fn supported(record: &Value, reader: &str) -> bool {
    !matches!(record["type"].as_str(), Some("UnknownNonGraphicalObject" | "UnknownEntity"))
        // DXF's VisualStyle callback consumes even the identity fields without
        // assigning them. DWG has no standalone TABLECONTENT reader (inline
        // TableEntity content is a different, supported path).
        && !(reader == "dxf" && record["type"] == "VisualStyle")
        && !(reader == "dwg" && record["type"] == "TableContent")
        // DwgObjectReader.readMTextAttributeObjectContextData is a TODO that
        // returns no template, so the object is never registered.
        && !(reader == "dwg" && record["type"] == "MTextAttributeObjectContextData")
        // VPortsTable.Add keeps the name as given instead of generating one,
        // and the table then refuses an empty key.
        && !(record["type"] == "VPort"
            && record["props"]["Name"].as_str().is_some_and(str::is_empty))
}

/// Apply reader registration policy after projection, before handle deduplication.
/// Constructed documents retain their explicit registration map.
pub(super) fn apply(doc: &CadDocument, reader: &str, records: &mut Vec<Value>) -> HashSet<u64> {
    if !source_document(doc) {
        return HashSet::new();
    }
    // R13/R14 drawings yield no ACDBPLACEHOLDER objects in the reference
    // reader (observed on every such corpus file), so the plot-style
    // dictionary's default entry is never registered.
    if reader == "dwg" && doc.version < DxfVersion::AC1015 {
        records.retain(|record| record["type"] != "AcdbPlaceHolder");
    }
    // First concrete representation wins, as in the canonical dumper.
    let mut index = HashMap::with_capacity(records.len());
    for (i, record) in records.iter().enumerate() {
        if let Some(h) = id(&record["handle"]).filter(|&h| h != 0) {
            index.entry(h).or_insert(i);
        }
    }
    let mut edges: HashMap<u64, Vec<u64>> = HashMap::new();
    let mut owners = HashMap::new();
    let mut explicit_extensions = HashSet::new();
    let mut names: HashMap<u64, String> = HashMap::new();
    let mut dictionaries: HashMap<u64, Vec<(String, u64)>> = HashMap::new();
    let mut roots = Vec::new();
    // Table.Add names an empty entry "unnamed<N>", the first N not yet used in
    // that table.
    let mut unnamed_entries = HashMap::new();
    let mut table_names: HashMap<u64, HashSet<String>> = HashMap::new();
    for record in records.iter().filter(|r| {
        r["type"] != "VPort"
            && r["props"]["SubclassMarker"]
                .as_str()
                .is_some_and(|marker| marker.ends_with("TableRecord"))
    }) {
        let Some(table) = id(&record["owner"]) else {
            continue;
        };
        let names = table_names.entry(table).or_default();
        let name = record["props"]["Name"].as_str().unwrap_or("");
        if name.is_empty() {
            if let Some(h) = id(&record["handle"]) {
                let mut n = 0;
                let name = loop {
                    let candidate = format!("unnamed{n}");
                    n += 1;
                    if names.insert(candidate.to_uppercase()) {
                        break candidate;
                    }
                };
                unnamed_entries.insert(h, name);
            }
        } else {
            names.insert(name.to_uppercase());
        }
    }
    let root = root_dictionary(doc, reader);
    if root != 0 {
        roots.push(root);
    }

    for (i, record) in records.iter().enumerate() {
        let Some(h) = id(&record["handle"]) else {
            continue;
        };
        if index.get(&h) != Some(&i) {
            continue;
        }
        let ty = record["type"].as_str().unwrap_or("");
        if ty.ends_with("Table") && record["props"]["SubclassMarker"] == "AcDbSymbolTable" {
            roots.push(h);
        }
        if let Some(owner) = id(&record["owner"]) {
            if let Some(&parent) = index.get(&owner) {
                let parent_type = records[parent]["type"].as_str().unwrap_or("");
                // Owning symbol/geometry collections, rather than arbitrary
                // source owner pointers, register their children.
                if (records[parent]["props"]["SubclassMarker"] == "AcDbSymbolTable"
                    && record["props"]["SubclassMarker"]
                        .as_str()
                        .is_some_and(|marker| marker.ends_with("TableRecord")))
                    || (record["props"].get("ProxyGeometries").is_some()
                        && match parent_type {
                            // A block lists its own entities (membership edges
                            // below); its BLOCK/ENDBLK markers and non-graphical
                            // objects follow the owner pointer. An entity that
                            // names the block as owner but is not in its list
                            // stays unregistered.
                            "BlockRecord" => {
                                matches!(ty, "Block" | "BlockEnd")
                                    || doc.get_entity(Handle::new(h)).is_none()
                            }
                            "Insert" | "Polyline2D" | "Polyline3D" | "PolyfaceMesh"
                            | "PolygonMesh" => true,
                            _ => false,
                        })
                {
                    edges.entry(owner).or_default().push(h);
                }
            }
        }
        let xdic = doc
            .xdic_by_handle
            .get(&Handle::new(h))
            .map(|h| h.value())
            .or_else(|| id(&record["props"]["XDictionary"]));
        if let Some(dict) = xdic.filter(|&h| h != 0) {
            edges.entry(h).or_default().push(dict);
            owners.insert(dict, h);
            explicit_extensions.insert(h);
        }
        if matches!(ty, "CadDictionary" | "CadDictionaryWithDefault") {
            let raw_handles = record["props"]["EntryHandles"].as_array();
            let raw_names = record["props"]["EntryNames"].as_array();
            let mut entries = Vec::new();
            let mut keys = HashSet::new();
            if let (Some(handles), Some(entry_names)) = (raw_handles, raw_names) {
                for (target, key) in handles.iter().zip(entry_names) {
                    let Some(target) = target.as_u64() else {
                        continue;
                    };
                    let Some(&child) = index.get(&target) else {
                        continue;
                    };
                    if !supported(&records[child], reader) {
                        continue;
                    }
                    let stored = records[child]["props"]["Name"].as_str().unwrap_or("");
                    // These style readers populate Description; their inherited
                    // Name starts empty in ACadSharp and comes from the key.
                    let dictionary_named = matches!(
                        records[child]["type"].as_str(),
                        Some("MultiLeaderStyle" | "TableStyle")
                    );
                    let name = if dictionary_named || stored.is_empty() || stored == UNMAPPED {
                        key.as_str().unwrap_or("")
                    } else {
                        stored
                    };
                    // CadDictionaryTemplate uses the object's name, with a
                    // case-insensitive dictionary rejecting duplicate keys.
                    if !keys.insert(name.to_uppercase()) {
                        continue;
                    }
                    entries.push((name.to_owned(), target));
                    owners.insert(target, h);
                    names.insert(target, name.to_owned());
                    edges.entry(h).or_default().push(target);
                }
            }
            dictionaries.insert(h, entries);
        }
        if ty == "EvaluationGraph" {
            if let Some(nodes) = record["props"]["Nodes"].as_array() {
                for node in nodes {
                    if let Some(expression) = id(&node["Expression"]) {
                        edges.entry(h).or_default().push(expression);
                        owners.insert(expression, h);
                    }
                }
            }
        }
    }
    // Match CadDocument::extension_dictionary_handle's native fallback without
    // its per-owner scan. Non-dictionary owners have at most one extension
    // dictionary; dictionary children instead require an explicit entry or
    // XDictionary reference. This also retains attachments whose typed reader
    // has no dedicated extension-dictionary field.
    let mut owned_dictionaries = HashMap::new();
    for (i, record) in records.iter().enumerate() {
        if record["type"] != "CadDictionary" {
            continue;
        }
        let Some(h) = id(&record["handle"]) else {
            continue;
        };
        if index.get(&h) != Some(&i) {
            continue;
        }
        let Some(owner) = id(&record["owner"]) else {
            continue;
        };
        let Some(&parent) = index.get(&owner) else {
            continue;
        };
        if !matches!(
            records[parent]["type"].as_str(),
            Some("CadDictionary" | "CadDictionaryWithDefault")
        ) && !explicit_extensions.contains(&owner)
        {
            owned_dictionaries
                .entry(owner)
                .and_modify(|value| *value = None)
                .or_insert(Some(h));
        }
    }
    for (owner, dictionary) in owned_dictionaries {
        if let Some(h) = dictionary {
            edges.entry(owner).or_default().push(h);
            owners.insert(h, owner);
        }
    }
    // Block entity membership is retained independently of source owner fields.
    for block in doc.block_records.iter() {
        let members = super::tables::view_entity_handles(doc, block);
        for h in members.iter() {
            owners.insert(h.value(), block.handle.value());
        }
        edges
            .entry(block.handle.value())
            .or_default()
            .extend(members.iter().map(|h| h.value()));
    }
    // Template lookup is broader than document registration: a known orphan
    // can still be an XRecord reference. Unknown-reader policy applies to both.
    let source_app_ids: Option<HashSet<_>> = doc
        .source_app_id_order
        .as_ref()
        .map(|names| names.iter().map(|name| name.to_uppercase()).collect());
    // A native constructor dimension style that the source table did not
    // contain is not part of the ACadSharp document unless a dimension-like
    // entity exists: attaching one to a block creates "Standard" from its
    // default style before the entity's own style reference is resolved.
    let unused_default_dimstyles: HashSet<_> = if has_dimension_entity(records) {
        HashSet::new()
    } else {
        doc.dim_styles
            .iter()
            .filter(|style| !style.source_present)
            .map(|style| style.handle.value())
            .collect()
    };
    let template_handles: HashSet<_> = index
        .iter()
        .filter_map(|(&h, &i)| {
            let record = &records[i];
            let native_default = (record["type"] == "DimensionStyle"
                && unused_default_dimstyles.contains(&h))
                || (record["type"] == "AppId"
                    && source_app_ids.as_ref().is_some_and(|source| {
                        !source.contains(
                            &record["props"]["Name"]
                                .as_str()
                                .unwrap_or("")
                                .to_uppercase(),
                        )
                    }));
            (supported(record, reader) && !native_default).then_some(h)
        })
        .collect();
    let mut registered = HashSet::with_capacity(index.len());
    while let Some(h) = roots.pop() {
        if !template_handles.contains(&h) || !registered.insert(h) {
            continue;
        }
        if let Some(children) = edges.get(&h) {
            roots.extend(children);
        }
    }
    // A DWG layout template links its block whether or not the layout itself
    // ends up registered, so any decoded layout counts. The DXF reader links
    // only layouts that its dictionary walk registered (observed on ce.dxf,
    // whose layout the reference never creates).
    let registered_layouts: HashSet<_> = if reader == "dxf" { &registered } else { &template_handles }
        .iter()
        .copied()
        .filter(|h| records[index[h]]["type"] == "Layout")
        .collect();
    let mut table_counts = HashMap::<u64, usize>::new();
    for &h in &registered {
        let record = &records[index[&h]];
        if let Some(owner) = id(&record["owner"]) {
            if let Some(&parent) = index.get(&owner) {
                if records[parent]["props"]["SubclassMarker"] == "AcDbSymbolTable"
                    && record["props"]["SubclassMarker"]
                        .as_str()
                        .is_some_and(|marker| marker.ends_with("TableRecord"))
                {
                    *table_counts.entry(owner).or_default() += 1;
                }
            }
        }
    }
    records.retain_mut(|record| {
        let Some(h) = id(&record["handle"]) else {
            return false;
        };
        if !registered.contains(&h) {
            return false;
        }
        if record["props"]["SubclassMarker"] == "AcDbSymbolTable" {
            record["props"]["Count"] = json!(table_counts.get(&h).copied().unwrap_or(0));
        }
        if record["type"] == "XRecord" {
            if let Some(crate::objects::ObjectType::XRecord(native)) =
                doc.objects.get(&Handle::new(h))
            {
                if let Some(entries) = record["props"].get_mut("Entries").and_then(Value::as_array_mut) {
                    for (entry, projected) in native.entries.iter().zip(entries) {
                        if let crate::objects::XRecordValue::Handle(target) = entry.value {
                            projected["Value"] = if template_handles.contains(&target.value()) {
                                handle(target)
                            } else {
                                json!(target.value())
                            };
                        }
                    }
                }
            }
        }
        if record["type"] == "BlockRecord" {
            if let Some(entities) = record["props"]["Entities"].as_array_mut() {
                entities.retain(|value| id(value).is_some_and(|h| registered.contains(&h)));
            }
            // A reference to something that is not a decoded layout cannot be linked.
            if id(&record["props"]["Layout"]).is_some_and(|h| !registered_layouts.contains(&h)) {
                record["props"]["Layout"] = Value::Null;
            }
        }
        if record["type"] == "EvaluationGraph" {
            if let Some(nodes) = record["props"]["Nodes"].as_array_mut() {
                for node in nodes {
                    // The graph template resolves an expression only to an object
                    // the reader produced; unsupported block parameters and
                    // proxy nodes leave it unset.
                    if !id(&node["Expression"]).is_some_and(|h| template_handles.contains(&h)) {
                        node["Expression"] = Value::Null;
                    }
                }
            }
        }
        if h == root && root == doc.header.named_objects_dict_handle.value() {
            record["owner"] = handle(Handle::NULL);
            record["props"]["Name"] = json!("");
        } else if let Some(&owner) = owners.get(&h) {
            record["owner"] = handle(Handle::new(owner));
        }
        if let Some(name) = names.get(&h) {
            // BinRecord hides the base Name property that dictionary
            // registration assigns; its public dumped Name remains null.
            if record["type"] != "AecBinRecord" && record["props"].get("Name").is_some() {
                record["props"]["Name"] = json!(name);
            }
        }
        if record["type"] == "Group" {
            // Group.IsUnnamed is a getter over the final dictionary-assigned
            // name, not the stored GROUP unnamed flag.
            let name = record["props"]["Name"].as_str().unwrap_or("");
            record["props"]["IsUnnamed"] = json!(name.trim().is_empty() || name.starts_with('*'));
        }
        let named = match record["type"].as_str() {
            Some("Block") => id(&record["owner"]),
            _ => Some(h),
        };
        if let Some(name) = named.and_then(|h| unnamed_entries.get(&h)) {
            record["props"]["Name"] = json!(name);
        }
        if let Some(entries) = dictionaries.get(&h) {
            // readCommonDictionary consumes the R14 byte without assigning
            // HardOwnerFlag. Both flags remain constructor defaults before
            // R2000, including dictionaries with a default entry.
            if reader == "dwg" && doc.version < DxfVersion::AC1015 {
                record["props"]["HardOwnerFlag"] = json!(false);
                record["props"]["ClonningFlags"] = json!(0);
            }
            record["props"]["EntryHandles"] = json!(entries
                .iter()
                .filter(|(_, h)| registered.contains(h))
                .map(|(_, h)| h)
                .collect::<Vec<_>>());
            record["props"]["EntryNames"] = json!(entries
                .iter()
                .filter(|(_, h)| registered.contains(h))
                .map(|(name, _)| name)
                .collect::<Vec<_>>());
        }
        true
    });
    template_handles
}

#[cfg(test)]
#[path = "registration_tests.rs"]
mod tests;
