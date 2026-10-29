//! The `ACAD_XREC_ROUNDTRIP` records of AutoCAD's own down-saves.
//!
//! A drawing that AutoCAD saves to an older version keeps what the older object records cannot hold in an XRECORD named
//! `ACAD_XREC_ROUNDTRIP` in the extension dictionary of the object (sample_AC1014.dwg to sample_AC1032.dwg are saves of one
//! drawing, and the DXF twins of R2000 to R2018 are its DXF files). The record is a list of groups: a group-102 name and the
//! values after it up to the next name. Each kind below is one group family with the objects that hold it and the versions
//! that need it:
//!
//! | kind | owner | where the record is | needed before |
//! |---|---|---|---|
//! | view style names | SECTIONVIEWSTYLE, DETAILVIEWSTYLE | `ACAD_XREC_ROUNDTRIP {DISPLAYNAME, FLAGS}` | R2018 |
//! | material | MATERIAL | `{MATERIAL}` | R2007 |
//! | shade plot | LAYOUT | `{SHADEPLOT, SHADEPLOTRESLEVEL, SHADEPLOTCUSTOMDPI}` | R2004 |
//! | view display | view (table record) | `{CAMERAPLOTTABLE, VISUALSTYLE}` | R2007 |
//! | VPORT display | the VPORT table's control object | `{VTR_<extents>_VISUALSTYLE, _GRIDDISPLAY, _GRIDMAJOR, _DEFAULTLIGHTING, _DEFAULTLIGHTINGTYPE, _BRIGHTNESS, _CONTRAST}` | R2007 |
//! | viewport display | VIEWPORT entity | `{VISUALSTYLE, GRIDDISPLAY, GRIDMAJOR, DEFAULTLIGHTING, DEFAULTLIGHTINGTYPE, BRIGHTNESS, CONTRAST}`, and `{SHADEPLOT}` | R2007, R2004 |
//! | solid history | 3DSOLID | `{ACAD_SOLID_HIST, ACAD_SOLID_HIST1, ACAD_SOLID_HIST2}` | R2007 |
//! | dimension linetypes | the root dictionary | dictionary `ACAD_2007HEADERVARS_RT`: XRECORDs `DIMLTYPE`, `DIMLTEX1`, `DIMLTEX2` | R2007 |
//! | uniform scale | block record | dictionary `AcDbBlockTableRecordData` (R14: upper case) of the extension dictionary: XRECORD `Edit` | R2007 |
//! | multi-line attribute | ATTDEF, ATTRIB | `ACAD_MLATT` (the embedded MTEXT) and `{ACAD_MATT_HEIGHT}` | R2018 |
//! | layer colour | LAYER | `{COLORACI, COLORTRUE, COLORNAME}` (the true colour, the colour-book identity) | R2004 |
//! | layer plot flag, line weight | LAYER | `{PLOTBIT}` (a layer that does not plot), `{LINEWEIGHT}` (a layer with a line weight) | R2000 |
//!
//! (The spline's `ACAD_SPLINE_DATA` is extended data: `autocad_conversions`. The R14 key of a table style's cell style map and the
//! TABLECONTENT / TABLEGEOMETRY objects of a table: `objects::table_style`, `entities::table`.)
//!
//! The native model holds the values themselves. A reader restores them from the record and takes what it applied out of the model
//! (`take_groups`, `take_chain`; the document keeps the changes, and the ACadSharp view, which shows what ACadSharp reads, and the
//! writers put the objects back: `reinstate`, `masked`); the writer
//! states the groups from the native values for the versions that need them, replacing an earlier copy and keeping any other
//! group; for a version that holds the values itself the writer drops the groups it applied, and a record that holds anything
//! else stays. A group that does not have the shape AutoCAD writes is another producer's data: it is neither read nor changed.
//! A drawing converted from another version (or made in memory) gets every group; one written again at the version it was read
//! from keeps the groups its record has (and, for a DWG, makes a group only for a value that differs from the default the
//! reader gives a file without the record).
//!
//! A DXF holds a record only where AutoCAD's own DXF of the version does (owner decision 2026-10-24): R2000 to R2013 view styles,
//! materials, dimension linetypes and multi-line attributes, R2000 and R2004 the uniform scale of blocks, and the layout's and the
//! viewport's shade plot at R2000. For the
//! other kinds a DXF refreshes the groups a record has (a drawing read from a DWG keeps its objects) and makes and drops none.
use crate::document::CadDocument;
use crate::objects::{ObjectType, XRecordEntry, XRecordValue};
use crate::types::{DxfVersion, Handle};

pub(crate) const KEY: &str = "ACAD_XREC_ROUNDTRIP";

/// One group of a record: its group-102 name and the entries after it.
#[derive(Debug, Clone, PartialEq)]
struct Group {
    name: String,
    entries: Vec<XRecordEntry>,
}

impl Group {
    fn new(name: &str, entries: Vec<XRecordEntry>) -> Self {
        Self { name: name.to_string(), entries }
    }
}

/// The groups of a record. Entries before the first name form a group with an empty name.
fn split(entries: &[XRecordEntry]) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for entry in entries {
        match (&entry.value, entry.code) {
            (XRecordValue::String(name), 102) => groups.push(Group { name: name.clone(), entries: Vec::new() }),
            _ => match groups.last_mut() {
                Some(group) => group.entries.push(entry.clone()),
                None => groups.push(Group { name: String::new(), entries: vec![entry.clone()] }),
            },
        }
    }
    groups
}

fn join(groups: &[Group]) -> Vec<XRecordEntry> {
    let mut entries = Vec::new();
    for group in groups {
        if !group.name.is_empty() {
            entries.push(XRecordEntry::string(102, group.name.clone()));
        }
        entries.extend(group.entries.iter().cloned());
    }
    entries
}

/// The entry of a group at `index` when it has `code`.
fn at(group: &Group, index: usize, code: i32) -> Option<&XRecordValue> {
    group.entries.get(index).filter(|entry| entry.code == code).map(|entry| &entry.value)
}

fn as_i32(value: &XRecordValue) -> Option<i32> {
    match value {
        XRecordValue::Int16(v) => Some(*v as i32),
        XRecordValue::Int32(v) => Some(*v),
        XRecordValue::Byte(v) => Some(*v as i32),
        XRecordValue::Bool(v) => Some(*v as i32),
        _ => None,
    }
}

fn as_f64(value: &XRecordValue) -> Option<f64> {
    match value {
        XRecordValue::Double(v) => Some(*v),
        _ => None,
    }
}

/// The round-trip record of every owner that holds one: `(owner, record)` in handle order.
fn records(document: &CadDocument) -> Vec<(Handle, Handle)> {
    let mut found: Vec<(Handle, Handle)> = document
        .objects
        .values()
        .filter_map(|object| {
            let ObjectType::Dictionary(dictionary) = object else {
                return None;
            };
            let record = dictionary.get(KEY)?;
            matches!(document.objects.get(&record), Some(ObjectType::XRecord(_))).then_some((dictionary.owner, record))
        })
        .collect();
    found.sort_by_key(|(owner, record)| (owner.value(), record.value()));
    found
}

fn groups_of(document: &CadDocument, record: Handle) -> Vec<Group> {
    match document.objects.get(&record) {
        Some(ObjectType::XRecord(record)) => split(&record.entries),
        _ => Vec::new(),
    }
}

/// The extension dictionaries of a document, looked up without the scan `CadDocument::extension_dictionary_handle` makes for an
/// object that has none (a writer that asks for every solid of a drawing would scan the objects once per solid).
struct Ext {
    /// The dictionaries owned by an object that is not a dictionary: the first (lowest handle) of each owner.
    by_owner: std::collections::HashMap<Handle, Handle>,
}

impl Ext {
    fn new(document: &CadDocument) -> Self {
        let mut by_owner: std::collections::HashMap<Handle, Handle> = std::collections::HashMap::new();
        for (handle, object) in &document.objects {
            let ObjectType::Dictionary(dictionary) = object else { continue };
            if dictionary.owner.is_null() || matches!(document.objects.get(&dictionary.owner), Some(ObjectType::Dictionary(_))) {
                continue;
            }
            let slot = by_owner.entry(dictionary.owner).or_insert(*handle);
            if handle.value() < slot.value() {
                *slot = *handle;
            }
        }
        Self { by_owner }
    }

    /// The extension dictionary of `owner`, as `CadDocument::extension_dictionary_handle` finds it.
    fn of(&self, document: &CadDocument, owner: Handle) -> Option<Handle> {
        if owner.is_null() {
            return None;
        }
        let stated = |handle: Option<Handle>| handle.filter(|handle| !handle.is_null());
        if let Some(handle) = document.get_entity(owner).and_then(|entity| stated(entity.common().xdictionary_handle)) {
            return Some(handle);
        }
        if let Some(handle) = stated(document.xdic_by_handle.get(&owner).copied()) {
            return Some(handle);
        }
        let typed = match document.objects.get(&owner) {
            Some(ObjectType::Dictionary(_)) => return stated(match document.objects.get(&owner) {
                Some(ObjectType::Dictionary(value)) => value.xdictionary_handle,
                _ => None,
            }),
            Some(ObjectType::ClassObject(value)) => value.xdictionary_handle,
            Some(ObjectType::Layout(value)) => value.xdictionary_handle,
            Some(ObjectType::XRecord(value)) => value.xdictionary_handle,
            Some(ObjectType::PlotSettings(value)) => value.xdictionary_handle,
            Some(ObjectType::VisualStyle(value)) => value.xdictionary_handle,
            Some(ObjectType::Material(value)) => value.xdictionary_handle,
            Some(ObjectType::ProxyObject(value)) => value.xdictionary_handle,
            _ => None,
        };
        stated(typed).or_else(|| self.by_owner.get(&owner).copied())
    }

    /// The extension dictionary of `owner`, made (owned by it, hard owner) when it has none.
    fn ensure(&mut self, document: &mut CadDocument, owner: Handle) -> Handle {
        if let Some(handle) = self.of(document, owner) {
            return handle;
        }
        let handle = document.allocate_handle();
        let mut dictionary = crate::objects::Dictionary::new();
        dictionary.handle = handle;
        dictionary.owner = owner;
        dictionary.hard_owner = true;
        document.objects.insert(handle, ObjectType::Dictionary(dictionary));
        if let Some(entity) = document.get_entity_mut(owner) {
            entity.common_mut().xdictionary_handle = Some(handle);
        }
        document.xdic_by_handle.insert(owner, handle);
        self.by_owner.insert(owner, handle);
        handle
    }

    /// The XRECORD `key` of the extension dictionary of `owner`, made when it is missing (with the cloning flags AutoCAD gives a
    /// record of `version`, listed by name).
    fn ensure_record(&mut self, document: &mut CadDocument, owner: Handle, key: &str, version: DxfVersion) -> Handle {
        let dictionary = self.ensure(document, owner);
        if let Some(record) = dictionary_at(document, dictionary).and_then(|dictionary| dictionary.get(key)) {
            if matches!(document.objects.get(&record), Some(ObjectType::XRecord(_))) {
                return record;
            }
        }
        let handle = document.allocate_handle();
        let mut record = crate::objects::XRecord::named(key);
        record.handle = handle;
        record.owner = dictionary;
        record.reactors = vec![dictionary];
        record.cloning_flags = chain_flags(version).1;
        document.objects.insert(handle, ObjectType::XRecord(record));
        if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&dictionary) {
            if let Some((_, target)) = entries.entries.iter_mut().find(|(name, _)| name.eq_ignore_ascii_case(key)) {
                *target = handle;
            } else {
                add_sorted(entries, key, handle, false);
            }
        }
        handle
    }

}

/// A record the writer made in an extension dictionary, stated as AutoCAD's own files state one: the dictionary is its reactor
/// (every XRECORD of a dictionary in sample_AC1015.dwg to sample_AC1032.dwg lists its dictionary) and it keeps the cloning flags of
/// the version (`chain_flags`).
pub(crate) fn state_new_record(document: &mut CadDocument, record: Handle, version: DxfVersion) {
    if let Some(ObjectType::XRecord(xrecord)) = document.objects.get_mut(&record) {
        if xrecord.reactors.is_empty() && !xrecord.owner.is_null() {
            xrecord.reactors = vec![xrecord.owner];
        }
        xrecord.cloning_flags = chain_flags(version).1;
    }
}

/// The handle of the round-trip record of `owner`.
fn record_of(document: &CadDocument, ext: &Ext, owner: Handle) -> Option<Handle> {
    let dictionary = ext.of(document, owner)?;
    let ObjectType::Dictionary(dictionary) = document.objects.get(&dictionary)? else {
        return None;
    };
    let record = dictionary.get(KEY)?;
    matches!(document.objects.get(&record), Some(ObjectType::XRecord(_))).then_some(record)
}

/// State `wanted` as the groups of `owner`'s record that `owns` names: a group the record has is replaced (or removed when
/// `wanted` lacks it), a group it lacks is appended. The record is created when `create` is set and there are groups to
/// state; a record left empty by the removal goes with its dictionary when nothing else is in it.
fn state(document: &mut CadDocument, ext: &mut Ext, owner: Handle, owns: &dyn Fn(&str) -> bool, wanted: Vec<Group>, create: bool, version: DxfVersion) {
    let existing = record_of(document, ext, owner);
    let Some(record) = existing.or_else(|| (create && !wanted.is_empty()).then(|| ext.ensure_record(document, owner, KEY, version))) else {
        return;
    };
    let current = groups_of(document, record);
    let mut groups: Vec<Group> = Vec::with_capacity(current.len() + wanted.len());
    let mut pending = wanted;
    for group in current {
        if !owns(&group.name) {
            groups.push(group);
        } else if let Some(position) = pending.iter().position(|wanted| wanted.name == group.name) {
            groups.push(pending.remove(position));
        }
    }
    groups.extend(pending);
    let entries = join(&groups);
    if let Some(ObjectType::XRecord(xrecord)) = document.objects.get_mut(&record) {
        if xrecord.entries != entries {
            xrecord.entries = entries;
            xrecord.entries_complete = true;
            xrecord.raw_dwg_data = None;
            xrecord.raw_dwg_version = None;
            xrecord.source_values.clear();
            xrecord.synchronize_object_references();
        }
    }
    if groups.is_empty() {
        remove_empty(document, ext, owner, record);
    }
}

/// Remove an empty record from the extension dictionary that held it. The dictionary stays, also when empty, as in AutoCAD's own
/// files of the versions that hold the values (`take_chain`).
fn remove_empty(document: &mut CadDocument, ext: &mut Ext, owner: Handle, record: Handle) {
    let Some(dictionary) = ext.of(document, owner) else {
        return;
    };
    if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&dictionary) {
        entries.entries.retain(|(_, handle)| *handle != record);
        entries.hard_owner_entries.retain(|name| entries.entries.iter().any(|(entry, _)| entry == name));
    }
    document.objects.remove(&record);
}

/// Whether writing the document as `version` is a conversion: the file it was read from, if any, is of another version.
fn converts(document: &CadDocument, version: DxfVersion) -> bool {
    !document.is_source_version(version)
}

/// Whether the document was made in memory: it was read from no file.
fn in_memory(document: &CadDocument) -> bool {
    document.dwg_source_version.is_none() && document.dxf_source_version.is_none()
}

// ---------------------------------------------------------------------------------------------------------------------
// What a reader takes out of the model. A record is the file's way of holding a value of an object its version has no field
// for: once the reader has given the object the value, the record (and each dictionary of a dictionary it leaves empty; the
// owner's extension dictionary stays) leaves the model, as `autocad_conversions` takes the extended-data records it applied. The document
// keeps each change (`Consumed`) so that the ACadSharp view (which shows the file as ACadSharp reads it) and a writer (which
// states the records again from the native values, keeping the objects the file had) can put them back (`reinstate`). Groups
// the reader did not apply stay where they are.
// ---------------------------------------------------------------------------------------------------------------------

/// One change a reader made when it took a record out of the model.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) enum Consumption {
    /// The entry `name` -> `target` at `position` of `dictionary` was removed.
    Entry { dictionary: Handle, position: usize, name: String, target: Handle, hard: bool },
    /// The object (as the file had it) was removed, or replaced by the groups the reader did not apply.
    Object(Handle, ObjectType),
    /// The reactors the document map held for a removed object.
    Reactors(Handle, Vec<Handle>),
    /// The extension dictionary `dictionary` of `owner` was removed with its last record: the link was in the document map
    /// (`in_map`) and in the owner's own field (`in_field`).
    XDictionary { owner: Handle, dictionary: Handle, in_map: bool, in_field: bool },
}

/// The changes a reader made, in order.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct Consumed {
    changes: Vec<Consumption>,
    /// The changes of the visual-style records (`consume_visual_styles`): a writer of R2013 or later does not put them back (those
    /// versions state the properties in the style and AutoCAD's files of them have no extension dictionary on a style).
    style_changes: Vec<Consumption>,
    /// The changes of the layer records (colour, plot flag, line weight): a DXF writer does not put them back (a DXF layer states all
    /// three itself, and AutoCAD's DXF of R2000 has no such record).
    layer_changes: Vec<Consumption>,
    /// The changes of the table styles' groups of the legacy rows' data types (`consume_table_style_group`): a DWG writer of R2010
    /// or later does not put them back, as it drops such a record of a style the model holds (`dwg_writer::prepare_roundtrip_records`:
    /// the R2010 TABLESTYLE record states the data types; AutoCAD's sample_AC1024.dwg to sample_AC1032.dwg keep the style #87's
    /// extension dictionary empty).
    table_style_changes: Vec<Consumption>,
    /// The MTEXT whose original text the reader restored from `ACAD_MTEXT_RT` (`restore_mtext`) with the original and the text the file
    /// had: the file spelled the paragraph breaks `\P` (or stacked a fraction another way) and the model holds the original. The view
    /// shows the file's text for such an MTEXT while its text is unedited (`mtext_file_value`); the writers state the file's text and
    /// the record again (`state_mtext`). By owner: the view and the writer look an MTEXT up once each.
    mtext: std::collections::BTreeMap<Handle, (String, String)>,
    /// The table records whose original name the reader restored from `ACAD_XREC_ROUNDTRIP {EXTNAMES}` (R14), with the name the file
    /// states: the view shows the file's name (`unrestore_names`), the writers state the group again (`state_extnames`).
    names: Vec<(Handle, String)>,
    /// The multileaders whose direction, attachments and dogleg length the reader restored from `ROUNDTRIP_MLCONTEXT` and
    /// `ACAD_ROUNDTRIP_2010_MLEADER`, by owner: the words the file's legacy record states (what ACadSharp reads) and the words
    /// restored. The view and the writers before R2010 state the legacy words while the restored ones are unedited
    /// (`unrestored_mleader`); the writers state the record again from the native words.
    mleaders: std::collections::BTreeMap<Handle, MLeaderRecord>,
}

impl Consumed {
    pub(crate) fn is_empty(&self) -> bool {
        self.changes.is_empty()
            && self.style_changes.is_empty()
            && self.layer_changes.is_empty()
            && self.table_style_changes.is_empty()
            && self.mtext.is_empty()
            && self.names.is_empty()
            && self.mleaders.is_empty()
    }

    /// Record that the name of the table record `owner` was restored from the file's `name` (the DWG builder).
    pub(crate) fn note_restored_name(&mut self, owner: Handle, name: String) {
        self.names.push((owner, name));
    }

    /// The handles of the objects the reader removed from `document` (the records it applied, and the dictionaries they left empty),
    /// sorted: the native dump states them, so that a check of the handle seed can see the handles the model no longer holds.
    #[cfg(feature = "acadsharp-compat")]
    pub(crate) fn removed_handles(&self, document: &CadDocument) -> Vec<Handle> {
        let mut removed: Vec<Handle> = self
            .changes
            .iter()
            .chain(self.style_changes.iter())
            .chain(self.layer_changes.iter())
            .chain(self.table_style_changes.iter())
            .filter_map(|change| match change {
                Consumption::Object(handle, _) if !document.objects.contains_key(handle) => Some(*handle),
                _ => None,
            })
            .collect();
        removed.sort();
        removed.dedup();
        removed
    }

    /// The objects the reader removed or reduced (the records it applied).
    fn taken(&self) -> std::collections::HashSet<Handle> {
        self.changes
            .iter()
            .chain(self.style_changes.iter())
            .chain(self.layer_changes.iter())
            .chain(self.table_style_changes.iter())
            .filter_map(|change| match change {
                Consumption::Object(handle, _) => Some(*handle),
                _ => None,
            })
            .collect()
    }
}

fn object_owner(object: &ObjectType) -> Handle {
    match object {
        ObjectType::Dictionary(value) => value.owner,
        ObjectType::XRecord(value) => value.owner,
        _ => Handle::NULL,
    }
}

fn take_object(document: &mut CadDocument, handle: Handle) {
    if let Some(object) = document.objects.remove(&handle) {
        document.consumed_records.changes.push(Consumption::Object(handle, object));
        if let Some(reactors) = document.reactors_by_handle.remove(&handle) {
            document.consumed_records.changes.push(Consumption::Reactors(handle, reactors));
        }
    }
}

/// Take the record `record` out of the model, with each dictionary of a dictionary it leaves empty (`ACAD_2007HEADERVARS_RT`,
/// `AcDbBlockTableRecordData`). The extension dictionary of the owner stays, also when empty: AutoCAD's files keep an extension
/// dictionary that held only round-trip records in the older ones (the Model layout #22's #194, the blocks #A91 and #A96's #A92
/// and #A97: present and empty in sample_AC1021.dwg to sample_AC1032.dwg, holding only the records in sample_AC1014.dwg and
/// sample_AC1015.dwg), and the root dictionary stays.
fn take_chain(document: &mut CadDocument, record: Handle) {
    let root = document.header.named_objects_dict_handle;
    let mut child = record;
    loop {
        let Some(parent) = document.objects.get(&child).map(object_owner) else { return };
        if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&parent) {
            if let Some(position) = dictionary.entries.iter().position(|(_, target)| *target == child) {
                let (name, target) = dictionary.entries.remove(position);
                let hard = dictionary.is_entry_hard_owner(&name);
                dictionary.hard_owner_entries.retain(|entry| !entry.eq_ignore_ascii_case(&name));
                document.consumed_records.changes.push(Consumption::Entry { dictionary: parent, position, name, target, hard });
            }
        }
        take_object(document, child);
        let Some(ObjectType::Dictionary(dictionary)) = document.objects.get(&parent) else { return };
        if !dictionary.entries.is_empty() || parent == root {
            return;
        }
        if !matches!(document.objects.get(&dictionary.owner), Some(ObjectType::Dictionary(_))) {
            return;
        }
        // A dictionary of a dictionary: it goes from its parent too.
        child = parent;
    }
}

/// Take the groups of `record` that `applied` names out of the model: the whole record when nothing else is in it.
fn take_groups(document: &mut CadDocument, record: Handle, applied: &[String]) {
    if applied.is_empty() {
        return;
    }
    let groups = groups_of(document, record);
    let kept: Vec<Group> = groups.iter().filter(|group| !applied.contains(&group.name)).cloned().collect();
    if kept.len() == groups.len() {
        return;
    }
    if kept.is_empty() {
        take_chain(document, record);
        return;
    }
    let Some(ObjectType::XRecord(xrecord)) = document.objects.get_mut(&record) else { return };
    let original = ObjectType::XRecord(xrecord.clone());
    xrecord.entries = join(&kept);
    xrecord.raw_dwg_data = None;
    xrecord.raw_dwg_version = None;
    xrecord.source_values.clear();
    xrecord.synchronize_object_references();
    document.consumed_records.changes.push(Consumption::Object(record, original));
}

/// Take out of the model the round-trip record of every visual style the reader restored from it, with the extension dictionary the
/// record leaves empty. AutoCAD's files from R14 to R2010 keep the properties after the 24th (R2010: the 28th) of a style, and the
/// flags of its property ids, in `ACAD_XREC_ROUNDTRIP` of the style's extension dictionary (24 styles in each of sample_AC1014.dwg,
/// sample_AC1015.dwg, sample_AC1018.dwg and sample_AC1024.dwg), and the files of R2013 and R2018 have neither record nor extension
/// dictionary on a style. The reader (`restore_visual_style_roundtrip`) has given the style its 58 properties; the record goes only
/// when it is exactly the one the writer states for them, so that nothing is lost (a record of another producer, or with a flag the
/// writer would not state, stays in the model).
pub(crate) fn consume_visual_styles(document: &mut CadDocument) {
    let r2010 = document.version == DxfVersion::AC1024;
    let flags = std::collections::HashMap::new();
    let mut found: Vec<(Handle, Handle, Handle)> = Vec::new();
    for (owner, object) in &document.objects {
        let ObjectType::VisualStyle(style) = object else { continue };
        if style.properties.len() < 58 {
            continue;
        }
        let Some(dictionary) = style.xdictionary_handle.or_else(|| document.xdic_by_handle.get(owner).copied()) else { continue };
        let Some(ObjectType::Dictionary(entries)) = document.objects.get(&dictionary) else { continue };
        let [(key, record)] = entries.entries.as_slice() else { continue };
        if !key.eq_ignore_ascii_case(KEY) {
            continue;
        }
        let Some(ObjectType::XRecord(xrecord)) = document.objects.get(record) else { continue };
        if xrecord.entries == crate::objects::visual_style_record_group(&style.properties, r2010, &flags) {
            found.push((*owner, dictionary, *record));
        }
    }
    found.sort();
    for (owner, dictionary, record) in found {
        let before = document.consumed_records.changes.len();
        take_chain(document, record);
        let in_map = document.xdic_by_handle.get(&owner) == Some(&dictionary);
        if in_map {
            document.xdic_by_handle.remove(&owner);
        }
        let in_field = matches!(document.objects.get_mut(&owner), Some(ObjectType::VisualStyle(style)) if style.xdictionary_handle == Some(dictionary));
        if let (true, Some(ObjectType::VisualStyle(style))) = (in_field, document.objects.get_mut(&owner)) {
            style.xdictionary_handle = None;
        }
        document.consumed_records.changes.push(Consumption::XDictionary { owner, dictionary, in_map, in_field });
        take_object(document, dictionary);
        let moved = document.consumed_records.changes.split_off(before);
        document.consumed_records.style_changes.extend(moved);
    }
}

/// Take the group `name` of the round-trip record `record` of a table style out of the model (the whole record when it holds nothing
/// else; the style's extension dictionary stays): `objects::table_style` calls it for the legacy rows' data types it restored, when
/// the group is exactly the one its writer states. The view and the writers put it back (`reinstate`; a DWG of R2010 or later
/// leaves it out, as it leaves out such a record the model holds).
pub(crate) fn consume_table_style_group(document: &mut CadDocument, record: Handle, name: &str) {
    let before = document.consumed_records.changes.len();
    take_groups(document, record, &[name.to_string()]);
    let moved = document.consumed_records.changes.split_off(before);
    document.consumed_records.table_style_changes.extend(moved);
}

/// Put back what the reader took out of the model: the objects as the file had them and their dictionary entries. An object
/// whose dictionary is gone (its owner was deleted with it) stays out; an entry whose name the dictionary has by now stays out.
pub(crate) fn reinstate(document: &mut CadDocument) {
    let mut changes = std::mem::take(&mut document.consumed_records.layer_changes);
    changes.extend(std::mem::take(&mut document.consumed_records.style_changes));
    changes.extend(std::mem::take(&mut document.consumed_records.changes));
    // made after the others (the table styles are restored after `restore_document`), so put back first
    changes.extend(std::mem::take(&mut document.consumed_records.table_style_changes));
    let mut skipped: std::collections::HashSet<Handle> = std::collections::HashSet::new();
    for change in changes.into_iter().rev() {
        match change {
            Consumption::Object(handle, object) => {
                let owner = object_owner(&object);
                let kept = document.objects.contains_key(&handle) || (!skipped.contains(&owner) && document.objects.contains_key(&owner));
                if kept {
                    document.objects.insert(handle, object);
                } else {
                    skipped.insert(handle);
                }
            }
            Consumption::Reactors(handle, reactors) => {
                if !skipped.contains(&handle) {
                    document.reactors_by_handle.entry(handle).or_insert(reactors);
                }
            }
            Consumption::XDictionary { owner, dictionary, in_map, in_field } => {
                if skipped.contains(&dictionary) {
                    continue;
                }
                if in_map {
                    document.xdic_by_handle.entry(owner).or_insert(dictionary);
                }
                if let (true, Some(ObjectType::VisualStyle(style))) = (in_field, document.objects.get_mut(&owner)) {
                    style.xdictionary_handle.get_or_insert(dictionary);
                }
            }
            Consumption::Entry { dictionary, position, name, target, hard } => {
                if skipped.contains(&target) {
                    continue;
                }
                if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&dictionary) {
                    if entries.get(&name).is_none() {
                        let at = position.min(entries.entries.len());
                        entries.entries.insert(at, (name.clone(), target));
                        if hard {
                            entries.set_entry_hard_owner(&name, true);
                            order_hard_owner_entries(entries);
                        }
                    }
                }
            }
        }
    }
}

/// The document with what its reader took out of the model put back (the document itself when the reader took nothing).
pub(crate) fn reinstated(document: &CadDocument) -> std::borrow::Cow<'_, CadDocument> {
    if document.consumed_records.is_empty() {
        return std::borrow::Cow::Borrowed(document);
    }
    let mut owned = document.clone();
    reinstate(&mut owned);
    unrestore_names(&mut owned);
    std::borrow::Cow::Owned(owned)
}

/// The hard-owner names of a dictionary in the order of its entries.
fn order_hard_owner_entries(dictionary: &mut crate::objects::Dictionary) {
    let position = |name: &String| dictionary.entries.iter().position(|(entry, _)| entry.eq_ignore_ascii_case(name)).unwrap_or(usize::MAX);
    let mut names = std::mem::take(&mut dictionary.hard_owner_entries);
    names.sort_by_key(|name| position(name));
    dictionary.hard_owner_entries = names;
}

/// Add the entry `key` -> `handle` where AutoCAD lists it: its dictionaries are sorted by name, compared in lower case
/// (sample_AC1015.dwg: `ACAD_XREC_ROUNDTRIP` before `ADVMATERIAL`, `ADSK_XREC_VTR_ANNOSCALE_DATA` before
/// `ADSK_XREC_VTRANIMATIONINFO`, `ACAD_2007HEADERVARS_RT` first in the root dictionary, `DIMLTEX1`, `DIMLTEX2`, `DIMLTYPE`).
fn add_sorted(dictionary: &mut crate::objects::Dictionary, key: &str, handle: Handle, hard: bool) {
    let lower = key.to_lowercase();
    let at = dictionary.entries.iter().position(|(name, _)| name.to_lowercase() > lower).unwrap_or(dictionary.entries.len());
    dictionary.entries.insert(at, (key.to_string(), handle));
    if hard {
        dictionary.set_entry_hard_owner(key, true);
        order_hard_owner_entries(dictionary);
    }
}

/// Whether the ACadSharp view of a document read from a file of its own version shows the defaults of the fields a record of
/// `version` restores (ACadSharp reads the record as an object and leaves the fields).
#[cfg(feature = "acadsharp-compat")]
pub(crate) fn masked(document: &CadDocument, holds: fn(DxfVersion) -> bool) -> bool {
    let version = document.version;
    document.is_source_version(version) && !holds(version)
}

// ---------------------------------------------------------------------------------------------------------------------
// View style names (SECTIONVIEWSTYLE, DETAILVIEWSTYLE): R2018 states the display name and the flags of the base class.
// ---------------------------------------------------------------------------------------------------------------------

const DISPLAY_NAME: &str = "DISPLAYNAME";
const FLAGS: &str = "FLAGS";

pub(crate) fn view_style_holds(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1032
}

fn view_style_base(object: &ObjectType) -> Option<&crate::objects::ModelDocViewStyle> {
    use crate::objects::ClassObjectData as D;
    match object {
        ObjectType::ClassObject(object) => match &object.data {
            D::SectionViewStyle(style) => Some(&style.base),
            D::DetailViewStyle(style) => Some(&style.base),
            _ => None,
        },
        _ => None,
    }
}

fn view_style_base_mut(object: &mut ObjectType) -> Option<&mut crate::objects::ModelDocViewStyle> {
    use crate::objects::ClassObjectData as D;
    match object {
        ObjectType::ClassObject(object) => match &mut object.data {
            D::SectionViewStyle(style) => Some(&mut style.base),
            D::DetailViewStyle(style) => Some(&mut style.base),
            _ => None,
        },
        _ => None,
    }
}

fn view_style_groups(base: &crate::objects::ModelDocViewStyle) -> Vec<Group> {
    vec![
        Group::new(DISPLAY_NAME, vec![XRecordEntry::string(1, base.display_name.clone())]),
        Group::new(FLAGS, vec![XRecordEntry::int32(90, base.flags)]),
    ]
}

/// Restore the view style's name and flags; the names of the groups applied.
fn restore_view_style(base: &mut crate::objects::ModelDocViewStyle, groups: &[Group]) -> Vec<String> {
    let mut applied = Vec::new();
    for group in groups {
        match group.name.as_str() {
            DISPLAY_NAME if group.entries.len() == 1 => {
                if let Some(XRecordValue::String(name)) = at(group, 0, 1) {
                    base.display_name = name.clone();
                    applied.push(group.name.clone());
                }
            }
            FLAGS if group.entries.len() == 1 => {
                if let Some(flags) = at(group, 0, 90).and_then(as_i32) {
                    base.flags = flags;
                    applied.push(group.name.clone());
                }
            }
            _ => {}
        }
    }
    applied
}

// ---------------------------------------------------------------------------------------------------------------------
// Material: R2007 states the reflectivity, translucence, self-illumination, illumination model, channel flags and mode.
// ---------------------------------------------------------------------------------------------------------------------

const MATERIAL: &str = "MATERIAL";

pub(crate) fn material_holds(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1021
}

fn material_group(material: &crate::objects::Material) -> Group {
    let source = |map: &crate::objects::MaterialMap| XRecordValue::Int16(map.source as i16);
    Group::new(
        MATERIAL,
        vec![
            XRecordEntry::double(148, material.translucence),
            XRecordEntry::double(149, material.self_illumination),
            XRecordEntry::double(149, material.reflectivity),
            XRecordEntry::int32(93, material.illumination_model),
            XRecordEntry::int32(94, material.channel_flags),
            XRecordEntry::new(282, XRecordValue::Byte(material.mode as u8)),
            XRecordEntry::new(72, source(&material.diffuse_map)),
            XRecordEntry::new(77, source(&material.specular_map)),
            XRecordEntry::new(171, source(&material.reflection_map)),
            XRecordEntry::new(175, source(&material.opacity_map)),
            XRecordEntry::new(179, source(&material.bump_map)),
            XRecordEntry::new(273, source(&material.refraction_map)),
        ],
    )
}

/// The values of a `MATERIAL` group of the shape AutoCAD writes: twelve entries with these group codes.
fn material_values(group: &Group) -> Option<[&XRecordValue; 12]> {
    const CODES: [i32; 12] = [148, 149, 149, 93, 94, 282, 72, 77, 171, 175, 179, 273];
    if group.entries.len() != CODES.len() {
        return None;
    }
    let mut values = [&group.entries[0].value; 12];
    for (index, code) in CODES.iter().enumerate() {
        let entry = &group.entries[index];
        if entry.code != *code {
            return None;
        }
        values[index] = &entry.value;
    }
    Some(values)
}

fn restore_material(material: &mut crate::objects::Material, groups: &[Group]) -> Vec<String> {
    let mut applied = Vec::new();
    for group in groups.iter().filter(|group| group.name == MATERIAL) {
        let Some(values) = material_values(group) else {
            continue;
        };
        let (Some(translucence), Some(self_illumination), Some(reflectivity)) =
            (as_f64(values[0]), as_f64(values[1]), as_f64(values[2]))
        else {
            continue;
        };
        let (Some(model), Some(flags), Some(mode)) = (as_i32(values[3]), as_i32(values[4]), as_i32(values[5])) else {
            continue;
        };
        material.translucence = translucence;
        material.self_illumination = self_illumination;
        material.reflectivity = reflectivity;
        material.illumination_model = model;
        material.channel_flags = flags;
        material.mode = mode;
        applied.push(group.name.clone());
    }
    applied
}

/// What the readers leave in the fields a `MATERIAL` group restores, for the ACadSharp view.
#[cfg(feature = "acadsharp-compat")]
pub(crate) fn unrestored_material(document: &CadDocument, material: &crate::objects::Material) -> Option<crate::objects::Material> {
    if !masked(document, material_holds) {
        return None;
    }
    let defaults = crate::objects::Material::new();
    let mut shown = material.clone();
    shown.translucence = defaults.translucence;
    shown.self_illumination = defaults.self_illumination;
    shown.reflectivity = defaults.reflectivity;
    shown.illumination_model = defaults.illumination_model;
    shown.channel_flags = defaults.channel_flags;
    shown.mode = defaults.mode;
    Some(shown)
}

// ---------------------------------------------------------------------------------------------------------------------
// Layout: R2004 states the shade plot mode, resolution level and custom resolution.
// ---------------------------------------------------------------------------------------------------------------------

/// The shade plot mode, resolution level and custom resolution a layout read from a file that states none has.
const LAYOUT_DEFAULT: (i16, i16, i16) = (0, 0, 300);

const SHADE_PLOT: &str = "SHADEPLOT";
const SHADE_PLOT_RESOLUTION: &str = "SHADEPLOTRESLEVEL";
const SHADE_PLOT_DPI: &str = "SHADEPLOTCUSTOMDPI";

pub(crate) fn layout_holds(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1018
}

fn layout_groups(layout: &crate::objects::Layout) -> Vec<Group> {
    vec![
        Group::new(SHADE_PLOT, vec![XRecordEntry::int16(70, layout.shade_plot_mode)]),
        Group::new(SHADE_PLOT_RESOLUTION, vec![XRecordEntry::int16(70, layout.shade_plot_resolution)]),
        Group::new(SHADE_PLOT_DPI, vec![XRecordEntry::int16(70, layout.shade_plot_dpi)]),
    ]
}

fn restore_layout(layout: &mut crate::objects::Layout, groups: &[Group]) -> Vec<String> {
    let value = |group: &Group| match group.entries.as_slice() {
        [entry] if entry.code == 70 => as_i32(&entry.value).and_then(|v| i16::try_from(v).ok()),
        _ => None,
    };
    let mut applied = Vec::new();
    for group in groups {
        match (group.name.as_str(), value(group)) {
            (SHADE_PLOT, Some(v)) => layout.shade_plot_mode = v,
            (SHADE_PLOT_RESOLUTION, Some(v)) => layout.shade_plot_resolution = v,
            (SHADE_PLOT_DPI, Some(v)) => layout.shade_plot_dpi = v,
            _ => continue,
        }
        applied.push(group.name.clone());
    }
    applied
}

/// What the readers leave in the fields the shade plot groups restore, for the ACadSharp view: `(mode, resolution, dpi)`.
#[cfg(feature = "acadsharp-compat")]
pub(crate) fn unrestored_shade_plot(document: &CadDocument, layout: &crate::objects::Layout) -> (i16, i16, i16) {
    if masked(document, layout_holds) {
        LAYOUT_DEFAULT
    } else {
        (layout.shade_plot_mode, layout.shade_plot_resolution, layout.shade_plot_dpi)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Display settings of a view, a VPORT and a VIEWPORT entity: R2007 states the visual style, the grid display, the default
// lighting, the brightness and the contrast; R2004 states the shade plot mode of an entity (R2000 and older do not).
// ---------------------------------------------------------------------------------------------------------------------

const VISUAL_STYLE: &str = "VISUALSTYLE";
const GRID_DISPLAY: &str = "GRIDDISPLAY";
const GRID_MAJOR: &str = "GRIDMAJOR";
const DEFAULT_LIGHTING: &str = "DEFAULTLIGHTING";
const DEFAULT_LIGHTING_TYPE: &str = "DEFAULTLIGHTINGTYPE";
const BRIGHTNESS: &str = "BRIGHTNESS";
const CONTRAST: &str = "CONTRAST";
const CAMERA_PLOTTABLE: &str = "CAMERAPLOTTABLE";
const DISPLAY: [&str; 7] = [VISUAL_STYLE, GRID_DISPLAY, GRID_MAJOR, DEFAULT_LIGHTING, DEFAULT_LIGHTING_TYPE, BRIGHTNESS, CONTRAST];

pub(crate) fn display_holds(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1021
}

pub(crate) fn shade_plot_holds(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1018
}

/// AutoCAD's DXF of R2000 states the shade plot mode of a viewport and none of the display settings.
fn shade_plot_dxf(version: DxfVersion) -> bool {
    version <= DxfVersion::AC1015
}

fn never(_: DxfVersion) -> bool {
    false
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Display {
    visual_style: Handle,
    grid: i16,
    major: i16,
    lighting: bool,
    lighting_type: i16,
    brightness: f64,
    contrast: f64,
}

impl Display {
    fn of_vport(vport: &crate::tables::VPort) -> Self {
        Self {
            visual_style: vport.visual_style_handle,
            grid: vport.grid_flags.to_bits(),
            major: vport.grid_major,
            lighting: vport.use_default_lights,
            lighting_type: vport.default_lighting_type,
            brightness: vport.brightness,
            contrast: vport.contrast,
        }
    }

    fn of_viewport(viewport: &crate::entities::Viewport) -> Self {
        Self {
            visual_style: viewport.visual_style_handle,
            grid: viewport.grid_flags.to_bits(),
            major: viewport.grid_major,
            lighting: viewport.default_lighting,
            lighting_type: viewport.default_lighting_type,
            brightness: viewport.brightness,
            contrast: viewport.contrast,
        }
    }

    /// The groups AutoCAD writes, named `<prefix><name>`; the visual style only when the object has one.
    fn groups(&self, prefix: &str) -> Vec<Group> {
        let name = |name: &str| format!("{prefix}{name}");
        let mut groups = Vec::new();
        if !self.visual_style.is_null() {
            groups.push(Group { name: name(VISUAL_STYLE), entries: vec![XRecordEntry::handle(340, self.visual_style)] });
        }
        groups.push(Group { name: name(GRID_DISPLAY), entries: vec![XRecordEntry::int16(70, self.grid)] });
        groups.push(Group { name: name(GRID_MAJOR), entries: vec![XRecordEntry::int16(70, self.major)] });
        groups.push(Group { name: name(DEFAULT_LIGHTING), entries: vec![XRecordEntry::new(280, XRecordValue::Byte(self.lighting as u8))] });
        groups.push(Group { name: name(DEFAULT_LIGHTING_TYPE), entries: vec![XRecordEntry::int16(70, self.lighting_type)] });
        groups.push(Group { name: name(BRIGHTNESS), entries: vec![XRecordEntry::double(141, self.brightness)] });
        groups.push(Group { name: name(CONTRAST), entries: vec![XRecordEntry::double(142, self.contrast)] });
        groups
    }

    /// Take the groups named `<prefix><name>` of the shape AutoCAD writes; the names of the groups applied.
    fn restore(&mut self, groups: &[Group], prefix: &str) -> Vec<String> {
        let short = |entry: &XRecordEntry| as_i32(&entry.value).and_then(|v| i16::try_from(v).ok());
        let mut applied = Vec::new();
        for group in groups {
            let Some(name) = group.name.strip_prefix(prefix) else {
                continue;
            };
            let [entry] = group.entries.as_slice() else {
                continue;
            };
            match (name, entry.code) {
                (VISUAL_STYLE, 340) => match entry.value.as_handle() {
                    Some(handle) => self.visual_style = handle,
                    None => continue,
                },
                (GRID_DISPLAY, 70) => match short(entry) {
                    Some(value) => self.grid = value,
                    None => continue,
                },
                (GRID_MAJOR, 70) => match short(entry) {
                    Some(value) => self.major = value,
                    None => continue,
                },
                (DEFAULT_LIGHTING, 280) => match as_i32(&entry.value) {
                    Some(value) => self.lighting = value != 0,
                    None => continue,
                },
                (DEFAULT_LIGHTING_TYPE, 70) => match short(entry) {
                    Some(value) => self.lighting_type = value,
                    None => continue,
                },
                (BRIGHTNESS, 141) => match as_f64(&entry.value) {
                    Some(value) => self.brightness = value,
                    None => continue,
                },
                (CONTRAST, 142) => match as_f64(&entry.value) {
                    Some(value) => self.contrast = value,
                    None => continue,
                },
                _ => continue,
            }
            applied.push(group.name.clone());
        }
        applied
    }
}

fn restore_vport_display(vport: &mut crate::tables::VPort, groups: &[Group], prefix: &str) -> Vec<String> {
    let mut display = Display::of_vport(vport);
    let applied = display.restore(groups, prefix);
    vport.visual_style_handle = display.visual_style;
    vport.grid_flags = crate::entities::GridFlags::from_bits(display.grid);
    vport.grid_major = display.major;
    vport.use_default_lights = display.lighting;
    vport.default_lighting_type = display.lighting_type;
    vport.brightness = display.brightness;
    vport.contrast = display.contrast;
    applied
}

fn restore_viewport(viewport: &mut crate::entities::Viewport, groups: &[Group], version: DxfVersion) -> Vec<String> {
    let mut applied = Vec::new();
    if !display_holds(version) {
        let mut display = Display::of_viewport(viewport);
        applied = display.restore(groups, "");
        viewport.visual_style_handle = display.visual_style;
        viewport.grid_flags = crate::entities::GridFlags::from_bits(display.grid);
        viewport.grid_major = display.major;
        viewport.default_lighting = display.lighting;
        viewport.default_lighting_type = display.lighting_type;
        viewport.brightness = display.brightness;
        viewport.contrast = display.contrast;
    }
    if !ucs_holds(version) {
        applied.extend(restore_viewport_ucs(viewport, groups));
    }
    if !shade_plot_holds(version) {
        for group in groups.iter().filter(|group| group.name == SHADE_PLOT) {
            if let [entry] = group.entries.as_slice() {
                if let (70, Some(mode)) = (entry.code, as_i32(&entry.value).and_then(|v| i16::try_from(v).ok())) {
                    viewport.shade_plot_mode = mode;
                    applied.push(group.name.clone());
                }
            }
        }
    }
    applied
}

/// The VPORT group prefix of AutoCAD: the table's control object holds one set per entry, named by its extents.
fn vport_prefix(vport: &crate::tables::VPort) -> String {
    format!("VTR_{:.3}_{:.3}_{:.3}_{:.3}_", vport.lower_left.x, vport.lower_left.y, vport.upper_right.x, vport.upper_right.y)
}

/// What the readers leave in the display fields of a VPORT, view or VIEWPORT entity, for the ACadSharp view.
#[cfg(feature = "acadsharp-compat")]
pub(crate) fn unrestored_vport(document: &CadDocument, vport: &crate::tables::VPort) -> Option<crate::tables::VPort> {
    if !masked(document, display_holds) && !masked(document, ucs_holds) {
        return None;
    }
    let defaults = crate::tables::VPort::new(vport.name.clone());
    let mut shown = vport.clone();
    unrestored_vport_ucs(document, &mut shown);
    if !masked(document, display_holds) {
        return Some(shown);
    }
    shown.visual_style_handle = defaults.visual_style_handle;
    shown.grid_flags = defaults.grid_flags;
    shown.grid_major = defaults.grid_major;
    shown.use_default_lights = defaults.use_default_lights;
    shown.default_lighting_type = defaults.default_lighting_type;
    shown.brightness = defaults.brightness;
    shown.contrast = defaults.contrast;
    Some(shown)
}

#[cfg(feature = "acadsharp-compat")]
pub(crate) fn unrestored_view(document: &CadDocument, view: &crate::tables::View) -> Option<crate::tables::View> {
    if !masked(document, display_holds) && !masked(document, ucs_holds) {
        return None;
    }
    let defaults = crate::tables::View::new(view.name.clone());
    let mut shown = view.clone();
    unrestored_view_ucs(document, &mut shown);
    if !masked(document, display_holds) {
        return Some(shown);
    }
    shown.visual_style_handle = defaults.visual_style_handle;
    shown.camera_plottable = defaults.camera_plottable;
    Some(shown)
}

#[cfg(feature = "acadsharp-compat")]
pub(crate) fn unrestored_viewport(document: &CadDocument, viewport: &crate::entities::Viewport) -> Option<crate::entities::Viewport> {
    let display = masked(document, display_holds);
    let shade = masked(document, shade_plot_holds);
    let ucs = masked(document, ucs_holds);
    if !display && !shade && !ucs {
        return None;
    }
    let defaults = crate::entities::Viewport::new();
    let mut shown = viewport.clone();
    unrestored_viewport_ucs(document, &mut shown);
    if display {
        shown.visual_style_handle = defaults.visual_style_handle;
        shown.grid_flags = defaults.grid_flags;
        shown.grid_major = defaults.grid_major;
        shown.default_lighting = defaults.default_lighting;
        shown.default_lighting_type = defaults.default_lighting_type;
        shown.brightness = defaults.brightness;
        shown.contrast = defaults.contrast;
    }
    if shade {
        shown.shade_plot_mode = defaults.shade_plot_mode;
    }
    Some(shown)
}

// ---------------------------------------------------------------------------------------------------------------------
// 3DSOLID: R2007 states the handle of the construction history in the object.
// ---------------------------------------------------------------------------------------------------------------------

const SOLID_HISTORY: &str = "ACAD_SOLID_HIST";
const SOLID_HISTORY_1: &str = "ACAD_SOLID_HIST1";
const SOLID_HISTORY_2: &str = "ACAD_SOLID_HIST2";

pub(crate) fn solid_history_holds(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1021
}

fn solid_groups(solid: &crate::entities::Solid3D) -> Vec<Group> {
    let Some(history) = solid.history_handle.filter(|handle| !handle.is_null()) else {
        return Vec::new();
    };
    vec![
        Group::new(SOLID_HISTORY, vec![XRecordEntry::handle(360, history)]),
        Group::new(SOLID_HISTORY_1, vec![XRecordEntry::int32(90, 0)]),
        Group::new(SOLID_HISTORY_2, vec![XRecordEntry::int32(90, solid.common.handle.value() as i32)]),
    ]
}

/// Restore the solid's history handle; the names of the groups applied (the two that follow it, `ACAD_SOLID_HIST1 90 0` and
/// `ACAD_SOLID_HIST2 90 <the solid's handle>`, go with it when they state what AutoCAD writes).
fn restore_solid(solid: &mut crate::entities::Solid3D, groups: &[Group]) -> Vec<String> {
    let mut applied = Vec::new();
    for group in groups.iter().filter(|group| group.name == SOLID_HISTORY) {
        if let [entry] = group.entries.as_slice() {
            if let (360, Some(handle)) = (entry.code, entry.value.as_handle()) {
                solid.history_handle = Some(handle);
                applied.push(group.name.clone());
            }
        }
    }
    if !applied.is_empty() {
        let own = solid.common.handle.value() as i32;
        for group in groups {
            let stated = |value: i32| matches!(group.entries.as_slice(), [entry] if entry.code == 90 && as_i32(&entry.value) == Some(value));
            if (group.name == SOLID_HISTORY_1 && stated(0)) || (group.name == SOLID_HISTORY_2 && stated(own)) {
                applied.push(group.name.clone());
            }
        }
    }
    applied
}

// ---------------------------------------------------------------------------------------------------------------------
// Multileader (R14 to R2007 DWG, R2000 to R2007 DXF): the record has no place for the attachment direction of the text (R2010), nor
// for the attachment types and the dogleg length of a vertical leader; AutoCAD's own files of the drawing keep them in
// `ACAD_XREC_ROUNDTRIP {ROUNDTRIP_MLCONTEXT, ACAD_ROUNDTRIP_2010_MLEADER}` of the extension dictionary of the multileader (the one
// vertical multileader #B27 of sample_AC1014.dwg to sample_AC1021.dwg and their DXF twins; the 14 horizontal ones have no record) and
// state the legacy approximation in the entity (the attachments 4 and 0, the dogleg length 0, where sample_AC1024.dwg states 0, 8, 8):
//   ROUNDTRIP_MLCONTEXT          272 = 1   174 = 0   175 = 8   91 = 0   40 = 0.0
//   ACAD_ROUNDTRIP_2010_MLEADER  272 = 1   173 = 0   174 = 8   41 = 8.0
// 272 is the direction (1 vertical, the value sample_AC1024.dwg states for the root #B27 and for the entity), 174 / 175 and 173 / 174 the
// left and right attachment of the context and of the entity (0 and 8, the R2010 words), 41 the dogleg length of the entity (8.0). The
// groups 91 = 0 and 40 = 0.0 of the context are in no other sample: they are stated as the sample states them, and a record that
// states other values for them is not read (it is another producer's data).
// ---------------------------------------------------------------------------------------------------------------------

const MLEADER_CONTEXT: &str = "ROUNDTRIP_MLCONTEXT";
const MLEADER_2010: &str = "ACAD_ROUNDTRIP_2010_MLEADER";

pub(crate) fn mleader_holds(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1024
}

/// AutoCAD's DXF of R2000, R2004 and R2007 holds the record (sample_AC1015_ascii.dxf, sample_AC1018_ascii.dxf, sample_AC1021_ascii.dxf).
fn mleader_dxf(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1015
}

/// The words of a multileader a record before R2010 restores.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct MLeaderWords {
    direction: i16,
    left: i16,
    right: i16,
    /// The dogleg length of the entity (not stated for the context: 0.0).
    dogleg: f64,
}

/// One multileader the reader restored: `(the legacy words, the restored words)` of the context's first leader root and of the entity.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub(crate) struct MLeaderRecord {
    context: Option<(MLeaderWords, MLeaderWords)>,
    entity: Option<(MLeaderWords, MLeaderWords)>,
}

fn mleader_context_words(mleader: &crate::entities::MultiLeader) -> Option<MLeaderWords> {
    let root = mleader.context.leader_roots.first()?;
    Some(MLeaderWords {
        direction: root.text_attachment_direction as i16,
        left: mleader.context.text_left_attachment as i16,
        right: mleader.context.text_right_attachment as i16,
        dogleg: 0.0,
    })
}

fn set_mleader_context_words(mleader: &mut crate::entities::MultiLeader, words: MLeaderWords) {
    use crate::entities::multileader::{TextAttachmentDirectionType, TextAttachmentType};
    if let Some(root) = mleader.context.leader_roots.first_mut() {
        root.text_attachment_direction = TextAttachmentDirectionType::from(words.direction);
    }
    mleader.context.text_left_attachment = TextAttachmentType::from(words.left);
    mleader.context.text_right_attachment = TextAttachmentType::from(words.right);
}

fn mleader_entity_words(mleader: &crate::entities::MultiLeader) -> MLeaderWords {
    MLeaderWords {
        direction: mleader.text_attachment_direction as i16,
        left: mleader.text_left_attachment as i16,
        right: mleader.text_right_attachment as i16,
        dogleg: mleader.dogleg_length,
    }
}

fn set_mleader_entity_words(mleader: &mut crate::entities::MultiLeader, words: MLeaderWords) {
    use crate::entities::multileader::{TextAttachmentDirectionType, TextAttachmentType};
    mleader.text_attachment_direction = TextAttachmentDirectionType::from(words.direction);
    mleader.text_left_attachment = TextAttachmentType::from(words.left);
    mleader.text_right_attachment = TextAttachmentType::from(words.right);
    mleader.dogleg_length = words.dogleg;
}

/// Whether a multileader is vertical: the one kind AutoCAD writes the records for (none for a horizontal one).
fn mleader_vertical(mleader: &crate::entities::MultiLeader) -> bool {
    use crate::entities::multileader::TextAttachmentDirectionType::Vertical;
    mleader.text_attachment_direction == Vertical || mleader.context.leader_roots.iter().any(|root| root.text_attachment_direction == Vertical)
}

/// The two groups of a multileader that is vertical (AutoCAD writes none for a horizontal one).
fn mleader_groups(mleader: &crate::entities::MultiLeader) -> Vec<Group> {
    let context = mleader_context_words(mleader);
    let Some(context) = context.filter(|_| mleader_vertical(mleader)) else {
        return Vec::new();
    };
    let entity = mleader_entity_words(mleader);
    vec![
        Group::new(
            MLEADER_CONTEXT,
            vec![
                XRecordEntry::int16(272, context.direction),
                XRecordEntry::int16(174, context.left),
                XRecordEntry::int16(175, context.right),
                XRecordEntry::int32(91, 0),
                XRecordEntry::double(40, 0.0),
            ],
        ),
        Group::new(
            MLEADER_2010,
            vec![
                XRecordEntry::int16(272, entity.direction),
                XRecordEntry::int16(173, entity.left),
                XRecordEntry::int16(174, entity.right),
                XRecordEntry::double(41, entity.dogleg),
            ],
        ),
    ]
}

fn mleader_direction(value: i32) -> Option<i16> {
    (0..=1).contains(&value).then_some(value as i16)
}

fn mleader_attachment(value: i32) -> Option<i16> {
    (0..=10).contains(&value).then_some(value as i16)
}

/// The words of a `ROUNDTRIP_MLCONTEXT` group of the shape AutoCAD writes.
fn mleader_context_group(group: &Group) -> Option<MLeaderWords> {
    let [direction, left, right, index, zero] = group.entries.as_slice() else { return None };
    let codes = [direction.code, left.code, right.code, index.code, zero.code];
    if codes != [272, 174, 175, 91, 40] || as_i32(&index.value) != Some(0) || as_f64(&zero.value) != Some(0.0) {
        return None;
    }
    Some(MLeaderWords {
        direction: mleader_direction(as_i32(&direction.value)?)?,
        left: mleader_attachment(as_i32(&left.value)?)?,
        right: mleader_attachment(as_i32(&right.value)?)?,
        dogleg: 0.0,
    })
}

/// The words of an `ACAD_ROUNDTRIP_2010_MLEADER` group of the shape AutoCAD writes.
fn mleader_entity_group(group: &Group) -> Option<MLeaderWords> {
    let [direction, left, right, dogleg] = group.entries.as_slice() else { return None };
    if [direction.code, left.code, right.code, dogleg.code] != [272, 173, 174, 41] {
        return None;
    }
    let dogleg = as_f64(&dogleg.value).filter(|value| value.is_finite() && *value >= 0.0)?;
    Some(MLeaderWords {
        direction: mleader_direction(as_i32(&direction.value)?)?,
        left: mleader_attachment(as_i32(&left.value)?)?,
        right: mleader_attachment(as_i32(&right.value)?)?,
        dogleg,
    })
}

/// Restore the direction, the attachments and the dogleg length a multileader's groups state; the names of the groups applied and
/// the words the file's legacy record stated.
fn restore_mleader(mleader: &mut crate::entities::MultiLeader, groups: &[Group]) -> (Vec<String>, MLeaderRecord) {
    let mut applied = Vec::new();
    let mut record = MLeaderRecord::default();
    let once = |name: &str| groups.iter().filter(|group| group.name == name).count() == 1;
    for group in groups {
        if group.name == MLEADER_CONTEXT && once(MLEADER_CONTEXT) {
            if let (Some(words), Some(legacy)) = (mleader_context_group(group), mleader_context_words(mleader)) {
                set_mleader_context_words(mleader, words);
                record.context = Some((legacy, words));
                applied.push(group.name.clone());
            }
        } else if group.name == MLEADER_2010 && once(MLEADER_2010) {
            if let Some(words) = mleader_entity_group(group) {
                let legacy = mleader_entity_words(mleader);
                set_mleader_entity_words(mleader, words);
                record.entity = Some((legacy, words));
                applied.push(group.name.clone());
            }
        }
    }
    (applied, record)
}

/// The multileader as the file's legacy record stated it (what ACadSharp reads, and what the writers before R2010 state again), when
/// the reader restored words from a record: each word that still has the restored value takes the legacy one, an edited word stays.
/// A multileader edited to horizontal has no record (`mleader_groups`), so its entity states its own words: the legacy ones are the
/// approximation that goes with the record only.
pub(crate) fn unrestored_mleader(document: &CadDocument, mleader: &crate::entities::MultiLeader) -> Option<crate::entities::MultiLeader> {
    let record = document.consumed_records.mleaders.get(&mleader.common.handle).filter(|_| mleader_vertical(mleader))?;
    let back = |now: MLeaderWords, legacy: MLeaderWords, restored: MLeaderWords| MLeaderWords {
        direction: if now.direction == restored.direction { legacy.direction } else { now.direction },
        left: if now.left == restored.left { legacy.left } else { now.left },
        right: if now.right == restored.right { legacy.right } else { now.right },
        dogleg: if now.dogleg == restored.dogleg { legacy.dogleg } else { now.dogleg },
    };
    let mut shown = mleader.clone();
    if let (Some((legacy, restored)), Some(now)) = (record.context, mleader_context_words(mleader)) {
        set_mleader_context_words(&mut shown, back(now, legacy, restored));
    }
    if let Some((legacy, restored)) = record.entity {
        set_mleader_entity_words(&mut shown, back(mleader_entity_words(mleader), legacy, restored));
    }
    Some(shown)
}

// ---------------------------------------------------------------------------------------------------------------------
// Records that are not in the object's own extension dictionary record: a dictionary of the root dictionary
// (`ACAD_2007HEADERVARS_RT`) and the dictionary of a block record's extension dictionary (`AcDbBlockTableRecordData`).
// ---------------------------------------------------------------------------------------------------------------------

const HEADER_VARS: &str = "ACAD_2007HEADERVARS_RT";
const DIM_LINETYPE: [&str; 3] = ["DIMLTYPE", "DIMLTEX1", "DIMLTEX2"];
const BLOCK_DATA: &str = "AcDbBlockTableRecordData";
const BLOCK_EDIT: &str = "Edit";

pub(crate) fn header_vars_holds(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1021
}

pub(crate) fn block_data_holds(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1021
}

/// AutoCAD's cloning words of a dictionary and the records in it: R14 states none.
fn chain_flags(version: DxfVersion) -> (i16, crate::objects::DictionaryCloningFlags) {
    if version >= DxfVersion::AC1015 {
        (1, crate::objects::DictionaryCloningFlags::KeepExisting)
    } else {
        (0, crate::objects::DictionaryCloningFlags::NotApplicable)
    }
}

fn entry_ci(dictionary: &crate::objects::Dictionary, name: &str) -> Option<Handle> {
    dictionary.get(name)
}

fn dictionary_at<'a>(document: &'a CadDocument, handle: Handle) -> Option<&'a crate::objects::Dictionary> {
    match document.objects.get(&handle) {
        Some(ObjectType::Dictionary(dictionary)) => Some(dictionary),
        _ => None,
    }
}

fn record_entries(document: &CadDocument, handle: Handle) -> Option<&[XRecordEntry]> {
    match document.objects.get(&handle) {
        Some(ObjectType::XRecord(record)) => Some(record.entries.as_slice()),
        _ => None,
    }
}

/// The dictionary `key` of `parent`, created (owned by `parent`, soft or hard) when missing.
fn ensure_dictionary(document: &mut CadDocument, parent: Handle, key: &str, hard: bool, version: DxfVersion) -> Handle {
    if let Some(handle) = dictionary_at(document, parent).and_then(|dictionary| entry_ci(dictionary, key)) {
        if dictionary_at(document, handle).is_some() {
            return handle;
        }
    }
    let handle = document.allocate_handle();
    let mut child = crate::objects::Dictionary::new();
    child.handle = handle;
    child.owner = parent;
    child.hard_owner = hard;
    child.duplicate_cloning = chain_flags(version).0;
    child.reactors = vec![parent];
    document.objects.insert(handle, ObjectType::Dictionary(child));
    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&parent) {
        add_sorted(dictionary, key, handle, hard);
    }
    handle
}

/// Make the XRECORD `key` of `dictionary` hold `entries` (created when missing), or remove it when `entries` is `None`.
fn set_record(document: &mut CadDocument, dictionary: Handle, key: &str, entries: Option<Vec<XRecordEntry>>, hard: bool, version: DxfVersion) {
    let existing = dictionary_at(document, dictionary).and_then(|dictionary| entry_ci(dictionary, key));
    match (existing, entries) {
        (Some(record), Some(entries)) => {
            if let Some(ObjectType::XRecord(xrecord)) = document.objects.get_mut(&record) {
                if xrecord.entries != entries {
                    xrecord.entries = entries;
                    xrecord.entries_complete = true;
                    xrecord.raw_dwg_data = None;
                    xrecord.raw_dwg_version = None;
                    xrecord.source_values.clear();
                    xrecord.synchronize_object_references();
                }
            }
        }
        (None, Some(entries)) => {
            let handle = document.allocate_handle();
            let mut xrecord = crate::objects::XRecord::named(key);
            xrecord.handle = handle;
            xrecord.owner = dictionary;
            xrecord.reactors = vec![dictionary];
            xrecord.cloning_flags = chain_flags(version).1;
            xrecord.entries = entries;
            xrecord.synchronize_object_references();
            document.objects.insert(handle, ObjectType::XRecord(xrecord));
            if let Some(ObjectType::Dictionary(parent)) = document.objects.get_mut(&dictionary) {
                add_sorted(parent, key, handle, hard);
            }
        }
        (Some(record), None) => {
            if let Some(ObjectType::Dictionary(parent)) = document.objects.get_mut(&dictionary) {
                parent.entries.retain(|(_, handle)| *handle != record);
                parent.hard_owner_entries.retain(|name| !name.eq_ignore_ascii_case(key));
            }
            document.objects.remove(&record);
        }
        (None, None) => {}
    }
}

/// Remove `dictionary` (entry `key` of `parent`) when nothing is left in it.
fn remove_if_empty(document: &mut CadDocument, parent: Handle, key: &str, dictionary: Handle) {
    if !dictionary_at(document, dictionary).is_some_and(|dictionary| dictionary.entries.is_empty()) {
        return;
    }
    document.objects.remove(&dictionary);
    if let Some(ObjectType::Dictionary(owner)) = document.objects.get_mut(&parent) {
        owner.entries.retain(|(_, handle)| *handle != dictionary);
        owner.hard_owner_entries.retain(|name| !name.eq_ignore_ascii_case(key));
    }
}

fn line_type_handle(document: &CadDocument, name: &str) -> Option<Handle> {
    document.line_types.get(name).map(|line_type| line_type.handle).filter(|handle| !handle.is_null())
}

fn line_type_name(document: &CadDocument, handle: Handle) -> Option<String> {
    document.line_types.iter().find(|line_type| line_type.handle == handle).map(|line_type| line_type.name.clone())
}

fn header_dim_line_types(document: &CadDocument) -> [&str; 3] {
    let header = &document.header;
    [header.dim_linetype_name.as_str(), header.dim_linetype1_name.as_str(), header.dim_linetype2_name.as_str()]
}

/// The linetype handles the `DIMLTYPE`, `DIMLTEX1` and `DIMLTEX2` records of a document state, when its header has any
/// linetype other than ByBlock.
fn header_vars_wanted(document: &CadDocument) -> Option<[Handle; 3]> {
    let names = header_dim_line_types(document);
    let header = &document.header;
    let stated = [header.dim_linetype_handle, header.dim_linetype1_handle, header.dim_linetype2_handle];
    if names.iter().all(|name| name.eq_ignore_ascii_case("ByBlock")) {
        return None;
    }
    let mut handles = [Handle::NULL; 3];
    for ((slot, name), stated) in handles.iter_mut().zip(names).zip(stated) {
        // The handle the header states, when it names the linetype; a document made in memory states the name only.
        *slot = line_type_name(document, stated).filter(|found| found.eq_ignore_ascii_case(name)).map(|_| stated).or_else(|| line_type_handle(document, name))?;
    }
    Some(handles)
}

fn state_header_vars(document: &mut CadDocument, version: DxfVersion, wanted: Option<[Handle; 3]>) {
    let root = document.header.named_objects_dict_handle;
    let existing = dictionary_at(document, root).and_then(|dictionary| entry_ci(dictionary, HEADER_VARS));
    let existing = existing.filter(|handle| dictionary_at(document, *handle).is_some());
    let Some(wanted) = wanted else {
        if let Some(dictionary) = existing {
            for key in DIM_LINETYPE {
                set_record(document, dictionary, key, None, false, version);
            }
            remove_if_empty(document, root, HEADER_VARS, dictionary);
        }
        return;
    };
    let dictionary = ensure_dictionary(document, root, HEADER_VARS, false, version);
    for (key, handle) in DIM_LINETYPE.into_iter().zip(wanted) {
        set_record(document, dictionary, key, Some(vec![XRecordEntry::handle(340, handle)]), false, version);
    }
}

/// Restore the dimension linetypes of the header; the records applied.
fn restore_header_vars(document: &mut CadDocument) -> Vec<Handle> {
    let root = document.header.named_objects_dict_handle;
    let Some(dictionary) = dictionary_at(document, root).and_then(|dictionary| entry_ci(dictionary, HEADER_VARS)) else {
        return Vec::new();
    };
    let mut found: [Option<(Handle, String)>; 3] = [None, None, None];
    let mut applied = Vec::new();
    for (slot, key) in found.iter_mut().zip(DIM_LINETYPE) {
        let Some(record) = dictionary_at(document, dictionary).and_then(|dictionary| entry_ci(dictionary, key)) else {
            continue;
        };
        if let Some([entry]) = record_entries(document, record) {
            if let (340, Some(handle)) = (entry.code, entry.value.as_handle()) {
                if let Some(name) = line_type_name(document, handle) {
                    *slot = Some((handle, name));
                    applied.push(record);
                }
            }
        }
    }
    let header = &mut document.header;
    if let Some((handle, name)) = found[0].take() {
        header.dim_linetype_handle = handle;
        header.dim_linetype_name = name;
    }
    if let Some((handle, name)) = found[1].take() {
        header.dim_linetype1_handle = handle;
        header.dim_linetype1_name = name;
    }
    if let Some((handle, name)) = found[2].take() {
        header.dim_linetype2_handle = handle;
        header.dim_linetype2_name = name;
    }
    applied
}

/// The values of the `Edit` record AutoCAD writes for a block that scales uniformly.
fn block_edit_entries() -> Vec<XRecordEntry> {
    vec![XRecordEntry::int16(1070, 0), XRecordEntry::int16(1070, 1), XRecordEntry::int16(1070, 1)]
}

/// The `Edit` record of a block record's `AcDbBlockTableRecordData` dictionary, with the dictionary.
fn block_data_of(document: &CadDocument, ext: &Ext, block: Handle) -> Option<(Handle, Handle, Option<Handle>)> {
    let handle = ext.of(document, block)?;
    let extension = dictionary_at(document, handle)?;
    let data = entry_ci(extension, BLOCK_DATA)?;
    let edit = dictionary_at(document, data)?.get(BLOCK_EDIT).filter(|record| record_entries(document, *record).is_some());
    Some((handle, data, edit))
}

fn state_block_data(document: &mut CadDocument, ext: &mut Ext, block: Handle, version: DxfVersion, uniform: bool) {
    let existing = block_data_of(document, ext, block);
    if uniform {
        let extension = ext.ensure(document, block);
        // R14 states the name in upper case.
        let key = if version >= DxfVersion::AC1015 { BLOCK_DATA } else { "ACDBBLOCKTABLERECORDDATA" };
        let data = match existing {
            Some((_, data, _)) => data,
            None => ensure_dictionary(document, extension, key, true, version),
        };
        set_record(document, data, BLOCK_EDIT, Some(block_edit_entries()), true, version);
    } else if let Some((extension, data, edit)) = existing {
        // Only the record AutoCAD writes goes; anything else in the dictionary stays.
        if edit.and_then(|edit| record_entries(document, edit)).is_some_and(|entries| entries == block_edit_entries().as_slice()) {
            set_record(document, data, BLOCK_EDIT, None, true, version);
            remove_if_empty(document, extension, BLOCK_DATA, data);
        }
    }
}

/// Restore the scale-uniformly flag of the blocks `(block, Edit record)`; the records applied.
fn restore_block_data(document: &mut CadDocument, blocks: &[(Handle, Handle)]) -> Vec<Handle> {
    let mut applied = Vec::new();
    for (owner, edit) in blocks {
        if let Some(block) = document.block_records.iter_mut().find(|block| block.handle == *owner) {
            block.scale_uniformly = true;
            applied.push(*edit);
        }
    }
    applied
}

// ---------------------------------------------------------------------------------------------------------------------
// Multi-line attributes: R2018 states the embedded MTEXT in the ATTDEF and ATTRIB records. Before it AutoCAD writes a single-line
// attribute and keeps the MTEXT in the record `ACAD_MLATT` (a DXF-like list) and its height in `ACAD_XREC_ROUNDTRIP
// {ACAD_MATT_HEIGHT}` (sample_AC1014.dwg to sample_AC1027.dwg, the attribute #796 and the attribute #79D of the block #79C, and the DXF
// twins of R2000 to R2013).
// ---------------------------------------------------------------------------------------------------------------------

const MLATT: &str = "ACAD_MLATT";
const MATT_HEIGHT: &str = "ACAD_MATT_HEIGHT";
const EMBEDDED_OBJECT: &str = "Embedded Object";
/// The group codes of the `ACAD_MLATT` record, in AutoCAD's order.
const MLATT_CODES: [i32; 15] = [70, 70, 70, 40, 2, 1, 10, 40, 41, 46, 71, 72, 1, 73, 44];

pub(crate) fn mlatt_holds(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1032
}

/// Whether the ACadSharp view of a document read from a file of its own version shows no embedded MTEXT in an attribute.
#[cfg(feature = "acadsharp-compat")]
pub(crate) fn mlatt_masked(document: &CadDocument) -> bool {
    masked(document, mlatt_holds)
}

fn mlatt_entries(flag: crate::entities::MTextFlag, tag: &str, text: &crate::entities::MText) -> Vec<XRecordEntry> {
    vec![
        XRecordEntry::int16(70, flag.to_value()),
        XRecordEntry::int16(70, 0),
        XRecordEntry::int16(70, 0),
        XRecordEntry::double(40, 1.0),
        XRecordEntry::string(2, tag),
        XRecordEntry::string(1, EMBEDDED_OBJECT),
        XRecordEntry::point3d(10, text.insertion_point.x, text.insertion_point.y, text.insertion_point.z),
        XRecordEntry::double(40, text.height),
        XRecordEntry::double(41, text.rectangle_width),
        XRecordEntry::double(46, text.rectangle_height.unwrap_or(0.0)),
        XRecordEntry::int16(71, text.attachment_point as i16),
        XRecordEntry::int16(72, text.drawing_direction as i16),
        XRecordEntry::string(1, text.value.clone()),
        XRecordEntry::int16(73, text.line_spacing_style as i16),
        XRecordEntry::double(44, text.line_spacing_factor),
    ]
}

/// The embedded MTEXT an `ACAD_MLATT` record of the shape AutoCAD writes states, with the flag of the attribute and its tag.
fn mlatt_text(
    entries: &[XRecordEntry],
    rotation: f64,
    style: &str,
    normal: crate::types::Vector3,
) -> Option<(crate::entities::MTextFlag, String, crate::entities::MText)> {
    if entries.len() != MLATT_CODES.len() || entries.iter().zip(MLATT_CODES).any(|(entry, code)| entry.code != code) {
        return None;
    }
    let int = |index: usize| as_i32(&entries[index].value).and_then(|v| i16::try_from(v).ok());
    let tag = entries[4].value.as_string()?.to_string();
    if entries[5].value.as_string()? != EMBEDDED_OBJECT {
        return None;
    }
    let (x, y, z) = entries[6].value.as_point3d()?;
    let flag = crate::entities::MTextFlag::from_value(int(0)?);
    let mut text = crate::entities::MText::new();
    text.insertion_point = crate::types::Vector3::new(x, y, z);
    text.height = as_f64(&entries[7].value)?;
    text.rectangle_width = as_f64(&entries[8].value)?;
    let rectangle_height = as_f64(&entries[9].value)?;
    text.rectangle_height = (rectangle_height != 0.0).then_some(rectangle_height);
    text.attachment_point = match int(10)? {
        1 => crate::entities::AttachmentPoint::TopLeft,
        2 => crate::entities::AttachmentPoint::TopCenter,
        3 => crate::entities::AttachmentPoint::TopRight,
        4 => crate::entities::AttachmentPoint::MiddleLeft,
        5 => crate::entities::AttachmentPoint::MiddleCenter,
        6 => crate::entities::AttachmentPoint::MiddleRight,
        7 => crate::entities::AttachmentPoint::BottomLeft,
        8 => crate::entities::AttachmentPoint::BottomCenter,
        9 => crate::entities::AttachmentPoint::BottomRight,
        _ => return None,
    };
    text.drawing_direction = match int(11)? {
        1 => crate::entities::DrawingDirection::LeftToRight,
        3 => crate::entities::DrawingDirection::TopToBottom,
        5 => crate::entities::DrawingDirection::ByStyle,
        _ => return None,
    };
    text.value = entries[12].value.as_string()?.to_string();
    text.line_spacing_style = match int(13)? {
        1 => crate::entities::LineSpacingStyle::AtLeast,
        2 => crate::entities::LineSpacingStyle::Exactly,
        _ => return None,
    };
    text.line_spacing_factor = as_f64(&entries[14].value)?;
    // What the attribute itself states.
    text.rotation = rotation;
    text.style = style.to_string();
    text.normal = normal;
    Some((flag, tag, text))
}

/// One multi-line attribute: the owner, the insert it hangs from (an attribute of an insert), the record and the height.
struct Multiline {
    owner: Handle,
    insert: Option<Handle>,
    entries: Vec<XRecordEntry>,
    height: f64,
}

fn multilines(document: &CadDocument) -> Vec<Multiline> {
    let mut found = Vec::new();
    for entity in document.entities() {
        match entity {
            crate::entities::EntityType::AttributeDefinition(attribute) => {
                if let Some(text) = attribute.embedded_mtext.as_deref() {
                    found.push(Multiline {
                        owner: attribute.common.handle,
                        insert: None,
                        entries: mlatt_entries(attribute.mtext_flag, &attribute.tag, text),
                        height: text.height,
                    });
                }
            }
            crate::entities::EntityType::Insert(insert) => {
                for attribute in &insert.attributes {
                    if let Some(text) = attribute.embedded_mtext.as_deref() {
                        found.push(Multiline {
                            owner: attribute.common.handle,
                            insert: Some(insert.common.handle),
                            entries: mlatt_entries(attribute.mtext_flag, &attribute.tag, text),
                            height: text.height,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    found
}

fn state_multiline(document: &mut CadDocument, ext: &mut Ext, multiline: &Multiline, version: DxfVersion) {
    let existed = ext.of(document, multiline.owner).is_some();
    let extension = ext.ensure(document, multiline.owner);
    // An attribute of an insert is not an entity of the document: its extension dictionary is stated in its own properties.
    if let Some(insert) = multiline.insert.filter(|_| !existed) {
        if let Some(crate::entities::EntityType::Insert(insert)) = document.get_entity_mut(insert) {
            if let Some(attribute) = insert.attributes.iter_mut().find(|attribute| attribute.common.handle == multiline.owner) {
                attribute.common.xdictionary_handle = Some(extension);
            }
        }
    }
    // AutoCAD numbers the objects of the chain in this order (sample_AC1014.dwg to sample_AC1027.dwg: the dictionary, the round-trip
    // record, then the record of the MTEXT) and lists the entries by name (`add_sorted`).
    state(
        document,
        ext,
        multiline.owner,
        &|name| name == MATT_HEIGHT,
        vec![Group::new(MATT_HEIGHT, vec![XRecordEntry::double(140, multiline.height)])],
        true,
        version,
    );
    set_record(document, extension, MLATT, Some(multiline.entries.clone()), true, version);
}

/// Drop the record of an attribute whose MTEXT the version holds itself (or that has none any more).
fn drop_multiline(document: &mut CadDocument, ext: &mut Ext, owner: Handle, version: DxfVersion) {
    let Some(extension) = ext.of(document, owner) else {
        return;
    };
    set_record(document, extension, MLATT, None, true, version);
    state(document, ext, owner, &|name| name == MATT_HEIGHT, Vec::new(), false, version);
}

/// Restore the embedded MTEXT of the attributes `owner -> (ACAD_MLATT record, its entries)`; the attributes applied, with the
/// record and the height of the MTEXT.
fn restore_multilines(
    document: &mut CadDocument,
    found: &std::collections::HashMap<Handle, (Handle, Vec<XRecordEntry>)>,
) -> std::collections::HashMap<Handle, (Handle, f64)> {
    let mut applied = std::collections::HashMap::new();
    if found.is_empty() {
        return applied;
    }
    let mut apply = |owner: Handle,
                     attribute_flag: &mut crate::entities::MTextFlag,
                     multiline: &mut bool,
                     embedded: &mut Option<Box<crate::entities::MText>>,
                     tag: &str,
                     rotation: f64,
                     style: &str,
                     normal: crate::types::Vector3| {
        let Some((record, entries)) = found.get(&owner) else { return };
        // The tag the record states is the attribute's own (a record of another tag was not made for this attribute).
        if let Some((flag, record_tag, text)) = mlatt_text(entries, rotation, style, normal) {
            if record_tag == tag && embedded.is_none() {
                applied.insert(owner, (*record, text.height));
                *attribute_flag = flag;
                *multiline = true;
                *embedded = Some(Box::new(text));
            }
        }
    };
    for entity in document.entities_mut_keep_raw() {
        match entity {
            crate::entities::EntityType::AttributeDefinition(attribute) => {
                if found.contains_key(&attribute.common.handle) {
                    let (rotation, style, normal) = (attribute.rotation, attribute.text_style.clone(), attribute.normal);
                    let tag = attribute.tag.clone();
                    apply(attribute.common.handle, &mut attribute.mtext_flag, &mut attribute.is_multiline, &mut attribute.embedded_mtext, &tag, rotation, &style, normal);
                }
            }
            crate::entities::EntityType::Insert(insert) => {
                for attribute in &mut insert.attributes {
                    if found.contains_key(&attribute.common.handle) {
                        let (rotation, style, normal) = (attribute.rotation, attribute.text_style.clone(), attribute.normal);
                        let tag = attribute.tag.clone();
                        apply(attribute.common.handle, &mut attribute.mtext_flag, &mut attribute.is_multiline, &mut attribute.embedded_mtext, &tag, rotation, &style, normal);
                    }
                }
            }
            _ => {}
        }
    }
    applied
}

// ---------------------------------------------------------------------------------------------------------------------
// The passes
// ---------------------------------------------------------------------------------------------------------------------

/// What one look at the dictionaries of a document finds: the round-trip records (owner, record), the blocks with the
/// `AcDbBlockTableRecordData` chain AutoCAD writes for a block that scales uniformly, and the attributes with an `ACAD_MLATT` record
/// (owner -> its entries).
struct Scan {
    records: Vec<(Handle, Handle)>,
    /// `(block, its Edit record)`.
    blocks: Vec<(Handle, Handle)>,
    /// `owner -> (its ACAD_MLATT record, the record's entries)`.
    multilines: std::collections::HashMap<Handle, (Handle, Vec<XRecordEntry>)>,
    /// `owner -> (its ACAD_MTEXT_RT record, the record's entries)`.
    mtexts: std::collections::HashMap<Handle, (Handle, Vec<XRecordEntry>)>,
}

fn scan(document: &CadDocument, want_blocks: bool, want_multilines: bool, want_mtexts: bool) -> Scan {
    let mut found = Scan { records: Vec::new(), blocks: Vec::new(), multilines: std::collections::HashMap::new(), mtexts: std::collections::HashMap::new() };
    for object in document.objects.values() {
        let ObjectType::Dictionary(dictionary) = object else { continue };
        if let Some(record) = dictionary.get(KEY).filter(|record| matches!(document.objects.get(record), Some(ObjectType::XRecord(_)))) {
            found.records.push((dictionary.owner, record));
        }
        if want_blocks {
            if let Some(edit) = entry_ci(dictionary, BLOCK_DATA).and_then(|data| dictionary_at(document, data)).and_then(|data| data.get(BLOCK_EDIT)) {
                if record_entries(document, edit).is_some_and(|entries| entries == block_edit_entries().as_slice()) {
                    found.blocks.push((dictionary.owner, edit));
                }
            }
        }
        if want_multilines {
            if let Some((record, entries)) = dictionary.get(MLATT).and_then(|record| Some((record, record_entries(document, record)?))) {
                found.multilines.insert(dictionary.owner, (record, entries.to_vec()));
            }
        }
        if want_mtexts {
            if let Some((record, entries)) = dictionary.get(MTEXT_RT).and_then(|record| Some((record, record_entries(document, record)?))) {
                found.mtexts.insert(dictionary.owner, (record, entries.to_vec()));
            }
        }
    }
    found.records.sort_by_key(|(owner, record)| (owner.value(), record.value()));
    found
}

/// Restore the native values from the records a reader found and take what it applied out of the model (`take_groups`,
/// `take_chain`). A file holds the groups only where its version needs them; a version that holds the values itself is left
/// alone.
pub(crate) fn restore_document(document: &mut CadDocument) {
    let version = document.version;
    if version >= DxfVersion::AC1032 {
        return;
    }
    consume_visual_styles(document);
    if !header_vars_holds(version) {
        for record in restore_header_vars(document) {
            take_chain(document, record);
        }
    }
    if !header_roundtrip_holds(version) {
        for record in restore_header_roundtrip(document) {
            take_chain(document, record);
        }
    }
    let found = scan(document, !block_data_holds(version), !mlatt_holds(version), !mtext_rt_holds(version));
    for record in restore_block_data(document, &found.blocks) {
        take_chain(document, record);
    }
    let multilines = restore_multilines(document, &found.multilines);
    for (record, _) in multilines.values() {
        take_chain(document, *record);
    }
    for record in restore_mtext(document, &found.mtexts) {
        take_chain(document, record);
    }
    let found = found.records;
    if found.is_empty() {
        return;
    }
    let vport_control = document.vports.handle();
    let tables = table_record_handles(document);
    for (owner, record) in found {
        let groups = groups_of(document, record);
        let applied = restore_record(document, owner, &groups, version, vport_control, &multilines);
        let before = document.consumed_records.changes.len();
        take_groups(document, record, &applied);
        if document.layers.iter().any(|layer| layer.handle == owner) {
            let moved = document.consumed_records.changes.split_off(before);
            document.consumed_records.layer_changes.extend(moved);
        }
        // The group `EXTNAMES` of a table record whose name is the drawing's (the DWG builder restored it before the references were
        // made; a DXF that carries the group has the names as they are): out of the model, put back for the view and the writers.
        // A DXF holds the group as the file had it (`extnames_applied`).
        if tables.contains(&owner) && extnames_applied(document, owner, &groups) {
            take_groups(document, record, &[EXTNAMES.to_string()]);
        }
    }
}

/// Restore the values the groups of the round-trip record of `owner` state; the names of the groups applied.
fn restore_record(
    document: &mut CadDocument,
    owner: Handle,
    groups: &[Group],
    version: DxfVersion,
    vport_control: Handle,
    multilines: &std::collections::HashMap<Handle, (Handle, f64)>,
) -> Vec<String> {
    if owner == vport_control {
        if display_holds(version) {
            return Vec::new();
        }
        let mut applied = Vec::new();
        for vport in document.vports.iter_mut() {
            let prefix = vport_prefix(vport);
            applied.extend(restore_vport_display(vport, groups, &prefix));
        }
        return applied;
    }
    // The height of a multi-line attribute's MTEXT, which its ACAD_MLATT record states too.
    if let Some((_, height)) = multilines.get(&owner) {
        return groups
            .iter()
            .filter(|group| group.name == MATT_HEIGHT && matches!(group.entries.as_slice(), [entry] if entry.code == 140 && as_f64(&entry.value) == Some(*height)))
            .map(|group| group.name.clone())
            .collect();
    }
    if let Some(layer) = document.layers.iter_mut().find(|layer| layer.handle == owner) {
        let mut applied = if layer_color_holds(version) { Vec::new() } else { restore_layer_color(layer, groups) };
        if !layer_plot_holds(version) {
            applied.extend(restore_layer_plot(layer, groups));
        }
        return applied;
    }
    // A version whose records hold the values restores nothing here: no scan of the table per record (the owner falls through to
    // the lookups below, which apply nothing to a table record).
    if !dim_style_holds(version) {
        if let Some(style) = document.dim_styles.iter_mut().find(|style| style.handle == owner) {
            return restore_dim_style(style, groups);
        }
    }
    match document.objects.get_mut(&owner) {
        Some(object) if view_style_base(object).is_some() => {
            return match view_style_base_mut(object).filter(|_| !view_style_holds(version)) {
                Some(base) => restore_view_style(base, groups),
                None => Vec::new(),
            };
        }
        Some(ObjectType::Material(material)) => {
            return if material_holds(version) { Vec::new() } else { restore_material(material, groups) };
        }
        Some(ObjectType::Layout(layout)) => {
            return if layout_holds(version) { Vec::new() } else { restore_layout(layout, groups) };
        }
        Some(_) => return Vec::new(),
        None => {}
    }
    if !display_holds(version) || !ucs_holds(version) {
        if let Some(view) = document.views.iter_mut().find(|view| view.handle == owner) {
            let mut applied = if display_holds(version) { Vec::new() } else { restore_view(view, groups) };
            if !ucs_holds(version) {
                applied.extend(restore_view_ucs(view, groups));
            }
            return applied;
        }
    }
    if !ucs_holds(version) {
        if let Some(vport) = document.vports.iter_mut().find(|vport| vport.handle == owner) {
            return restore_vport_ucs(vport, groups);
        }
    }
    // Look before taking the entity: only a viewport or a solid changes.
    let wanted = match document.get_entity(owner) {
        Some(crate::entities::EntityType::Viewport(_)) => true,
        Some(crate::entities::EntityType::Solid3D(_)) => !solid_history_holds(version),
        Some(crate::entities::EntityType::MultiLeader(_)) => !mleader_holds(version),
        _ => false,
    };
    if !wanted {
        return Vec::new();
    }
    let mut restored = None;
    let applied = match document.get_entity_mut_keep_raw(owner) {
        Some(crate::entities::EntityType::Viewport(viewport)) => restore_viewport(viewport, groups, version),
        Some(crate::entities::EntityType::Solid3D(solid)) => restore_solid(solid, groups),
        Some(crate::entities::EntityType::MultiLeader(mleader)) => {
            let (applied, record) = restore_mleader(mleader, groups);
            restored = Some(record);
            applied
        }
        _ => Vec::new(),
    };
    if let Some(record) = restored.filter(|record| *record != MLeaderRecord::default()) {
        document.consumed_records.mleaders.insert(owner, record);
    }
    applied
}

// ---------------------------------------------------------------------------------------------------------------------
// Layer colour (R14 and R2000 DWG): a layer of those versions states an ACI index; the true colour and the colour-book identity of a
// layer go to `ACAD_XREC_ROUNDTRIP {COLORACI, COLORTRUE, COLORNAME}` (sample_AC1014.dwg and sample_AC1015.dwg: the layers
// LAYER_TRUE_COLOR, LAYER_COLOR_BOOK and COLOR_125_33_79; `70` the index the record states, `90` the R2004 colour word `0xC2`
// red green blue, `1` the identity `<book>$<name>` of a colour-book colour). AutoCAD's DXF of R2000 holds the colour in group 420.
// ---------------------------------------------------------------------------------------------------------------------

const LAYER_ACI: &str = "COLORACI";
const LAYER_TRUE: &str = "COLORTRUE";
const LAYER_BOOK: &str = "COLORNAME";

pub(crate) fn layer_color_holds(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1018
}

/// The groups that keep the colour of a layer whose colour is not an index: none for an index, ByLayer or ByBlock.
fn layer_color_groups(layer: &crate::tables::Layer) -> Vec<Group> {
    let crate::types::Color::Rgb { r, g, b } = layer.color else {
        return Vec::new();
    };
    let word = (0xC2u32 << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
    let mut groups = vec![
        Group::new(LAYER_ACI, vec![XRecordEntry::int16(70, layer.color.approximate_index())]),
        Group::new(LAYER_TRUE, vec![XRecordEntry::int32(90, word as i32)]),
    ];
    if let (Some(book), Some(name)) = (&layer.book_name, &layer.color_name) {
        groups.push(Group::new(LAYER_BOOK, vec![XRecordEntry::string(1, format!("{book}${name}"))]));
    }
    groups
}

/// Restore the colour of a layer from its groups. The record is the layer's own when the index it states is the one the layer
/// holds; the groups applied are named.
fn restore_layer_color(layer: &mut crate::tables::Layer, groups: &[Group]) -> Vec<String> {
    let find = |name: &str| groups.iter().find(|group| group.name == name);
    let index = find(LAYER_ACI).and_then(|group| match group.entries.as_slice() {
        [entry] if entry.code == 70 => as_i32(&entry.value),
        _ => None,
    });
    let word = find(LAYER_TRUE).and_then(|group| match group.entries.as_slice() {
        [entry] if entry.code == 90 => as_i32(&entry.value),
        _ => None,
    });
    let (Some(index), Some(word)) = (index, word) else {
        return Vec::new();
    };
    let word = word as u32;
    if word >> 24 != 0xC2 || !(1..=255).contains(&index) || layer.color != crate::types::Color::Index(index as u8) {
        return Vec::new();
    }
    let identity = find(LAYER_BOOK).and_then(|group| match group.entries.as_slice() {
        [entry] if entry.code == 1 => entry.value.as_string().and_then(|text| text.split_once('$')).map(|(book, name)| (book.to_string(), name.to_string())),
        _ => None,
    });
    layer.color = crate::types::Color::Rgb { r: (word >> 16) as u8, g: (word >> 8) as u8, b: word as u8 };
    let mut applied = vec![LAYER_ACI.to_string(), LAYER_TRUE.to_string()];
    if let Some((book, name)) = identity {
        layer.book_name = Some(book);
        layer.color_name = Some(name);
        applied.push(LAYER_BOOK.to_string());
    }
    applied
}

const LAYER_PLOT: &str = "PLOTBIT";
const LAYER_WEIGHT: &str = "LINEWEIGHT";

/// R14 has no plot flag and no line weight in a layer record (R2000 has both): AutoCAD's R14 file keeps `{PLOTBIT 280 0}` of a
/// layer that does not plot and `{LINEWEIGHT 280 <DWG index>}` of a layer with a line weight (sample_AC1014.dwg: LAYER_NOPLOT and
/// DEFPOINTS 0; LAYER_LW_035 9 and LAYER_LW_050 11; the other 15 layers have neither and plot with the default weight).
pub(crate) fn layer_plot_holds(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1015
}

fn layer_plot_groups(layer: &crate::tables::Layer) -> Vec<Group> {
    let mut groups = Vec::new();
    if !layer.is_plottable {
        groups.push(Group::new(LAYER_PLOT, vec![XRecordEntry::new(280, XRecordValue::Byte(0))]));
    }
    if matches!(layer.line_weight, crate::types::LineWeight::Value(_)) {
        groups.push(Group::new(LAYER_WEIGHT, vec![XRecordEntry::new(280, XRecordValue::Byte(layer.line_weight.to_dwg_index()))]));
    }
    groups
}

/// Restore the plot flag and the line weight of a layer from its groups; the groups applied.
fn restore_layer_plot(layer: &mut crate::tables::Layer, groups: &[Group]) -> Vec<String> {
    let mut applied = Vec::new();
    for group in groups {
        let [entry] = group.entries.as_slice() else { continue };
        if entry.code != 280 {
            continue;
        }
        match (group.name.as_str(), as_i32(&entry.value)) {
            (LAYER_PLOT, Some(0)) => layer.is_plottable = false,
            (LAYER_WEIGHT, Some(index)) if (0..24).contains(&index) => layer.line_weight = crate::types::LineWeight::from_dwg_index(index as u8),
            _ => continue,
        }
        applied.push(group.name.clone());
    }
    applied
}

/// What ACadSharp reads of the plot flag and line weight of a layer of a file of its own version that lacks them (R14): the
/// defaults, whatever the record applied.
#[cfg(feature = "acadsharp-compat")]
pub(crate) fn unrestored_layer_plot(document: &CadDocument) -> Option<(bool, crate::types::LineWeight)> {
    masked(document, layer_plot_holds).then_some((true, crate::types::LineWeight::Default))
}

/// The colour ACadSharp reads for a layer of a file of its own version before R2004 whose record the reader applied: the index
/// the record states.
#[cfg(feature = "acadsharp-compat")]
pub(crate) fn unrestored_layer_color(document: &CadDocument, layer: &crate::tables::Layer) -> Option<crate::types::Color> {
    if !masked(document, layer_color_holds) || !matches!(layer.color, crate::types::Color::Rgb { .. }) {
        return None;
    }
    let dictionary = document.extension_dictionary_handle(layer.handle)?;
    let record = dictionary_at(document, dictionary)?.get(KEY)?;
    let groups = groups_of(document, record);
    let group = groups.iter().find(|group| group.name == LAYER_ACI)?;
    match group.entries.as_slice() {
        [entry] if entry.code == 70 => as_i32(&entry.value).map(|index| crate::types::Color::Index(index as u8)),
        _ => None,
    }
}

fn restore_view(view: &mut crate::tables::View, groups: &[Group]) -> Vec<String> {
    let mut applied = Vec::new();
    for group in groups {
        match (group.name.as_str(), group.entries.as_slice()) {
            (CAMERA_PLOTTABLE, [entry]) if entry.code == 70 => match as_i32(&entry.value) {
                Some(value) => view.camera_plottable = value != 0,
                None => continue,
            },
            (VISUAL_STYLE, [entry]) if entry.code == 340 => match entry.value.as_handle() {
                Some(handle) => view.visual_style_handle = handle,
                None => continue,
            },
            _ => continue,
        }
        applied.push(group.name.clone());
    }
    applied
}

// ---------------------------------------------------------------------------------------------------------------------
// Names of the table records (R14): R14 states the name of a symbol table record in upper case, a character it does not allow as `_`
// (`1.5in Square - Medium Blue` is `1_5IN_SQUARE_-_MEDIUM_BLUE`, `0 @ 1` is `0___1`, `Layer1` is `LAYER1`), and keeps the name the
// drawing had in `ACAD_XREC_ROUNDTRIP {EXTNAMES 1 <name in R14> 2 <name>}`, the first group of the record, of every record whose name
// changed (sample_AC1014.dwg: 20 layers, 12 applications, 10 blocks, 5 dimension styles, 4 text styles, 3 line types, the view and the
// VPORT; the other 66 table records of the sample have a name R14 states as it is). The anonymous blocks (`my-dynamic-block`, `*U18`)
// and the text styles of a shape file (`unnamed0`) keep their names and have no record. The record exists for the extension dictionary
// of dictionaries too (the layouts, the visual styles), where the keys keep their case in the sample for names R14 accepts: not
// reproduced, those records stay in the model. A name with a character no sample shows (anything beside letters, digits, `_`, `-`,
// `$`, `*`, a space, `.` and `@`) is neither changed nor recorded.
//
// The model holds the names of the drawing: the reader restores them before the entities name their layers, styles and blocks
// (`dwg_document_builder`), the writer states the upper-case names (`r14_name`) and the groups from the model's names.
// ---------------------------------------------------------------------------------------------------------------------

const EXTNAMES: &str = "EXTNAMES";

/// Whether a file of `version` states the names of its table records in upper case.
pub(crate) fn extnames_needed(version: DxfVersion) -> bool {
    version == DxfVersion::AC1014
}

/// The application AutoCAD adds when it saves a drawing with attributes: its own file of R14 states the name as it is, mixed case, with
/// no `EXTNAMES` group (sample_AC1014.dwg: the application #125A `AcDbAttr`, the one mixed-case application of 44).
pub(crate) fn app_stated_as_is(name: &str) -> bool {
    name == "AcDbAttr"
}

/// The name R14 states for `name` (upper case, characters outside the symbol alphabet as `_`); `None` when it is the same text or
/// when `name` has a character no sample shows.
pub(crate) fn r14_name(name: &str) -> Option<String> {
    let mut out = String::with_capacity(name.len());
    for character in name.chars() {
        match character {
            'a'..='z' => out.push(character.to_ascii_uppercase()),
            'A'..='Z' | '0'..='9' | '_' | '-' | '$' | '*' => out.push(character),
            ' ' | '.' | '@' => out.push('_'),
            _ => return None,
        }
    }
    (out != name).then_some(out)
}

/// The pairs `(name in R14, name)` of the `EXTNAMES` group of a record.
pub(crate) fn extnames_pairs(entries: &[XRecordEntry]) -> Vec<(String, String)> {
    let groups = split(entries);
    let Some(group) = groups.iter().find(|group| group.name == EXTNAMES) else { return Vec::new() };
    let mut pairs = Vec::new();
    for pair in group.entries.chunks(2) {
        if let [first, second] = pair {
            if let (1, Some(upper), 2, Some(name)) = (first.code, first.value.as_string(), second.code, second.value.as_string()) {
                pairs.push((upper.to_string(), name.to_string()));
            }
        }
    }
    pairs
}

/// Put the names of the file back for the ACadSharp view (which reads R14's upper-case names): the table records the reader restored,
/// and the header names that refer to them. An edited name stays as it is.
fn unrestore_names(document: &mut CadDocument) {
    let names = std::mem::take(&mut document.consumed_records.names);
    for (owner, file) in &names {
        macro_rules! back {
            ($table:expr) => {
                let found = $table.iter().find(|record| record.handle == *owner).map(|record| record.name.clone());
                if let Some(current) = found {
                    if r14_name(&current).as_deref() == Some(file.as_str()) {
                        let _ = $table.rename(&current, file.clone());
                        rename_header_references(&mut document.header, &current, file);
                        continue;
                    }
                }
            };
        }
        back!(document.layers);
        back!(document.line_types);
        back!(document.text_styles);
        back!(document.dim_styles);
        back!(document.app_ids);
        back!(document.views);
        back!(document.ucss);
        back!(document.vports);
        let found = document.block_records.iter().find(|record| record.handle == *owner).map(|record| record.name.clone());
        if let Some(current) = found {
            if r14_name(&current).as_deref() == Some(file.as_str()) {
                let marker = document.block_records.get(&current).map(|record| (record.block_entity_handle, record.block_end_handle));
                let _ = document.block_records.rename(&current, file.clone());
                rename_header_references(&mut document.header, &current, file);
                if let Some((block, _)) = marker {
                    if let Some(crate::entities::EntityType::Block(block)) = document.get_entity_mut(block) {
                        if block.name == current {
                            block.name = file.clone();
                        }
                    }
                }
            }
        }
    }
    document.consumed_records.names = names;
}

fn rename_header_references(header: &mut crate::document::HeaderVariables, old: &str, new: &str) {
    for name in [
        &mut header.current_layer_name,
        &mut header.current_text_style_name,
        &mut header.current_dimstyle_name,
        &mut header.current_linetype_name,
        &mut header.dim_text_style_name,
    ] {
        if name.eq_ignore_ascii_case(old) {
            *name = new.to_string();
        }
    }
}

/// Whether the `EXTNAMES` group of `groups` is applied to the table record `owner`: its pairs name the record (the name the file states
/// and the name of the drawing) and the record already has the name of the drawing.
fn extnames_applied(document: &CadDocument, owner: Handle, groups: &[Group]) -> bool {
    let Some(group) = groups.iter().find(|group| group.name == EXTNAMES) else { return false };
    let [first, second] = group.entries.as_slice() else { return false };
    let (1, Some(upper), 2, Some(name)) = (first.code, first.value.as_string(), second.code, second.value.as_string()) else { return false };
    if r14_name(name).as_deref() != Some(upper) {
        return false;
    }
    macro_rules! has {
        ($table:expr) => {
            if $table.iter().any(|record| record.handle == owner && record.name == name) {
                return true;
            }
        };
    }
    has!(document.layers);
    has!(document.line_types);
    has!(document.text_styles);
    has!(document.dim_styles);
    has!(document.app_ids);
    has!(document.block_records);
    has!(document.views);
    has!(document.ucss);
    has!(document.vports);
    false
}

/// The handles of the table records of the document.
fn table_record_handles(document: &CadDocument) -> std::collections::HashSet<Handle> {
    let mut found = std::collections::HashSet::new();
    found.extend(document.layers.iter().map(|record| record.handle));
    found.extend(document.line_types.iter().map(|record| record.handle));
    found.extend(document.text_styles.iter().map(|record| record.handle));
    found.extend(document.dim_styles.iter().map(|record| record.handle));
    found.extend(document.app_ids.iter().map(|record| record.handle));
    found.extend(document.block_records.iter().map(|record| record.handle));
    found.extend(document.views.iter().map(|record| record.handle));
    found.extend(document.ucss.iter().map(|record| record.handle));
    found.extend(document.vports.iter().map(|record| record.handle));
    found.remove(&Handle::NULL);
    found
}

/// The names the writer states in upper case: `(owner, name in R14, name)` of every table record whose name R14 changes.
/// Whether the record is a down-saved dynamic block definition the file stated anonymous (the bare "*U", flag 1) while its public
/// name is kept in the extended data (`AcDbDynamicBlockTrueName`), which the reader restores as the record's name. The writer states
/// such a record as it was (anonymous, under the bare "*U"), so its name is not a name the file states and `EXTNAMES` does not cover it.
pub(crate) fn is_dynamic_definition_stated_anonymous(document: &CadDocument, record: &crate::tables::BlockRecord) -> bool {
    record.source_flags & 1 != 0
        && !record.flags.anonymous
        && !record.name.starts_with('*')
        && ["AcDbDynamicBlockTrueName", "ACDBDYNAMICBLOCKTRUENAME"].iter().any(|application| {
            document.object_xdata.get(&record.handle).is_some_and(|data| data.get_record(application).is_some())
                || document.app_ids.get(application).is_some_and(|app| {
                    document.eed_by_handle.get(&record.handle).is_some_and(|records| records.iter().any(|(handle, _)| *handle == app.handle.value()))
                })
        })
}

fn extnames_wanted(document: &CadDocument) -> Vec<(Handle, String, String)> {
    let mut found: Vec<(Handle, String, String)> = Vec::new();
    let mut add = |handle: Handle, name: &str, exempt: bool| {
        if !exempt && !handle.is_null() {
            if let Some(upper) = r14_name(name) {
                found.push((handle, upper, name.to_string()));
            }
        }
    };
    for record in document.layers.iter() {
        add(record.handle, &record.name, false);
    }
    for record in document.line_types.iter() {
        add(record.handle, &record.name, false);
    }
    for record in document.text_styles.iter() {
        add(record.handle, &record.name, record.is_shape_file);
    }
    for record in document.dim_styles.iter() {
        add(record.handle, &record.name, false);
    }
    // The applications the writer leaves out (an unused default) have no record to name.
    let skipped = crate::io::dwg::dwg_stream_writers::object_writer::DwgObjectWriter::skipped_app_ids_of(document);
    for record in document.app_ids.iter() {
        add(record.handle, &record.name, skipped.contains(&record.name.to_ascii_uppercase()) || app_stated_as_is(&record.name));
    }
    for record in document.block_records.iter() {
        add(record.handle, &record.name, record.flags.anonymous || is_dynamic_definition_stated_anonymous(document, record));
    }
    for record in document.views.iter() {
        add(record.handle, &record.name, false);
    }
    for record in document.ucss.iter() {
        add(record.handle, &record.name, false);
    }
    for record in document.vports.iter() {
        add(record.handle, &record.name, false);
    }
    found
}

// ---------------------------------------------------------------------------------------------------------------------
// MTEXT text (R14 and R2000): the MTEXT of R2004 and later holds a paragraph break as a line feed and the one of R14 and R2000 as `\P`
// (sample_AC1018.dwg `this is a Mtext<LF>with multiple lines in it`, sample_AC1015.dwg `this is a Mtext\Pwith multiple lines in it`; their
// DXF twins `^J` and `\P`). AutoCAD's down-save writes the converted text and keeps the original in the MTEXT's extension
// dictionary: `ACAD_MTEXT_RT`, a record of `40 <checksum of the converted text>` and the original text in chunks of 250 characters
// (group 1; sample_AC1015.dwg: the MTEXT #3EC, #3F5, #3F6, #779 have it, the checksum is the sum of every UTF-16 unit times its
// 1-based position, as the table cell checksum; 5 of the 36 MTEXT of sample_AC1014.dwg). AutoCAD reads the original back when the
// checksum still matches the text. A record exists for every MTEXT whose text changed in the conversion; two other conversions are not
// reproduced: the stacked fraction `{\H1x;\S1#4;}` becomes `\S1/4;` (one sample), and R2004 keeps a record `ACAD_MTEXT_2008_RT` for a
// text with a character the code page lacks (`\U+2205`), whose text is the escaped form again and tells nothing the text lacks.
// ---------------------------------------------------------------------------------------------------------------------

const MTEXT_RT: &str = "ACAD_MTEXT_RT";
const MTEXT_RT_CHUNK: usize = 250;

pub(crate) fn mtext_rt_holds(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1018
}

/// The text a file before R2004 states for `original` (the paragraph breaks as `\P`); `None` when it states the original itself.
fn mtext_file_text(original: &str) -> Option<String> {
    original.contains('\n').then(|| original.replace('\n', "\\P"))
}

fn mtext_checksum(text: &str) -> f64 {
    crate::entities::table::cell_text_checksum(text)
}

fn mtext_rt_entries(original: &str, file: &str) -> Vec<XRecordEntry> {
    let mut entries = vec![XRecordEntry::double(40, mtext_checksum(file))];
    let characters: Vec<char> = original.chars().collect();
    for chunk in characters.chunks(MTEXT_RT_CHUNK) {
        entries.push(XRecordEntry::string(1, chunk.iter().collect::<String>()));
    }
    entries
}

/// The original text a record of the shape AutoCAD writes states, with the checksum.
fn mtext_rt_original(entries: &[XRecordEntry]) -> Option<(f64, String)> {
    let (first, chunks) = entries.split_first()?;
    let checksum = (first.code == 40).then(|| as_f64(&first.value)).flatten()?;
    let mut original = String::new();
    for chunk in chunks {
        if chunk.code != 1 {
            return None;
        }
        original.push_str(chunk.value.as_string()?);
    }
    (!chunks.is_empty()).then_some((checksum, original))
}

/// Restore the original text of the MTEXT whose `ACAD_MTEXT_RT` record states it for the text the file holds (the checksum matches the
/// text: the line feeds the file spelled `\P`, a stacked fraction `{\H1x;\S1#4;}` the file spelled `\S1/4;`; sample_AC1015.dwg and its
/// R2004 twin sample_AC1018.dwg, whose MTEXT #662 holds the original); the records applied.
fn restore_mtext(document: &mut CadDocument, found: &std::collections::HashMap<Handle, (Handle, Vec<XRecordEntry>)>) -> Vec<Handle> {
    let mut applied = Vec::new();
    let mut restored = Vec::new();
    // In the order of the owners: the records are taken out in this order (a hash map's order changes from run to run).
    let mut owners: Vec<&Handle> = found.keys().collect();
    owners.sort();
    for owner in owners {
        let (record, entries) = &found[owner];
        let Some((checksum, original)) = mtext_rt_original(entries) else { continue };
        let Some(crate::entities::EntityType::MText(text)) = document.get_entity(*owner) else { continue };
        if mtext_checksum(&text.value) == checksum {
            restored.push((*owner, original, text.value.clone()));
            applied.push(*record);
        }
    }
    for (owner, original, file) in restored {
        if let Some(crate::entities::EntityType::MText(text)) = document.get_entity_mut_keep_raw(owner) {
            text.value = original.clone();
        }
        document.consumed_records.mtext.insert(owner, (original, file));
    }
    applied
}

/// State the record of an MTEXT before R2004 (`Some(file text, record entries)`) or drop the one that no longer fits (`None`).
fn state_mtext(document: &mut CadDocument, ext: &mut Ext, owner: Handle, wanted: Option<(String, Vec<XRecordEntry>)>, version: DxfVersion) {
    match wanted {
        Some((file, entries)) => {
            if let Some(crate::entities::EntityType::MText(text)) = document.get_entity_mut(owner) {
                text.value = file;
            }
            let extension = ext.ensure(document, owner);
            set_record(document, extension, MTEXT_RT, Some(entries), true, version);
        }
        None => {
            if let Some(extension) = ext.of(document, owner) {
                set_record(document, extension, MTEXT_RT, None, true, version);
            }
        }
    }
}

/// The text ACadSharp reads for an MTEXT of a file read before R2004 whose original the reader restored: the file's spelling, while the
/// text is the original (an edit shows as it is).
#[cfg(feature = "acadsharp-compat")]
pub(crate) fn mtext_file_value<'a>(document: &CadDocument, owner: Handle, value: &'a str) -> std::borrow::Cow<'a, str> {
    match document.consumed_records.mtext.get(&owner) {
        Some((original, file)) if original == value => std::borrow::Cow::Owned(file.clone()),
        Some(_) => mtext_file_text(value).map_or(std::borrow::Cow::Borrowed(value), std::borrow::Cow::Owned),
        None => std::borrow::Cow::Borrowed(value),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Hatch gradient (R14 and R2000): the HATCH entity of those versions has no gradient; the hatch is a solid fill with the two colours as
// the nearest indexes in `GradientColor1ACI` / `GradientColor2ACI` and the gradient itself in `ACAD_XREC_ROUNDTRIP {Gradient1}`
// (sample_AC1014.dwg and sample_AC1015.dwg: the hatches #371 and #376, the two whose gradient is switched on; the DXF of R2000 has the
// record too). The record is one binary chunk (group 1004):
//
//   u32 123 + the index of the gradient type in AutoCAD's order (LINEAR 0, CYLINDER, INVCYLINDER, SPHERICAL 3, ..)   u32 2   u32 1
//   32 zero bytes   u32 2 (colours)   u32 0   u32 0
//   colour 1: u32 24, u32 1, u32 colour word (0xC2 red green blue), 12 zero bytes      f64 1.0
//   colour 2: u32 24, u32 1, u32 colour word, 12 zero bytes
//   u32 length of the gradient name and its terminator, the name, 0
//
// (the two samples, LINEAR and SPHERICAL with the colours 0x0000FF and 0xFFFF00, are 123 and 126 bytes and are reproduced byte for
// byte). The 32 zero bytes are the angle, the shift and the tint among others: the samples hold only the defaults, so the writer states
// the record for a gradient of two true colours whose angle, shift and tint are 0 and no other (any other gradient is the
// ACI colours of the entity data only, UNVERIFIED how AutoCAD states them). A DXF-sourced record in the corpus states the type index
// without a name (`dwg_reader::recover_roundtrip_gradients` reads both).
// ---------------------------------------------------------------------------------------------------------------------

const GRADIENT: &str = "Gradient1";
const GRADIENT_TYPES: [&str; 9] = [
    "LINEAR",
    "CYLINDER",
    "INVCYLINDER",
    "SPHERICAL",
    "HEMISPHERICAL",
    "CURVED",
    "INVSPHERICAL",
    "INVHEMISPHERICAL",
    "INVCURVED",
];

pub(crate) fn gradient_holds(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1018
}

fn gradient_dxf(version: DxfVersion) -> bool {
    version <= DxfVersion::AC1015
}

/// The chunk AutoCAD states for a gradient of two true colours with the default angle, shift and tint; `None` for any other gradient.
fn gradient_blob(gradient: &crate::entities::hatch::HatchGradientPattern) -> Option<Vec<u8>> {
    use crate::types::Color;
    let [first, second] = gradient.colors.as_slice() else { return None };
    let (Color::Rgb { r: r1, g: g1, b: b1 }, Color::Rgb { r: r2, g: g2, b: b2 }) = (first.color, second.color) else { return None };
    if !gradient.enabled
        || gradient.is_single_color
        || gradient.angle != 0.0
        || gradient.shift != 0.0
        || gradient.color_tint != 0.0
        || first.value != 0.0
        || second.value != 1.0
        || !gradient.name.is_ascii()
    {
        return None;
    }
    let kind = GRADIENT_TYPES.iter().position(|name| *name == gradient.name)? as u32;
    let word = |r: u8, g: u8, b: u8| 0xC200_0000u32 | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
    let mut out: Vec<u8> = Vec::with_capacity(123);
    for value in [123 + kind, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0] {
        out.extend(value.to_le_bytes());
    }
    for (index, colour) in [word(r1, g1, b1), word(r2, g2, b2)].into_iter().enumerate() {
        for value in [24u32, 1, colour, 0, 0, 0] {
            out.extend(value.to_le_bytes());
        }
        if index == 0 {
            out.extend(1.0f64.to_le_bytes());
        }
    }
    out.extend((gradient.name.len() as u32 + 1).to_le_bytes());
    out.extend(gradient.name.as_bytes());
    out.push(0);
    Some(out)
}

fn gradient_group(gradient: &crate::entities::hatch::HatchGradientPattern) -> Option<Group> {
    gradient_blob(gradient).map(|blob| Group::new(GRADIENT, vec![XRecordEntry::new(1004, XRecordValue::Chunk(blob))]))
}

/// Take the `Gradient1` group out of the round-trip record of a hatch whose gradient the reader rebuilt from it
/// (`dwg_reader::recover_roundtrip_gradients`), when the group is exactly the one the writer states for that gradient: the record is
/// the file's way of holding the gradient, not CAD data. The view and the writers put it back (`reinstate`).
pub(crate) fn consume_gradient(document: &mut CadDocument, hatch: Handle) {
    let Some(crate::entities::EntityType::Hatch(entity)) = document.get_entity(hatch) else { return };
    let Some(wanted) = gradient_group(&entity.gradient_color) else { return };
    let Some(dictionary) = document.extension_dictionary_handle(hatch) else { return };
    let Some(record) = dictionary_at(document, dictionary).and_then(|dictionary| entry_ci(dictionary, KEY)) else { return };
    if groups_of(document, record).iter().any(|group| *group == wanted) {
        take_groups(document, record, &[GRADIENT.to_string()]);
    }
}

/// Whether a kind states its groups in a file of `version`: a DWG needs them where the version lacks the field, a DXF where
/// AutoCAD's own DXF of the version holds them.
type Predicate = fn(DxfVersion) -> bool;

fn layout_dxf(version: DxfVersion) -> bool {
    version <= DxfVersion::AC1015
}

fn dxf_r2000(version: DxfVersion) -> bool {
    version <= DxfVersion::AC1015
}

fn dxf_before_r2007(version: DxfVersion) -> bool {
    version < DxfVersion::AC1021
}

fn always(_: DxfVersion) -> bool {
    true
}

/// Which groups of a record a job states.
enum Owns {
    Exact(&'static [&'static str]),
    Prefix(String),
    Names(Vec<String>),
}

impl Owns {
    fn names(&self, name: &str) -> bool {
        match self {
            Owns::Exact(names) => names.contains(&name),
            Owns::Prefix(prefix) => name.starts_with(prefix.as_str()),
            Owns::Names(names) => names.iter().any(|candidate| candidate == name),
        }
    }
}

struct Job {
    owner: Handle,
    owns: Owns,
    wanted: Vec<Group>,
    /// The groups a file of the version that states none of the values gives the reader: `Some` lets a drawing written again at the
    /// DWG version it was read from state a group its record lacks when the values differ from these (a group that has none here
    /// differs); `None` keeps such a drawing to the groups its record has (a value of the model whose reader default depends on
    /// the file, or a drawing read from a DXF, whose fields are in the objects themselves).
    fresh: Option<Vec<Group>>,
    /// A DXF of a version below the one that holds the values, whose twin AutoCAD's DXF has no such record: the groups the record
    /// has are refreshed from the values and no group is made or dropped (the objects of a drawing read from a DWG stay as they are).
    leave: bool,
}

/// A DXF states none of the layer records (`layer_changes`), and AutoCAD's DXF of R2000 has no extension dictionary on the layers
/// that held only such a record in the DWG (sample_AC1015_ascii.dxf: only `0`, `Layer2` and the two `@ 1` layers have one; the first
/// two hold none in sample_AC1015.dwg either, the layers `Layer_true_color`, `Layer_color_book` and `color_125_33_79` have one with
/// the record there and none in the DXF): the dictionary a consumed record left empty goes with the record. A dictionary the file
/// held empty, or with another entry, stays.
fn drop_emptied_layer_dictionaries(document: &mut CadDocument) {
    let changes = std::mem::take(&mut document.consumed_records.layer_changes);
    let mut dictionaries: Vec<Handle> = changes
        .iter()
        .filter_map(|change| match change {
            Consumption::Entry { dictionary, .. } => Some(*dictionary),
            _ => None,
        })
        .collect();
    dictionaries.sort();
    dictionaries.dedup();
    for dictionary in dictionaries {
        let Some(ObjectType::Dictionary(entries)) = document.objects.get(&dictionary) else { continue };
        let owner = entries.owner;
        if !entries.entries.is_empty() || !document.layers.iter().any(|layer| layer.handle == owner) {
            continue;
        }
        document.objects.remove(&dictionary);
        document.reactors_by_handle.remove(&dictionary);
        if document.xdic_by_handle.get(&owner) == Some(&dictionary) {
            document.xdic_by_handle.remove(&owner);
        }
    }
}

/// State the groups of every kind the output version needs, from the native values, and drop the groups the version holds
/// natively. `dxf` selects the DXF rules.
pub(crate) fn prepare_for_write(document: &mut std::borrow::Cow<'_, CadDocument>, version: DxfVersion, dxf: bool) {
    prepare_records_for_write(document, version, dxf);
    // After the records are stated from the native values: a multileader the reader restored states the legacy words of its file
    // below R2010 (the record holds the others).
    if version < DxfVersion::AC1024 && !document.consumed_records.mleaders.is_empty() {
        let shown: Vec<crate::entities::MultiLeader> = document
            .consumed_records
            .mleaders
            .keys()
            .filter_map(|handle| match document.get_entity(*handle) {
                Some(crate::entities::EntityType::MultiLeader(mleader)) => unrestored_mleader(document, mleader),
                _ => None,
            })
            .collect();
        let owned = document.to_mut();
        for mleader in shown {
            if let Some(crate::entities::EntityType::MultiLeader(target)) = owned.get_entity_mut_keep_raw(mleader.common.handle) {
                **target = mleader;
            }
        }
    }
}

fn prepare_records_for_write(document: &mut std::borrow::Cow<'_, CadDocument>, version: DxfVersion, dxf: bool) {
    // The records the reader took out of the model are the starting point: the objects of the file, refreshed below. R2013 and later
    // hold the properties of a visual style in the style: its record and extension dictionary stay out.
    if version >= DxfVersion::AC1027 && !document.consumed_records.style_changes.is_empty() {
        document.to_mut().consumed_records.style_changes.clear();
    }
    if dxf && !document.consumed_records.layer_changes.is_empty() {
        drop_emptied_layer_dictionaries(document.to_mut());
    }
    if !dxf && version >= DxfVersion::AC1024 && !document.consumed_records.table_style_changes.is_empty() {
        document.to_mut().consumed_records.table_style_changes.clear();
    }
    let taken = document.consumed_records.taken();
    if !document.consumed_records.is_empty() {
        reinstate(document.to_mut());
    }
    // A table record added in memory has no handle yet, and a record cannot be owned by it: the writers give the handles later.
    if document.has_null_table_entries() {
        document.to_mut().assign_table_entry_handles();
    }
    let needs = |holds: Predicate, dxf_states: Predicate| !holds(version) && (!dxf || dxf_states(version));
    let leaves = |holds: Predicate, dxf_states: Predicate| dxf && !holds(version) && !dxf_states(version);
    let mut ext = Ext::new(document);
    let converting = converts(document, version);
    // A drawing converted from a file of another version gets every group of an object, as AutoCAD's down-saves state them; a
    // drawing made in memory gets the groups of an object only where a value differs from what a reader of a file without the
    // record gives (the value would be lost otherwise).
    let converted = converting && !in_memory(document);
    // The drawing is a DWG written again at its own version (a DXF source keeps its fields in the objects themselves).
    let written_again_dwg = !converting && document.dwg_source_version == Some(version);
    let have: std::collections::HashSet<Handle> = records(document).into_iter().map(|(owner, _)| owner).collect();
    let has_record = |owner: Handle| have.contains(&owner);
    let mut jobs: Vec<Job> = Vec::new();
    // Objects.
    let mut handles: Vec<Handle> = document
        .objects
        .iter()
        .filter(|(_, object)| view_style_base(object).is_some() || matches!(object, ObjectType::Material(_) | ObjectType::Layout(_)))
        .map(|(handle, _)| *handle)
        .collect();
    handles.sort();
    for handle in handles {
        let Some(object) = document.objects.get(&handle) else { continue };
        match object {
            ObjectType::Material(material) => {
                let defaults = crate::objects::Material::new();
                let wanted = if needs(material_holds, always)
                    && (converted || has_record(handle) || material_group(material) != material_group(&defaults))
                {
                    vec![material_group(material)]
                } else {
                    Vec::new()
                };
                jobs.push(Job { owner: handle, owns: Owns::Exact(&[MATERIAL]), wanted, fresh: Some(vec![material_group(&defaults)]), leave: leaves(material_holds, always) });
            }
            ObjectType::Layout(layout) => {
                let differs = (layout.shade_plot_mode, layout.shade_plot_resolution, layout.shade_plot_dpi) != LAYOUT_DEFAULT;
                let wanted = if needs(layout_holds, layout_dxf) && (converted || has_record(handle) || differs) {
                    layout_groups(layout)
                } else {
                    Vec::new()
                };
                let fresh = vec![
                    Group::new(SHADE_PLOT, vec![XRecordEntry::int16(70, LAYOUT_DEFAULT.0)]),
                    Group::new(SHADE_PLOT_RESOLUTION, vec![XRecordEntry::int16(70, LAYOUT_DEFAULT.1)]),
                    Group::new(SHADE_PLOT_DPI, vec![XRecordEntry::int16(70, LAYOUT_DEFAULT.2)]),
                ];
                jobs.push(Job { owner: handle, owns: Owns::Exact(&[SHADE_PLOT, SHADE_PLOT_RESOLUTION, SHADE_PLOT_DPI]), wanted, fresh: Some(fresh), leave: leaves(layout_holds, layout_dxf) });
            }
            object => {
                let Some(base) = view_style_base(object) else { continue };
                let wanted = if needs(view_style_holds, always) && (converted || has_record(handle) || !base.display_name.is_empty() || base.flags != 0) {
                    view_style_groups(base)
                } else {
                    Vec::new()
                };
                let fresh = vec![Group::new(DISPLAY_NAME, vec![XRecordEntry::string(1, "")]), Group::new(FLAGS, vec![XRecordEntry::int32(90, 0)])];
                jobs.push(Job { owner: handle, owns: Owns::Exact(&[DISPLAY_NAME, FLAGS]), wanted, fresh: Some(fresh), leave: leaves(view_style_holds, always) });
            }
        }
    }
    // Tables.
    // AutoCAD's R14 file shows each of the colour, the plot flag and the line weight of a layer alone in its record (after
    // `EXTNAMES`), never two of them: the order of a record that holds more than one is not known, so such a layer states none of
    // them (the values fall back to what the layer record holds: the index, plotting, the default weight).
    let unknown_layout = |layer: &crate::tables::Layer| {
        let colour = usize::from(needs(layer_color_holds, never) && !layer_color_groups(layer).is_empty());
        let plot = if needs(layer_plot_holds, never) { layer_plot_groups(layer).len() } else { 0 };
        colour + plot > 1
    };
    for layer in document.layers.iter() {
        let groups = layer_color_groups(layer);
        let wanted = if needs(layer_color_holds, never) && !unknown_layout(layer) && (converted || has_record(layer.handle) || !groups.is_empty()) { groups } else { Vec::new() };
        jobs.push(Job { owner: layer.handle, owns: Owns::Exact(&[LAYER_ACI, LAYER_TRUE, LAYER_BOOK]), wanted, fresh: Some(Vec::new()), leave: leaves(layer_color_holds, never) });
    }
    for layer in document.layers.iter() {
        let groups = layer_plot_groups(layer);
        let wanted = if needs(layer_plot_holds, never) && !unknown_layout(layer) && (converted || has_record(layer.handle) || !groups.is_empty()) { groups } else { Vec::new() };
        jobs.push(Job { owner: layer.handle, owns: Owns::Exact(&[LAYER_PLOT, LAYER_WEIGHT]), wanted, fresh: Some(Vec::new()), leave: leaves(layer_plot_holds, never) });
    }
    for style in document.dim_styles.iter() {
        let wanted = if needs(dim_style_holds, never) { dim_style_groups(style) } else { Vec::new() };
        jobs.push(Job { owner: style.handle, owns: Owns::Exact(&[R14_ROUNDTRIP]), wanted, fresh: None, leave: leaves(dim_style_holds, never) });
    }
    for vport in document.vports.iter() {
        // The groups are built only for a version that needs them (a writer of a later version makes no defaults per record).
        let wanted = if needs(ucs_holds, never) { vport_ucs_wanted(vport) } else { Vec::new() };
        let unchanged = !wanted.is_empty() && wanted == vport_ucs_wanted(&crate::tables::VPort::new(vport.name.clone()));
        let wanted = if converted || has_record(vport.handle) || !unchanged { wanted } else { Vec::new() };
        jobs.push(Job { owner: vport.handle, owns: Owns::Exact(&[UCS_GROUP, VIEW_GROUP]), wanted, fresh: None, leave: leaves(ucs_holds, never) });
    }
    for view in document.views.iter() {
        let defaults = crate::tables::View::new(view.name.clone());
        let differs = view.camera_plottable != defaults.camera_plottable || view.visual_style_handle != defaults.visual_style_handle;
        let mut wanted = Vec::new();
        if needs(display_holds, never) && (converted || has_record(view.handle) || differs) {
            wanted.push(Group::new(CAMERA_PLOTTABLE, vec![XRecordEntry::int16(70, view.camera_plottable as i16)]));
            if !view.visual_style_handle.is_null() {
                wanted.push(Group::new(VISUAL_STYLE, vec![XRecordEntry::handle(340, view.visual_style_handle)]));
            }
        }
        jobs.push(Job { owner: view.handle, owns: Owns::Exact(&[CAMERA_PLOTTABLE, VISUAL_STYLE]), wanted, fresh: None, leave: leaves(display_holds, never) });
    }
    for view in document.views.iter() {
        let wanted = if needs(ucs_holds, never) { view_ucs_wanted(view) } else { Vec::new() };
        let unchanged = !wanted.is_empty() && wanted == view_ucs_wanted(&crate::tables::View::new(view.name.clone()));
        let wanted = if converted || has_record(view.handle) || !unchanged { wanted } else { Vec::new() };
        jobs.push(Job { owner: view.handle, owns: Owns::Exact(&[UCS_GROUP, VIEW_GROUP]), wanted, fresh: None, leave: leaves(ucs_holds, never) });
    }
    let control = document.vports.handle();
    if !control.is_null() {
        for vport in document.vports.iter() {
            let prefix = vport_prefix(vport);
            let display = Display::of_vport(vport);
            let defaults = Display::of_vport(&crate::tables::VPort::new(vport.name.clone()));
            let wanted = if needs(display_holds, never) && (converted || has_record(control) || display != defaults) {
                display.groups(&prefix)
            } else {
                Vec::new()
            };
            jobs.push(Job { owner: control, owns: Owns::Prefix(prefix), wanted, fresh: None, leave: leaves(display_holds, never) });
        }
    }
    // Entities.
    for entity in document.entities() {
        let handle = entity.common().handle;
        match entity {
            crate::entities::EntityType::Viewport(viewport) => {
                // The UCS and the render mode come first (sample_AC1014.dwg), then the shade plot mode and the display settings.
                let wanted = if needs(ucs_holds, never) { viewport_ucs_wanted(viewport) } else { Vec::new() };
                let unchanged = !wanted.is_empty() && wanted == viewport_ucs_wanted(&crate::entities::Viewport::new());
                let wanted = if converted || has_record(handle) || !unchanged { wanted } else { Vec::new() };
                jobs.push(Job { owner: handle, owns: Owns::Exact(&[UCS_GROUP, VIEW_GROUP]), wanted, fresh: None, leave: leaves(ucs_holds, never) });
                // AutoCAD's record states the shade plot mode first, then the display settings (sample_AC1014.dwg, sample_AC1015.dwg).
                let wanted = if needs(shade_plot_holds, shade_plot_dxf) && (converted || has_record(handle) || viewport.shade_plot_mode != 0) {
                    vec![Group::new(SHADE_PLOT, vec![XRecordEntry::int16(70, viewport.shade_plot_mode)])]
                } else {
                    Vec::new()
                };
                let fresh = vec![Group::new(SHADE_PLOT, vec![XRecordEntry::int16(70, 0)])];
                jobs.push(Job { owner: handle, owns: Owns::Exact(&[SHADE_PLOT]), wanted, fresh: Some(fresh), leave: leaves(shade_plot_holds, shade_plot_dxf) });
                let display = Display::of_viewport(viewport);
                let defaults = Display::of_viewport(&crate::entities::Viewport::new());
                let wanted = if needs(display_holds, never) && (converted || has_record(handle) || display != defaults) {
                    display.groups("")
                } else {
                    Vec::new()
                };
                jobs.push(Job { owner: handle, owns: Owns::Exact(&DISPLAY), wanted, fresh: None, leave: leaves(display_holds, never) });
            }
            crate::entities::EntityType::Hatch(hatch) => {
                // The gradient of a hatch before R2004 (R14 and R2000 DWG, R2000 DXF).
                let wanted: Vec<Group> = if needs(gradient_holds, gradient_dxf) { gradient_group(&hatch.gradient_color).into_iter().collect() } else { Vec::new() };
                jobs.push(Job { owner: handle, owns: Owns::Exact(&[GRADIENT]), wanted, fresh: Some(Vec::new()), leave: leaves(gradient_holds, gradient_dxf) });
            }
            crate::entities::EntityType::MultiLeader(mleader) => {
                let wanted = if needs(mleader_holds, mleader_dxf) { mleader_groups(mleader) } else { Vec::new() };
                jobs.push(Job { owner: handle, owns: Owns::Exact(&[MLEADER_CONTEXT, MLEADER_2010]), wanted, fresh: Some(Vec::new()), leave: leaves(mleader_holds, mleader_dxf) });
            }
            crate::entities::EntityType::Solid3D(solid) => {
                let wanted = if needs(solid_history_holds, never) { solid_groups(solid) } else { Vec::new() };
                jobs.push(Job { owner: handle, owns: Owns::Exact(&[SOLID_HISTORY, SOLID_HISTORY_1, SOLID_HISTORY_2]), wanted, fresh: Some(Vec::new()), leave: leaves(solid_history_holds, never) });
            }
            _ => {}
        }
    }
    // A job with nothing to state matters only where a record to refresh or to drop exists.
    jobs.retain(|job| !job.wanted.is_empty() || has_record(job.owner));
    // The linetypes of the dimension header variables.
    let root = document.header.named_objects_dict_handle;
    let header_chain = dictionary_at(document, root).is_some_and(|dictionary| entry_ci(dictionary, HEADER_VARS).is_some());
    let header_vars: Option<Option<[Handle; 3]>> = if needs(header_vars_holds, dxf_before_r2007) {
        // A chain whose linetypes the header does not hold stays: it was not applied. A document written again at the version it
        // was read from states the chain only when its file had one.
        let wanted = header_vars_wanted(document).filter(|_| converting || written_again_dwg || header_chain);
        (wanted.is_some() || (header_chain && header_chain_applied(document))).then_some(wanted)
    } else {
        header_chain.then_some(None)
    };
    // The header variables R14 has no place for. A document converted from another version states the records AutoCAD does; one written
    // again at R14 keeps the records its chain has (the records of the others stay in the model), and states a record it lacks only
    // for a value that differs from what a reader of a file without it gives.
    const HEADER_RECORDS: [&str; 2] = [DIMSTYLEDATA, INSUNITS];
    let header_rt: Option<Vec<(&'static str, Vec<XRecordEntry>)>> = if needs(header_roundtrip_holds, never) {
        let mut wanted = header_roundtrip_wanted(&document.header);
        if !converting {
            let defaults = header_roundtrip_wanted(&crate::document::HeaderVariables::default());
            wanted.retain(|(name, entries)| {
                header_roundtrip_record(document, name).is_some()
                    || (written_again_dwg && defaults.iter().find(|(default, _)| default == name).map_or(true, |(_, default)| default != entries))
            });
        }
        (!wanted.is_empty() || HEADER_RECORDS.iter().any(|name| header_roundtrip_record(document, name).is_some())).then_some(wanted)
    } else if has_header_roundtrip(document) && !dxf {
        // A version that holds the values itself: the records the reader applied go (they are put back above), as AutoCAD's up-save drops them.
        Some(Vec::new())
    } else {
        None
    };
    // The scale-uniformly flag of the blocks: a document written again at the version it was read from states the chain only for the
    // blocks that had one (a file of that version without the chain holds the flag some other way, or not at all).
    // AutoCAD's DXF of R2000 and R2004 holds the chain too (sample_AC1015_ascii.dxf, sample_AC1018_ascii.dxf: the 4 blocks).
    let block_jobs: Vec<(Handle, bool)> = if needs(block_data_holds, dxf_before_r2007) {
        document
            .block_records
            .iter()
            .filter(|block| (block.scale_uniformly && (converting || written_again_dwg)) || block_data_of(document, &ext, block.handle).is_some())
            .map(|block| (block.handle, block.scale_uniformly))
            .collect()
    } else {
        document
            .block_records
            .iter()
            .filter(|block| block_data_of(document, &ext, block.handle).is_some())
            .map(|block| (block.handle, false))
            .collect()
    };
    // The multi-line attributes: the record of those that have an embedded MTEXT, the drop of the records the reader applied for the
    // others (a record the reader did not apply is not this writer's to drop).
    let wanted_multilines: Vec<Multiline> = if needs(mlatt_holds, always) {
        multilines(document)
            .into_iter()
            .filter(|multiline| converting || written_again_dwg || ext.of(document, multiline.owner).is_some_and(|extension| {
                dictionary_at(document, extension).is_some_and(|dictionary| dictionary.get(MLATT).is_some())
            }))
            .collect()
    } else {
        Vec::new()
    };
    let stated: std::collections::HashSet<Handle> = wanted_multilines.iter().map(|multiline| multiline.owner).collect();
    let dropped_multilines: Vec<Handle> = document
        .objects
        .values()
        .filter_map(|object| {
            let ObjectType::Dictionary(dictionary) = object else { return None };
            let record = dictionary.get(MLATT)?;
            let entries = record_entries(document, record)?;
            let ours = entries.len() == MLATT_CODES.len() && entries.iter().zip(MLATT_CODES).all(|(entry, code)| entry.code == code);
            (ours && taken.contains(&record) && !stated.contains(&dictionary.owner)).then_some(dictionary.owner)
        })
        .collect();
    // The names of the table records before R2000 (R14 DWG): upper case, the name of the drawing in `EXTNAMES`. A record of another
    // version states none (the group a reader took out of a file of R14 is dropped).
    let table_owners: std::collections::HashSet<Handle> = table_record_handles(document);
    let names_wanted: Vec<(Handle, String, String)> = if !dxf && extnames_needed(version) { extnames_wanted(document) } else { Vec::new() };
    let names_stated: std::collections::HashSet<Handle> = names_wanted.iter().map(|(owner, ..)| *owner).collect();
    let names_stale: Vec<Handle> = records(document)
        .into_iter()
        .filter(|(owner, record)| {
            !dxf && table_owners.contains(owner) && !names_stated.contains(owner) && groups_of(document, *record).iter().any(|group| group.name == EXTNAMES)
        })
        .map(|(owner, _)| owner)
        .collect();
    // The MTEXT before R2004: the paragraph breaks as `\\P` and the original in `ACAD_MTEXT_RT` (R2000 DXF too: its twin holds the record).
    let mtext_jobs: Vec<(Handle, Option<(String, Vec<XRecordEntry>)>)> = if needs(mtext_rt_holds, dxf_r2000) {
        document
            .entities()
            .filter_map(|entity| {
                let crate::entities::EntityType::MText(text) = entity else { return None };
                let owner = text.common.handle;
                let consumed = document.consumed_records.mtext.get(&owner);
                let restored = consumed.filter(|(original, _)| *original == text.value);
                let wanted = match restored {
                    Some((original, file)) => Some((file.clone(), mtext_rt_entries(original, file))),
                    None => mtext_file_text(&text.value).map(|file| {
                        let entries = mtext_rt_entries(&text.value, &file);
                        (file, entries)
                    }),
                };
                // A record the reader did not apply (another shape or text: the unicode and the stacking conversions) is the file's own: kept.
                let ours = consumed.is_some()
                    && ext
                        .of(document, owner)
                        .and_then(|extension| dictionary_at(document, extension))
                        .and_then(|dictionary| dictionary.get(MTEXT_RT))
                        .and_then(|record| record_entries(document, record))
                        .is_some_and(|entries| mtext_rt_original(entries).is_some());
                (wanted.is_some() || ours).then_some((owner, wanted))
            })
            .collect()
    } else if mtext_rt_holds(version) {
        // A version that holds the text itself: the record the reader applied (it is put back above) goes, as AutoCAD's up-save drops it.
        document
            .consumed_records
            .mtext
            .keys()
            .filter(|owner| {
                ext.of(document, **owner)
                    .and_then(|extension| dictionary_at(document, extension))
                    .and_then(|dictionary| dictionary.get(MTEXT_RT))
                    .and_then(|record| record_entries(document, record))
                    .is_some_and(|entries| mtext_rt_original(entries).is_some())
            })
            .map(|owner| (*owner, None))
            .collect()
    } else {
        Vec::new()
    };
    if jobs.is_empty()
        && header_rt.is_none()
        && header_vars.is_none()
        && block_jobs.is_empty()
        && wanted_multilines.is_empty()
        && dropped_multilines.is_empty()
        && mtext_jobs.is_empty()
        && names_wanted.is_empty()
        && names_stale.is_empty()
    {
        return;
    }
    let owned = document.to_mut();
    // First, so that the group heads the record as it does in AutoCAD's files.
    for (owner, upper, name) in names_wanted {
        let group = Group::new(EXTNAMES, vec![XRecordEntry::string(1, upper), XRecordEntry::string(2, name)]);
        state(owned, &mut ext, owner, &|group| group == EXTNAMES, vec![group], true, version);
    }
    for owner in names_stale {
        state(owned, &mut ext, owner, &|group| group == EXTNAMES, Vec::new(), false, version);
    }
    for job in jobs {
        let mut wanted = job.wanted;
        let mut owns = job.owns;
        if job.leave {
            // Refresh what the record has; make and drop nothing.
            let existing: Vec<String> = record_of(owned, &ext, job.owner)
                .map(|record| groups_of(owned, record).into_iter().map(|group| group.name).collect())
                .unwrap_or_default();
            wanted.retain(|group| existing.contains(&group.name));
            if wanted.is_empty() {
                continue;
            }
            let names: Vec<String> = wanted.iter().map(|group| group.name.clone()).collect();
            owns = Owns::Names(names);
        } else if !converting {
            // A document written again at the version it was read from keeps the groups its record has (with the values it holds
            // now); a group the record lacks is stated only for a DWG whose reader default is known and differs from the value.
            let existing: Vec<String> = record_of(owned, &ext, job.owner)
                .map(|record| groups_of(owned, record).into_iter().map(|group| group.name).collect())
                .unwrap_or_default();
            wanted.retain(|group| {
                existing.contains(&group.name)
                    || (written_again_dwg
                        && job.fresh.as_ref().is_some_and(|fresh| fresh.iter().find(|default| default.name == group.name).map_or(true, |default| default.entries != group.entries)))
            });
        }
        state(owned, &mut ext, job.owner, &|name| owns.names(name), wanted, true, version);
    }
    if let Some(wanted) = header_vars {
        state_header_vars(owned, version, wanted);
    }
    if let Some(wanted) = header_rt {
        state_header_roundtrip(owned, version, wanted, &HEADER_RECORDS);
    }
    for (block, uniform) in block_jobs {
        state_block_data(owned, &mut ext, block, version, uniform);
    }
    for multiline in &wanted_multilines {
        state_multiline(owned, &mut ext, multiline, version);
    }
    for owner in dropped_multilines {
        drop_multiline(owned, &mut ext, owner, version);
    }
    for (owner, wanted) in mtext_jobs {
        state_mtext(owned, &mut ext, owner, wanted, version);
    }
}

/// Whether every record of the chain names a linetype of the document: the chain was applied by the reader.
fn header_chain_applied(document: &CadDocument) -> bool {
    let root = document.header.named_objects_dict_handle;
    let Some(dictionary) = dictionary_at(document, root).and_then(|dictionary| entry_ci(dictionary, HEADER_VARS)) else {
        return false;
    };
    let Some(chain) = dictionary_at(document, dictionary) else {
        return false;
    };
    chain.entries.iter().all(|(key, record)| {
        DIM_LINETYPE.contains(&key.as_str())
            && match record_entries(document, *record) {
                Some([entry]) => entry.code == 340 && entry.value.as_handle().and_then(|handle| line_type_name(document, handle)).is_some(),
                _ => false,
            }
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// What R14 has no field for in a dimension style, a VPORT, a view and a viewport (R14 DWG; R5b-12). AutoCAD's own R14 file of the
// drawing (sample_AC1014.dwg from sample_AC1015.dwg to sample_AC1032.dwg) keeps them in `ACAD_XREC_ROUNDTRIP`:
//
// * a dimension style: `{ACADR14ROUNDTRIP 1000 DSTYLE 1002 { (1070 <group code of the variable> <value>)... 1002 }}` for the variables
//   R14's record lacks: DIMDSEP (278, value 70; ISO-25 and Beschriftung state 44), DIMBLK (342, the handle 340 of the arrowhead's
//   block record; custom_style_ltypes states `#C80`) and DIMLWD (371, value 70; custom_style_ltypes states 15). A style whose three
//   variables are the defaults (46, none, -2: the other three styles) has none. The sample shows the pair 342, 371 in that order and
//   no style with 278 besides: the order is the codes' (UNVERIFIED for a style that has 278 and another).
// * a viewport, a VPORT and a view: `{UCS ..}` and `{VIEW 281}`. The UCS group is the head, the UCS origin, x axis and y axis (three
//   groups 10), the elevation (146) and the orthographic type (79): the head of a viewport and a VPORT is `74` (the UCS icon at the
//   origin) and `71` (the UCS per viewport), the head of a view `72` (a UCS is associated); VIEW is the render mode (281). Every
//   viewport (6), the VPORT and the view of the sample has both groups, in the record after `EXTNAMES`, a view's after the display
//   groups (`CAMERAPLOTTABLE`, `VISUALSTYLE`), a viewport's before the shade plot and display groups. A viewport also states
//   `{NEWVIEWCONTROL 280 4}` between VIEW and SHADEPLOT, 4 in all six: no field of the model matches it and the sample shows no other
//   value, so it is neither written nor read (it stays in a record the reader finds).
// ---------------------------------------------------------------------------------------------------------------------

const R14_ROUNDTRIP: &str = "ACADR14ROUNDTRIP";
const DSTYLE: &str = "DSTYLE";
const UCS_GROUP: &str = "UCS";
const VIEW_GROUP: &str = "VIEW";

/// Whether a dimension style of `version` holds DIMDSEP, DIMBLK and DIMLWD itself.
pub(crate) fn dim_style_holds(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1015
}

/// Whether a viewport, VPORT and view of `version` hold their UCS and render mode.
pub(crate) fn ucs_holds(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1015
}

/// The name R14 states for an arrowhead block (DIMBLK): AutoCAD's own R14 file states `BOXBLANK` for the block `_BoxBlank` both in the
/// header and in the dimension style (sample_AC1014.dwg, G5-RC-S012). The sample shows this one arrowhead only; another standard
/// arrowhead is stated under the name of its block as before (UNVERIFIED how R14 spells it).
pub(crate) fn r14_arrow_name(block: &str) -> Option<&'static str> {
    const NAMES: [(&str, &str); 1] = [("_BoxBlank", "BOXBLANK")];
    NAMES.iter().find(|(name, _)| name.eq_ignore_ascii_case(block)).map(|(_, r14)| *r14)
}

fn dim_style_defaults() -> crate::tables::DimStyle {
    crate::tables::DimStyle::new("")
}

fn dim_style_groups(style: &crate::tables::DimStyle) -> Vec<Group> {
    let defaults = dim_style_defaults();
    let mut data: Vec<XRecordEntry> = Vec::new();
    if style.dimdsep != defaults.dimdsep {
        data.extend([XRecordEntry::int16(1070, 278), XRecordEntry::int16(70, style.dimdsep)]);
    }
    if !style.dimblk.is_null() {
        data.extend([XRecordEntry::int16(1070, 342), XRecordEntry::handle(340, style.dimblk)]);
    }
    if style.dimlwd != defaults.dimlwd {
        data.extend([XRecordEntry::int16(1070, 371), XRecordEntry::int16(70, style.dimlwd)]);
    }
    if data.is_empty() {
        return Vec::new();
    }
    let mut entries = vec![XRecordEntry::string(1000, DSTYLE), XRecordEntry::string(1002, "{")];
    entries.extend(data);
    entries.push(XRecordEntry::string(1002, "}"));
    vec![Group::new(R14_ROUNDTRIP, entries)]
}

/// The variables a `DSTYLE` group states: `(group code, value)`; `None` when the group is not of the shape AutoCAD writes (a variable
/// no sample shows, a value of another type, a repeated variable).
fn dim_style_values(group: &Group) -> Option<Vec<(i16, XRecordValue)>> {
    let entries = group.entries.as_slice();
    let is = |entry: &XRecordEntry, code: i32, text: &str| entry.code == code && entry.value.as_string() == Some(text);
    let (open, rest) = entries.split_first()?;
    let (head, rest) = rest.split_first()?;
    let (close, pairs) = rest.split_last()?;
    if !is(open, 1000, DSTYLE) || !is(head, 1002, "{") || !is(close, 1002, "}") || pairs.len() % 2 != 0 {
        return None;
    }
    let mut values: Vec<(i16, XRecordValue)> = Vec::new();
    for pair in pairs.chunks(2) {
        let (name, value) = (&pair[0], &pair[1]);
        let XRecordValue::Int16(code) = name.value else { return None };
        if name.code != 1070 {
            return None;
        }
        match (code, value.code, &value.value) {
            (278 | 371, 70, XRecordValue::Int16(_)) => {}
            (342, 340, XRecordValue::Handle(handle)) if !handle.is_null() => {}
            _ => return None,
        }
        if values.iter().any(|(seen, _)| *seen == code) {
            return None;
        }
        values.push((code, value.value.clone()));
    }
    (!values.is_empty()).then_some(values)
}

/// Restore DIMDSEP, DIMBLK and DIMLWD of a dimension style from its `ACADR14ROUNDTRIP` group; the groups applied.
fn restore_dim_style(style: &mut crate::tables::DimStyle, groups: &[Group]) -> Vec<String> {
    let mut applied = Vec::new();
    for group in groups.iter().filter(|group| group.name == R14_ROUNDTRIP) {
        let Some(values) = dim_style_values(group) else { continue };
        for (code, value) in values {
            match (code, value) {
                (278, XRecordValue::Int16(value)) => style.dimdsep = value,
                (371, XRecordValue::Int16(value)) => style.dimlwd = value,
                (342, XRecordValue::Handle(handle)) => style.dimblk = handle,
                _ => {}
            }
        }
        applied.push(group.name.clone());
    }
    applied
}

/// What ACadSharp reads of a dimension style of a file of its own version that lacks DIMDSEP, DIMBLK and DIMLWD (R14): the defaults.
#[cfg(feature = "acadsharp-compat")]
pub(crate) fn unrestored_dim_style(document: &CadDocument, style: &crate::tables::DimStyle) -> Option<crate::tables::DimStyle> {
    if !masked(document, dim_style_holds) {
        return None;
    }
    let defaults = dim_style_defaults();
    let mut shown = style.clone();
    shown.dimdsep = defaults.dimdsep;
    shown.dimlwd = defaults.dimlwd;
    shown.dimblk = defaults.dimblk;
    Some(shown)
}

/// The UCS of a viewport, a VPORT or a view as the groups of its round-trip record: `head` is the entries before the UCS origin.
fn ucs_groups(head: Vec<XRecordEntry>, origin: crate::types::Vector3, x_axis: crate::types::Vector3, y_axis: crate::types::Vector3, elevation: f64, ortho: i16, render_mode: i16) -> Vec<Group> {
    let mut entries = head;
    entries.extend([
        XRecordEntry::point3d(10, origin.x, origin.y, origin.z),
        XRecordEntry::point3d(10, x_axis.x, x_axis.y, x_axis.z),
        XRecordEntry::point3d(10, y_axis.x, y_axis.y, y_axis.z),
        XRecordEntry::double(146, elevation),
        XRecordEntry::int16(79, ortho),
    ]);
    vec![Group::new(UCS_GROUP, entries), Group::new(VIEW_GROUP, vec![XRecordEntry::new(281, XRecordValue::Byte(render_mode as u8))])]
}

/// The values of a `UCS` and a `VIEW` group: the head values (0 or 1), the origin, x axis and y axis, the elevation, the orthographic
/// type and the render mode; `None` for groups that do not have the shape AutoCAD writes (`head_codes` the codes before the origin).
type UcsValues = (Vec<i16>, [crate::types::Vector3; 3], f64, i16, i16);

fn ucs_values(groups: &[Group], head_codes: &[i32]) -> Option<UcsValues> {
    let ucs = groups.iter().find(|group| group.name == UCS_GROUP)?;
    let view = groups.iter().find(|group| group.name == VIEW_GROUP)?;
    let entries = ucs.entries.as_slice();
    if entries.len() != head_codes.len() + 5 {
        return None;
    }
    let mut head = Vec::new();
    for (entry, code) in entries.iter().zip(head_codes) {
        match &entry.value {
            XRecordValue::Int16(value) if entry.code == *code && matches!(*value, 0 | 1) => head.push(*value),
            _ => return None,
        }
    }
    let rest = &entries[head_codes.len()..];
    let point = |entry: &XRecordEntry| match &entry.value {
        XRecordValue::Point3D(x, y, z) if entry.code == 10 => Some(crate::types::Vector3::new(*x, *y, *z)),
        _ => None,
    };
    let origin = point(&rest[0])?;
    let x_axis = point(&rest[1])?;
    let y_axis = point(&rest[2])?;
    let elevation = (rest[3].code == 146).then(|| as_f64(&rest[3].value)).flatten()?;
    let ortho = match (&rest[4].value, rest[4].code) {
        (XRecordValue::Int16(value), 79) => *value,
        _ => return None,
    };
    let render = match view.entries.as_slice() {
        [entry] if entry.code == 281 => as_i32(&entry.value).filter(|mode| (0..=6).contains(mode))? as i16,
        _ => return None,
    };
    Some((head, [origin, x_axis, y_axis], elevation, ortho, render))
}

fn viewport_ucs_wanted(viewport: &crate::entities::Viewport) -> Vec<Group> {
    ucs_groups(
        vec![XRecordEntry::int16(74, viewport.ucs_at_origin as i16), XRecordEntry::int16(71, viewport.ucs_per_viewport as i16)],
        viewport.ucs_origin,
        viewport.ucs_x_axis,
        viewport.ucs_y_axis,
        viewport.elevation,
        viewport.ucs_ortho_type,
        viewport.render_mode.to_value(),
    )
}

fn vport_ucs_wanted(vport: &crate::tables::VPort) -> Vec<Group> {
    ucs_groups(
        vec![XRecordEntry::int16(74, vport.ucs_at_origin as i16), XRecordEntry::int16(71, vport.ucs_per_viewport as i16)],
        vport.ucs_origin,
        vport.ucs_x_axis,
        vport.ucs_y_axis,
        vport.ucs_elevation,
        vport.ucs_ortho_type,
        vport.render_mode.to_value(),
    )
}

fn view_ucs_wanted(view: &crate::tables::View) -> Vec<Group> {
    ucs_groups(
        vec![XRecordEntry::int16(72, view.ucs_associated as i16)],
        view.ucs_origin,
        view.ucs_x_axis,
        view.ucs_y_axis,
        view.ucs_elevation,
        view.ucs_ortho_type,
        view.render_mode.to_value(),
    )
}

fn restore_viewport_ucs(viewport: &mut crate::entities::Viewport, groups: &[Group]) -> Vec<String> {
    let Some((head, [origin, x_axis, y_axis], elevation, ortho, render)) = ucs_values(groups, &[74, 71]) else { return Vec::new() };
    viewport.ucs_at_origin = head[0] != 0;
    viewport.ucs_per_viewport = head[1] != 0;
    viewport.ucs_origin = origin;
    viewport.ucs_x_axis = x_axis;
    viewport.ucs_y_axis = y_axis;
    viewport.elevation = elevation;
    viewport.ucs_ortho_type = ortho;
    viewport.render_mode = crate::entities::ViewportRenderMode::from_value(render);
    vec![UCS_GROUP.to_string(), VIEW_GROUP.to_string()]
}

fn restore_vport_ucs(vport: &mut crate::tables::VPort, groups: &[Group]) -> Vec<String> {
    let Some((head, [origin, x_axis, y_axis], elevation, ortho, render)) = ucs_values(groups, &[74, 71]) else { return Vec::new() };
    vport.ucs_at_origin = head[0] != 0;
    vport.ucs_per_viewport = head[1] != 0;
    vport.ucs_origin = origin;
    vport.ucs_x_axis = x_axis;
    vport.ucs_y_axis = y_axis;
    vport.ucs_elevation = elevation;
    vport.ucs_ortho_type = ortho;
    vport.render_mode = crate::entities::ViewportRenderMode::from_value(render);
    vec![UCS_GROUP.to_string(), VIEW_GROUP.to_string()]
}

fn restore_view_ucs(view: &mut crate::tables::View, groups: &[Group]) -> Vec<String> {
    let Some((head, [origin, x_axis, y_axis], elevation, ortho, render)) = ucs_values(groups, &[72]) else { return Vec::new() };
    view.ucs_associated = head[0] != 0;
    view.ucs_origin = origin;
    view.ucs_x_axis = x_axis;
    view.ucs_y_axis = y_axis;
    view.ucs_elevation = elevation;
    view.ucs_ortho_type = ortho;
    view.render_mode = crate::entities::ViewportRenderMode::from_value(render);
    vec![UCS_GROUP.to_string(), VIEW_GROUP.to_string()]
}

/// What ACadSharp reads of the UCS and render mode of a VPORT, a view and a viewport of a file of its own version that lacks them
/// (R14): the defaults.
#[cfg(feature = "acadsharp-compat")]
pub(crate) fn unrestored_vport_ucs(document: &CadDocument, vport: &mut crate::tables::VPort) {
    if !masked(document, ucs_holds) {
        return;
    }
    let defaults = crate::tables::VPort::new(vport.name.clone());
    vport.ucs_at_origin = defaults.ucs_at_origin;
    vport.ucs_per_viewport = defaults.ucs_per_viewport;
    vport.ucs_origin = defaults.ucs_origin;
    vport.ucs_x_axis = defaults.ucs_x_axis;
    vport.ucs_y_axis = defaults.ucs_y_axis;
    vport.ucs_elevation = defaults.ucs_elevation;
    vport.ucs_ortho_type = defaults.ucs_ortho_type;
    vport.render_mode = defaults.render_mode;
}

#[cfg(feature = "acadsharp-compat")]
pub(crate) fn unrestored_view_ucs(document: &CadDocument, view: &mut crate::tables::View) {
    if !masked(document, ucs_holds) {
        return;
    }
    let defaults = crate::tables::View::new(view.name.clone());
    view.ucs_associated = defaults.ucs_associated;
    view.ucs_origin = defaults.ucs_origin;
    view.ucs_x_axis = defaults.ucs_x_axis;
    view.ucs_y_axis = defaults.ucs_y_axis;
    view.ucs_elevation = defaults.ucs_elevation;
    view.ucs_ortho_type = defaults.ucs_ortho_type;
    view.render_mode = defaults.render_mode;
}

#[cfg(feature = "acadsharp-compat")]
pub(crate) fn unrestored_viewport_ucs(document: &CadDocument, viewport: &mut crate::entities::Viewport) {
    if !masked(document, ucs_holds) {
        return;
    }
    let defaults = crate::entities::Viewport::new();
    viewport.ucs_at_origin = defaults.ucs_at_origin;
    viewport.ucs_per_viewport = defaults.ucs_per_viewport;
    viewport.ucs_origin = defaults.ucs_origin;
    viewport.ucs_x_axis = defaults.ucs_x_axis;
    viewport.ucs_y_axis = defaults.ucs_y_axis;
    viewport.elevation = defaults.elevation;
    viewport.ucs_ortho_type = defaults.ucs_ortho_type;
    viewport.render_mode = defaults.render_mode;
}

// ---------------------------------------------------------------------------------------------------------------------
// The header variables R14 has no place for (R14 DWG; R5b-12). AutoCAD's R14 file keeps them in the dictionary
// `ACDBHEADERROUNDTRIPXREC` of the root dictionary: one XRECORD named by the variable (not hard-owned, cloning flags 0, the records
// not cloned; 33 records in sample_AC1014.dwg, sorted by name). This is read and written for two of them, the ones the model has a
// field for and the sample shows a value of:
//
// * `INSUNITS {90 <units>}` (the sample: 1, the drawing's units), and
// * `DIMSTYLEDATA {1000 DSTYLE 1002 { (1070 <code> <value>)... 1002 }}` for the header's dimension variables R14's header lacks, in
//   the shape of a dimension style's `ACADR14ROUNDTRIP` group: DIMDSEP 278, DIMBLK 342 (the arrowhead's block record) and DIMLWD 371
//   (sample: `342 #C80, 371 15`, the values of the style custom_style_ltypes, which the header holds as the current ones).
//
// The others (TSTACKALIGN, TSTACKSIZE, HYPERLINKBASE, STYLESHEET, LAYOUTDICT, PLOTSETDICT, PLOTSTYLNAMDICT, LWETCUNION, CEPSNTYPE,
// FINGERPRINTGUID, VERSIONGUID, PSVPSCALE and the 14 `PUCS*` / `UCS*` records) are not read and not written: the mapping of
// `LWETCUNION` (10781 in the sample, a packed word) to header variables is in no sample, and the rest are not part of the model's
// header or are derived. They stay in the model as the records a reader found.
// ---------------------------------------------------------------------------------------------------------------------

const HEADER_ROUNDTRIP: &str = "ACDBHEADERROUNDTRIPXREC";
const INSUNITS: &str = "INSUNITS";
const DIMSTYLEDATA: &str = "DIMSTYLEDATA";

/// Whether a header of `version` holds INSUNITS and the dimension variables above itself.
pub(crate) fn header_roundtrip_holds(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1015
}

/// The records (name, entries) AutoCAD states for a header.
fn header_roundtrip_wanted(header: &crate::document::HeaderVariables) -> Vec<(&'static str, Vec<XRecordEntry>)> {
    let defaults = crate::document::HeaderVariables::default();
    let mut data: Vec<XRecordEntry> = Vec::new();
    if header.dim_decimal_separator != defaults.dim_decimal_separator {
        data.extend([XRecordEntry::int16(1070, 278), XRecordEntry::int16(70, header.dim_decimal_separator as u32 as i16)]);
    }
    if !header.dim_arrow_block_handle.is_null() {
        data.extend([XRecordEntry::int16(1070, 342), XRecordEntry::handle(340, header.dim_arrow_block_handle)]);
    }
    if header.dim_line_weight != defaults.dim_line_weight {
        data.extend([XRecordEntry::int16(1070, 371), XRecordEntry::int16(70, header.dim_line_weight)]);
    }
    let mut wanted = Vec::new();
    if !data.is_empty() {
        let mut entries = vec![XRecordEntry::string(1000, DSTYLE), XRecordEntry::string(1002, "{")];
        entries.extend(data);
        entries.push(XRecordEntry::string(1002, "}"));
        wanted.push((DIMSTYLEDATA, entries));
    }
    wanted.push((INSUNITS, vec![XRecordEntry::int32(90, header.insertion_units as i32)]));
    wanted
}

/// The record `key` of the header chain of the document, if it has one.
fn header_roundtrip_record(document: &CadDocument, key: &str) -> Option<Handle> {
    let root = document.header.named_objects_dict_handle;
    let chain = dictionary_at(document, root).and_then(|root| entry_ci(root, HEADER_ROUNDTRIP))?;
    let record = dictionary_at(document, chain)?.get(key)?;
    matches!(document.objects.get(&record), Some(ObjectType::XRecord(_))).then_some(record)
}

fn has_header_roundtrip(document: &CadDocument) -> bool {
    let root = document.header.named_objects_dict_handle;
    dictionary_at(document, root).is_some_and(|root| entry_ci(root, HEADER_ROUNDTRIP).is_some())
}

/// Restore INSUNITS and the dimension variables of the header from the chain; the records applied.
fn restore_header_roundtrip(document: &mut CadDocument) -> Vec<Handle> {
    let mut applied = Vec::new();
    if let Some(record) = header_roundtrip_record(document, INSUNITS) {
        if let Some([entry]) = record_entries(document, record) {
            if let (90, XRecordValue::Int32(units)) = (entry.code, &entry.value) {
                if let Ok(units) = i16::try_from(*units) {
                    document.header.insertion_units = units;
                    applied.push(record);
                }
            }
        }
    }
    if let Some(record) = header_roundtrip_record(document, DIMSTYLEDATA) {
        let group = Group::new(R14_ROUNDTRIP, record_entries(document, record).map(<[XRecordEntry]>::to_vec).unwrap_or_default());
        if let Some(values) = dim_style_values(&group) {
            for (code, value) in values {
                match (code, value) {
                    (278, XRecordValue::Int16(value)) => document.header.dim_decimal_separator = char::from_u32(value as u16 as u32).unwrap_or('.'),
                    (371, XRecordValue::Int16(value)) => document.header.dim_line_weight = value,
                    (342, XRecordValue::Handle(handle)) => document.header.dim_arrow_block_handle = handle,
                    _ => {}
                }
            }
            applied.push(record);
        }
    }
    applied
}

/// State the chain records from the header: a record is replaced, made, or removed when the header no longer needs it.
fn state_header_roundtrip(document: &mut CadDocument, version: DxfVersion, wanted: Vec<(&'static str, Vec<XRecordEntry>)>, states: &[&'static str]) {
    let root = document.header.named_objects_dict_handle;
    let existing = dictionary_at(document, root).and_then(|root| entry_ci(root, HEADER_ROUNDTRIP)).filter(|handle| dictionary_at(document, *handle).is_some());
    if wanted.is_empty() {
        if let Some(chain) = existing {
            for key in states {
                set_record(document, chain, key, None, false, version);
            }
            remove_if_empty(document, root, HEADER_ROUNDTRIP, chain);
        }
        return;
    }
    let chain = ensure_dictionary(document, root, HEADER_ROUNDTRIP, false, version);
    for key in states {
        let entries = wanted.iter().find(|(name, _)| name == key).map(|(_, entries)| entries.clone());
        set_record(document, chain, key, entries, false, version);
    }
}

#[cfg(test)]
mod mleader_tests {
    use super::*;
    use crate::entities::multileader::{LeaderRoot, TextAttachmentDirectionType as Direction, TextAttachmentType as Attach};
    use crate::entities::{EntityType, MultiLeader};

    /// AutoCAD's records of the vertical multileader #B27 (sample_AC1014.dwg to sample_AC1021.dwg and the DXF twins).
    fn autocad_entries() -> Vec<XRecordEntry> {
        join(&[
            Group::new(
                MLEADER_CONTEXT,
                vec![XRecordEntry::int16(272, 1), XRecordEntry::int16(174, 0), XRecordEntry::int16(175, 8), XRecordEntry::int32(91, 0), XRecordEntry::double(40, 0.0)],
            ),
            Group::new(MLEADER_2010, vec![XRecordEntry::int16(272, 1), XRecordEntry::int16(173, 0), XRecordEntry::int16(174, 8), XRecordEntry::double(41, 8.0)]),
        ])
    }

    /// The multileader as AutoCAD's R2004 file states it (sample_AC1018.dwg: left attachment 4, right 0, no dogleg, horizontal) with its
    /// round-trip records; `read` has the reader's work done.
    fn drawing(read: bool) -> (CadDocument, Handle) {
        let mut document = CadDocument::with_version(DxfVersion::AC1018);
        document.dwg_source_version = Some(DxfVersion::AC1018);
        let mut mleader = MultiLeader::new();
        mleader.context.leader_roots = vec![LeaderRoot::new(0)];
        mleader.text_left_attachment = Attach::BottomOfBottomLine;
        mleader.text_right_attachment = Attach::TopOfTopLine;
        mleader.context.text_left_attachment = Attach::BottomOfBottomLine;
        mleader.context.text_right_attachment = Attach::TopOfTopLine;
        mleader.dogleg_length = 0.0;
        let handle = document.add_entity(EntityType::MultiLeader(Box::new(mleader))).unwrap();
        let record = document.ensure_xrecord(handle, KEY);
        let Some(ObjectType::XRecord(record)) = document.objects.get_mut(&record) else { panic!("record") };
        record.entries = autocad_entries();
        record.entries_complete = true;
        if read {
            restore_document(&mut document);
        }
        (document, handle)
    }

    fn mleader(document: &CadDocument, handle: Handle) -> MultiLeader {
        match document.get_entity(handle) {
            Some(EntityType::MultiLeader(mleader)) => (**mleader).clone(),
            _ => panic!("no multileader"),
        }
    }

    fn words(mleader: &MultiLeader) -> (Direction, Direction, i16, i16, i16, i16, f64) {
        (
            mleader.text_attachment_direction,
            mleader.context.leader_roots[0].text_attachment_direction,
            mleader.text_left_attachment as i16,
            mleader.text_right_attachment as i16,
            mleader.context.text_left_attachment as i16,
            mleader.context.text_right_attachment as i16,
            mleader.dogleg_length,
        )
    }

    fn record_entries_of(document: &CadDocument, handle: Handle) -> Option<Vec<XRecordEntry>> {
        document.xrecord(handle, KEY).map(|record| record.entries.clone())
    }

    /// The ACadSharp view of a restored multileader shows the words the file's entity states (ACadSharp does not read the records into
    /// the entity): those of the same drawing before the reader's work (review of R5b-15: the view had no test).
    #[cfg(feature = "acadsharp-compat")]
    #[test]
    fn the_acadsharp_view_of_a_restored_multileader_shows_the_file_words() {
        let shown = |document: &CadDocument| {
            let dump = crate::compat::acadsharp::dump_document(document, "", "new");
            let props = dump["objects"].as_array().unwrap().iter().find(|object| object["type"] == "MultiLeader").expect("a multileader")["props"].clone();
            ["TextAttachmentDirection", "TextLeftAttachment", "TextRightAttachment", "LandingDistance"].map(|key| props[key].clone())
        };
        let (file, _) = drawing(false);
        let (read, _) = drawing(true);
        assert_eq!(shown(&read), shown(&file));
        assert_ne!(shown(&read)[1], serde_json::json!(0), "not the restored left attachment");
    }

    /// The reader restores the words the records state and takes the records out of the model; the ACadSharp view and the writers before
    /// R2010 state the legacy words of the file.
    #[test]
    fn the_records_of_a_vertical_multileader_are_read_and_the_file_words_stay_in_the_view() {
        let (document, handle) = drawing(true);
        assert_eq!(words(&mleader(&document, handle)), (Direction::Vertical, Direction::Vertical, 0, 8, 0, 8, 8.0));
        assert!(record_entries_of(&document, handle).is_none(), "the record is out of the model");
        let shown = unrestored_mleader(&document, &mleader(&document, handle)).expect("a restored multileader");
        assert_eq!(words(&shown), (Direction::Horizontal, Direction::Horizontal, 4, 0, 4, 0, 0.0), "what ACadSharp reads");
        // an edit after the read wins, word by word
        let mut edited = document.clone();
        let Some(EntityType::MultiLeader(target)) = edited.get_entity_mut(handle) else { panic!() };
        target.text_attachment_direction = Direction::Horizontal;
        target.dogleg_length = 5.0;
        let shown = unrestored_mleader(&edited, &mleader(&edited, handle)).unwrap();
        assert_eq!(words(&shown), (Direction::Horizontal, Direction::Horizontal, 4, 0, 4, 0, 5.0));
    }

    /// Written again before R2010 the record is AutoCAD's and the entity states the legacy words; written for R2010 and later the record is
    /// gone and the entity states the restored words; an edit that makes the multileader horizontal drops the record.
    #[test]
    fn a_vertical_multileader_states_the_records_before_r2010_and_not_after() {
        let (document, handle) = drawing(true);
        for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1021] {
            for dxf in [false, true] {
                let mut prepared = std::borrow::Cow::Borrowed(&document);
                prepare_for_write(&mut prepared, version, dxf);
                assert_eq!(record_entries_of(&prepared, handle).unwrap(), autocad_entries(), "{version:?} dxf {dxf}");
                assert_eq!(words(&mleader(&prepared, handle)), (Direction::Horizontal, Direction::Horizontal, 4, 0, 4, 0, 0.0), "{version:?} dxf {dxf}");
            }
        }
        for dxf in [false, true] {
            let mut prepared = std::borrow::Cow::Borrowed(&document);
            prepare_for_write(&mut prepared, DxfVersion::AC1024, dxf);
            assert!(record_entries_of(&prepared, handle).is_none_or(|entries| entries.is_empty()), "dxf {dxf}: R2010 states the words itself");
            assert_eq!(words(&mleader(&prepared, handle)), (Direction::Vertical, Direction::Vertical, 0, 8, 0, 8, 8.0));
        }
        let mut edited = document.clone();
        let Some(EntityType::MultiLeader(target)) = edited.get_entity_mut(handle) else { panic!() };
        target.text_attachment_direction = Direction::Horizontal;
        target.context.leader_roots[0].text_attachment_direction = Direction::Horizontal;
        let mut prepared = std::borrow::Cow::Borrowed(&edited);
        prepare_for_write(&mut prepared, DxfVersion::AC1018, false);
        assert!(record_entries_of(&prepared, handle).is_none_or(|entries| entries.is_empty()), "a horizontal multileader has no record");
        // without the record the legacy words would replace the multileader's own attachments and dogleg length, which the entity holds
        assert_eq!(words(&mleader(&prepared, handle)), (Direction::Horizontal, Direction::Horizontal, 0, 8, 0, 8, 8.0), "its own words");
        assert!(unrestored_mleader(&edited, &mleader(&edited, handle)).is_none(), "the view shows its own words too");
    }

    /// A drawing of R2010 or later (no consumed record) gets the records AutoCAD writes for a vertical multileader when it is saved
    /// before R2010, none for a horizontal one; a record of another shape is another producer's data and is neither read nor changed.
    #[test]
    fn a_down_save_states_the_records_for_a_vertical_multileader_only() {
        let (mut document, handle) = drawing(true);
        document.consumed_records = Consumed::default();
        document.dwg_source_version = Some(DxfVersion::AC1032);
        document.version = DxfVersion::AC1032;
        let record = document.xrecord(handle, KEY);
        assert!(record.is_none(), "the records were taken out of the model by the read above");
        let mut prepared = std::borrow::Cow::Borrowed(&document);
        prepare_for_write(&mut prepared, DxfVersion::AC1018, false);
        assert_eq!(record_entries_of(&prepared, handle).unwrap(), autocad_entries(), "converted from R2018");
        let (horizontal, other) = {
            let mut horizontal = document.clone();
            let Some(EntityType::MultiLeader(target)) = horizontal.get_entity_mut(handle) else { panic!() };
            target.text_attachment_direction = Direction::Horizontal;
            target.context.leader_roots[0].text_attachment_direction = Direction::Horizontal;
            (horizontal, handle)
        };
        let mut prepared = std::borrow::Cow::Borrowed(&horizontal);
        prepare_for_write(&mut prepared, DxfVersion::AC1018, false);
        assert!(record_entries_of(&prepared, other).is_none(), "no record for a horizontal multileader");
        // the same groups with another value in the part the sample states as 0: not read
        let (mut foreign, handle) = drawing(false);
        let record = foreign.ensure_xrecord(handle, KEY);
        let Some(ObjectType::XRecord(xrecord)) = foreign.objects.get_mut(&record) else { panic!() };
        xrecord.entries[4] = XRecordEntry::int32(91, 3);
        let before = xrecord.entries.clone();
        restore_document(&mut foreign);
        assert_eq!(words(&mleader(&foreign, handle)).0, Direction::Vertical, "the entity group has the shape and is read");
        let left = record_entries_of(&foreign, handle).unwrap();
        assert_eq!(left, before[..6].to_vec(), "only the group of another shape stays, as it was");
    }
}
