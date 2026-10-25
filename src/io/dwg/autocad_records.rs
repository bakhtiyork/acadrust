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
//!
//! (The spline's `ACAD_SPLINE_DATA` is extended data: `autocad_conversions`. The R14 key of a table style's cell style map and the
//! TABLECONTENT / TABLEGEOMETRY objects of a table: `objects::table_style`, `entities::table`.)
//!
//! The native model holds the values themselves. A reader restores them from the record (the record stays an object of the
//! document, so the ACadSharp view of the file is unchanged: the view shows what ACadSharp reads, see `masked`); the writer
//! states the groups from the native values for the versions that need them, replacing an earlier copy and keeping any other
//! group; for a version that holds the values itself the writer drops the groups it applied, and a record that holds anything
//! else stays. A group that does not have the shape AutoCAD writes is another producer's data: it is neither read nor changed.
//! A drawing converted from another version (or made in memory) gets every group; one written again at the version it was read
//! from keeps the groups its record has (and, for a DWG, makes a group only for a value that differs from the default the
//! reader gives a file without the record).
//!
//! A DXF holds a record only where AutoCAD's own DXF of the version does (owner decision 2026-10-24): R2000 to R2013 view styles,
//! materials, dimension linetypes and multi-line attributes, and the layout's and the viewport's shade plot at R2000. For the
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

    /// The XRECORD `key` of the extension dictionary of `owner`, made when it is missing.
    fn ensure_record(&mut self, document: &mut CadDocument, owner: Handle, key: &str) -> Handle {
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
        document.objects.insert(handle, ObjectType::XRecord(record));
        if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&dictionary) {
            if let Some((_, target)) = entries.entries.iter_mut().find(|(name, _)| name.eq_ignore_ascii_case(key)) {
                *target = handle;
            } else {
                entries.add_entry(key, handle);
            }
        }
        handle
    }

    fn forget(&mut self, owner: Handle) {
        self.by_owner.remove(&owner);
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
fn state(document: &mut CadDocument, ext: &mut Ext, owner: Handle, owns: &dyn Fn(&str) -> bool, wanted: Vec<Group>, create: bool) {
    let existing = record_of(document, ext, owner);
    let Some(record) = existing.or_else(|| (create && !wanted.is_empty()).then(|| ext.ensure_record(document, owner, KEY))) else {
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

/// Remove an empty record and, when it was the only entry, the extension dictionary that held it.
fn remove_empty(document: &mut CadDocument, ext: &mut Ext, owner: Handle, record: Handle) {
    let Some(dictionary) = ext.of(document, owner) else {
        return;
    };
    let mut now_empty = false;
    if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&dictionary) {
        entries.entries.retain(|(_, handle)| *handle != record);
        entries.hard_owner_entries.retain(|name| entries.entries.iter().any(|(entry, _)| entry == name));
        now_empty = entries.entries.is_empty();
    }
    document.objects.remove(&record);
    if now_empty {
        document.objects.remove(&dictionary);
        document.xdic_by_handle.remove(&owner);
        ext.forget(owner);
        match document.objects.get_mut(&owner) {
            Some(ObjectType::ClassObject(object)) => object.xdictionary_handle = None,
            Some(ObjectType::Layout(object)) => object.xdictionary_handle = None,
            Some(ObjectType::Material(object)) => object.xdictionary_handle = None,
            _ => {}
        }
    }
}

/// Whether writing the document as `version` is a conversion: the file it was read from, if any, is of another version.
fn converts(document: &CadDocument, version: DxfVersion) -> bool {
    !document.is_source_version(version)
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

fn restore_view_style(base: &mut crate::objects::ModelDocViewStyle, groups: &[Group]) {
    for group in groups {
        match group.name.as_str() {
            DISPLAY_NAME if group.entries.len() == 1 => {
                if let Some(XRecordValue::String(name)) = at(group, 0, 1) {
                    base.display_name = name.clone();
                }
            }
            FLAGS if group.entries.len() == 1 => {
                if let Some(flags) = at(group, 0, 90).and_then(as_i32) {
                    base.flags = flags;
                }
            }
            _ => {}
        }
    }
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

fn restore_material(material: &mut crate::objects::Material, groups: &[Group]) {
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
    }
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

fn restore_layout(layout: &mut crate::objects::Layout, groups: &[Group]) {
    let value = |group: &Group| match group.entries.as_slice() {
        [entry] if entry.code == 70 => as_i32(&entry.value).and_then(|v| i16::try_from(v).ok()),
        _ => None,
    };
    for group in groups {
        match (group.name.as_str(), value(group)) {
            (SHADE_PLOT, Some(v)) => layout.shade_plot_mode = v,
            (SHADE_PLOT_RESOLUTION, Some(v)) => layout.shade_plot_resolution = v,
            (SHADE_PLOT_DPI, Some(v)) => layout.shade_plot_dpi = v,
            _ => {}
        }
    }
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

    /// Take the groups named `<prefix><name>` of the shape AutoCAD writes.
    fn restore(&mut self, groups: &[Group], prefix: &str) {
        for group in groups {
            let Some(name) = group.name.strip_prefix(prefix) else {
                continue;
            };
            let [entry] = group.entries.as_slice() else {
                continue;
            };
            match (name, entry.code) {
                (VISUAL_STYLE, 340) => {
                    if let Some(handle) = entry.value.as_handle() {
                        self.visual_style = handle;
                    }
                }
                (GRID_DISPLAY, 70) => self.grid = as_i32(&entry.value).and_then(|v| i16::try_from(v).ok()).unwrap_or(self.grid),
                (GRID_MAJOR, 70) => self.major = as_i32(&entry.value).and_then(|v| i16::try_from(v).ok()).unwrap_or(self.major),
                (DEFAULT_LIGHTING, 280) => self.lighting = as_i32(&entry.value).map_or(self.lighting, |v| v != 0),
                (DEFAULT_LIGHTING_TYPE, 70) => {
                    self.lighting_type = as_i32(&entry.value).and_then(|v| i16::try_from(v).ok()).unwrap_or(self.lighting_type)
                }
                (BRIGHTNESS, 141) => self.brightness = as_f64(&entry.value).unwrap_or(self.brightness),
                (CONTRAST, 142) => self.contrast = as_f64(&entry.value).unwrap_or(self.contrast),
                _ => {}
            }
        }
    }
}

fn restore_vport_display(vport: &mut crate::tables::VPort, groups: &[Group], prefix: &str) {
    let mut display = Display::of_vport(vport);
    display.restore(groups, prefix);
    vport.visual_style_handle = display.visual_style;
    vport.grid_flags = crate::entities::GridFlags::from_bits(display.grid);
    vport.grid_major = display.major;
    vport.use_default_lights = display.lighting;
    vport.default_lighting_type = display.lighting_type;
    vport.brightness = display.brightness;
    vport.contrast = display.contrast;
}

fn restore_viewport(viewport: &mut crate::entities::Viewport, groups: &[Group], version: DxfVersion) {
    if !display_holds(version) {
        let mut display = Display::of_viewport(viewport);
        display.restore(groups, "");
        viewport.visual_style_handle = display.visual_style;
        viewport.grid_flags = crate::entities::GridFlags::from_bits(display.grid);
        viewport.grid_major = display.major;
        viewport.default_lighting = display.lighting;
        viewport.default_lighting_type = display.lighting_type;
        viewport.brightness = display.brightness;
        viewport.contrast = display.contrast;
    }
    if !shade_plot_holds(version) {
        for group in groups.iter().filter(|group| group.name == SHADE_PLOT) {
            if let [entry] = group.entries.as_slice() {
                if let (70, Some(mode)) = (entry.code, as_i32(&entry.value).and_then(|v| i16::try_from(v).ok())) {
                    viewport.shade_plot_mode = mode;
                }
            }
        }
    }
}

/// The VPORT group prefix of AutoCAD: the table's control object holds one set per entry, named by its extents.
fn vport_prefix(vport: &crate::tables::VPort) -> String {
    format!("VTR_{:.3}_{:.3}_{:.3}_{:.3}_", vport.lower_left.x, vport.lower_left.y, vport.upper_right.x, vport.upper_right.y)
}

/// What the readers leave in the display fields of a VPORT, view or VIEWPORT entity, for the ACadSharp view.
#[cfg(feature = "acadsharp-compat")]
pub(crate) fn unrestored_vport(document: &CadDocument, vport: &crate::tables::VPort) -> Option<crate::tables::VPort> {
    if !masked(document, display_holds) {
        return None;
    }
    let defaults = crate::tables::VPort::new(vport.name.clone());
    let mut shown = vport.clone();
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
    if !masked(document, display_holds) {
        return None;
    }
    let defaults = crate::tables::View::new(view.name.clone());
    let mut shown = view.clone();
    shown.visual_style_handle = defaults.visual_style_handle;
    shown.camera_plottable = defaults.camera_plottable;
    Some(shown)
}

#[cfg(feature = "acadsharp-compat")]
pub(crate) fn unrestored_viewport(document: &CadDocument, viewport: &crate::entities::Viewport) -> Option<crate::entities::Viewport> {
    let display = masked(document, display_holds);
    let shade = masked(document, shade_plot_holds);
    if !display && !shade {
        return None;
    }
    let defaults = crate::entities::Viewport::new();
    let mut shown = viewport.clone();
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

fn restore_solid(solid: &mut crate::entities::Solid3D, groups: &[Group]) {
    for group in groups.iter().filter(|group| group.name == SOLID_HISTORY) {
        if let [entry] = group.entries.as_slice() {
            if let (360, Some(handle)) = (entry.code, entry.value.as_handle()) {
                solid.history_handle = Some(handle);
            }
        }
    }
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
        dictionary.add_entry(key, handle);
        if hard {
            dictionary.set_entry_hard_owner(key, true);
        }
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
                parent.add_entry(key, handle);
                if hard {
                    parent.set_entry_hard_owner(key, true);
                }
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

fn restore_header_vars(document: &mut CadDocument) {
    let root = document.header.named_objects_dict_handle;
    let Some(dictionary) = dictionary_at(document, root).and_then(|dictionary| entry_ci(dictionary, HEADER_VARS)) else {
        return;
    };
    let mut found: [Option<(Handle, String)>; 3] = [None, None, None];
    for (slot, key) in found.iter_mut().zip(DIM_LINETYPE) {
        let Some(record) = dictionary_at(document, dictionary).and_then(|dictionary| entry_ci(dictionary, key)) else {
            continue;
        };
        if let Some([entry]) = record_entries(document, record) {
            if let (340, Some(handle)) = (entry.code, entry.value.as_handle()) {
                if let Some(name) = line_type_name(document, handle) {
                    *slot = Some((handle, name));
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

fn restore_block_data(document: &mut CadDocument, blocks: &[Handle]) {
    for owner in blocks {
        if let Some(block) = document.block_records.iter_mut().find(|block| block.handle == *owner) {
            block.scale_uniformly = true;
        }
    }
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
    // record, then the record of the MTEXT) and lists the entries by name.
    state(
        document,
        ext,
        multiline.owner,
        &|name| name == MATT_HEIGHT,
        vec![Group::new(MATT_HEIGHT, vec![XRecordEntry::double(140, multiline.height)])],
        true,
    );
    set_record(document, extension, MLATT, Some(multiline.entries.clone()), true, version);
    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get_mut(&extension).filter(|_| !existed) {
        let position = |dictionary: &crate::objects::Dictionary, name: &str| dictionary.entries.iter().position(|(entry, _)| entry == name);
        if let (Some(mlatt), Some(round_trip)) = (position(dictionary, MLATT), position(dictionary, KEY)) {
            if mlatt > round_trip {
                let entry = dictionary.entries.remove(mlatt);
                dictionary.entries.insert(round_trip, entry);
            }
        }
    }
}

/// Drop the record of an attribute whose MTEXT the version holds itself (or that has none any more).
fn drop_multiline(document: &mut CadDocument, ext: &mut Ext, owner: Handle, version: DxfVersion) {
    let Some(extension) = ext.of(document, owner) else {
        return;
    };
    set_record(document, extension, MLATT, None, true, version);
    state(document, ext, owner, &|name| name == MATT_HEIGHT, Vec::new(), false);
}

fn restore_multilines(document: &mut CadDocument, found: &std::collections::HashMap<Handle, Vec<XRecordEntry>>) {
    if found.is_empty() {
        return;
    }
    let apply = |attribute_flag: &mut crate::entities::MTextFlag,
                 multiline: &mut bool,
                 embedded: &mut Option<Box<crate::entities::MText>>,
                 tag: &str,
                 entries: &[XRecordEntry],
                 rotation: f64,
                 style: &str,
                 normal: crate::types::Vector3| {
        // The tag the record states is the attribute's own (a record of another tag was not made for this attribute).
        if let Some((flag, record_tag, text)) = mlatt_text(entries, rotation, style, normal) {
            if record_tag == tag && embedded.is_none() {
                *attribute_flag = flag;
                *multiline = true;
                *embedded = Some(Box::new(text));
            }
        }
    };
    for entity in document.entities_mut_keep_raw() {
        match entity {
            crate::entities::EntityType::AttributeDefinition(attribute) => {
                if let Some(entries) = found.get(&attribute.common.handle) {
                    let (rotation, style, normal) = (attribute.rotation, attribute.text_style.clone(), attribute.normal);
                    let tag = attribute.tag.clone();
                    apply(&mut attribute.mtext_flag, &mut attribute.is_multiline, &mut attribute.embedded_mtext, &tag, entries, rotation, &style, normal);
                }
            }
            crate::entities::EntityType::Insert(insert) => {
                for attribute in &mut insert.attributes {
                    if let Some(entries) = found.get(&attribute.common.handle) {
                        let (rotation, style, normal) = (attribute.rotation, attribute.text_style.clone(), attribute.normal);
                        let tag = attribute.tag.clone();
                        apply(&mut attribute.mtext_flag, &mut attribute.is_multiline, &mut attribute.embedded_mtext, &tag, entries, rotation, &style, normal);
                    }
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The passes
// ---------------------------------------------------------------------------------------------------------------------

/// What one look at the dictionaries of a document finds: the round-trip records (owner, record), the blocks with the
/// `AcDbBlockTableRecordData` chain AutoCAD writes for a block that scales uniformly, and the attributes with an `ACAD_MLATT` record
/// (owner -> its entries).
struct Scan {
    records: Vec<(Handle, Handle)>,
    blocks: Vec<Handle>,
    multilines: std::collections::HashMap<Handle, Vec<XRecordEntry>>,
}

fn scan(document: &CadDocument, want_blocks: bool, want_multilines: bool) -> Scan {
    let mut found = Scan { records: Vec::new(), blocks: Vec::new(), multilines: std::collections::HashMap::new() };
    for object in document.objects.values() {
        let ObjectType::Dictionary(dictionary) = object else { continue };
        if let Some(record) = dictionary.get(KEY).filter(|record| matches!(document.objects.get(record), Some(ObjectType::XRecord(_)))) {
            found.records.push((dictionary.owner, record));
        }
        if want_blocks {
            if let Some(edit) = entry_ci(dictionary, BLOCK_DATA).and_then(|data| dictionary_at(document, data)).and_then(|data| data.get(BLOCK_EDIT)) {
                if record_entries(document, edit).is_some_and(|entries| entries == block_edit_entries().as_slice()) {
                    found.blocks.push(dictionary.owner);
                }
            }
        }
        if want_multilines {
            if let Some(entries) = dictionary.get(MLATT).and_then(|record| record_entries(document, record)) {
                found.multilines.insert(dictionary.owner, entries.to_vec());
            }
        }
    }
    found.records.sort_by_key(|(owner, record)| (owner.value(), record.value()));
    found
}

/// Restore the native values from the records a reader found (the records stay in the document). A file holds the groups
/// only where its version needs them; a version that holds the values itself is left alone.
pub(crate) fn restore_document(document: &mut CadDocument) {
    let version = document.version;
    if version >= DxfVersion::AC1032 {
        return;
    }
    if !header_vars_holds(version) {
        restore_header_vars(document);
    }
    let found = scan(document, !block_data_holds(version), !mlatt_holds(version));
    restore_block_data(document, &found.blocks);
    restore_multilines(document, &found.multilines);
    let found = found.records;
    if found.is_empty() {
        return;
    }
    let vport_control = document.vports.handle();
    for (owner, record) in found {
        let groups = groups_of(document, record);
        if owner == vport_control && !display_holds(version) {
            for vport in document.vports.iter_mut() {
                let prefix = vport_prefix(vport);
                restore_vport_display(vport, &groups, &prefix);
            }
            continue;
        }
        match document.objects.get_mut(&owner) {
            Some(object) if view_style_base(object).is_some() => {
                if !view_style_holds(version) {
                    if let Some(base) = view_style_base_mut(object) {
                        restore_view_style(base, &groups);
                    }
                }
                continue;
            }
            Some(ObjectType::Material(material)) => {
                if !material_holds(version) {
                    restore_material(material, &groups);
                }
                continue;
            }
            Some(ObjectType::Layout(layout)) => {
                if !layout_holds(version) {
                    restore_layout(layout, &groups);
                }
                continue;
            }
            Some(_) => continue,
            None => {}
        }
        if let Some(view) = document.views.iter_mut().find(|view| view.handle == owner) {
            if !display_holds(version) {
                restore_view(view, &groups);
            }
            continue;
        }
        match document.get_entity_mut_keep_raw(owner) {
            Some(crate::entities::EntityType::Viewport(viewport)) => restore_viewport(viewport, &groups, version),
            Some(crate::entities::EntityType::Solid3D(solid)) if !solid_history_holds(version) => restore_solid(solid, &groups),
            _ => {}
        }
    }
}

fn restore_view(view: &mut crate::tables::View, groups: &[Group]) {
    for group in groups {
        match (group.name.as_str(), group.entries.as_slice()) {
            (CAMERA_PLOTTABLE, [entry]) if entry.code == 70 => {
                if let Some(value) = as_i32(&entry.value) {
                    view.camera_plottable = value != 0;
                }
            }
            (VISUAL_STYLE, [entry]) if entry.code == 340 => {
                if let Some(handle) = entry.value.as_handle() {
                    view.visual_style_handle = handle;
                }
            }
            _ => {}
        }
    }
}

/// Whether a kind states its groups in a file of `version`: a DWG needs them where the version lacks the field, a DXF where
/// AutoCAD's own DXF of the version holds them.
type Predicate = fn(DxfVersion) -> bool;

fn layout_dxf(version: DxfVersion) -> bool {
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

/// State the groups of every kind the output version needs, from the native values, and drop the groups the version holds
/// natively. `dxf` selects the DXF rules.
pub(crate) fn prepare_for_write(document: &mut std::borrow::Cow<'_, CadDocument>, version: DxfVersion, dxf: bool) {
    // A table record added in memory has no handle yet, and a record cannot be owned by it: the writers give the handles later.
    if document.has_null_table_entries() {
        document.to_mut().assign_table_entry_handles();
    }
    let needs = |holds: Predicate, dxf_states: Predicate| !holds(version) && (!dxf || dxf_states(version));
    let leaves = |holds: Predicate, dxf_states: Predicate| dxf && !holds(version) && !dxf_states(version);
    let mut ext = Ext::new(document);
    let converting = converts(document, version);
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
                    && (converting || has_record(handle) || material_group(material) != material_group(&defaults))
                {
                    vec![material_group(material)]
                } else {
                    Vec::new()
                };
                jobs.push(Job { owner: handle, owns: Owns::Exact(&[MATERIAL]), wanted, fresh: Some(vec![material_group(&defaults)]), leave: leaves(material_holds, always) });
            }
            ObjectType::Layout(layout) => {
                let differs = (layout.shade_plot_mode, layout.shade_plot_resolution, layout.shade_plot_dpi) != LAYOUT_DEFAULT;
                let wanted = if needs(layout_holds, layout_dxf) && (converting || has_record(handle) || differs) {
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
                let wanted = if needs(view_style_holds, always) && (converting || has_record(handle) || !base.display_name.is_empty() || base.flags != 0) {
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
    for view in document.views.iter() {
        let defaults = crate::tables::View::new(view.name.clone());
        let differs = view.camera_plottable != defaults.camera_plottable || view.visual_style_handle != defaults.visual_style_handle;
        let mut wanted = Vec::new();
        if needs(display_holds, never) && (converting || has_record(view.handle) || differs) {
            wanted.push(Group::new(CAMERA_PLOTTABLE, vec![XRecordEntry::int16(70, view.camera_plottable as i16)]));
            if !view.visual_style_handle.is_null() {
                wanted.push(Group::new(VISUAL_STYLE, vec![XRecordEntry::handle(340, view.visual_style_handle)]));
            }
        }
        jobs.push(Job { owner: view.handle, owns: Owns::Exact(&[CAMERA_PLOTTABLE, VISUAL_STYLE]), wanted, fresh: None, leave: leaves(display_holds, never) });
    }
    let control = document.vports.handle();
    if !control.is_null() {
        for vport in document.vports.iter() {
            let prefix = vport_prefix(vport);
            let display = Display::of_vport(vport);
            let defaults = Display::of_vport(&crate::tables::VPort::new(vport.name.clone()));
            let wanted = if needs(display_holds, never) && (converting || has_record(control) || display != defaults) {
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
                let display = Display::of_viewport(viewport);
                let defaults = Display::of_viewport(&crate::entities::Viewport::new());
                let wanted = if needs(display_holds, never) && (converting || has_record(handle) || display != defaults) {
                    display.groups("")
                } else {
                    Vec::new()
                };
                jobs.push(Job { owner: handle, owns: Owns::Exact(&DISPLAY), wanted, fresh: None, leave: leaves(display_holds, never) });
                let wanted = if needs(shade_plot_holds, shade_plot_dxf) && (converting || has_record(handle) || viewport.shade_plot_mode != 0) {
                    vec![Group::new(SHADE_PLOT, vec![XRecordEntry::int16(70, viewport.shade_plot_mode)])]
                } else {
                    Vec::new()
                };
                let fresh = vec![Group::new(SHADE_PLOT, vec![XRecordEntry::int16(70, 0)])];
                jobs.push(Job { owner: handle, owns: Owns::Exact(&[SHADE_PLOT]), wanted, fresh: Some(fresh), leave: leaves(shade_plot_holds, shade_plot_dxf) });
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
    // The scale-uniformly flag of the blocks: a document written again at the version it was read from states the chain only for the
    // blocks that had one (a file of that version without the chain holds the flag some other way, or not at all).
    let block_jobs: Vec<(Handle, bool)> = if leaves(block_data_holds, never) {
        // AutoCAD's DXF holds no such chain: the chains a drawing read from a DWG has stay as they are.
        Vec::new()
    } else if needs(block_data_holds, never) {
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
    // The multi-line attributes: the record of those that have an embedded MTEXT, the drop of the records of the others.
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
            (ours && !stated.contains(&dictionary.owner)).then_some(dictionary.owner)
        })
        .collect();
    if jobs.is_empty() && header_vars.is_none() && block_jobs.is_empty() && wanted_multilines.is_empty() && dropped_multilines.is_empty() {
        return;
    }
    let owned = document.to_mut();
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
        state(owned, &mut ext, job.owner, &|name| owns.names(name), wanted, true);
    }
    if let Some(wanted) = header_vars {
        state_header_vars(owned, version, wanted);
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
