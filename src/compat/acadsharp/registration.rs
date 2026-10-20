//! The default ACadSharp readers register reachable objects, not every decoded
//! record. Keep the native document intact and construct that graph once here.
use super::*;
use std::collections::{HashMap, HashSet};

pub(super) fn source_document(doc: &CadDocument) -> bool {
    doc.header.source_handle_seed.is_some() || doc.header.dxf_source_variables.is_some()
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
}

/// Apply reader registration policy after projection, before handle deduplication.
/// Constructed documents retain their explicit registration map.
pub(super) fn apply(doc: &CadDocument, reader: &str, records: &mut Vec<Value>) {
    if !source_document(doc) {
        return;
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
    let root = doc.header.named_objects_dict_handle.value();
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
                        && matches!(
                            parent_type,
                            "BlockRecord"
                                | "Insert"
                                | "Polyline2D"
                                | "Polyline3D"
                                | "PolyfaceMesh"
                                | "PolygonMesh"
                        ))
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
        for h in &block.entity_handles {
            owners.insert(h.value(), block.handle.value());
        }
        edges
            .entry(block.handle.value())
            .or_default()
            .extend(block.entity_handles.iter().map(|h| h.value()));
    }
    // Template lookup is broader than document registration: a known orphan
    // can still be an XRecord reference. Unknown-reader policy applies to both.
    let source_app_ids: Option<HashSet<_>> = doc
        .source_app_id_order
        .as_ref()
        .map(|names| names.iter().map(|name| name.to_uppercase()).collect());
    let template_handles: HashSet<_> = index
        .iter()
        .filter_map(|(&h, &i)| {
            let record = &records[i];
            let native_default = record["type"] == "AppId"
                && source_app_ids.as_ref().is_some_and(|source| {
                    !source.contains(
                        &record["props"]["Name"]
                            .as_str()
                            .unwrap_or("")
                            .to_uppercase(),
                    )
                });
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
                if let Some(entries) = record["props"]["Entries"].as_array_mut() {
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
        }
        if h == root {
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
}

#[cfg(test)]
#[path = "registration_tests.rs"]
mod tests;
