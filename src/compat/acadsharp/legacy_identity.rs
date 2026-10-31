//! R12's source templates and document registration use separate allocators.
//! Replay both without changing native identities or relying on fixture names.
use super::*;
use std::collections::{HashMap, HashSet};

const TABLES: &[(&str, &str, &str)] = &[
    ("APPID", "AppIdsTable", "AppId"),
    ("STYLE", "TextStylesTable", "TextStyle"),
    ("LTYPE", "LineTypesTable", "LineType"),
    ("LAYER", "LayersTable", "Layer"),
    ("UCS", "UCSTable", "UCS"),
    ("VIEW", "ViewsTable", "View"),
    ("BLOCK_RECORD", "BlockRecordsTable", "BlockRecord"),
    ("DIMSTYLE", "DimensionStylesTable", "DimensionStyle"),
    ("VPORT", "VPortsTable", "VPort"),
];
const DICTIONARIES: &[&str] = &[
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
    "ACAD_PDFDEFINITIONS",
];
fn id(v: &Value) -> u64 {
    v.as_str()
        .and_then(|s| s.strip_prefix('#'))
        .and_then(|s| u64::from_str_radix(s, 16).ok())
        .unwrap_or(0)
}
fn key(ty: &str, name: &str) -> (String, String) {
    (ty.into(), name.to_uppercase())
}

struct Node {
    value: Value,
    old: u64,
    template: u64,
    final_handle: u64,
    rewritten: bool,
}
struct Replay {
    nodes: Vec<Node>,
    pending: Vec<usize>,
    templates: HashSet<u64>,
    initial: u64,
    occupied: HashSet<u64>,
    cursor: u64,
}
impl Replay {
    fn add(&mut self, value: Value, source: u64, template: bool) -> usize {
        let i = self.nodes.len();
        let assigned = if template && (source == 0 || !self.templates.insert(source)) {
            self.pending.push(i);
            0
        } else {
            source
        };
        self.initial = self.initial.max(assigned);
        self.nodes.push(Node {
            old: id(&value["handle"]),
            value,
            template: assigned,
            final_handle: 0,
            rewritten: false,
        });
        i
    }
    fn register(&mut self, i: usize) {
        if self.nodes[i].final_handle != 0 {
            return;
        }
        let h = self.nodes[i].template;
        let h = if h == 0 || self.occupied.contains(&h) {
            while self.occupied.contains(&self.cursor) {
                self.cursor += 1;
            }
            self.cursor
        } else {
            h
        };
        self.cursor = self.cursor.max(h + 1);
        self.occupied.insert(h);
        self.nodes[i].final_handle = h;
    }
}

pub(super) fn applies(doc: &CadDocument, reader: &str) -> bool {
    reader == "dxf" && doc.version == DxfVersion::AC1009 && doc.legacy_dxf_tables.is_some()
}

/// Rewrite only canonical references, never arbitrary strings resembling handles.
fn rewrite(value: &mut Value, name: &str, refs: &HashSet<&str>, map: &HashMap<u64, u64>) {
    match value {
        Value::Array(pairs) if name == "ExtendedData" => {
            for pair in pairs {
                if let Some(&new) = map.get(&id(&pair[0])) {
                    pair[0] = handle(Handle::new(new));
                }
                // RawValue (including numeric layer/handle records) is a
                // stored source number, not a registered CadObject reference.
            }
        }
        Value::String(_) if refs.contains(name) => {
            if let Some(&new) = map.get(&id(value)) {
                *value = handle(Handle::new(new));
            }
        }
        Value::Number(n) if name == "EntryHandles" => {
            if let Some(new) = n.as_u64().and_then(|old| map.get(&old)) {
                *value = json!(new);
            }
        }
        Value::Array(items) => {
            for item in items {
                rewrite(item, name, refs, map);
            }
        }
        Value::Object(items) => {
            for (key, item) in items {
                rewrite(item, key, refs, map);
            }
        }
        _ => {}
    }
}
fn dictionary(h: u64, owner: u64, name: &str) -> Value {
    record(
        Handle::new(h),
        "CadDictionary",
        Handle::new(owner),
        props(
            "CadDictionary",
            [
                ("ClonningFlags", json!(0)),
                ("EntryHandles", json!([])),
                ("EntryNames", json!([])),
                ("ExtendedData", json!([])),
                ("HardOwnerFlag", json!(false)),
                ("HasDynamicSubclass", json!(false)),
                ("Name", json!(name)),
                ("ObjectName", json!("DICTIONARY")),
                ("ObjectType", json!(42)),
                ("Reactors", json!([])),
                ("SubclassMarker", json!("AcDbDictionary")),
                ("XDictionary", Value::Null),
            ],
        ),
    )
}

/// Handle rewrite retained for rendering one deferred record after replay.
pub(super) struct Rewrite {
    refs: HashSet<&'static str>,
    map: HashMap<u64, u64>,
}

impl Rewrite {
    pub(super) fn apply(&self, value: &mut Value) {
        rewrite(value, "", &self.refs, &self.map);
    }
}

pub(super) fn apply(doc: &CadDocument, header: &mut Value, records: &mut Vec<Value>) -> Rewrite {
    let source_tables = doc.legacy_dxf_tables.as_ref().expect("R12 source tables");
    let source_blocks = doc.legacy_dxf_blocks.as_deref().unwrap_or(&[]);
    let mut prototypes = HashMap::new();
    let mut by_handle = HashMap::new();
    for record in records.iter() {
        prototypes
            .entry(key(
                record["type"].as_str().unwrap_or(""),
                record["props"]["Name"].as_str().unwrap_or(""),
            ))
            .or_insert_with(|| record.clone());
        by_handle
            .entry(id(&record["handle"]))
            .or_insert_with(|| record.clone());
    }
    let seed = doc.header.source_handle_seed.unwrap_or(1).max(1);
    let mut r = Replay {
        nodes: vec![],
        pending: vec![],
        templates: HashSet::new(),
        initial: seed.max(doc.source_explicit_handle_max.unwrap_or(0)),
        occupied: HashSet::new(),
        cursor: seed,
    };
    let mut table_nodes = HashMap::new();
    let mut entries: HashMap<String, Vec<usize>> = HashMap::new();
    let mut unnamed = 0;
    // DxfTablesSectionReader adds entries before their table. Empty tables
    // return before AddTemplate and retain their builder constructor identity.
    for table in source_tables {
        let Some(&(_, ty, entry_ty)) = TABLES.iter().find(|t| t.0 == table.name) else {
            continue;
        };
        for entry in &table.entries {
            let value = if let Some(style) = &entry.text_style {
                let mut out = Vec::new();
                tables::text_style(doc, style, &mut out);
                out.pop()
            } else {
                prototypes.get(&key(entry_ty, &entry.name)).cloned()
            };
            if let Some(mut value) = value {
                if entry.name.is_empty() {
                    value["props"]["Name"] = json!(format!("unnamed{unnamed}"));
                    unnamed += 1;
                }
                let i = r.add(value, entry.handle.value(), true);
                entries.entry(table.name.clone()).or_default().push(i);
            }
        }
        if let Some(value) = prototypes.get(&key(ty, "")).cloned() {
            let i = r.add(value, table.handle.value(), !table.entries.is_empty());
            table_nodes.insert(table.name.clone(), i);
        }
    }
    for &(name, ty, _) in TABLES {
        if !table_nodes.contains_key(name) {
            if let Some(value) = prototypes.get(&key(ty, "")).cloned() {
                let i = r.add(value, 0, false);
                table_nodes.insert(name.into(), i);
            }
        }
    }
    let mut block_children = HashMap::new();
    for block in source_blocks {
        let Some(value) = prototypes.get(&key("BlockRecord", &block.name)).cloned() else {
            continue;
        };
        let old_begin = id(&value["props"]["BlockEntity"]);
        let old_end = id(&value["props"]["BlockEnd"]);
        let i = r.add(value, 0, true);
        entries.entry("BLOCK_RECORD".into()).or_default().push(i);
        let mut end = by_handle.get(&old_end).cloned().unwrap_or_else(|| {
            record(
                Handle::new(old_end),
                "BlockEnd",
                Handle::NULL,
                props("BlockEnd", []),
            )
        });
        end["owner"] = handle(Handle::new(r.nodes[i].old));
        let e = r.add(end, block.end_handle.value(), true);
        let mut begin = prototypes
            .get(&key("Block", &block.name))
            .cloned()
            .unwrap_or_else(|| {
                record(
                    Handle::new(old_begin),
                    "Block",
                    Handle::NULL,
                    props("Block", []),
                )
            });
        begin["owner"] = handle(Handle::new(r.nodes[i].old));
        let b = r.add(begin, block.begin_handle.value(), true);
        block_children.insert(i, (b, e));
    }
    // A missing model-space BLOCK has a template only for the record; its
    // constructor's begin/end acquire handles during document registration.
    if !entries.get("BLOCK_RECORD").into_iter().flatten().any(|&i| {
        r.nodes[i].value["props"]["Name"]
            .as_str()
            .is_some_and(|s| s.eq_ignore_ascii_case("*Model_Space"))
    }) {
        if let Some(value) = prototypes.get(&key("BlockRecord", "*Model_Space")).cloned() {
            let old_begin = id(&value["props"]["BlockEntity"]);
            let old_end = id(&value["props"]["BlockEnd"]);
            let i = r.add(value, 0, true);
            entries.entry("BLOCK_RECORD".into()).or_default().push(i);
            let b = r.add(
                by_handle.get(&old_begin).cloned().unwrap_or_else(|| {
                    record(
                        Handle::new(old_begin),
                        "Block",
                        Handle::NULL,
                        props("Block", []),
                    )
                }),
                0,
                false,
            );
            let e = r.add(
                by_handle.get(&old_end).cloned().unwrap_or_else(|| {
                    record(
                        Handle::new(old_end),
                        "BlockEnd",
                        Handle::NULL,
                        props("BlockEnd", []),
                    )
                }),
                0,
                false,
            );
            block_children.insert(i, (b, e));
        }
    }
    for &i in &r.pending {
        r.initial += 1;
        r.nodes[i].template = r.initial;
    }
    let mut zero_layer = entries
        .get("LAYER")
        .into_iter()
        .flatten()
        .copied()
        .find(|&i| r.nodes[i].value["props"]["Name"] == "0");
    let mut bylayer = entries
        .get("LTYPE")
        .into_iter()
        .flatten()
        .copied()
        .find(|&i| {
            r.nodes[i].value["props"]["Name"]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case("ByLayer"))
        });
    for &(name, _, _) in TABLES {
        if let Some(&i) = table_nodes.get(name) {
            r.register(i);
        }
        for i in entries.get(name).cloned().unwrap_or_default() {
            r.register(i);
            if let Some(&(b, e)) = block_children.get(&i) {
                r.register(b);
                if zero_layer.is_none() {
                    if let Some(value) = prototypes.get(&key("Layer", "0")).cloned() {
                        let l = r.add(value, 0, false);
                        r.register(l);
                        zero_layer = Some(l);
                        entries.entry("LAYER".into()).or_default().push(l);
                    }
                }
                // AssignDocument on the first entity resolves its default
                // line type and inserts ByLayer if absent from source LTYPE.
                if bylayer.is_none() {
                    if let Some(value) = prototypes.get(&key("LineType", "ByLayer")).cloned() {
                        let l = r.add(value, 0, false);
                        r.register(l);
                        bylayer = Some(l);
                        entries.entry("LTYPE".into()).or_default().push(l);
                    }
                }
                r.register(e);
            }
        }
    }
    let mut map = HashMap::new();
    for node in &r.nodes {
        if node.old != 0 {
            map.insert(node.old, node.final_handle);
        }
    }
    let mut refs: HashSet<_> = SCHEMA
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut c = line.split('\t');
            c.next()?;
            let member = c.next()?;
            (c.next()? == "handle").then_some(member)
        })
        .collect();
    refs.extend([
        "handle",
        "owner",
        "Reactors",
        "Entities",
        "AttributeDefinitions",
        "Viewports",
    ]);
    let model_space = r
        .nodes
        .iter()
        .find(|n| {
            n.value["type"] == "BlockRecord"
                && n.value["props"]["Name"]
                    .as_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case("*Model_Space"))
        })
        .map(|n| n.final_handle);
    let selected: HashSet<_> = r.nodes.iter().map(|n| n.old).collect();
    let mut out = Vec::new();
    for value in records.iter() {
        let ty = value["type"].as_str().unwrap_or("");
        if selected.contains(&id(&value["handle"]))
            || ty == "Block"
            || ty == "BlockEnd"
            || value["props"].get("ProxyGeometries").is_none()
            || ty == "UnknownEntity"
        {
            continue;
        }
        // The DXF reader's exact enum switch rejects non-linear dimension
        // types carrying the user-positioned-text bit (native data stays).
        if ty == "DimensionDiameter" && doc.get_entity(Handle::new(id(&value["handle"]))).is_some_and(super::view::rejects_legacy_dimension) { continue; }
        let mut value = value.clone();
        if ty == "Viewport" && super::view::ownerless_entities_record(doc, Handle::new(id(&value["handle"]))) {
            // Ownerless ENTITIES records enter ModelSpace even if group 67
            // would place them in native PaperSpace. BLOCKS records are absent
            // from this provenance map and retain their block membership.
            if let Some(model) = model_space {
                value["owner"] = handle(Handle::new(model));
            }
        } else {
            rewrite(&mut value["owner"], "owner", &refs, &map);
        }
        rewrite(&mut value["props"], "", &refs, &map);
        let h = id(&value["handle"]);
        r.occupied.insert(h);
        r.cursor = r.cursor.max(h + 1);
        out.push(value);
    }
    // The entity projector emits nested records before their parent. R12
    // promotes ownerless INSERT children into the block collection, where the
    // source order remains INSERT, ATTRIBs, then SEQEND.
    let out_index: HashMap<_, _> = out
        .iter()
        .map(|value| (id(&value["handle"]), value))
        .collect();
    let mut promoted = HashSet::new();
    let mut following: HashMap<u64, Vec<Value>> = HashMap::new();
    for entity in &doc.entities {
        let crate::entities::EntityType::Insert(insert) = entity.as_ref() else {
            continue;
        };
        let parent = insert.common.handle.value();
        let Some(parent_record) = out_index.get(&parent) else {
            continue;
        };
        let children = insert
            .attributes
            .iter()
            .map(|a| a.common.handle.value())
            .chain(insert.seqend.iter().map(|end| end.common.handle.value()));
        for child in children {
            if let Some(child_record) = out_index.get(&child) {
                if child_record["owner"] == parent_record["owner"] {
                    promoted.insert(child);
                    following
                        .entry(parent)
                        .or_default()
                        .push(child_record["handle"].clone());
                }
            }
        }
    }
    let mut block_entities: HashMap<u64, Vec<Value>> = HashMap::new();
    let mut definitions: HashMap<u64, Vec<Value>> = HashMap::new();
    let mut viewports: HashMap<u64, Vec<Value>> = HashMap::new();
    for entity in &out {
        if !promoted.contains(&id(&entity["handle"])) {
            let members = block_entities.entry(id(&entity["owner"])).or_default();
            members.push(entity["handle"].clone());
            members.extend(following.remove(&id(&entity["handle"])).unwrap_or_default());
        }
        if entity["type"] == "Viewport" {
            viewports
                .entry(id(&entity["owner"]))
                .or_default()
                .push(entity["handle"].clone());
        }
        if entity["type"] == "AttributeDefinition" {
            definitions
                .entry(id(&entity["owner"]))
                .or_default()
                .push(entity["handle"].clone());
        }
    }
    for (name, &i) in &table_nodes {
        r.nodes[i].value["props"]["Count"] = json!(entries.get(name).map_or(0, Vec::len));
    }
    for (i, (b, e)) in block_children {
        let mut value = r.nodes[i].value.clone();
        rewrite(&mut value, "", &refs, &map);
        value["props"]["BlockEntity"] = handle(Handle::new(r.nodes[b].final_handle));
        value["props"]["BlockEnd"] = handle(Handle::new(r.nodes[e].final_handle));
        value["props"]["Layout"] = Value::Null;
        value["props"]["Entities"] = json!(block_entities
            .remove(&r.nodes[i].final_handle)
            .unwrap_or_default());
        value["props"]["AttributeDefinitions"] = json!(definitions
            .remove(&r.nodes[i].final_handle)
            .unwrap_or_default());
        value["props"]["Viewports"] = json!(viewports
            .remove(&r.nodes[i].final_handle)
            .unwrap_or_default());
        r.nodes[i].value = value;
        // Already rewritten; prevent a second mapping through overlapping IDs.
        r.nodes[i].rewritten = true;
    }
    let standard_style = r
        .nodes
        .iter()
        .find(|n| {
            n.value["type"] == "TextStyle"
                && n.value["props"]["Name"]
                    .as_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case("Standard"))
        })
        .map(|n| n.final_handle);
    for node in &mut r.nodes {
        if !node.rewritten {
            rewrite(&mut node.value, "", &refs, &map);
        }
        if node.value["type"] == "DimensionStyle" && node.value["props"]["Style"].is_null() {
            if let Some(style) = standard_style {
                node.value["props"]["Style"] = handle(Handle::new(style));
            }
        }
        node.value["handle"] = handle(Handle::new(node.final_handle));
        out.push(node.value.clone());
    }
    let root = r.cursor;
    r.cursor += 1;
    let mut root_value = dictionary(root, 0, "ROOT");
    root_value["owner"] = handle(Handle::NULL);
    for name in DICTIONARIES {
        let h = r.cursor;
        r.cursor += 1;
        root_value["props"]["EntryHandles"]
            .as_array_mut()
            .unwrap()
            .push(json!(h));
        root_value["props"]["EntryNames"]
            .as_array_mut()
            .unwrap()
            .push(json!(name));
        out.push(dictionary(h, root, name));
    }
    out.push(root_value);
    rewrite(header, "", &refs, &map);
    header["HandleSeed"] = json!(r.cursor);
    // Header references may still point at a native default replaced by a
    // source record of the same name; resolve by the source header name.
    for (property, ty, name) in [
        (
            "CurrentLayer",
            "Layer",
            doc.header.current_layer_name.as_str(),
        ),
        (
            "CurrentLineType",
            "LineType",
            doc.header.current_linetype_name.as_str(),
        ),
        (
            "CurrentTextStyle",
            "TextStyle",
            doc.header.current_text_style_name.as_str(),
        ),
        (
            "CurrentDimensionStyle",
            "DimensionStyle",
            doc.header.current_dimstyle_name.as_str(),
        ),
        (
            "DimensionTextStyle",
            "TextStyle",
            doc.header.dim_text_style_name.as_str(),
        ),
    ] {
        if let Some(record) = out.iter().find(|v| {
            v["type"] == ty
                && v["props"]["Name"]
                    .as_str()
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
        }) {
            header[property] = record["handle"].clone();
        }
    }
    *records = out;
    Rewrite { refs, map }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::dxf::DxfReader;
    use std::io::Cursor;

    fn source(extra: &str, entity: &str) -> CadDocument {
        let text = format!("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n9\n$HANDSEED\n5\n100\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLTYPE\n0\nLTYPE\n2\nContinuous\n70\n0\n0\nENDTAB\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\n0\n70\n0\n62\n7\n6\nContinuous\n0\nENDTAB\n0\nTABLE\n2\nSTYLE\n0\nSTYLE\n2\nSTANDARD\n70\n0\n{extra}0\nENDTAB\n0\nTABLE\n2\nAPPID\n0\nAPPID\n2\nACAD\n70\n0\n0\nENDTAB\n0\nTABLE\n2\nDIMSTYLE\n0\nDIMSTYLE\n2\nSTANDARD\n70\n0\n0\nENDTAB\n0\nTABLE\n2\nVPORT\n0\nVPORT\n2\n*ACTIVE\n70\n0\n0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\n*Model_Space\n70\n0\n0\nENDBLK\n5\n20\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entity}0\nENDSEC\n0\nEOF\n");
        DxfReader::from_reader(Cursor::new(text.into_bytes()))
            .unwrap()
            .read()
            .unwrap()
    }
    fn named<'a>(dump: &'a Value, ty: &str, name: &str) -> &'a Value {
        dump["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["type"] == ty && v["props"]["Name"] == name)
            .unwrap()
    }
    #[test]
    fn preserves_source_handles_and_rewrites_nested_generated_references() {
        let document = source("", "0\nLINE\n5\n30\n8\n0\n10\n0\n20\n0\n11\n1\n21\n1\n");
        let native = document.clone();
        let dump = dump_reader_document(&document, "test.dxf", "dxf");
        assert_eq!(named(&dump, "Layer", "0")["handle"], "#103");
        assert_eq!(
            named(&dump, "BlockRecord", "*Model_Space")["handle"],
            "#10D"
        );
        assert_eq!(
            named(&dump, "BlockRecord", "*Model_Space")["props"]["BlockEnd"],
            "#20"
        );
        assert_eq!(named(&dump, "DimensionStyle", "STANDARD")["handle"], "#111");
        assert_eq!(dump["header"]["CurrentLayer"], "#103");
        assert_eq!(dump["header"]["HandleSeed"], 0x123);
        let line = dump["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["type"] == "Line")
            .unwrap();
        assert_eq!(line["handle"], "#30");
        assert_eq!(line["owner"], "#10D");
        assert_eq!(line["props"]["Layer"], "#103");
        assert_eq!(document, native);
    }
    #[test]
    fn source_order_and_duplicate_unnamed_styles_drive_template_slots() {
        let document = source("0\nSTYLE\n2\n\n70\n1\n3\nfirst.shx\n0\nSTYLE\n2\n#105\n70\n0\n0\nSTYLE\n2\n\n70\n1\n3\nsecond.shx\n", "");
        let dump = dump_reader_document(&document, "test.dxf", "dxf");
        assert_eq!(named(&dump, "TextStyle", "unnamed0")["handle"], "#106");
        assert_eq!(
            named(&dump, "TextStyle", "unnamed0")["props"]["Filename"],
            "first.shx"
        );
        assert_eq!(named(&dump, "TextStyle", "#105")["handle"], "#107");
        assert_eq!(named(&dump, "TextStyle", "unnamed1")["handle"], "#108");
        assert_eq!(
            named(&dump, "TextStyle", "unnamed1")["props"]["Filename"],
            "second.shx"
        );
    }
    #[test]
    fn explicit_entity_above_stale_seed_sets_template_floor() {
        let document = source("", "0\nLINE\n5\n500\n8\n0\n10\n0\n20\n0\n11\n1\n21\n1\n");
        let dump = dump_reader_document(&document, "test.dxf", "dxf");
        assert_eq!(named(&dump, "Layer", "0")["handle"], "#503");
        assert!(dump["objects"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["type"] == "Line" && v["handle"] == "#500"));
    }
    #[test]
    fn r12_exact_dimension_flag_dispatch_preserves_the_native_dimension() {
        let document = source("", "0\nDIMENSION\n5\n30\n70\n3\n10\n0\n20\n0\n15\n1\n25\n0\n0\nDIMENSION\n5\n31\n70\n131\n10\n0\n20\n0\n15\n1\n25\n0\n");
        assert!(document.get_entity(Handle::new(0x30)).is_some());
        assert!(document.get_entity(Handle::new(0x31)).is_some());
        let dump = dump_reader_document(&document, "test.dxf", "dxf");
        let objects = dump["objects"].as_array().unwrap();
        assert!(objects
            .iter()
            .any(|v| v["handle"] == "#30" && v["type"] == "DimensionDiameter"));
        assert!(!objects.iter().any(|v| v["handle"] == "#31"));
    }

    #[test]
    fn ownerless_entities_viewport_registers_in_model_space() {
        let document = source(
            "",
            "0\nVIEWPORT\n5\n30\n67\n1\n8\n0\n10\n0\n20\n0\n40\n1\n41\n1\n69\n2\n",
        );
        let dump = dump_reader_document(&document, "test.dxf", "dxf");
        let model = named(&dump, "BlockRecord", "*Model_Space");
        let viewport = dump["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["handle"] == "#30")
            .unwrap();
        assert_eq!(viewport["owner"], model["handle"]);
        assert!(model["props"]["Entities"]
            .as_array()
            .unwrap()
            .contains(&json!("#30")));
    }

    #[test]
    fn promoted_insert_children_keep_source_collection_order() {
        let document = source("", "0\nINSERT\n5\n30\n2\n*Model_Space\n66\n1\n10\n0\n20\n0\n0\nATTRIB\n5\n31\n1\nlabel\n2\nTAG\n70\n0\n10\n0\n20\n0\n40\n1\n0\nSEQEND\n5\n32\n");
        let dump = dump_reader_document(&document, "test.dxf", "dxf");
        assert_eq!(
            named(&dump, "BlockRecord", "*Model_Space")["props"]["Entities"],
            json!(["#30", "#31", "#32"])
        );
    }

    #[test]
    fn is_noop_without_source_seed_or_for_non_r12() {
        let mut document = CadDocument::with_version(DxfVersion::AC1009);
        assert!(!applies(&document, "dxf"));
        document.legacy_dxf_tables = Some(vec![]);
        document.version = DxfVersion::AC1015;
        assert!(!applies(&document, "dxf"));
    }
}
