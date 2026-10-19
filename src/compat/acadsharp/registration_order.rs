//! Compatibility-only identities allocated while ACadSharp registers objects.
//!
//! ACadSharp's reader can allocate collection defaults before it attaches a
//! source `SEQEND`. The compatibility view preserves that registration
//! identity while leaving the native document's source identity intact.
use super::*;
use crate::entities::EntityCommon;
use std::collections::{HashMap, HashSet};

fn source_document(doc: &CadDocument) -> bool {
    doc.header.source_handle_seed.is_some() || doc.header.dxf_source_variables.is_some()
}

fn modern_dxf(doc: &CadDocument, reader: &str) -> bool {
    source_document(doc) && reader.eq_ignore_ascii_case("dxf") && doc.version >= DxfVersion::AC1015
}

fn handle(value: &Value) -> Option<u64> {
    u64::from_str_radix(value.as_str()?.strip_prefix('#')?, 16).ok()
}

fn numeric_handle(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| handle(value))
}

fn is_seqend(record: &Value) -> bool {
    record["type"] == "Seqend"
}

fn is_sequence_child(record: &Value) -> bool {
    matches!(
        record["type"].as_str(),
        Some(
            "AttributeEntity"
                | "Vertex2D"
                | "Vertex3D"
                | "VertexFaceMesh"
                | "VertexFaceRecord"
                | "PolygonMeshVertex"
        )
    )
}

fn sequence_child_owners(records: &[Value]) -> HashSet<u64> {
    records
        .iter()
        .filter(|record| is_sequence_child(record))
        .filter_map(|record| handle(&record["owner"]))
        .collect()
}

fn remappable_seqend(
    doc: &CadDocument,
    reader: &str,
    child_owners: &HashSet<u64>,
    record: &Value,
) -> bool {
    if !modern_dxf(doc, reader) || !is_seqend(record) {
        return false;
    }
    handle(&record["owner"]).is_some_and(|owner| child_owners.contains(&owner))
}

const UPDATE_COLLECTION_DICTIONARIES: [&str; 2] = ["ACAD_IMAGE_DICT", "ACAD_PDFDEFINITIONS"];

fn dictionary_entries(records: &[Value], owner_handle: u64) -> (Vec<u64>, Vec<String>) {
    let owner = format!("#{owner_handle:X}");
    records
        .iter()
        .filter(|record| record["owner"] == owner)
        .filter_map(|record| {
            let child = handle(&record["handle"])?;
            let name = record["props"]["Name"].as_str()?.to_owned();
            if name.is_empty() || name == UNMAPPED {
                return None;
            }
            Some((child, name))
        })
        .unzip()
}

fn synthetic_dictionary(root: u64, target: u64, name: &str, records: &[Value]) -> Value {
    let (entry_handles, entry_names) = dictionary_entries(records, target);
    let has_entries = !entry_handles.is_empty();
    record(
        Handle::new(target),
        "CadDictionary",
        Handle::new(root),
        props(
            "CadDictionary",
            [
                ("ClonningFlags", json!(if has_entries { 1 } else { 0 })),
                ("EntryHandles", json!(entry_handles)),
                ("EntryNames", json!(entry_names)),
                ("ExtendedData", json!([])),
                ("HardOwnerFlag", json!(false)),
                ("HasDynamicSubclass", json!(false)),
                ("Name", json!(name)),
                ("ObjectName", json!("DICTIONARY")),
                ("ObjectType", json!(42)),
                (
                    "Reactors",
                    json!(if has_entries {
                        vec![format!("#{root:X}")]
                    } else {
                        Vec::<String>::new()
                    }),
                ),
                ("SubclassMarker", json!("AcDbDictionary")),
                ("XDictionary", Value::Null),
            ],
        ),
    )
}

/// Return the next handle in ACadSharp's source registration allocator.
///
/// The native header seed has already advanced as native defaults were
/// created. ACadSharp starts at the original source seed and advances past
/// retained source records, so `Header.HandleSeed` is deliberately ignored.
/// Source `SEQEND` identities are replaced below, so they do not reserve
/// slots in this compatibility allocator.
fn first_allocated_handle(
    doc: &CadDocument,
    reader: &str,
    child_owners: &HashSet<u64>,
    records: &[Value],
) -> u64 {
    let source_seed = doc.header.source_handle_seed.unwrap_or(0);
    records
        .iter()
        .filter(|record| !remappable_seqend(doc, reader, child_owners, record))
        .filter_map(|record| handle(&record["handle"]))
        .max()
        .map(|max| source_seed.max(max.saturating_add(1)))
        .unwrap_or(source_seed)
}

/// Recreate dictionaries that ACadSharp's `UpdateCollections` materializes
/// from the named-object dictionary. Missing entries are appended at the
/// compatibility allocator cursor. Existing entries and source records are
/// left intact, including a source object near the cursor.
fn apply_update_collection_dictionaries(doc: &CadDocument, reader: &str, records: &mut Vec<Value>) {
    if !source_document(doc) || doc.version < DxfVersion::AC1012 {
        return;
    }
    let root = doc.header.named_objects_dict_handle.value();
    if root == 0 {
        return;
    }
    let Some(root_index) = records.iter().position(|record| {
        handle(&record["handle"]) == Some(root)
            && matches!(
                record["type"].as_str(),
                Some("CadDictionary" | "CadDictionaryWithDefault")
            )
    }) else {
        return;
    };
    let Some(mut entry_names) = records[root_index]["props"]["EntryNames"]
        .as_array()
        .cloned()
    else {
        return;
    };
    let Some(mut entry_handles) = records[root_index]["props"]["EntryHandles"]
        .as_array()
        .cloned()
    else {
        return;
    };

    let mut occupied = records
        .iter()
        .filter_map(|record| handle(&record["handle"]))
        .collect::<HashSet<_>>();
    occupied.extend(entry_handles.iter().filter_map(numeric_handle));
    let child_owners = sequence_child_owners(records);
    let mut next = first_allocated_handle(doc, reader, &child_owners, records);
    let mut additions = Vec::with_capacity(UPDATE_COLLECTION_DICTIONARIES.len());
    for name in UPDATE_COLLECTION_DICTIONARIES {
        let already_present =
            entry_names
                .iter()
                .zip(&entry_handles)
                .any(|(entry_name, entry_handle)| {
                    entry_name
                        .as_str()
                        .is_some_and(|entry_name| entry_name.eq_ignore_ascii_case(name))
                        && numeric_handle(entry_handle).is_some()
                });
        if already_present {
            continue;
        }
        while occupied.contains(&next) {
            next = next.saturating_add(1);
        }
        let target = next;
        next = next.saturating_add(1);
        occupied.insert(target);
        entry_names.push(json!(name));
        entry_handles.push(json!(target));
        additions.push((name, target));
    }
    records[root_index]["props"]["EntryNames"] = Value::Array(entry_names);
    records[root_index]["props"]["EntryHandles"] = Value::Array(entry_handles);

    for (name, target) in additions {
        // The allocator skips every occupied source handle, so this append
        // cannot replace a source object at the same handle.
        let dictionary = synthetic_dictionary(root, target, name, records);
        records.push(dictionary);
    }
}

fn default_seqend_props(doc: &CadDocument, target: u64, owner: u64) -> Value {
    let common = EntityCommon {
        handle: Handle::new(target),
        owner_handle: Handle::new(owner),
        ..EntityCommon::default()
    };
    props(
        "Seqend",
        entities::common(doc, &common, "SEQEND", 6, "AcDbEntity", false),
    )
}

/// ACadSharp's R12 DXF reader assigns ownerless INSERT children to the owning block
/// collection when the source marker has no explicit owner. Native compound
/// ownership remains on the INSERT; project the source-aware registry owner.
fn apply_legacy_entity_owners(doc: &CadDocument, reader: &str, records: &mut [Value]) {
    if !source_document(doc)
        || !reader.eq_ignore_ascii_case("dxf")
        || doc.version != DxfVersion::AC1009
    {
        return;
    }
    let owners = records
        .iter()
        .filter_map(|record| {
            (record["type"] == "Insert")
                .then(|| handle(&record["handle"]).map(|handle| (handle, record["owner"].clone())))
                .flatten()
        })
        .collect::<HashMap<_, _>>();
    let child_owners = sequence_child_owners(records);
    let owner_updates = records
        .iter()
        .enumerate()
        .filter_map(|(index, record)| {
            if !matches!(record["type"].as_str(), Some("Seqend" | "AttributeEntity")) {
                return None;
            }
            {
                let source_owners = doc.source_entity_owners.as_ref()?;
                let source_handle = handle(&record["handle"])?;
                let source_owner = source_owners.get(&Handle::new(source_handle))?;
                // The DXF entity reader normalizes INSERT SEQEND ownership to
                // the INSERT. ACadSharp reparents only a source marker that
                // had no explicit 330; an explicit source owner is already
                // meaningful and must stay in the projected record.
                if !source_owner.is_null() {
                    return None;
                }
            }
            let owner = handle(&record["owner"])?;
            let block_owner = owners.get(&owner)?;
            child_owners
                .contains(&owner)
                .then(|| (index, block_owner.clone()))
        })
        .collect::<Vec<_>>();
    for (index, owner) in owner_updates {
        records[index]["owner"] = owner;
    }
}

/// Apply ACadSharp's registration-order projection to a canonical object
/// list. Call this after reachability filtering. Modern DXF sequence markers
/// receive collection-default common properties and handles allocated in
/// retained entity order; other readers retain source sequence identities.
///
/// The return value is the number of modern-DXF sequence records changed.
pub(super) fn apply(doc: &CadDocument, reader: &str, records: &mut Vec<Value>) -> usize {
    apply_legacy_entity_owners(doc, reader, records);
    apply_update_collection_dictionaries(doc, reader, records);
    if !modern_dxf(doc, reader) {
        return 0;
    }

    let child_owners = sequence_child_owners(records);
    let mut next = first_allocated_handle(doc, reader, &child_owners, records);
    let mut changed = 0;
    let indices = records
        .iter()
        .enumerate()
        .filter(|(_, record)| remappable_seqend(doc, reader, &child_owners, record))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    for index in indices {
        let record = &mut records[index];
        let expected_props = default_seqend_props(doc, next, handle(&record["owner"]).unwrap_or(0));
        let projected = format!("#{next:X}");
        if record["handle"] != projected || record["props"] != expected_props {
            record["handle"] = Value::String(projected);
            record["props"] = expected_props;
            changed += 1;
        }
        next = next.saturating_add(1);
    }
    changed
}

/// Compute the projected `$HANDSEED` from the source seed and retained
/// records. This is separate because the header is emitted beside the object
/// projection.
pub(super) fn handle_seed(doc: &CadDocument, records: &[Value]) -> u64 {
    if !source_document(doc) {
        return doc.header.handle_seed;
    }
    let max_record = records
        .iter()
        .filter_map(|record| handle(&record["handle"]))
        .max()
        .unwrap_or(0);
    doc.header
        .source_handle_seed
        .unwrap_or(0)
        .max(max_record.saturating_add(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(handle: &str, ty: &str) -> Value {
        json!({"handle": handle, "type": ty})
    }

    fn owned_record(handle: &str, ty: &str, owner: &str) -> Value {
        let mut value = record(handle, ty);
        value["owner"] = json!(owner);
        value
    }

    fn source_doc(version: DxfVersion) -> CadDocument {
        let mut doc = CadDocument::with_version(version);
        doc.header.source_handle_seed = Some(0x40);
        doc.header.handle_seed = 0x200;
        doc
    }

    #[test]
    fn modern_dxf_allocates_seqends_after_retained_source_records() {
        let doc = source_doc(DxfVersion::AC1015);
        let mut records = vec![
            owned_record("#42A", "Seqend", "#90"),
            owned_record("#91", "Vertex2D", "#90"),
            record("#90", "Polyline2D"),
            owned_record("#431", "Seqend", "#1FF"),
            owned_record("#1FE", "Vertex2D", "#1FF"),
            record("#1FF", "Line"),
        ];

        assert_eq!(apply(&doc, "dxf", &mut records), 2);
        assert_eq!(records[0]["handle"], "#200");
        assert_eq!(records[3]["handle"], "#201");
        assert_eq!(handle_seed(&doc, &records), 0x202);
    }

    #[test]
    fn explicit_source_seed_below_max_retained_handle_advances_cursor() {
        let doc = source_doc(DxfVersion::AC1015);
        let mut records = vec![
            record("#500", "Line"),
            owned_record("#42A", "Seqend", "#100"),
            owned_record("#101", "Vertex2D", "#100"),
        ];
        assert_eq!(apply(&doc, "dxf", &mut records), 1);
        assert_eq!(records[1]["handle"], "#501");
        assert_eq!(handle_seed(&doc, &records), 0x502);
    }

    #[test]
    fn source_handles_are_preserved_outside_modern_dxf_but_seed_tracks_source() {
        for (version, reader) in [(DxfVersion::AC1014, "dxf"), (DxfVersion::AC1015, "dwg")] {
            let doc = source_doc(version);
            let mut records = vec![record("#42A", "Seqend")];
            assert_eq!(apply(&doc, reader, &mut records), 0);
            assert_eq!(records[0]["handle"], "#42A");
            assert_eq!(handle_seed(&doc, &records), 0x42B);
        }
    }

    #[test]
    fn constructed_documents_keep_native_sequence_handles() {
        let doc = CadDocument::with_version(DxfVersion::AC1032);
        let mut records = vec![record("#42A", "Seqend")];
        assert_eq!(apply(&doc, "dxf", &mut records), 0);
        assert_eq!(records[0]["handle"], "#42A");
        assert_eq!(handle_seed(&doc, &records), doc.header.handle_seed);
    }

    #[test]
    fn no_vertex_polyline_does_not_allocate_a_sequence_marker() {
        let doc = source_doc(DxfVersion::AC1032);
        let mut records = vec![record("#100", "Polyline2D")];
        assert_eq!(apply(&doc, "dxf", &mut records), 0);
        assert_eq!(records.len(), 1);
        assert_eq!(handle_seed(&doc, &records), 0x101);
    }

    #[test]
    fn update_collections_appends_missing_named_dictionaries_without_replacement() {
        let mut doc = source_doc(DxfVersion::AC1032);
        doc.header.named_objects_dict_handle = Handle::new(0xC);
        let root = super::record(
            Handle::new(0xC),
            "CadDictionary",
            Handle::NULL,
            super::props(
                "CadDictionary",
                [
                    ("ClonningFlags", json!(1)),
                    ("EntryHandles", json!([])),
                    ("EntryNames", json!([])),
                    ("ExtendedData", json!([])),
                    ("HardOwnerFlag", json!(false)),
                    ("HasDynamicSubclass", json!(false)),
                    ("Name", json!("ROOT")),
                    ("ObjectName", json!("DICTIONARY")),
                    ("ObjectType", json!(42)),
                    ("Reactors", json!([])),
                    ("SubclassMarker", json!("AcDbDictionary")),
                    ("XDictionary", Value::Null),
                ],
            ),
        );
        let source = record("#1FF", "Line");
        let mut records = vec![root, source.clone()];

        assert_eq!(apply(&doc, "dwg", &mut records), 0);
        assert_eq!(
            records[0]["props"]["EntryNames"],
            json!(["ACAD_IMAGE_DICT", "ACAD_PDFDEFINITIONS"])
        );
        assert_eq!(records[0]["props"]["EntryHandles"], json!([0x200, 0x201]));
        assert_eq!(records[1], source);
        assert!(records.iter().any(|record| {
            record["handle"] == "#200"
                && record["type"] == "CadDictionary"
                && record["props"]["Name"] == "ACAD_IMAGE_DICT"
        }));
        assert!(records.iter().any(|record| {
            record["handle"] == "#201"
                && record["type"] == "CadDictionary"
                && record["props"]["Name"] == "ACAD_PDFDEFINITIONS"
        }));
    }

    #[test]
    fn update_collections_leaves_existing_dictionary_entries_untouched() {
        let mut doc = source_doc(DxfVersion::AC1032);
        doc.header.named_objects_dict_handle = Handle::new(0xC);
        let root = super::record(
            Handle::new(0xC),
            "CadDictionary",
            Handle::NULL,
            super::props(
                "CadDictionary",
                [
                    ("ClonningFlags", json!(1)),
                    ("EntryHandles", json!([0x150])),
                    ("EntryNames", json!(["ACAD_IMAGE_DICT"])),
                    ("ExtendedData", json!([])),
                    ("HardOwnerFlag", json!(false)),
                    ("HasDynamicSubclass", json!(false)),
                    ("Name", json!("ROOT")),
                    ("ObjectName", json!("DICTIONARY")),
                    ("ObjectType", json!(42)),
                    ("Reactors", json!([])),
                    ("SubclassMarker", json!("AcDbDictionary")),
                    ("XDictionary", Value::Null),
                ],
            ),
        );
        let existing = super::record(
            Handle::new(0x150),
            "CadDictionary",
            Handle::new(0xC),
            super::props(
                "CadDictionary",
                [
                    ("ClonningFlags", json!(0)),
                    ("EntryHandles", json!([])),
                    ("EntryNames", json!([])),
                    ("ExtendedData", json!([])),
                    ("HardOwnerFlag", json!(true)),
                    ("HasDynamicSubclass", json!(true)),
                    ("Name", json!("ACAD_IMAGE_DICT")),
                    ("ObjectName", json!("DICTIONARY")),
                    ("ObjectType", json!(42)),
                    ("Reactors", json!([])),
                    ("SubclassMarker", json!("AcDbDictionary")),
                    ("XDictionary", Value::Null),
                ],
            ),
        );
        let before = existing.clone();
        let mut records = vec![root, existing];
        apply(&doc, "dwg", &mut records);
        assert_eq!(records[1], before);
        assert_eq!(records[0]["props"]["EntryHandles"], json!([0x150, 0x151]));
        assert_eq!(
            records[0]["props"]["EntryNames"],
            json!(["ACAD_IMAGE_DICT", "ACAD_PDFDEFINITIONS"])
        );
    }

    #[test]
    fn modern_dxf_replaces_sequence_common_properties_with_defaults() {
        let doc = source_doc(DxfVersion::AC1015);
        let mut seqend = record("#42A", "Seqend");
        seqend["owner"] = json!("#100");
        seqend["props"] = json!({"ObjectName":"wrong", "Layer":"#DEAD", "LineType":"#BEEF"});
        let records_child = owned_record("#90", "Vertex2D", "#100");
        let mut records = vec![record("#100", "Polyline2D"), seqend, records_child];
        assert_eq!(apply(&doc, "dxf", &mut records), 1);
        assert_eq!(records[1]["handle"], "#101");
        assert_eq!(
            records[1]["props"],
            default_seqend_props(&doc, 0x101, 0x100)
        );
        assert_eq!(records[1]["owner"], "#100");
    }

    #[test]
    fn r12_insert_sequence_owners_follow_entities_reader_registration() {
        let mut doc = source_doc(DxfVersion::AC1009);
        doc.source_entity_owners = Some([(Handle::new(0x706), Handle::NULL)].into_iter().collect());
        let mut records = vec![record("#24B0", "BlockRecord"), record("#704", "Insert")];
        records[1]["owner"] = json!("#24B0");
        let mut seqend = record("#706", "Seqend");
        seqend["owner"] = json!("#704");
        records.push(seqend);
        records.push(owned_record("#705", "AttributeEntity", "#704"));
        assert_eq!(apply(&doc, "dxf", &mut records), 0);
        assert_eq!(records[2]["owner"], "#24B0");
    }

    #[test]
    fn r12_explicit_seqend_owner_is_not_reparented() {
        let mut doc = source_doc(DxfVersion::AC1009);
        doc.source_entity_owners = Some(
            [(Handle::new(0x706), Handle::new(0x704))]
                .into_iter()
                .collect(),
        );
        let mut records = vec![record("#24B0", "BlockRecord"), record("#704", "Insert")];
        records[1]["owner"] = json!("#24B0");
        records.push(owned_record("#706", "Seqend", "#704"));
        records.push(owned_record("#705", "AttributeEntity", "#704"));
        assert_eq!(apply(&doc, "dxf", &mut records), 0);
        assert_eq!(records[2]["owner"], "#704");
    }

    #[test]
    fn r12_ownerless_attributes_join_the_enclosing_block() {
        let mut doc = source_doc(DxfVersion::AC1009);
        doc.source_entity_owners = Some(
            [
                (Handle::new(0x101), Handle::NULL),
                (Handle::new(0x102), Handle::new(0x100)),
            ]
            .into_iter()
            .collect(),
        );
        let mut records = vec![
            owned_record("#100", "Insert", "#50"),
            owned_record("#101", "AttributeEntity", "#100"),
            owned_record("#102", "AttributeEntity", "#100"),
        ];
        apply(&doc, "dxf", &mut records);
        assert_eq!(records[1]["owner"], "#50");
        assert_eq!(records[2]["owner"], "#100");
    }

    #[test]
    fn empty_modern_sequence_preserves_source_seqend_and_handle_seed() {
        let doc = source_doc(DxfVersion::AC1032);
        let mut records = vec![
            record("#100", "Polyline2D"),
            owned_record("#500", "Seqend", "#100"),
            record("#200", "Insert"),
            owned_record("#501", "Seqend", "#200"),
        ];
        assert_eq!(apply(&doc, "dxf", &mut records), 0);
        assert_eq!(records[1]["handle"], "#500");
        assert_eq!(records[3]["handle"], "#501");
        assert_eq!(handle_seed(&doc, &records), 0x502);
    }

    #[test]
    fn empty_r12_insert_does_not_override_seqend_owner() {
        let doc = source_doc(DxfVersion::AC1009);
        let mut records = vec![record("#24B0", "BlockRecord"), record("#704", "Insert")];
        records[1]["owner"] = json!("#24B0");
        let mut seqend = owned_record("#706", "Seqend", "#704");
        seqend["props"] = json!({"source_owner": "#704"});
        records.push(seqend);
        assert_eq!(apply(&doc, "dxf", &mut records), 0);
        assert_eq!(records[2]["owner"], "#704");
    }
}
