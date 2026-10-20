// SPDX-License-Identifier: MPL-2.0
//! Plan registration using identity metadata, then render one record at a time.
//!
//! Handles cannot simply be streamed in native collection order: the reader
//! view filters unreachable objects and can allocate/rewrite identities. The
//! plan retains those graph edges and borrowed source recipes, never geometry,
//! appearance, text, or other arbitrary property payloads.

use super::*;
use std::io::Write;

const SOURCE: &str = "!source";

struct Sources<'a> {
    tables: Vec<Box<dyn Fn() -> Value + 'a>>,
    entities: Vec<entities::Record<'a>>,
    objects: objects::Records<'a>,
}

impl<'a> Sources<'a> {
    fn new(doc: &'a CadDocument, reader: &'a str) -> Self {
        Self {
            tables: tables::records(doc),
            entities: entities::records(doc, reader),
            objects: objects::Records::new(doc, reader),
        }
    }

    fn len(&self) -> usize {
        self.tables.len() + self.entities.len() + self.objects.len()
    }

    fn render(&self, doc: &CadDocument, reader: &str, index: usize) -> Value {
        if index < self.tables.len() {
            return self.tables[index]();
        }
        let index = index - self.tables.len();
        if index < self.entities.len() {
            return self.entities[index].render(doc, reader);
        }
        self.objects.render(index - self.entities.len())
    }

    fn metadata(&self, doc: &CadDocument, reader: &str, index: usize) -> Value {
        let mut value = if let Some(entity) = index
            .checked_sub(self.tables.len())
            .and_then(|index| self.entities.get(index))
        {
            entity.metadata(doc, reader)
        } else {
            self.render(doc, reader, index)
        };
        retain_metadata(&mut value);
        value[SOURCE] = json!(index);
        value
    }
}

/// These are the only properties read by the registration/replay passes.
/// ProxyGeometries is a presence marker; Nodes needs only Expression edges.
fn retain_metadata(value: &mut Value) {
    let p = value["props"].as_object_mut().expect("record properties");
    p.retain(|key, _| {
        matches!(
            key.as_str(),
            "Name"
                | "SubclassMarker"
                | "ProxyGeometries"
                | "XDictionary"
                | "EntryHandles"
                | "EntryNames"
                | "Nodes"
                | "Entities"
                | "BlockEntity"
                | "BlockEnd"
                | "Style"
        )
    });
    if let Some(proxy) = p.get_mut("ProxyGeometries") {
        *proxy = Value::Null;
    }
    if let Some(nodes) = p.get_mut("Nodes").and_then(Value::as_array_mut) {
        for node in nodes {
            if let Some(fields) = node.as_object_mut() {
                fields.retain(|key, _| key == "Expression");
            }
        }
    }
}

pub(super) fn write_view(
    doc: &CadDocument,
    file: &str,
    reader: &str,
    read_view: bool,
    writer: &mut impl Write,
) -> serde_json::Result<()> {
    let sources = Sources::new(doc, reader);
    let mut records: Vec<_> = (0..sources.len())
        .map(|index| sources.metadata(doc, reader, index))
        .collect();
    let resolution = read_view.then(|| common::Resolution::new(doc, reader, &records));
    if let Some(resolution) = &resolution {
        for value in &mut records {
            resolution.apply(value);
        }
    }
    let mut projected_header = header::header(doc);
    let mut legacy_rewrite = None;
    let mut templates = None;
    if read_view && legacy_identity::applies(doc, reader) {
        registration_order::apply(doc, reader, &mut records);
        legacy_rewrite = Some(legacy_identity::apply(
            doc,
            &mut projected_header,
            &mut records,
        ));
    } else if read_view {
        templates = Some(registration::apply(doc, reader, &mut records));
        registration_order::apply(doc, reader, &mut records);
        projected_header["HandleSeed"] = json!(registration_order::handle_seed(doc, &records));
    }
    let defaults = (read_view && registration::source_document(doc))
        .then(|| common::TableDefaults::new(&records));

    // Preserve the first representation of each nonzero final handle.
    let mut sorted = BTreeMap::new();
    for value in records {
        let h = u64::from_str_radix(
            value["handle"].as_str().unwrap().trim_start_matches('#'),
            16,
        )
        .expect("canonical handle");
        if h != 0 {
            sorted.entry(h).or_insert(value);
        }
    }

    writer
        .write_all(b"{\"format\":1,\"file\":")
        .map_err(serde_json::Error::io)?;
    serde_json::to_writer(&mut *writer, file)?;
    writer
        .write_all(b",\"reader\":")
        .map_err(serde_json::Error::io)?;
    serde_json::to_writer(&mut *writer, reader)?;
    writer
        .write_all(b",\"status\":\"ok\",\"error\":null,\"notifications\":")
        .map_err(serde_json::Error::io)?;
    serde_json::to_writer(&mut *writer, &notifications(Some(doc)))?;
    writer
        .write_all(b",\"version\":")
        .map_err(serde_json::Error::io)?;
    serde_json::to_writer(&mut *writer, &version(doc.version))?;
    writer
        .write_all(b",\"header\":")
        .map_err(serde_json::Error::io)?;
    serde_json::to_writer(&mut *writer, &projected_header)?;
    writer
        .write_all(b",\"classes\":")
        .map_err(serde_json::Error::io)?;
    serde_json::to_writer(&mut *writer, &header::classes(doc))?;
    writer
        .write_all(b",\"summaryInfo\":")
        .map_err(serde_json::Error::io)?;
    serde_json::to_writer(&mut *writer, &header::summary(doc))?;
    writer
        .write_all(b",\"objects\":[")
        .map_err(serde_json::Error::io)?;
    for (index, mut projected) in sorted.into_values().enumerate() {
        if index != 0 {
            writer.write_all(b",").map_err(serde_json::Error::io)?;
        }
        let mut value = if let Some(source) = projected.get(SOURCE).and_then(Value::as_u64) {
            let mut value = sources.render(doc, reader, source as usize);
            if let Some(resolution) = &resolution {
                resolution.apply(&mut value);
            }
            // Template lookup includes supported orphans, not just registered
            // objects. Resolve XRecord payloads only while their record is live.
            if let Some(templates) = templates
                .as_ref()
                .filter(|_| registration::source_document(doc))
            {
                if value["type"] == "XRecord" {
                    let h = u64::from_str_radix(
                        value["handle"].as_str().unwrap().trim_start_matches('#'),
                        16,
                    )
                    .unwrap();
                    if let Some(crate::objects::ObjectType::XRecord(native)) =
                        doc.objects.get(&Handle::new(h))
                    {
                        if let Some(entries) = value["props"]["Entries"].as_array_mut() {
                            for (entry, projected) in native.entries.iter().zip(entries) {
                                if let crate::objects::XRecordValue::Handle(target) = entry.value {
                                    projected["Value"] = if templates.contains(&target.value()) {
                                        handle(target)
                                    } else {
                                        json!(target.value())
                                    };
                                }
                            }
                        }
                    }
                }
            }
            if let Some(rewrite) = &legacy_rewrite {
                rewrite.apply(&mut value);
            }
            value["handle"] = projected["handle"].take();
            value["owner"] = projected["owner"].take();
            for (key, replacement) in projected["props"].as_object_mut().unwrap() {
                // These two fields are planning markers, not replacements.
                if key == "Nodes" || (key == "ProxyGeometries" && replacement.is_null()) {
                    continue;
                }
                value["props"][key] = replacement.take();
            }
            value
        } else {
            // Synthesized defaults carry their full (small) schema already.
            projected
        };
        if let Some(defaults) = &defaults {
            defaults.apply(&mut value);
        }
        serde_json::to_writer(&mut *writer, &value)?;
    }
    writer.write_all(b"]}").map_err(serde_json::Error::io)
}
