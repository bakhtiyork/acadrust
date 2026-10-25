//! Extended data that AutoCAD adds when it saves a drawing to an older version and consumes when it saves it to a newer one.
//!
//! Besides the `RT*` records of `autocad_roundtrip`, AutoCAD keeps what an older format cannot hold in extended data of
//! the applications below. Each row names the sample files that show it (sample_AC1014.dwg to sample_AC1032.dwg are
//! down-saves of one drawing; their DXF twins hold the same records from R2000 to R2007):
//!
//! | record | holder | versions that hold the record | native counterpart |
//! |---|---|---|---|
//! | `ACAD_MLEADERVER 1070 2` | MULTILEADER, MLEADERSTYLE | before R2010 | the class version (`MultiLeader::dwg_version`) |
//! | `AcDbAttr 1070 0 1070 1` | ATTDEF, ATTRIB | before R2007 | the lock-position flag |
//! | `ACAD_DSTYLE_DIMTEXT_FILL 1070 376 1070 <DIMTFILL> 1070 377 1004 <colour>` | DIMSTYLE | before R2007 | `DIMTFILL`, `DIMTFILLCLR` |
//! | `ACAD_DSTYLE_DIM_LINETYPE`, `..._EXT1_...`, `..._EXT2_...` `1070 380/381/382 1005 <handle>` | DIMSTYLE | before R2007 | `DIMLTYPE`, `DIMLTEX1`, `DIMLTEX2` |
//! | `ACAD_OBJECT_CHANGE_GUID 1070 1 1071 .. 1070 x2 1070 x8` | 3DSOLID, REGION, BODY | before R2013 | the revision GUID of the modeler data |
//! | `ACAD 1000 AcDbSavedByObjectVersion 1070 0` | VISUALSTYLE | before R2018 | none |
//! | `ACAD 1000 DBColXD 1070 <index> 1071 <colour> 1000 <book>$<name>` | DBCOLOR | before R2018 | the colour of the object |
//! | `ACAD 1000 DesignCenter Data { 1070 1 1070 <units> }` | BLOCK_RECORD | before R2007 | the insertion units (`autocad_roundtrip` of the reader) |
//! | `CONTENTBLOCKDESCRIPTION 1071 0 1000 <text>`, `CONTENTBLOCKICON 1071 0 1004 <127 bytes> ..` | BLOCK_RECORD | before R2000 | `description`, `preview_data` |
//! | `ACAD_SPLINE_DATA 1070 100 1070 <a> 1070 101 1070 <b> 1070 102 1070 <knot parameterization>` | SPLINE | before R2013 | the closed and periodic flags of a fit-point spline, `knot_parameterization` |
//!
//! A DXF holds them where AutoCAD's own DXF does (R5b-7: the table above for R2000 and R2004; the stamp from R2000 to R2013,
//! `DBColXD` from R2004 to R2013, the dimension style records not in R12, whose DXF has extended data on entities only).
//!
//! The native model holds the values themselves: the readers restore them from the records (the records stay in the
//! extended data as the file had them, so the ACadSharp view of the file is unchanged); the writers state the record
//! from the native value for the versions that lack the field and drop it for the versions that have it, so a
//! down-save carries what AutoCAD's carries and an up-save carries none that is stale. A record that does not have the
//! shape AutoCAD writes is another application's or another producer's data and is left alone.
use crate::document::CadDocument;
use crate::entities::EntityType;
use crate::objects::ObjectType;
use crate::types::{Color, DxfVersion, Handle};
use crate::xdata::{ExtendedData, ExtendedDataRecord, XDataValue as X};

const MLEADER_VERSION: &str = "ACAD_MLEADERVER";
const ATTRIBUTE_LOCK: &str = "AcDbAttr";
const DIM_FILL: &str = "ACAD_DSTYLE_DIMTEXT_FILL";
const DIM_LINETYPES: [(&str, i16); 3] = [
    ("ACAD_DSTYLE_DIM_LINETYPE", 380),
    ("ACAD_DSTYLE_DIM_EXT1_LINETYPE", 381),
    ("ACAD_DSTYLE_DIM_EXT2_LINETYPE", 382),
];
const CHANGE_GUID: &str = "ACAD_OBJECT_CHANGE_GUID";
const ACAD: &str = "ACAD";
const SAVED_BY_VERSION: &str = "AcDbSavedByObjectVersion";
const BOOK_COLOR: &str = "DBColXD";
const BLOCK_DESCRIPTION: &str = "CONTENTBLOCKDESCRIPTION";
const BLOCK_ICON: &str = "CONTENTBLOCKICON";
const SPLINE_DATA: &str = "ACAD_SPLINE_DATA";

/// The first version that holds each value natively.
fn holds_mleader_version(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1024
}
fn holds_attribute_lock(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1021
}
fn holds_dimension_fill(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1021
}
fn holds_modeler_revision(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1027
}
fn states_saved_by_version(version: DxfVersion) -> bool {
    version < DxfVersion::AC1032
}
fn holds_block_content(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1015
}
fn holds_spline_data(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1027
}

/// The `DBColXD` block is in AutoCAD's DWG files from R14 to R2013 and in its DXF files from R2004 to R2013 (sample_AC1015_ascii.dxf
/// has none).
fn states_book_color_block(version: DxfVersion, dxf: bool) -> bool {
    version < DxfVersion::AC1032 && (!dxf || version >= DxfVersion::AC1018)
}

// ── the records ──────────────────────────────────────────────────────────────────────────────────────────────────

/// The values of `application`'s record.
fn values<'a>(data: &'a ExtendedData, application: &str) -> Option<&'a [X]> {
    data.get_record(application).map(|record| record.values.as_slice())
}

fn app_handle(document: &CadDocument, application: &str) -> Option<u64> {
    document.app_ids.get(application).map(|app| app.handle.value())
}

/// Replace the record of `application` by `wanted` (the record is made when there is none: at the position `at` the file had it
/// at, else at the end), or drop it when `wanted` is `None`, and drop the verbatim block of the application with it so the
/// writer states the record. True when something changed.
fn set_record(data: &mut ExtendedData, application: &str, handle: Option<u64>, wanted: Option<Vec<X>>, at: Option<usize>) -> bool {
    let existing = data.get_record(application).map(|record| record.values.clone());
    if existing == wanted {
        return false;
    }
    match wanted {
        Some(wanted) => {
            let slot = data
                .records()
                .iter()
                .position(|record| record.application_name.eq_ignore_ascii_case(application));
            match slot {
                Some(index) => data.records_mut()[index].values = wanted,
                None => {
                    let mut record = ExtendedDataRecord::new(application);
                    record.values = wanted;
                    match at {
                        Some(at) => data.insert_record(at.min(data.len()), record),
                        None => data.add_record(record),
                    }
                }
            }
        }
        None => {
            data.remove_record_ignore_case(application);
        }
    }
    if let Some(handle) = handle {
        data.raw_dwg_eed.retain(|(app, _)| *app != handle);
    }
    true
}

/// The record of `application` taken out of `data` with the position it had among the records of its owner before any was taken:
/// `taken` are the records already taken from the same owner (ascending). The verbatim block of the application goes with it.
fn take_record(
    data: &mut ExtendedData,
    application: &str,
    handle: Option<u64>,
    taken: &[(usize, ExtendedDataRecord)],
) -> Option<(usize, ExtendedDataRecord)> {
    let at = data
        .records()
        .iter()
        .position(|record| record.application_name.eq_ignore_ascii_case(application))?;
    let record = data.remove_record_ignore_case(application)?;
    if let Some(handle) = handle {
        data.raw_dwg_eed.retain(|(app, _)| *app != handle);
    }
    let original = taken.iter().fold(at, |position, (index, _)| if *index <= position { position + 1 } else { position });
    Some((original, record))
}

/// Remember a record taken out of the owner `handle`.
fn keep_taken(
    stash: &mut std::collections::HashMap<Handle, Vec<(usize, ExtendedDataRecord)>>,
    handle: Handle,
    entry: (usize, ExtendedDataRecord),
) {
    keep_taken_in(stash.entry(handle).or_default(), entry);
}

/// Put a taken record into the list of its owner (ascending positions).
fn keep_taken_in(list: &mut Vec<(usize, ExtendedDataRecord)>, entry: (usize, ExtendedDataRecord)) {
    let at = list.partition_point(|(index, _)| *index < entry.0);
    list.insert(at, entry);
}

/// The position the file had the record of `application` of `owner` at, when it was taken out of the owner.
fn taken_position(document: &CadDocument, owner: Handle, application: &str) -> Option<usize> {
    document
        .consumed_xdata
        .get(&owner)?
        .iter()
        .find(|(_, record)| record.application_name.eq_ignore_ascii_case(application))
        .map(|(index, _)| *index)
}

/// `wanted` in the `ACAD` record of `data` at the block that starts with the string `label` and has `length` values (the
/// block is made at the end of the record when there is none); `None` drops the block (and the record when it is empty).
fn set_block(
    data: &mut ExtendedData,
    handle: Option<u64>,
    label: &str,
    length: usize,
    wanted: Option<Vec<X>>,
    record_at: Option<usize>,
) -> bool {
    let current: Vec<X> = values(data, ACAD).map(<[X]>::to_vec).unwrap_or_default();
    let at = current.iter().position(|value| matches!(value, X::String(text) if text == label));
    let mut next = current.clone();
    match (at, wanted) {
        (Some(at), Some(wanted)) => {
            let end = (at + length).min(next.len());
            next.splice(at..end, wanted);
        }
        (Some(at), None) => {
            let end = (at + length).min(next.len());
            next.drain(at..end);
        }
        (None, Some(wanted)) => next.extend(wanted),
        (None, None) => {}
    }
    if next == current {
        return false;
    }
    set_record(data, ACAD, handle, (!next.is_empty()).then_some(next), record_at)
}

/// Run `change` on the extended data of an object or table record (its structured records and its verbatim blocks), and
/// put the result back.
fn with_object<R>(document: &mut CadDocument, handle: Handle, change: impl FnOnce(&mut ExtendedData) -> R) -> R {
    let mut data = document.object_xdata.remove(&handle).unwrap_or_default();
    data.raw_dwg_eed = document.eed_by_handle.remove(&handle).unwrap_or_default();
    let result = change(&mut data);
    let raw = std::mem::take(&mut data.raw_dwg_eed);
    if !raw.is_empty() {
        document.eed_by_handle.insert(handle, raw);
    }
    if !data.is_empty() {
        document.object_xdata.insert(handle, data);
    }
    result
}

/// Make the application of a record known to the drawing (the DWG stores a block under the handle of the application).
fn ensure_application(document: &mut CadDocument, application: &str) {
    if document.app_ids.get(application).is_some() {
        return;
    }
    let mut app = crate::tables::AppId::new(application);
    app.handle = document.allocate_handle();
    document.app_ids.add(app).ok();
}

// ── block description and preview ───────────────────────────────────────────────────────────────────────────────

/// The description of a block: `CONTENTBLOCKDESCRIPTION 1071 0 1000 <text>` (sample_AC1014.dwg, the block #A7E). An empty text
/// is no description (the writer states no record for it), so such a record stays data of the file.
fn block_description(data: &ExtendedData) -> Option<String> {
    match values(data, BLOCK_DESCRIPTION)? {
        [X::Integer32(0), X::String(text)] if !text.is_empty() => Some(text.clone()),
        _ => None,
    }
}

fn description_record(text: &str) -> Vec<X> {
    vec![X::Integer32(0), X::String(text.to_string())]
}

/// The preview image of a block: `CONTENTBLOCKICON 1071 0` and its bytes in chunks of 127 as `1004` (sample_AC1014.dwg: the
/// 4,136 bytes of the preview of sample_AC1015.dwg in 32 chunks of 127 and one of 72). Only a record in exactly that form (the one
/// `icon_record` states again) is taken: bytes in chunks of another size stay data of the file, byte for byte.
fn block_icon(data: &ExtendedData) -> Option<Vec<u8>> {
    match values(data, BLOCK_ICON)? {
        record @ [X::Integer32(0), chunks @ ..] if !chunks.is_empty() && chunks.iter().all(|chunk| matches!(chunk, X::BinaryData(bytes) if !bytes.is_empty())) => {
            let bytes: Vec<u8> = chunks.iter().flat_map(|chunk| if let X::BinaryData(bytes) = chunk { bytes.as_slice() } else { &[] }).copied().collect();
            (icon_record(&bytes) == record).then_some(bytes)
        }
        _ => None,
    }
}

fn icon_record(bytes: &[u8]) -> Vec<X> {
    std::iter::once(X::Integer32(0)).chain(bytes.chunks(127).map(|chunk| X::BinaryData(chunk.to_vec()))).collect()
}

// ── colour ───────────────────────────────────────────────────────────────────────────────────────────────────────

/// The packed colour of an R2004 `CMC`: blue, green, red and the method (`0xC2` RGB, `0xC3` index, `0xC0` ByLayer, `0xC1`
/// ByBlock) as a little-endian long.
fn packed_color(color: Color) -> u32 {
    match color {
        Color::Rgb { r, g, b } => (b as u32) | ((g as u32) << 8) | ((r as u32) << 16) | (0xC2u32 << 24),
        Color::Index(index) => (index as u32 & 0xFF) | (0xC3u32 << 24),
        Color::ByLayer => 0xC0u32 << 24,
        Color::ByBlock => 0xC1u32 << 24,
        Color::None => 0xC8u32 << 24,
    }
}

fn unpacked_color(packed: u32) -> Option<Color> {
    let [blue, green, red, method] = packed.to_le_bytes();
    match method {
        0xC2 => Some(Color::from_rgb(red, green, blue)),
        0xC3 => Some(Color::from_index(blue as i16)),
        0xC0 => Some(Color::ByLayer),
        0xC1 => Some(Color::ByBlock),
        _ => None,
    }
}

// ── MLEADERVER ───────────────────────────────────────────────────────────────────────────────────────────────────

fn mleader_version(data: &ExtendedData) -> Option<i16> {
    match values(data, MLEADER_VERSION)? {
        [X::Integer16(version)] => Some(*version),
        _ => None,
    }
}

// ── AcDbAttr ─────────────────────────────────────────────────────────────────────────────────────────────────────

fn attribute_locked(data: &ExtendedData) -> bool {
    matches!(values(data, ATTRIBUTE_LOCK), Some([X::Integer16(0), X::Integer16(1)]))
}

// ── DIMSTYLE ─────────────────────────────────────────────────────────────────────────────────────────────────────

/// The text fill of an `ACAD_DSTYLE_DIMTEXT_FILL` record: `DIMTFILL` and the colour of `DIMTFILLCLR`. The colour is a
/// 24-byte block: its size (24), 1, the packed colour and three zero longs (sample_AC1018.dwg, style #994).
fn dimension_fill(data: &ExtendedData) -> Option<(i16, Color)> {
    match values(data, DIM_FILL)? {
        [X::Integer16(376), X::Integer16(fill), X::Integer16(377), X::BinaryData(color)]
            if color.len() == 24 && color[..8] == [24, 0, 0, 0, 1, 0, 0, 0] && color[12..] == [0u8; 12] =>
        {
            let packed = u32::from_le_bytes([color[8], color[9], color[10], color[11]]);
            Some((*fill, unpacked_color(packed)?))
        }
        _ => None,
    }
}

fn dimension_fill_record(fill: i16, color: Color) -> Vec<X> {
    let mut block = vec![24u8, 0, 0, 0, 1, 0, 0, 0];
    block.extend(packed_color(color).to_le_bytes());
    block.extend([0u8; 12]);
    vec![X::Integer16(376), X::Integer16(fill), X::Integer16(377), X::BinaryData(block)]
}

fn dimension_linetype(data: &ExtendedData, application: &str, code: i16) -> Option<Handle> {
    match values(data, application)? {
        [X::Integer16(found), X::Handle(handle)] if *found == code => Some(*handle),
        _ => None,
    }
}

// ── 3DSOLID, REGION, BODY ────────────────────────────────────────────────────────────────────────────────────────

fn revision_of(data: &ExtendedData) -> Option<crate::entities::solid3d::AcisRevision> {
    match values(data, CHANGE_GUID)? {
        [X::Integer16(1), X::Integer32(major), X::Integer16(minor1), X::Integer16(minor2), rest @ ..] if rest.len() == 8 => {
            let mut bytes = [0u8; 8];
            for (slot, value) in bytes.iter_mut().zip(rest) {
                match value {
                    X::Integer16(byte) if (0..=255).contains(byte) => *slot = *byte as u8,
                    _ => return None,
                }
            }
            Some(crate::entities::solid3d::AcisRevision {
                has_guid: true,
                major: *major as u32,
                minor1: *minor1,
                minor2: *minor2,
                bytes,
                end_marker: 0,
            })
        }
        _ => None,
    }
}

fn revision_record(revision: &crate::entities::solid3d::AcisRevision) -> Option<Vec<X>> {
    revision.has_guid.then(|| {
        let mut record = vec![
            X::Integer16(1),
            X::Integer32(revision.major as i32),
            X::Integer16(revision.minor1),
            X::Integer16(revision.minor2),
        ];
        record.extend(revision.bytes.iter().map(|byte| X::Integer16(*byte as i16)));
        record
    })
}

// ── VISUALSTYLE, DBCOLOR ─────────────────────────────────────────────────────────────────────────────────────────

/// The record AutoCAD keeps for a spline before R2013: three properties, each a tag and a value (sample_AC1014.dwg to
/// sample_AC1024.dwg, the fit-point spline #434: `100 0 101 0 102 2`). The third is the knot parameterization; the first two are the
/// closed and the periodic flag (a fit-point spline of an older file has neither: the fit-point form of the record has no place
/// for them. UNVERIFIED: the sample states the open spline only).
fn spline_data(data: &ExtendedData) -> Option<(bool, bool, i16)> {
    match values(data, SPLINE_DATA)? {
        [X::Integer16(100), X::Integer16(closed @ (0 | 1)), X::Integer16(101), X::Integer16(periodic @ (0 | 1)), X::Integer16(102), X::Integer16(knots)] => {
            Some((*closed == 1, *periodic == 1, *knots))
        }
        _ => None,
    }
}

fn spline_record(spline: &crate::entities::Spline) -> Vec<X> {
    vec![
        X::Integer16(100),
        X::Integer16(spline.flags.closed as i16),
        X::Integer16(101),
        X::Integer16(spline.flags.periodic as i16),
        X::Integer16(102),
        X::Integer16(spline.knot_parameterization as i16),
    ]
}

fn saved_by_version_block() -> Vec<X> {
    vec![X::String(SAVED_BY_VERSION.into()), X::Integer16(0)]
}

/// Whether `record` is the visual style stamp alone (compared without building the block).
fn is_saved_by_version_stamp(record: &[X]) -> bool {
    matches!(record, [X::String(label), X::Integer16(0)] if label == SAVED_BY_VERSION)
}

/// The `DBColXD` block of a colour-book object: the nearest index, the packed colour and `<book>$<name>`.
fn book_color_block(book: &crate::objects::BookColor) -> Option<Vec<X>> {
    let (r, g, b) = match book.color {
        Color::Rgb { r, g, b } => (r, g, b),
        color => color.rgb()?,
    };
    let identity = crate::io::dxf::join_color_book_name(Some(&book.book_name), Some(&book.color_name))?;
    Some(vec![
        X::String(BOOK_COLOR.into()),
        X::Integer16(crate::types::aci_table::nearest_aci(r, g, b) as i16),
        X::Integer32(packed_color(Color::from_rgb(r, g, b)) as i32),
        X::String(identity),
    ])
}

fn book_color_in(record: &[X]) -> Option<(Color, String)> {
    let at = record.iter().position(|value| matches!(value, X::String(text) if text == BOOK_COLOR))?;
    match record.get(at + 1..at + 4)? {
        [X::Integer16(_), X::Integer32(packed), X::String(identity)] => Some((unpacked_color(*packed as u32)?, identity.clone())),
        _ => None,
    }
}

/// Whether the `DBColXD` block of `record` still describes `book`, a colour-book object read from a drawing before R2004 (its
/// DBCOLOR record holds the nearest index only, the block that index, the colour and the names): an object whose index was changed
/// after the read no longer is the colour the block names.
fn block_describes(record: &[X], book: &crate::objects::BookColor) -> bool {
    let Some(at) = record.iter().position(|value| matches!(value, X::String(text) if text == BOOK_COLOR)) else { return false };
    matches!((record.get(at + 1), book.color), (Some(X::Integer16(index)), Color::Index(own)) if *index == own as i16)
}

/// The values of a record the model took out of the object `handle`.
fn taken_values<'a>(document: &'a CadDocument, handle: Handle, application: &str) -> Option<&'a [X]> {
    document
        .consumed_xdata
        .get(&handle)?
        .iter()
        .find(|(_, record)| record.application_name.eq_ignore_ascii_case(application))
        .map(|(_, record)| record.values.as_slice())
}

// ── read ─────────────────────────────────────────────────────────────────────────────────────────────────────────

/// Whether `entity` holds a record whose value its model holds natively from `version` on.
fn entity_needs_restore(entity: &EntityType, version: DxfVersion) -> bool {
    match entity {
        EntityType::MultiLeader(leader) if !holds_mleader_version(version) => mleader_version(&leader.common.extended_data).is_some(),
        EntityType::AttributeDefinition(attribute) if !holds_attribute_lock(version) => attribute_locked(&attribute.common.extended_data),
        EntityType::Insert(insert) if !holds_attribute_lock(version) => {
            insert.attributes.iter().any(|attribute| attribute_locked(&attribute.common.extended_data))
        }
        // Only a fit-point spline takes the record in: AutoCAD's own conversion of such a spline is a control-point spline, whose
        // record states what the fit data was (the native model has no fit data for it): the record stays as the file had it.
        EntityType::Spline(spline) if !holds_spline_data(version) => !spline.fit_points.is_empty() && spline_data(&spline.common.extended_data).is_some(),
        EntityType::Solid3D(solid) if !holds_modeler_revision(version) => revision_of(&solid.common.extended_data).is_some(),
        EntityType::Region(region) if !holds_modeler_revision(version) => revision_of(&region.common.extended_data).is_some(),
        EntityType::Body(body) if !holds_modeler_revision(version) => revision_of(&body.common.extended_data).is_some(),
        _ => false,
    }
}

/// Restore the native values of the records a drawing below the version that holds them keeps in extended data, and take the
/// records out of the model (they are what the format needs, not data: `consumed_xdata` keeps them for the ACadSharp view and for
/// a writer of the same version).
pub(crate) fn restore_document(document: &mut CadDocument) {
    let version = document.version;
    let handle_of = |name: &str| app_handle(document, name);
    let (mleader_app, attribute_app, guid_app, spline_app) =
        (handle_of(MLEADER_VERSION), handle_of(ATTRIBUTE_LOCK), handle_of(CHANGE_GUID), handle_of(SPLINE_DATA));
    // A drawing that registers none of the applications holds none of their records: no entity is looked at.
    let registered = mleader_app.is_some() || attribute_app.is_some() || guid_app.is_some() || spline_app.is_some();
    if registered
        && !(holds_mleader_version(version) && holds_attribute_lock(version) && holds_modeler_revision(version) && holds_spline_data(version))
    {
        // Look first and copy only the entities that change (the records are shared until then).
        let changing: Vec<usize> = document
            .entities
            .iter()
            .enumerate()
            .filter_map(|(index, entity)| entity_needs_restore(entity, version).then_some(index))
            .collect();
        for index in changing {
            let stash = &mut document.consumed_xdata;
            match std::sync::Arc::make_mut(&mut document.entities[index]) {
                EntityType::MultiLeader(leader) => {
                    let handle = leader.common.handle;
                    if let Some(found) = mleader_version(&leader.common.extended_data) {
                        leader.dwg_version = found;
                    }
                    if let Some(entry) = take_record(&mut leader.common.extended_data, MLEADER_VERSION, mleader_app, stash.get(&handle).map_or(&[], Vec::as_slice)) {
                        keep_taken(stash, handle, entry);
                    }
                }
                EntityType::AttributeDefinition(attribute) => {
                    let handle = attribute.common.handle;
                    attribute.lock_position = true;
                    if let Some(entry) = take_record(&mut attribute.common.extended_data, ATTRIBUTE_LOCK, attribute_app, stash.get(&handle).map_or(&[], Vec::as_slice)) {
                        keep_taken(stash, handle, entry);
                    }
                }
                EntityType::Insert(insert) => {
                    for attribute in &mut insert.attributes {
                        if attribute_locked(&attribute.common.extended_data) {
                            let handle = attribute.common.handle;
                            attribute.lock_position = true;
                            if let Some(entry) = take_record(&mut attribute.common.extended_data, ATTRIBUTE_LOCK, attribute_app, stash.get(&handle).map_or(&[], Vec::as_slice)) {
                                keep_taken(stash, handle, entry);
                            }
                        }
                    }
                }
                EntityType::Spline(spline) => {
                    if let Some((closed, periodic, knots)) = spline_data(&spline.common.extended_data) {
                        spline.knot_parameterization = i32::from(knots);
                        // A fit-point spline has no closed or periodic flag in a record before R2013; the flags 1 of the
                        // R2013 record state the fit-point method.
                        spline.flags.closed = closed;
                        spline.flags.periodic = periodic;
                        spline.dwg_flags1 |= 1 | 8;
                        if closed {
                            spline.dwg_flags1 |= 4;
                        } else {
                            spline.dwg_flags1 &= !4;
                        }
                    }
                    let handle = spline.common.handle;
                    if let Some(entry) = take_record(&mut spline.common.extended_data, SPLINE_DATA, spline_app, stash.get(&handle).map_or(&[], Vec::as_slice)) {
                        keep_taken(stash, handle, entry);
                    }
                }
                // The identifier of the entity is the GUID of its revision, as the reader of a DWG that holds the revision states it.
                EntityType::Solid3D(solid) => {
                    if let Some(revision) = revision_of(&solid.common.extended_data) {
                        solid.acis_data.revision = revision;
                        if solid.uid.is_empty() {
                            solid.uid = solid.acis_data.revision.guid_string().unwrap_or_default();
                        }
                    }
                    let handle = solid.common.handle;
                    if let Some(entry) = take_record(&mut solid.common.extended_data, CHANGE_GUID, guid_app, stash.get(&handle).map_or(&[], Vec::as_slice)) {
                        keep_taken(stash, handle, entry);
                    }
                }
                EntityType::Region(region) => {
                    if let Some(revision) = revision_of(&region.common.extended_data) {
                        region.acis_data.revision = revision;
                        if region.uid.is_empty() {
                            region.uid = region.acis_data.revision.guid_string().unwrap_or_default();
                        }
                    }
                    let handle = region.common.handle;
                    if let Some(entry) = take_record(&mut region.common.extended_data, CHANGE_GUID, guid_app, stash.get(&handle).map_or(&[], Vec::as_slice)) {
                        keep_taken(stash, handle, entry);
                    }
                }
                EntityType::Body(body) => {
                    if let Some(revision) = revision_of(&body.common.extended_data) {
                        body.acis_data.revision = revision;
                        if body.uid.is_empty() {
                            body.uid = body.acis_data.revision.guid_string().unwrap_or_default();
                        }
                    }
                    let handle = body.common.handle;
                    if let Some(entry) = take_record(&mut body.common.extended_data, CHANGE_GUID, guid_app, stash.get(&handle).map_or(&[], Vec::as_slice)) {
                        keep_taken(stash, handle, entry);
                    }
                }
                _ => {}
            }
        }
    }
    // The multileader styles (before R2010) and, before R2018, a visual style whose record is the stamp alone and a colour-book
    // object whose record is the colour block alone: the format needs these records, the drawing does not (a record with other
    // values is another producer's data and stays). Only objects with extended data can hold one: one pass over those, not over
    // the object map.
    let (styles, stamps) = (!holds_mleader_version(version), states_saved_by_version(version));
    let (mleader_app, acad) = (app_handle(document, MLEADER_VERSION), app_handle(document, ACAD));
    if (styles && mleader_app.is_some()) || (stamps && acad.is_some()) {
        let taken: Vec<(Handle, &'static str, Option<u64>)> = document
            .object_xdata
            .iter()
            .filter_map(|(handle, data)| match document.objects.get(handle)? {
                ObjectType::MultiLeaderStyle(_) if styles && mleader_version(data).is_some() => Some((*handle, MLEADER_VERSION, mleader_app)),
                ObjectType::VisualStyle(_) if stamps && values(data, ACAD).is_some_and(is_saved_by_version_stamp) => Some((*handle, ACAD, acad)),
                ObjectType::BookColor(_)
                    if stamps && values(data, ACAD).is_some_and(|record| record.len() == 4 && book_color_in(record).is_some()) =>
                {
                    Some((*handle, ACAD, acad))
                }
                _ => None,
            })
            .collect();
        for (handle, application, app) in taken {
            let mut list = document.consumed_xdata.remove(&handle).unwrap_or_default();
            if let Some(entry) = with_object(document, handle, |data| take_record(data, application, app, &list)) {
                keep_taken_in(&mut list, entry);
            }
            if !list.is_empty() {
                document.consumed_xdata.insert(handle, list);
            }
        }
    }
    if !holds_dimension_fill(version) {
        let handles: Vec<Handle> = document.dim_styles.iter().map(|style| style.handle).collect();
        let apps: Vec<Option<u64>> = [DIM_FILL].into_iter().chain(DIM_LINETYPES.map(|(name, _)| name)).map(|name| app_handle(document, name)).collect();
        for handle in handles {
            let restored = {
                let Some(data) = document.object_xdata.get(&handle) else { continue };
                (
                    dimension_fill(data),
                    DIM_LINETYPES.map(|(application, code)| dimension_linetype(data, application, code)),
                )
            };
            if restored.0.is_none() && restored.1.iter().all(Option::is_none) {
                continue;
            }
            if let Some(style) = document.dim_styles.iter_mut().find(|style| style.handle == handle) {
                if let Some((fill, color)) = restored.0 {
                    style.dimtfill = fill;
                    style.dimtfillclr = color.approximate_index();
                    style.dimtfillclr_true_color = color.is_true_color().then_some(color);
                }
                let [line, first, second] = restored.1;
                if let Some(handle) = line {
                    style.dimltex_handle = handle;
                }
                if let Some(handle) = first {
                    style.dimltex1_handle = handle;
                }
                if let Some(handle) = second {
                    style.dimltex2_handle = handle;
                }
            }
            // The records that parsed go (the ones that did not stay: another producer's data).
            let ours: Vec<(&str, Option<u64>)> = std::iter::once(DIM_FILL)
                .chain(DIM_LINETYPES.map(|(name, _)| name))
                .zip(apps.iter().copied())
                .zip([restored.0.is_some(), restored.1[0].is_some(), restored.1[1].is_some(), restored.1[2].is_some()])
                .filter(|(_, parsed)| *parsed)
                .map(|(name, _)| name)
                .collect();
            let mut taken = document.consumed_xdata.get(&handle).cloned().unwrap_or_default();
            let mut entries = Vec::new();
            with_object(document, handle, |data| {
                for (name, app) in ours {
                    if let Some(entry) = take_record(data, name, app, &taken) {
                        let at = taken.partition_point(|(index, _)| *index < entry.0);
                        taken.insert(at, (entry.0, entry.1.clone()));
                        entries.push(entry);
                    }
                }
            });
            for entry in entries {
                keep_taken(&mut document.consumed_xdata, handle, entry);
            }
        }
    }
    // The description and the preview image of a block record (before R2000: records of its extended data). A value the file
    // also states in the group of R2000 (a DXF of another producer) stays that value, and the record stays data of the file.
    if !holds_block_content(version) && (app_handle(document, BLOCK_DESCRIPTION).is_some() || app_handle(document, BLOCK_ICON).is_some()) {
        let apps = [app_handle(document, BLOCK_DESCRIPTION), app_handle(document, BLOCK_ICON)];
        let restored: Vec<(Handle, Option<String>, Option<Vec<u8>>)> = document
            .block_records
            .iter()
            .filter_map(|record| {
                let data = document.object_xdata.get(&record.handle)?;
                let text = block_description(data).filter(|_| record.description.is_empty());
                let icon = block_icon(data).filter(|_| record.preview_data.is_empty());
                (text.is_some() || icon.is_some()).then_some((record.handle, text, icon))
            })
            .collect();
        for (handle, text, icon) in restored {
            let parsed = [text.is_some(), icon.is_some()];
            if let Some(record) = document.block_records.iter_mut().find(|record| record.handle == handle) {
                if let Some(text) = text {
                    record.description = text;
                }
                if let Some(icon) = icon {
                    record.preview_data = icon;
                }
            }
            let mut list = document.consumed_xdata.remove(&handle).unwrap_or_default();
            with_object(document, handle, |data| {
                for ((name, app), parsed) in [BLOCK_DESCRIPTION, BLOCK_ICON].into_iter().zip(apps).zip(parsed) {
                    if parsed {
                        if let Some(entry) = take_record(data, name, app, &list) {
                            keep_taken_in(&mut list, entry);
                        }
                    }
                }
            });
            if !list.is_empty() {
                document.consumed_xdata.insert(handle, list);
            }
        }
    }
}

/// A colour-book object read from a drawing before R2004 holds the nearest index only (its DBCOLOR record has neither the colour
/// nor the names): the colour and the names are in the `DBColXD` block of its extended data. A drawing written as R2004 or later
/// states them in the object (sample_AC1015.dwg and sample_AC1018.dwg: the same DBCOLOR). The model keeps what the file said, as
/// ACadSharp reads it; the writers state the colour (so `prepare_book_colors` finds the object the entities name).
pub(crate) fn restore_book_colors(document: &mut std::borrow::Cow<'_, CadDocument>, version: DxfVersion) {
    if version < DxfVersion::AC1018 {
        return;
    }
    let restored: Vec<(Handle, Color, String)> = document
        .objects
        .iter()
        .filter_map(|(handle, object)| match object {
            ObjectType::BookColor(book) if !book.color.is_true_color() => {
                let record = document
                    .object_xdata
                    .get(handle)
                    .and_then(|data| values(data, ACAD))
                    .filter(|record| book_color_in(record).is_some())
                    .or_else(|| taken_values(document, *handle, ACAD))?;
                // A block that names another index than the object's is stale (the colour was changed after the read).
                if !block_describes(record, book) {
                    return None;
                }
                let (color, identity) = book_color_in(record)?;
                color.is_true_color().then_some((*handle, color, identity))
            }
            _ => None,
        })
        .collect();
    if restored.is_empty() {
        return;
    }
    let document = document.to_mut();
    for (handle, color, identity) in restored {
        if let Some(ObjectType::BookColor(book)) = document.objects.get_mut(&handle) {
            book.color = color;
            if book.book_name.is_empty() && book.color_name.is_empty() {
                let (book_name, color_name) = crate::io::dxf::split_color_book_name(&identity);
                book.book_name = book_name.unwrap_or_default();
                book.color_name = color_name.unwrap_or_default();
            }
        }
    }
}

// ── write ────────────────────────────────────────────────────────────────────────────────────────────────────────

/// What a drawing written as `version` states in extended data: the document with the records of the values `version`
/// cannot hold, and without the records of the values it holds. Borrowed when nothing changes.
pub(crate) fn prepare_for_write(document: &mut std::borrow::Cow<'_, CadDocument>, version: DxfVersion, dxf: bool) {
    let application = |document: &CadDocument, name: &str| app_handle(document, name);
    // The stamps that carry no data of the drawing (the multileader version, the visual style stamp, the colour-book block) are
    // added by AutoCAD when it converts a drawing of another version; a document made in memory, and a drawing written again
    // at its own version, state the ones they have.
    let converting = document
        .dwg_source_version
        .or(document.dxf_source_version)
        .is_some_and(|source| source != version);

    // A version that holds a value natively has no record for it: a record the model still holds there was not taken in by the
    // reader (a file of that version, or a document made in memory, states it) and stays as it is. A record of another shape
    // than AutoCAD's is another producer's data and is never touched.
    // The source was older than the version the drawing is written as (AutoCAD drops the stamps of such a drawing).
    let from_older = |holding: DxfVersion| {
        document.dwg_source_version.or(document.dxf_source_version).is_some_and(|source| source < holding)
    };

    // Entities: collect the changes first (the entity records are shared and copied on write).
    // (owner, the record to state or none, the position the file had the record at)
    let mut leaders: Vec<(Handle, Option<Vec<X>>, Option<usize>)> = Vec::new();
    let mut attributes: Vec<(Handle, Option<usize>, Option<Vec<X>>, Option<usize>)> = Vec::new();
    let mut solids: Vec<(Handle, Option<Vec<X>>, Option<usize>)> = Vec::new();
    let mut splines: Vec<(Handle, Option<Vec<X>>, Option<usize>)> = Vec::new();
    let written_again = document.dwg_source_version.or(document.dxf_source_version) == Some(version);
    for entity in document.entities() {
        match entity {
            // AutoCAD's DXF files hold no ACAD_SPLINE_DATA (sample_AC1015_ascii.dxf to sample_AC1027_ascii.dxf).
            // R2013 holds the parameterization in the record of the spline itself: AutoCAD's record of a control-point spline (kept as the
            // file had it) goes, as it does from AutoCAD's own R2013 file (sample_AC1027.dwg has none).
            EntityType::Spline(spline) if holds_spline_data(version) && spline_data(&spline.common.extended_data).is_some() => {
                splines.push((spline.common.handle, None, taken_position(document, spline.common.handle, SPLINE_DATA)));
            }
            EntityType::Spline(spline) if !holds_spline_data(version) && !dxf && !spline.fit_points.is_empty() => {
                let data = &spline.common.extended_data;
                let ours = spline_data(data).is_some();
                if ours || values(data, SPLINE_DATA).is_none() {
                    let at = taken_position(document, spline.common.handle, SPLINE_DATA);
                    // A fit-point spline of a conversion (or of a document made in memory), a spline whose file stated the record,
                    // and a record the model holds are stated from the native values; a drawing written again at the version it
                    // was read from states the record only where its file had one.
                    let wanted = ((!spline.fit_points.is_empty() && !written_again) || at.is_some() || ours).then(|| spline_record(spline));
                    if values(data, SPLINE_DATA) != wanted.as_deref() && (wanted.is_some() || ours) {
                        splines.push((spline.common.handle, wanted, at));
                    }
                }
            }
            EntityType::MultiLeader(leader) if !holds_mleader_version(version) => {
                let data = &leader.common.extended_data;
                let ours = mleader_version(data).is_some();
                if ours || values(data, MLEADER_VERSION).is_none() {
                    let at = taken_position(document, leader.common.handle, MLEADER_VERSION);
                    let wanted = (converting || at.is_some() || ours).then(|| vec![X::Integer16(leader.dwg_version)]);
                    if wanted.is_some() && values(data, MLEADER_VERSION) != wanted.as_deref() {
                        leaders.push((leader.common.handle, wanted, at));
                    }
                }
            }
            EntityType::AttributeDefinition(attribute) if !holds_attribute_lock(version) => {
                if let Some(wanted) = attribute_change(&attribute.common.extended_data, version, attribute.lock_position) {
                    attributes.push((attribute.common.handle, None, wanted, taken_position(document, attribute.common.handle, ATTRIBUTE_LOCK)));
                }
            }
            EntityType::Insert(insert) if !holds_attribute_lock(version) => {
                for (index, attribute) in insert.attributes.iter().enumerate() {
                    if let Some(wanted) = attribute_change(&attribute.common.extended_data, version, attribute.lock_position) {
                        attributes.push((insert.common.handle, Some(index), wanted, taken_position(document, attribute.common.handle, ATTRIBUTE_LOCK)));
                    }
                }
            }
            EntityType::Solid3D(solid) => {
                solid_change(&mut solids, version, document, solid.common.handle, &solid.common.extended_data, &solid.acis_data);
            }
            EntityType::Region(region) => {
                solid_change(&mut solids, version, document, region.common.handle, &region.common.extended_data, &region.acis_data);
            }
            EntityType::Body(body) => {
                solid_change(&mut solids, version, document, body.common.handle, &body.common.extended_data, &body.acis_data);
            }
            _ => {}
        }
    }

    // Objects and table records.
    let mut styles: Vec<(Handle, Option<Vec<X>>, Option<usize>)> = Vec::new();
    let mut visual: Vec<(Handle, bool, Option<usize>)> = Vec::new();
    let mut books: Vec<(Handle, Option<Vec<X>>, Option<usize>)> = Vec::new();
    for (handle, object) in &document.objects {
        match object {
            ObjectType::MultiLeaderStyle(_) if !holds_mleader_version(version) => {
                let data = document.object_xdata.get(handle);
                let existing = data.and_then(mleader_version);
                if existing.is_none() && data.and_then(|d| values(d, MLEADER_VERSION)).is_some() {
                    continue;
                }
                let stashed = document.consumed_xdata.get(handle).and_then(|list| {
                    list.iter().find_map(|(index, record)| match record.values.as_slice() {
                        [X::Integer16(found)] if record.application_name.eq_ignore_ascii_case(MLEADER_VERSION) => Some((*index, *found)),
                        _ => None,
                    })
                });
                let current = existing.or(stashed.map(|(_, found)| found));
                let wanted = (converting || current.is_some()).then(|| vec![X::Integer16(current.unwrap_or(2))]);
                if wanted.is_some() && data.and_then(|d| values(d, MLEADER_VERSION)) != wanted.as_deref() {
                    styles.push((*handle, wanted, stashed.map(|(index, _)| index)));
                }
            }
            // AutoCAD's DXF files of R2000 to R2013 hold the stamp in each of their 24 visual styles (sample_AC1015_ascii.dxf to
            // sample_AC1027_ascii.dxf), none at R2018: a DXF gets it for a conversion as a DWG does.
            ObjectType::VisualStyle(_) => {
                let data = document.object_xdata.get(handle);
                let label = data.is_some_and(|d| has_label(d, SAVED_BY_VERSION));
                let exact = data.is_some_and(|d| has_block(d, SAVED_BY_VERSION, &saved_by_version_block()));
                let stashed = taken_values(document, *handle, ACAD).is_some();
                let adds = states_saved_by_version(version) && !label && (stashed || converting);
                // R2018 has no stamp: AutoCAD drops it from a drawing of an older version (an R2018 file that states one keeps
                // it). A block of another value is another producer's data: left alone.
                if adds || (!states_saved_by_version(version) && exact && from_older(DxfVersion::AC1032)) {
                    visual.push((*handle, adds, taken_position(document, *handle, ACAD)));
                }
            }
            // AutoCAD's R2000 DXF has no colour block: a DXF of that version gets none for a conversion and keeps the one its DWG
            // source had. R2018 states none for a conversion; an R2018 file that has one keeps it (cadviewer SE-Barcelona.dwg).
            ObjectType::BookColor(book) if states_book_color_block(version, dxf) || (dxf && version < DxfVersion::AC1032) => {
                let data = document.object_xdata.get(handle);
                let taken = taken_values(document, *handle, ACAD);
                if !states_book_color_block(version, dxf) && taken.is_none() {
                    continue;
                }
                let stated = data.and_then(|d| values(d, ACAD)).and_then(book_color_in);
                // The block the model holds stays while it names the object's colour (AutoCAD's nearest index is not recomputed).
                if stated.as_ref().is_some_and(|(color, identity)| {
                    *color == book.color
                        && crate::io::dxf::join_color_book_name(Some(&book.book_name), Some(&book.color_name)).as_deref() == Some(identity.as_str())
                }) {
                    continue;
                }
                let known = stated.is_some() || taken.is_some() || (converting && states_book_color_block(version, dxf));
                // The block of an object that holds the nearest index only (a DBCOLOR record before R2004) is not made from the
                // object: the block the file had goes back as it was, while it still names the object's index.
                let wanted = match taken {
                    Some(taken) if !book.color.is_true_color() => block_describes(taken, book).then(|| taken.to_vec()),
                    _ => book_color_block(book),
                };
                if let Some(wanted) = wanted.filter(|_| known) {
                    if !data.is_some_and(|d| has_block(d, BOOK_COLOR, &wanted)) {
                        books.push((*handle, Some(wanted), taken_position(document, *handle, ACAD)));
                    }
                }
            }
            _ => {}
        }
    }
    let mut dimension_styles: Vec<(Handle, DimensionRecords)> = Vec::new();
    // AutoCAD's R12 DXF holds extended data on entities only (sample_AC1009_ascii.dxf: 87 records, none on a table record), so
    // a DXF of R12 states neither the fill nor the line types of a dimension style in a record.
    if !holds_dimension_fill(version) && !(dxf && version < DxfVersion::AC1012) {
        for style in document.dim_styles.iter() {
            let positions = DIM_APPLICATIONS.map(|name| taken_position(document, style.handle, name));
            let data = document.object_xdata.get(&style.handle);
            let wanted = DimensionRecords::of(style, data, positions);
            if wanted.differs_from(data) {
                dimension_styles.push((style.handle, wanted));
            }
        }
    }
    // The description and the preview image of a block are native from R2000; R13 and R14 state them in records of the block
    // record's extended data (sample_AC1014.dwg), after the records it had. They go back where the reader of such a file took them
    // from; a block of a newer drawing gets them after its other records (AutoCAD's R14 file states them after `DesignCenter Data`,
    // which the writer adds last, under application handles of its own numbering: UNVERIFIED in AutoCAD, the records are the
    // same). A record the model still holds stays.
    let mut block_contents: Vec<(Handle, [Option<Vec<X>>; 2], [Option<usize>; 2])> = Vec::new();
    if !holds_block_content(version) && version >= DxfVersion::AC1012 {
        for record in document.block_records.iter() {
            let data = document.object_xdata.get(&record.handle);
            let at = [BLOCK_DESCRIPTION, BLOCK_ICON].map(|name| taken_position(document, record.handle, name));
            let wanted = [
                (!record.description.is_empty() && data.and_then(|data| values(data, BLOCK_DESCRIPTION)).is_none())
                    .then(|| description_record(&record.description)),
                (!record.preview_data.is_empty() && data.and_then(|data| values(data, BLOCK_ICON)).is_none())
                    .then(|| icon_record(&record.preview_data)),
            ];
            if wanted.iter().any(Option::is_some) {
                block_contents.push((record.handle, wanted, at));
            }
        }
    }
    // The units a reader took from `DesignCenter Data` of a drawing before R2007 are native from R2007 on, where AutoCAD states no
    // such record; the record of a file of R2007 or later was not taken in (city_base_map.dwg, R2013, has two) and stays.
    let mut design_center: Vec<Handle> = Vec::new();
    if version >= DxfVersion::AC1021 && from_older(DxfVersion::AC1021) {
        for record in document.block_records.iter() {
            if document
                .object_xdata
                .get(&record.handle)
                .is_some_and(|data| design_center_units(data).is_some())
            {
                design_center.push(record.handle);
            }
        }
    }

    if leaders.is_empty()
        && attributes.is_empty()
        && solids.is_empty()
        && splines.is_empty()
        && styles.is_empty()
        && visual.is_empty()
        && books.is_empty()
        && dimension_styles.is_empty()
        && block_contents.is_empty()
        && design_center.is_empty()
    {
        return;
    }
    let document = document.to_mut();
    for name in [MLEADER_VERSION, ATTRIBUTE_LOCK, DIM_FILL, CHANGE_GUID, SPLINE_DATA]
        .into_iter()
        .chain(DIM_LINETYPES.map(|(name, _)| name))
    {
        // Only a record that is going to be stated needs its application.
        let needed = match name {
            MLEADER_VERSION => leaders.iter().any(|(_, wanted, _)| wanted.is_some()) || styles.iter().any(|(_, wanted, _)| wanted.is_some()),
            ATTRIBUTE_LOCK => attributes.iter().any(|(_, _, wanted, _)| wanted.is_some()),
            CHANGE_GUID => solids.iter().any(|(_, wanted, _)| wanted.is_some()),
            SPLINE_DATA => splines.iter().any(|(_, wanted, _)| wanted.is_some()),
            other => dimension_styles.iter().any(|(_, records)| {
                DIM_APPLICATIONS.iter().zip(&records.wanted).any(|(name, wanted)| *name == other && wanted.is_some())
            }),
        };
        if needed {
            ensure_application(document, name);
        }
    }
    for (handle, wanted, at) in leaders {
        let app = application(document, MLEADER_VERSION);
        if let Some(entity) = document.get_entity_mut(handle) {
            set_record(&mut entity.common_mut().extended_data, MLEADER_VERSION, app, wanted, at);
        }
    }
    for (handle, index, wanted, at) in attributes {
        let app = application(document, ATTRIBUTE_LOCK);
        if let Some(entity) = document.get_entity_mut(handle) {
            match (entity, index) {
                (EntityType::Insert(insert), Some(index)) => {
                    set_record(&mut insert.attributes[index].common.extended_data, ATTRIBUTE_LOCK, app, wanted, at);
                }
                (entity, None) => {
                    set_record(&mut entity.common_mut().extended_data, ATTRIBUTE_LOCK, app, wanted, at);
                }
                _ => {}
            }
        }
    }
    for (handle, wanted, at) in splines {
        let app = application(document, SPLINE_DATA);
        if let Some(entity) = document.get_entity_mut(handle) {
            set_record(&mut entity.common_mut().extended_data, SPLINE_DATA, app, wanted, at);
        }
    }
    for (handle, wanted, at) in solids {
        let app = application(document, CHANGE_GUID);
        if let Some(entity) = document.get_entity_mut(handle) {
            set_record(&mut entity.common_mut().extended_data, CHANGE_GUID, app, wanted, at);
        }
    }
    for (handle, wanted, at) in styles {
        let app = application(document, MLEADER_VERSION);
        with_object(document, handle, |data| set_record(data, MLEADER_VERSION, app, wanted, at));
    }
    let acad = application(document, ACAD);
    for (handle, wanted, at) in visual {
        let block = wanted.then(saved_by_version_block);
        with_object(document, handle, |data| set_block(data, acad, SAVED_BY_VERSION, 2, block, at));
    }
    for (handle, wanted, at) in books {
        with_object(document, handle, |data| set_block(data, acad, BOOK_COLOR, 4, wanted, at));
    }
    for (handle, records) in dimension_styles {
        let apps = DIM_APPLICATIONS.map(|name| application(document, name));
        with_object(document, handle, |data| {
            for (index, wanted) in records.wanted.into_iter().enumerate() {
                if !records.foreign[index] {
                    set_record(data, DIM_APPLICATIONS[index], apps[index], wanted, records.at[index]);
                }
            }
        });
    }
    for (name, index) in [(BLOCK_DESCRIPTION, 0), (BLOCK_ICON, 1)] {
        if block_contents.iter().any(|(_, wanted, _)| wanted[index].is_some()) {
            ensure_application(document, name);
        }
    }
    for (handle, wanted, at) in block_contents {
        let apps = [BLOCK_DESCRIPTION, BLOCK_ICON].map(|name| application(document, name));
        with_object(document, handle, |data| {
            for (index, name) in [BLOCK_DESCRIPTION, BLOCK_ICON].into_iter().enumerate() {
                if let Some(record) = wanted[index].clone() {
                    set_record(data, name, apps[index], Some(record), at[index]);
                }
            }
        });
    }
    for handle in design_center {
        with_object(document, handle, |data| {
            if design_center_units(data).is_some() {
                strip_design_center(data, acad);
            }
        });
    }
}

fn attribute_record(version: DxfVersion, locked: bool) -> Option<Vec<X>> {
    (!holds_attribute_lock(version) && locked).then(|| vec![X::Integer16(0), X::Integer16(1)])
}

/// The `AcDbAttr` record an attribute written as `version` (before R2007) states, when that changes its extended data (`Some(None)`:
/// the record goes). A record of another shape than AutoCAD's is another producer's data: `None`, left as it is.
fn attribute_change(data: &ExtendedData, version: DxfVersion, locked: bool) -> Option<Option<Vec<X>>> {
    let ours = attribute_locked(data);
    if !ours && values(data, ATTRIBUTE_LOCK).is_some() {
        return None;
    }
    let wanted = attribute_record(version, locked);
    ((wanted.is_some() || ours) && values(data, ATTRIBUTE_LOCK) != wanted.as_deref()).then_some(wanted)
}

/// The record a modeler entity of `version` states for its revision GUID, when it changes the extended data. R2013 and later
/// hold the revision natively: a record the model still holds there was not taken in by the reader and stays, as does a record
/// of another shape than AutoCAD's.
fn solid_change(
    changes: &mut Vec<(Handle, Option<Vec<X>>, Option<usize>)>,
    version: DxfVersion,
    document: &CadDocument,
    handle: Handle,
    data: &ExtendedData,
    acis: &crate::entities::AcisData,
) {
    if holds_modeler_revision(version) {
        return;
    }
    let current = revision_of(data);
    if current.is_none() && values(data, CHANGE_GUID).is_some() {
        return;
    }
    let wanted = revision_record(&acis.revision);
    if values(data, CHANGE_GUID) != wanted.as_deref() && (wanted.is_some() || current.is_some()) {
        changes.push((handle, wanted, taken_position(document, handle, CHANGE_GUID)));
    }
}

/// The applications of the records of a dimension style before R2007: the text fill, then the three line types.
const DIM_APPLICATIONS: [&str; 4] = [DIM_FILL, DIM_LINETYPES[0].0, DIM_LINETYPES[1].0, DIM_LINETYPES[2].0];

/// The records of a dimension style written before R2007.
#[derive(Clone, Debug)]
struct DimensionRecords {
    /// Per application of `DIM_APPLICATIONS`: the record to state, `None` for none.
    wanted: [Option<Vec<X>>; 4],
    /// The records of another shape than AutoCAD's: another producer's data, left as they are.
    foreign: [bool; 4],
    /// The positions the file had the records at (when they were taken out of the style).
    at: [Option<usize>; 4],
}

impl DimensionRecords {
    fn of(style: &crate::tables::DimStyle, data: Option<&ExtendedData>, at: [Option<usize>; 4]) -> Self {
        let color = style.dimtfillclr_true_color.unwrap_or_else(|| Color::from_index(style.dimtfillclr));
        // AutoCAD also states the record of a fill that is off when its colour is not the default (autodesk
        // architectural_-_annotation_scaling_and_multileaders.dwg: fill 0, ByLayer), and a drawing written again states the
        // record its file had.
        let fill = (style.dimtfill != 0 || color != Color::ByBlock || at[0].is_some()).then(|| dimension_fill_record(style.dimtfill, color));
        let mut wanted = [fill, None, None, None];
        let handles = [style.dimltex_handle, style.dimltex1_handle, style.dimltex2_handle];
        for (index, (_, code)) in DIM_LINETYPES.iter().enumerate() {
            if !handles[index].is_null() {
                wanted[index + 1] = Some(vec![X::Integer16(*code), X::Handle(handles[index])]);
            }
        }
        let foreign = std::array::from_fn(|index| {
            data.is_some_and(|data| {
                let ours = match index {
                    0 => dimension_fill(data).is_some(),
                    _ => dimension_linetype(data, DIM_LINETYPES[index - 1].0, DIM_LINETYPES[index - 1].1).is_some(),
                };
                !ours && values(data, DIM_APPLICATIONS[index]).is_some()
            })
        });
        Self { wanted, foreign, at }
    }

    /// Whether stating these records changes `data`.
    fn differs_from(&self, data: Option<&ExtendedData>) -> bool {
        (0..4).any(|index| !self.foreign[index] && data.and_then(|data| values(data, DIM_APPLICATIONS[index])) != self.wanted[index].as_deref())
    }
}

fn has_block(data: &ExtendedData, label: &str, block: &[X]) -> bool {
    values(data, ACAD).is_some_and(|record| {
        record
            .iter()
            .position(|value| matches!(value, X::String(text) if text == label))
            .is_some_and(|at| record.get(at..at + block.len()) == Some(block))
    })
}

fn has_label(data: &ExtendedData, label: &str) -> bool {
    values(data, ACAD).is_some_and(|record| record.iter().any(|value| matches!(value, X::String(text) if text == label)))
}

fn design_center_units(data: &ExtendedData) -> Option<i16> {
    crate::tables::block_record::design_center_units(values(data, ACAD)?)
}

fn strip_design_center(data: &mut ExtendedData, acad: Option<u64>) {
    let Some(record) = values(data, ACAD) else { return };
    let units = crate::tables::block_record::design_center_units_values(design_center_units(data).unwrap_or(0));
    if record.get(..units.len()) == Some(units.as_slice()) {
        let rest = record[units.len()..].to_vec();
        set_record(data, ACAD, acad, (!rest.is_empty()).then_some(rest), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{AttributeDefinition, AttributeEntity, Insert, MultiLeader, Region, Solid3D};
    use crate::objects::{BookColor, MultiLeaderStyle, VisualStyle};
    use crate::tables::{AppId, DimStyle};
    use crate::types::Vector3;
    use std::borrow::Cow;

    fn document(version: DxfVersion) -> CadDocument {
        let mut document = CadDocument::with_version(version);
        for name in [
            MLEADER_VERSION,
            ATTRIBUTE_LOCK,
            DIM_FILL,
            DIM_LINETYPES[0].0,
            DIM_LINETYPES[1].0,
            DIM_LINETYPES[2].0,
            CHANGE_GUID,
        ] {
            let mut app = AppId::new(name);
            app.handle = document.allocate_handle();
            document.app_ids.add(app).ok();
        }
        document
    }

    fn record(application: &str, values: Vec<X>) -> ExtendedDataRecord {
        let mut record = ExtendedDataRecord::new(application);
        record.values = values;
        record
    }

    fn written(document: &CadDocument, version: DxfVersion) -> CadDocument {
        let mut prepared = Cow::Borrowed(document);
        prepare_for_write(&mut prepared, version, false);
        prepared.into_owned()
    }

    fn written_as_dxf(document: &CadDocument, version: DxfVersion) -> CadDocument {
        let mut prepared = Cow::Borrowed(document);
        prepare_for_write(&mut prepared, version, true);
        prepared.into_owned()
    }

    /// sample_AC1018.dwg: the attribute definitions state `AcDbAttr {0, 1}` for a locked position, and AutoCAD's R2010 file of
    /// the drawing (sample_AC1024.dwg) states the lock natively and none of the record.
    #[test]
    fn the_attribute_lock_is_restored_from_its_record_and_stated_again_only_below_r2007() {
        let mut old = document(DxfVersion::AC1018);
        let mut definition = AttributeDefinition::new("TAG".into(), "Prompt".into(), "".into());
        definition.common.extended_data.add_record(record(ATTRIBUTE_LOCK, vec![X::Integer16(0), X::Integer16(1)]));
        let handle = old.add_entity(EntityType::AttributeDefinition(definition)).unwrap();
        let mut insert = Insert::new("B", Vector3::ZERO);
        let mut attribute = AttributeEntity::new("TAG".into(), "v".into());
        attribute.common.extended_data.add_record(record(ATTRIBUTE_LOCK, vec![X::Integer16(0), X::Integer16(1)]));
        // another application's data and an unlocked attribute are left alone
        attribute.common.extended_data.add_record(record("PLUGIN", vec![X::Integer16(7)]));
        insert.attributes.push(attribute);
        insert.attributes.push(AttributeEntity::new("OTHER".into(), "w".into()));
        let insert_handle = old.add_entity(EntityType::Insert(insert)).unwrap();
        restore_document(&mut old);
        let locked = |document: &CadDocument| match document.get_entity(handle) {
            Some(EntityType::AttributeDefinition(a)) => a.lock_position,
            _ => panic!("attribute definition"),
        };
        assert!(locked(&old));
        let attributes = |document: &CadDocument| match document.get_entity(insert_handle) {
            Some(EntityType::Insert(i)) => i.attributes.iter().map(|a| a.lock_position).collect::<Vec<_>>(),
            _ => panic!("insert"),
        };
        assert_eq!(attributes(&old), vec![true, false]);
        // R2007 and later state the lock natively: the record goes, the plug-in's record stays.
        let up = written(&old, DxfVersion::AC1024);
        assert!(match up.get_entity(handle) {
            Some(EntityType::AttributeDefinition(a)) => a.common.extended_data.get_record(ATTRIBUTE_LOCK).is_none() && a.lock_position,
            _ => false,
        });
        match up.get_entity(insert_handle) {
            Some(EntityType::Insert(i)) => {
                assert!(i.attributes[0].common.extended_data.get_record(ATTRIBUTE_LOCK).is_none());
                assert!(i.attributes[0].common.extended_data.get_record("PLUGIN").is_some());
                assert!(i.attributes[1].common.extended_data.get_record(ATTRIBUTE_LOCK).is_none());
            }
            _ => panic!("insert"),
        }
        // A native lock written below R2007 is stated again, in AutoCAD's shape; an unlocked attribute states none.
        let mut modern = document(DxfVersion::AC1032);
        let mut definition = AttributeDefinition::new("T".into(), "P".into(), "".into());
        definition.lock_position = true;
        let handle = modern.add_entity(EntityType::AttributeDefinition(definition)).unwrap();
        let mut free = AttributeDefinition::new("F".into(), "P".into(), "".into());
        free.lock_position = false;
        let free_handle = modern.add_entity(EntityType::AttributeDefinition(free)).unwrap();
        let down = written(&modern, DxfVersion::AC1018);
        match down.get_entity(handle) {
            Some(EntityType::AttributeDefinition(a)) => {
                assert_eq!(values(&a.common.extended_data, ATTRIBUTE_LOCK), Some(&[X::Integer16(0), X::Integer16(1)][..]));
            }
            _ => panic!("attribute definition"),
        }
        match down.get_entity(free_handle) {
            Some(EntityType::AttributeDefinition(a)) => assert!(a.common.extended_data.get_record(ATTRIBUTE_LOCK).is_none()),
            _ => panic!("attribute definition"),
        }
        // R2007 keeps the field: nothing is stated.
        let r2007 = written(&modern, DxfVersion::AC1021);
        assert!(match r2007.get_entity(handle) {
            Some(EntityType::AttributeDefinition(a)) => a.common.extended_data.is_empty(),
            _ => false,
        });
    }

    /// A record the model took out is kept with the position the file had it at: the ACadSharp view and a writer of the same
    /// version put it back there, between the other applications' records.
    #[test]
    fn a_record_taken_out_returns_to_the_position_the_file_had_it_at() {
        let mut old = document(DxfVersion::AC1018);
        let mut definition = AttributeDefinition::new("TAG".into(), "P".into(), "".into());
        definition.common.extended_data.add_record(record("FIRST", vec![X::Integer16(1)]));
        definition.common.extended_data.add_record(record(ATTRIBUTE_LOCK, vec![X::Integer16(0), X::Integer16(1)]));
        definition.common.extended_data.add_record(record("LAST", vec![X::Integer16(3)]));
        let handle = old.add_entity(EntityType::AttributeDefinition(definition)).unwrap();
        restore_document(&mut old);
        let names = |document: &CadDocument| match document.get_entity(handle) {
            Some(EntityType::AttributeDefinition(a)) => {
                a.common.extended_data.records().iter().map(|record| record.application_name.clone()).collect::<Vec<_>>()
            }
            _ => panic!("attribute definition"),
        };
        assert_eq!(names(&old), ["FIRST", "LAST"], "the record is not data of the model");
        assert_eq!(old.consumed_xdata.get(&handle).map(|list| list.iter().map(|(index, record)| (*index, record.application_name.as_str())).collect::<Vec<_>>()), Some(vec![(1, ATTRIBUTE_LOCK)]));
        // the drawing written again at its own version has the record back in the middle
        assert_eq!(names(&written(&old, DxfVersion::AC1018)), ["FIRST", ATTRIBUTE_LOCK, "LAST"]);
        // and a drawing of a newer version does not have it
        assert_eq!(names(&written(&old, DxfVersion::AC1024)), ["FIRST", "LAST"]);
    }

    /// sample_AC1018.dwg: every MULTILEADER and MLEADERSTYLE states `ACAD_MLEADERVER 1070 2`; AutoCAD's R2010 file has none.
    #[test]
    fn the_multileader_version_record_exists_below_r2010_only() {
        let mut modern = document(DxfVersion::AC1032);
        modern.dwg_source_version = Some(DxfVersion::AC1032);
        let leader = MultiLeader::new();
        let handle = modern.add_entity(EntityType::MultiLeader(Box::new(leader))).unwrap();
        let mut style = MultiLeaderStyle::new("Standard");
        style.handle = modern.allocate_handle();
        let style_handle = style.handle;
        modern.objects.insert(style_handle, ObjectType::MultiLeaderStyle(style));
        let down = written(&modern, DxfVersion::AC1018);
        match down.get_entity(handle) {
            Some(EntityType::MultiLeader(l)) => {
                assert_eq!(values(&l.common.extended_data, MLEADER_VERSION), Some(&[X::Integer16(2)][..]));
            }
            _ => panic!("multileader"),
        }
        assert_eq!(values(down.object_xdata.get(&style_handle).unwrap(), MLEADER_VERSION), Some(&[X::Integer16(2)][..]));
        // read back: the class version is the record's, and an up-save drops the records
        let mut old = down.clone();
        old.version = DxfVersion::AC1018;
        if let Some(EntityType::MultiLeader(l)) = old.get_entity_mut(handle) {
            l.common.extended_data.records_mut()[0].values = vec![X::Integer16(3)];
            l.dwg_version = 0;
        }
        restore_document(&mut old);
        match old.get_entity(handle) {
            Some(EntityType::MultiLeader(l)) => assert_eq!(l.dwg_version, 3),
            _ => panic!("multileader"),
        }
        let up = written(&old, DxfVersion::AC1024);
        assert!(match up.get_entity(handle) {
            Some(EntityType::MultiLeader(l)) => l.common.extended_data.is_empty(),
            _ => false,
        });
        assert!(up.object_xdata.get(&style_handle).is_none());
        // R2007 still states it
        assert!(written(&modern, DxfVersion::AC1021).object_xdata.get(&style_handle).is_some());
        assert!(written(&modern, DxfVersion::AC1024).object_xdata.get(&style_handle).is_none());
    }

    /// sample_AC1018.dwg, style #994: `ACAD_DSTYLE_DIMTEXT_FILL {376, 2, 377, <24-byte colour>}` for DIMTFILL 2 and the true colour
    /// 0x367542; style #C88: the three `..._LINETYPE` records with the handles of the linetypes.
    #[test]
    fn the_text_fill_and_line_types_of_a_dimension_style_follow_the_version() {
        let fill = vec![
            X::Integer16(376),
            X::Integer16(2),
            X::Integer16(377),
            X::BinaryData(vec![0x18, 0, 0, 0, 1, 0, 0, 0, 0x42, 0x75, 0x36, 0xC2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        ];
        let mut old = document(DxfVersion::AC1018);
        let mut filled = DimStyle::new("Filled");
        filled.handle = old.allocate_handle();
        let mut typed = DimStyle::new("Typed");
        typed.handle = old.allocate_handle();
        let (filled_handle, typed_handle) = (filled.handle, typed.handle);
        old.dim_styles.add(filled).ok();
        old.dim_styles.add(typed).ok();
        let mut data = ExtendedData::new();
        data.add_record(record(DIM_FILL, fill.clone()));
        old.object_xdata.insert(filled_handle, data);
        let mut data = ExtendedData::new();
        for (name, code) in DIM_LINETYPES {
            data.add_record(record(name, vec![X::Integer16(code), X::Handle(Handle::new(0x6D9 + code as u64))]));
        }
        old.object_xdata.insert(typed_handle, data);
        restore_document(&mut old);
        let style = old.dim_styles.get("Filled").unwrap();
        assert_eq!(style.dimtfill, 2);
        assert_eq!(style.dimtfillclr_true_color, Some(Color::from_rgb(0x36, 0x75, 0x42)));
        let style = old.dim_styles.get("Typed").unwrap();
        assert_eq!(
            [style.dimltex_handle, style.dimltex1_handle, style.dimltex2_handle],
            [Handle::new(0x6D9 + 380), Handle::new(0x6D9 + 381), Handle::new(0x6D9 + 382)]
        );
        // R2007 and later hold the fields: the records go. Below, they are stated again from the fields.
        let up = written(&old, DxfVersion::AC1021);
        assert!(up.object_xdata.get(&filled_handle).is_none() && up.object_xdata.get(&typed_handle).is_none());
        let again = written(&up, DxfVersion::AC1018);
        assert_eq!(values(again.object_xdata.get(&filled_handle).unwrap(), DIM_FILL), Some(fill.as_slice()));
        assert_eq!(
            values(again.object_xdata.get(&typed_handle).unwrap(), DIM_LINETYPES[1].0),
            Some(&[X::Integer16(381), X::Handle(Handle::new(0x6D9 + 381))][..])
        );
        // AutoCAD's R12 DXF has no extended data on table records (sample_AC1009_ascii.dxf); R2000 to R2004 DXF files state them
        let r12 = written_as_dxf(&up, DxfVersion::AC1009);
        assert!(r12.object_xdata.get(&filled_handle).is_none() && r12.object_xdata.get(&typed_handle).is_none());
        let r2000 = written_as_dxf(&up, DxfVersion::AC1015);
        assert_eq!(values(r2000.object_xdata.get(&filled_handle).unwrap(), DIM_FILL), Some(fill.as_slice()));
        // A style without a text fill or line types states nothing; an unlocked fill mode (0) states no record.
        let mut plain = DimStyle::new("Plain");
        plain.handle = up.dim_styles.get("Filled").unwrap().handle;
        let mut with_other = ExtendedData::new();
        with_other.add_record(record("ACAD_DSTYLE_DIMJAG", vec![X::Integer16(388), X::Real(1.5)]));
        let mut third = document(DxfVersion::AC1032);
        let mut style = DimStyle::new("Third");
        style.handle = third.allocate_handle();
        let third_handle = style.handle;
        third.dim_styles.add(style).ok();
        third.object_xdata.insert(third_handle, with_other.clone());
        assert!(written(&third, DxfVersion::AC1018).object_xdata.get(&third_handle).unwrap().get_record(DIM_FILL).is_none());
        assert_eq!(written(&third, DxfVersion::AC1018).object_xdata.get(&third_handle).unwrap(), &with_other);
        let _ = plain;
    }

    /// sample_AC1018.dwg: `ACAD_OBJECT_CHANGE_GUID {1, major, minor, minor, 8 bytes}` of the solids and regions; AutoCAD's
    /// R2013 file holds the GUID in the modeler data (the revision of the entity).
    #[test]
    fn the_change_guid_of_a_solid_is_its_modeler_revision_below_r2013() {
        let guid = vec![
            X::Integer16(1),
            X::Integer32(437334824),
            X::Integer16(-5267),
            X::Integer16(-11187),
            X::Integer16(130),
            X::Integer16(77),
            X::Integer16(120),
            X::Integer16(179),
            X::Integer16(54),
            X::Integer16(104),
            X::Integer16(249),
            X::Integer16(231),
        ];
        let mut old = document(DxfVersion::AC1024);
        let mut solid = Solid3D::new();
        solid.common.extended_data.add_record(record(CHANGE_GUID, guid.clone()));
        let solid_handle = old.add_entity(EntityType::Solid3D(solid)).unwrap();
        let mut region = Region::new();
        region.common.extended_data.add_record(record(CHANGE_GUID, vec![X::Integer16(2), X::Integer32(1)]));
        let region_handle = old.add_entity(EntityType::Region(region)).unwrap();
        restore_document(&mut old);
        let revision = match old.get_entity(solid_handle) {
            Some(EntityType::Solid3D(s)) => s.acis_data.revision.clone(),
            _ => panic!("solid"),
        };
        assert!(revision.has_guid);
        assert_eq!(revision.guid_string().unwrap(), "{1a113328-eb6d-d44d-824d-78b33668f9e7}");
        // the identifier of the entity is that GUID, as a reader of a file that holds the revision states it
        match old.get_entity(solid_handle) {
            Some(EntityType::Solid3D(s)) => assert_eq!(s.uid, "{1a113328-eb6d-d44d-824d-78b33668f9e7}"),
            _ => panic!("solid"),
        }
        match old.get_entity(region_handle) {
            Some(EntityType::Region(r)) => assert!(!r.acis_data.revision.has_guid, "a record of another shape is not a GUID"),
            _ => panic!("region"),
        }
        // R2013 and later: the record goes (the revision is native). Below: it is stated again, byte for byte.
        let up = written(&old, DxfVersion::AC1027);
        match up.get_entity(solid_handle) {
            Some(EntityType::Solid3D(s)) => {
                assert!(s.common.extended_data.get_record(CHANGE_GUID).is_none());
                assert_eq!(s.acis_data.revision, revision);
            }
            _ => panic!("solid"),
        }
        let down = written(&up, DxfVersion::AC1018);
        match down.get_entity(solid_handle) {
            Some(EntityType::Solid3D(s)) => assert_eq!(values(&s.common.extended_data, CHANGE_GUID), Some(guid.as_slice())),
            _ => panic!("solid"),
        }
        // the region's record of another shape is another producer's data: it stays below R2013 and above
        match written(&old, DxfVersion::AC1032).get_entity(region_handle) {
            Some(EntityType::Region(r)) => assert!(r.common.extended_data.get_record(CHANGE_GUID).is_some()),
            _ => panic!("region"),
        }
    }

    /// sample_AC1014.dwg to sample_AC1027.dwg: every visual style states `ACAD {AcDbSavedByObjectVersion, 0}`, AutoCAD's R2018
    /// file none. A colour-book object states `DBColXD` (nearest index, packed colour, `book$name`) up to R2013.
    #[test]
    fn the_visual_style_stamp_and_the_colour_book_block_exist_below_r2018() {
        let mut modern = document(DxfVersion::AC1032);
        modern.dwg_source_version = Some(DxfVersion::AC1032);
        let mut style = VisualStyle::new();
        style.handle = modern.allocate_handle();
        let style_handle = style.handle;
        modern.objects.insert(style_handle, ObjectType::VisualStyle(style));
        let mut book = BookColor::new();
        book.handle = modern.allocate_handle();
        book.book_name = "RAL CLASSIC".into();
        book.color_name = "RAL 1006".into();
        book.color = Color::from_rgb(226, 144, 0);
        let book_handle = book.handle;
        modern.objects.insert(book_handle, ObjectType::BookColor(book));
        let down = written(&modern, DxfVersion::AC1027);
        assert_eq!(
            values(down.object_xdata.get(&style_handle).unwrap(), ACAD),
            Some(&[X::String(SAVED_BY_VERSION.into()), X::Integer16(0)][..])
        );
        assert_eq!(
            values(down.object_xdata.get(&book_handle).unwrap(), ACAD),
            Some(
                &[
                    X::String("DBColXD".into()),
                    X::Integer16(42),
                    X::Integer32(0xC2E2_9000u32 as i32),
                    X::String("RAL CLASSIC$RAL 1006".into())
                ][..]
            )
        );
        // AutoCAD's DXF files hold the stamp from R2000 to R2013 (sample_AC1015_ascii.dxf to sample_AC1027_ascii.dxf: 24 of 24
        // visual styles) and none at R2018; they hold the colour block from R2004 (not at R2000: sample_AC1015_ascii.dxf)
        let dxf = written_as_dxf(&modern, DxfVersion::AC1018);
        assert_eq!(dxf.object_xdata.get(&style_handle), down.object_xdata.get(&style_handle));
        assert!(written_as_dxf(&modern, DxfVersion::AC1015).object_xdata.get(&style_handle).is_some());
        assert!(written_as_dxf(&modern, DxfVersion::AC1032).object_xdata.get(&style_handle).is_none());
        assert!(written_as_dxf(&down, DxfVersion::AC1027).object_xdata.get(&style_handle).is_some(), "a DXF keeps the stamp its DWG source had");
        // A drawing read from an R2004 file holds neither (they are what the format needs) and states them again where they were.
        let mut read = down.clone();
        read.version = DxfVersion::AC1018;
        read.dwg_source_version = Some(DxfVersion::AC1018);
        restore_document(&mut read);
        assert!(read.object_xdata.get(&style_handle).is_none() && read.object_xdata.get(&book_handle).is_none());
        assert!(read.consumed_xdata.contains_key(&style_handle) && read.consumed_xdata.contains_key(&book_handle));
        assert_eq!(written(&read, DxfVersion::AC1018).object_xdata.get(&style_handle), down.object_xdata.get(&style_handle));
        assert_eq!(written(&read, DxfVersion::AC1018).object_xdata.get(&book_handle), down.object_xdata.get(&book_handle));
        assert_eq!(written_as_dxf(&read, DxfVersion::AC1018).object_xdata.get(&style_handle), down.object_xdata.get(&style_handle));
        assert!(written(&read, DxfVersion::AC1032).object_xdata.get(&style_handle).is_none());
        assert!(written(&read, DxfVersion::AC1032).object_xdata.get(&book_handle).is_none());
        assert!(dxf.object_xdata.get(&book_handle).is_some());
        assert!(written_as_dxf(&modern, DxfVersion::AC1015).object_xdata.get(&book_handle).is_none());
        assert!(written_as_dxf(&down, DxfVersion::AC1015).object_xdata.get(&book_handle).is_some(), "a DXF keeps the block its DWG source had");
        // R2018 states neither for a drawing of an older version (above: the records were taken in by the reader); a model that
        // holds them although they were not taken in (an R2018 file that states them, cadviewer SE-Barcelona.dwg's colour block, or a
        // document made in memory) keeps them
        let up = written(&down, DxfVersion::AC1032);
        assert_eq!(up.object_xdata.get(&style_handle), down.object_xdata.get(&style_handle));
        assert_eq!(up.object_xdata.get(&book_handle), down.object_xdata.get(&book_handle));
        // the stamp of a drawing read from an older version goes; another application's data in the same ACAD record stays
        let mut kept = down.clone();
        kept.dwg_source_version = Some(DxfVersion::AC1027);
        kept.object_xdata.get_mut(&style_handle).unwrap().records_mut()[0].values.extend([X::String("Other".into()), X::Integer16(5)]);
        let up = written(&kept, DxfVersion::AC1032);
        assert_eq!(values(up.object_xdata.get(&style_handle).unwrap(), ACAD), Some(&[X::String("Other".into()), X::Integer16(5)][..]));
        // a stamp with another value is not AutoCAD's: it stays at R2018, and is not overwritten below
        let mut other = modern.clone();
        let mut data = ExtendedData::new();
        data.add_record(record(ACAD, vec![X::String(SAVED_BY_VERSION.into()), X::Integer16(4)]));
        other.object_xdata.insert(style_handle, data.clone());
        assert_eq!(written(&other, DxfVersion::AC1032).object_xdata.get(&style_handle), Some(&data));
        assert_eq!(written(&other, DxfVersion::AC1027).object_xdata.get(&style_handle), Some(&data));
        // a colour-book object before R2004 holds the nearest index; a drawing written as R2004 or later states the colour of the block
        let mut r2000 = down.clone();
        r2000.version = DxfVersion::AC1015;
        if let Some(ObjectType::BookColor(book)) = r2000.objects.get_mut(&book_handle) {
            book.color = Color::from_index(42);
            book.book_name.clear();
            book.color_name.clear();
        }
        let mut old = Cow::Borrowed(&r2000);
        restore_book_colors(&mut old, DxfVersion::AC1015);
        assert!(matches!(old, Cow::Borrowed(_)), "R2000 has no colour in the object: nothing to state");
        restore_book_colors(&mut old, DxfVersion::AC1018);
        match old.objects.get(&book_handle) {
            Some(ObjectType::BookColor(book)) => {
                assert_eq!(book.color, Color::from_rgb(226, 144, 0));
                assert_eq!((book.book_name.as_str(), book.color_name.as_str()), ("RAL CLASSIC", "RAL 1006"));
            }
            _ => panic!("colour book"),
        }
        // the model of the drawing read keeps the index, as the file said
        match r2000.objects.get(&book_handle) {
            Some(ObjectType::BookColor(book)) => assert_eq!(book.color, Color::from_index(42)),
            _ => panic!("colour book"),
        }
    }

    /// sample_AC1018.dwg: a block record states its units in `ACAD DesignCenter Data`; AutoCAD's R2007 and later files do not.
    #[test]
    fn the_design_center_units_of_a_block_record_do_not_survive_into_r2007() {
        let mut document = document(DxfVersion::AC1018);
        document.dwg_source_version = Some(DxfVersion::AC1018);
        let handle = document.block_records.iter().next().map(|record| record.handle).unwrap();
        let mut data = ExtendedData::new();
        data.add_record(record(ACAD, crate::tables::block_record::design_center_units_values(4)));
        document.object_xdata.insert(handle, data.clone());
        assert_eq!(written(&document, DxfVersion::AC1018).object_xdata.get(&handle), Some(&data));
        assert!(written(&document, DxfVersion::AC1021).object_xdata.get(&handle).is_none());
        // the record of a file of R2007 or later was not taken in by the reader (cadviewer City_base_map.dwg, R2013): it stays
        let mut r2013 = document.clone();
        r2013.version = DxfVersion::AC1027;
        r2013.dwg_source_version = Some(DxfVersion::AC1027);
        assert_eq!(written(&r2013, DxfVersion::AC1027).object_xdata.get(&handle), Some(&data));
        assert_eq!(written(&r2013, DxfVersion::AC1032).object_xdata.get(&handle), Some(&data));
        let mut shared = data;
        shared.records_mut()[0].values.extend([X::String("Other".into())]);
        document.object_xdata.insert(handle, shared);
        assert_eq!(
            values(written(&document, DxfVersion::AC1024).object_xdata.get(&handle).unwrap(), ACAD),
            Some(&[X::String("Other".into())][..])
        );
    }

    /// sample_AC1014.dwg: the blocks #A7E and #77F state the preview image of sample_AC1015.dwg (4,136 bytes: 32 `1004` chunks of
    /// 127 and one of 72) in `CONTENTBLOCKICON 1071 0 ...` and #A7E its description in `CONTENTBLOCKDESCRIPTION 1071 0 1000
    /// <text>`, after the records it had; R2000 states them in the block record (`description`, `preview_data`).
    #[test]
    fn the_description_and_preview_of_a_block_are_records_before_r2000() {
        use crate::tables::BlockRecord;
        let image: Vec<u8> = (0..4136u32).map(|i| (i % 251) as u8).collect();
        let mut old = document(DxfVersion::AC1014);
        old.dwg_source_version = Some(DxfVersion::AC1014);
        for name in [BLOCK_DESCRIPTION, BLOCK_ICON, "OTHER"] {
            let mut app = AppId::new(name);
            app.handle = old.allocate_handle();
            old.app_ids.add(app).ok();
        }
        let mut block = BlockRecord::new("my-dynamic-block");
        block.handle = old.allocate_handle();
        let handle = block.handle;
        old.block_records.add(block).ok();
        let mut data = ExtendedData::new();
        data.add_record(record("OTHER", vec![X::Integer16(1)]));
        data.add_record(record(BLOCK_DESCRIPTION, description_record("My dynamic block description.")));
        data.add_record(record(BLOCK_ICON, icon_record(&image)));
        old.object_xdata.insert(handle, data.clone());
        // the record holds the image in chunks of 127
        let X::BinaryData(last) = data.get_record(BLOCK_ICON).unwrap().values.last().unwrap().clone() else { panic!("chunk") };
        assert_eq!((data.get_record(BLOCK_ICON).unwrap().values.len(), last.len()), (1 + 33, 72));

        restore_document(&mut old);
        let block = old.block_records.iter().find(|record| record.handle == handle).unwrap();
        assert_eq!((block.description.as_str(), block.preview_data.as_slice()), ("My dynamic block description.", image.as_slice()));
        let left = old.object_xdata.get(&handle).unwrap();
        assert!(left.get_record(BLOCK_DESCRIPTION).is_none() && left.get_record(BLOCK_ICON).is_none() && left.get_record("OTHER").is_some());
        assert_eq!(old.consumed_xdata.get(&handle).unwrap().iter().map(|(at, _)| *at).collect::<Vec<_>>(), [1, 2]);

        // written again at R14, the records are where the file had them; R2000 and later state the values in the record
        let again = written(&old, DxfVersion::AC1014);
        assert_eq!(again.object_xdata.get(&handle), Some(&data));
        let names = |version: DxfVersion| written(&old, version).object_xdata.get(&handle).unwrap().records().iter().map(|r| r.application_name.clone()).collect::<Vec<_>>();
        assert_eq!(names(DxfVersion::AC1015), ["OTHER"]);
        assert_eq!(names(DxfVersion::AC1032), ["OTHER"]);

        // an edit of the values is stated in the records the block had
        let mut edited = old.clone();
        edited.block_records.iter_mut().for_each(|record| record.description = "edited".into());
        let again = written(&edited, DxfVersion::AC1014);
        assert_eq!(block_description(again.object_xdata.get(&handle).unwrap()).as_deref(), Some("edited"));
        // a block of a newer drawing gets the records at R14 (R5b-8: they were left out, and an R14 file read back lost the description and
        // the preview; AutoCAD states them after DesignCenter Data under application handles of its own numbering: UNVERIFIED), and R2000
        // states the values in the block record itself
        let mut modern = document(DxfVersion::AC1015);
        let mut block = BlockRecord::new("b");
        block.handle = modern.allocate_handle();
        block.description = "d".into();
        block.preview_data = image[..300].to_vec();
        let handle = block.handle;
        modern.block_records.add(block).ok();
        let mut other = ExtendedData::new();
        other.add_record(record("OTHER", vec![X::Integer16(1)]));
        modern.object_xdata.insert(handle, other);
        let down = written(&modern, DxfVersion::AC1014);
        let data = down.object_xdata.get(&handle).unwrap();
        assert_eq!(block_description(data).as_deref(), Some("d"));
        assert_eq!(block_icon(data).as_deref(), Some(&image[..300]));
        assert!(down.app_ids.get(BLOCK_DESCRIPTION).is_some() && down.app_ids.get(BLOCK_ICON).is_some());
        assert_eq!(data.records().len(), 3, "the record it had and the two new ones");
        let same = written(&modern, DxfVersion::AC1015);
        assert_eq!(same.object_xdata.get(&handle).unwrap().records().len(), 1);
        assert!(same.app_ids.get(BLOCK_ICON).is_none());
        // records of another shape are another producer's data: not taken in
        let mut foreign = document(DxfVersion::AC1014);
        let mut app = AppId::new(BLOCK_ICON);
        app.handle = foreign.allocate_handle();
        foreign.app_ids.add(app).ok();
        let mut block = BlockRecord::new("f");
        block.handle = foreign.allocate_handle();
        let foreign_handle = block.handle;
        foreign.block_records.add(block).ok();
        let mut data = ExtendedData::new();
        data.add_record(record(BLOCK_ICON, vec![X::Integer32(0), X::String("not an image".into())]));
        foreign.object_xdata.insert(foreign_handle, data.clone());
        restore_document(&mut foreign);
        assert_eq!(foreign.object_xdata.get(&foreign_handle), Some(&data));
        assert!(foreign.block_records.iter().find(|record| record.handle == foreign_handle).unwrap().preview_data.is_empty());

        // nor are an empty description (no value the writer would state again), image bytes in chunks of another size than
        // AutoCAD's 127 (the record would not be written back byte for byte), or a record of a block that already has the value
        // (a DXF of another producer that also states group 4 or 310): each stays data of the file
        let mut app = AppId::new(BLOCK_DESCRIPTION);
        app.handle = foreign.allocate_handle();
        foreign.app_ids.add(app).ok();
        let cases: [(Vec<(&str, Vec<X>)>, &str, &[u8]); 4] = [
            (vec![(BLOCK_DESCRIPTION, description_record(""))], "", &[]),
            (vec![(BLOCK_ICON, std::iter::once(X::Integer32(0)).chain(image[..300].chunks(100).map(|chunk| X::BinaryData(chunk.to_vec()))).collect())], "", &[]),
            (vec![(BLOCK_DESCRIPTION, description_record("from the record"))], "from group 4", &[]),
            (vec![(BLOCK_ICON, icon_record(&image[..300]))], "", &[7, 7, 7]),
        ];
        for (index, (records, description, preview)) in cases.into_iter().enumerate() {
            let mut block = BlockRecord::new(&format!("kept{index}"));
            block.handle = foreign.allocate_handle();
            block.description = description.into();
            block.preview_data = preview.to_vec();
            let kept_handle = block.handle;
            foreign.block_records.add(block).ok();
            let mut data = ExtendedData::new();
            for (name, values) in records {
                data.add_record(record(name, values));
            }
            foreign.object_xdata.insert(kept_handle, data.clone());
            restore_document(&mut foreign);
            assert_eq!(foreign.object_xdata.get(&kept_handle), Some(&data), "case {index}");
            assert!(foreign.consumed_xdata.get(&kept_handle).is_none(), "case {index}");
            let block = foreign.block_records.iter().find(|record| record.handle == kept_handle).unwrap();
            assert_eq!((block.description.as_str(), block.preview_data.as_slice()), (description, preview), "case {index}");
            assert_eq!(written(&foreign, DxfVersion::AC1014).object_xdata.get(&kept_handle), Some(&data), "case {index}");
        }
    }

    /// A version that holds a value natively has no record for it, so a record the model holds there was not taken in by the
    /// reader (a file of that version states it: the corpus has `ACAD_DSTYLE_DIMTEXT_FILL` in an R2007 file, `DBColXD` in an R2018
    /// one, `DesignCenter Data` in an R2013 one): a drawing written again at that version keeps it.
    #[test]
    fn a_record_the_reader_did_not_take_in_is_kept_by_a_version_that_holds_the_value() {
        let fill = vec![
            X::Integer16(376),
            X::Integer16(0),
            X::Integer16(377),
            X::BinaryData(vec![0x18, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        ];
        let guid = vec![X::Integer16(1), X::Integer32(7), X::Integer16(1), X::Integer16(2)]
            .into_iter()
            .chain((0..8).map(X::Integer16))
            .collect::<Vec<_>>();
        let mut read = document(DxfVersion::AC1032);
        read.dwg_source_version = Some(DxfVersion::AC1032);
        let mut definition = AttributeDefinition::new("T".into(), "P".into(), "".into());
        definition.common.extended_data.add_record(record(ATTRIBUTE_LOCK, vec![X::Integer16(0), X::Integer16(1)]));
        let attribute = read.add_entity(EntityType::AttributeDefinition(definition)).unwrap();
        let mut leader = MultiLeader::new();
        leader.common.extended_data.add_record(record(MLEADER_VERSION, vec![X::Integer16(2)]));
        let leader = read.add_entity(EntityType::MultiLeader(Box::new(leader))).unwrap();
        let mut solid = Solid3D::new();
        solid.common.extended_data.add_record(record(CHANGE_GUID, guid));
        let solid = read.add_entity(EntityType::Solid3D(solid)).unwrap();
        let mut style = DimStyle::new("Filled");
        style.handle = read.allocate_handle();
        let style_handle = style.handle;
        read.dim_styles.add(style).ok();
        let mut data = ExtendedData::new();
        data.add_record(record(DIM_FILL, fill));
        read.object_xdata.insert(style_handle, data.clone());
        let mut mleader_style = MultiLeaderStyle::new("Standard");
        mleader_style.handle = read.allocate_handle();
        let mleader_style_handle = mleader_style.handle;
        read.objects.insert(mleader_style_handle, ObjectType::MultiLeaderStyle(mleader_style));
        let mut stamp = ExtendedData::new();
        stamp.add_record(record(MLEADER_VERSION, vec![X::Integer16(2)]));
        read.object_xdata.insert(mleader_style_handle, stamp.clone());
        let again = written(&read, DxfVersion::AC1032);
        for handle in [attribute, leader, solid] {
            assert_eq!(again.get_entity(handle).map(|e| e.common().extended_data.clone()), read.get_entity(handle).map(|e| e.common().extended_data.clone()));
        }
        assert_eq!(again.object_xdata.get(&style_handle), Some(&data));
        assert_eq!(again.object_xdata.get(&mleader_style_handle), Some(&stamp));
        // R2007 (the fill and the lock) and R2013 (the revision) likewise
        let mut r2007 = read.clone();
        r2007.version = DxfVersion::AC1021;
        r2007.dwg_source_version = Some(DxfVersion::AC1021);
        assert_eq!(written(&r2007, DxfVersion::AC1021).object_xdata.get(&style_handle), Some(&data));
        assert!(model_record(&written(&r2007, DxfVersion::AC1021), attribute, ATTRIBUTE_LOCK));
        assert!(model_record(&written(&read, DxfVersion::AC1027), solid, CHANGE_GUID));
    }

    fn model_record(document: &CadDocument, handle: Handle, application: &str) -> bool {
        document.get_entity(handle).is_some_and(|entity| entity.common().extended_data.get_record(application).is_some())
    }

    /// A record of another shape than AutoCAD's is another producer's data: a writer of a version below the one that holds the
    /// value neither replaces it by the record of the native value nor drops it.
    #[test]
    fn a_record_of_another_shape_is_never_rewritten() {
        let mut modern = document(DxfVersion::AC1032);
        let mut definition = AttributeDefinition::new("T".into(), "P".into(), "".into());
        definition.lock_position = true;
        let other_lock = record(ATTRIBUTE_LOCK, vec![X::Integer16(0), X::Integer16(0), X::Integer16(9)]);
        definition.common.extended_data.add_record(other_lock.clone());
        let attribute = modern.add_entity(EntityType::AttributeDefinition(definition)).unwrap();
        let mut leader = MultiLeader::new();
        let other_version = record(MLEADER_VERSION, vec![X::Integer16(2), X::Integer16(5)]);
        leader.common.extended_data.add_record(other_version.clone());
        let leader = modern.add_entity(EntityType::MultiLeader(Box::new(leader))).unwrap();
        let mut solid = Solid3D::new();
        solid.acis_data.revision.has_guid = true;
        let other_guid = record(CHANGE_GUID, vec![X::Integer16(2), X::Integer32(1)]);
        solid.common.extended_data.add_record(other_guid.clone());
        let solid = modern.add_entity(EntityType::Solid3D(solid)).unwrap();
        let mut style = DimStyle::new("Typed");
        style.handle = modern.allocate_handle();
        style.dimltex_handle = Handle::new(0x6D9);
        let style_handle = style.handle;
        modern.dim_styles.add(style).ok();
        let mut data = ExtendedData::new();
        data.add_record(record(DIM_LINETYPES[0].0, vec![X::Integer16(380), X::String("DASHED".into())]));
        modern.object_xdata.insert(style_handle, data.clone());
        let down = written(&modern, DxfVersion::AC1018);
        let records = |handle: Handle| down.get_entity(handle).unwrap().common().extended_data.records().to_vec();
        assert_eq!(records(attribute), vec![other_lock]);
        assert_eq!(records(leader), vec![other_version]);
        assert_eq!(records(solid), vec![other_guid]);
        assert_eq!(down.object_xdata.get(&style_handle), Some(&data));
    }

    /// A text fill that is off with a colour that is not the default is stated as AutoCAD states it (fill 0, the colour), and a
    /// drawing read with such a record writes it again where the file had it: the colour is not lost.
    #[test]
    fn a_text_fill_that_is_off_keeps_its_colour() {
        let mut modern = document(DxfVersion::AC1032);
        let mut style = DimStyle::new("Off");
        style.handle = modern.allocate_handle();
        style.dimtfillclr = 256;
        let handle = style.handle;
        modern.dim_styles.add(style).ok();
        let off = vec![
            X::Integer16(376),
            X::Integer16(0),
            X::Integer16(377),
            X::BinaryData(vec![0x18, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        ];
        let down = written(&modern, DxfVersion::AC1018);
        assert_eq!(down.object_xdata.get(&handle).and_then(|data| values(data, DIM_FILL)), Some(off.as_slice()));
        let mut read = down.clone();
        read.version = DxfVersion::AC1018;
        read.dwg_source_version = Some(DxfVersion::AC1018);
        restore_document(&mut read);
        assert_eq!(read.dim_styles.get("Off").map(|style| (style.dimtfill, style.dimtfillclr)), Some((0, 256)));
        assert_eq!(written(&read, DxfVersion::AC1018).object_xdata.get(&handle).and_then(|data| values(data, DIM_FILL)), Some(off.as_slice()));
        // the default (off, ByBlock) states nothing
        let mut plain = document(DxfVersion::AC1032);
        let mut style = DimStyle::new("Plain");
        style.handle = plain.allocate_handle();
        let plain_handle = style.handle;
        plain.dim_styles.add(style).ok();
        assert!(written(&plain, DxfVersion::AC1018).object_xdata.get(&plain_handle).is_none());
    }

    /// The `DBColXD` block of a colour-book object read before R2004 names the object's index: once the index is changed, the
    /// block is stale and neither restores the old colour nor is written again.
    #[test]
    fn an_edited_colour_book_object_does_not_take_its_old_block_back() {
        let mut old = document(DxfVersion::AC1015);
        old.dwg_source_version = Some(DxfVersion::AC1015);
        let mut book = BookColor::new();
        book.handle = old.allocate_handle();
        book.color = Color::from_index(42);
        let handle = book.handle;
        old.objects.insert(handle, ObjectType::BookColor(book));
        let mut data = ExtendedData::new();
        data.add_record(record(
            ACAD,
            vec![X::String(BOOK_COLOR.into()), X::Integer16(42), X::Integer32(0xC2E2_9000u32 as i32), X::String("RAL CLASSIC$RAL 1006".into())],
        ));
        old.object_xdata.insert(handle, data.clone());
        restore_document(&mut old);
        assert!(old.consumed_xdata.contains_key(&handle));
        // unedited: the same version states the block again, R2004 the colour of the block
        assert_eq!(written(&old, DxfVersion::AC1015).object_xdata.get(&handle), Some(&data));
        let mut up = Cow::Borrowed(&old);
        restore_book_colors(&mut up, DxfVersion::AC1018);
        assert!(matches!(up.objects.get(&handle), Some(ObjectType::BookColor(book)) if book.color == Color::from_rgb(226, 144, 0)));
        // edited: neither
        let mut edited = old.clone();
        if let Some(ObjectType::BookColor(book)) = edited.objects.get_mut(&handle) {
            book.color = Color::from_index(5);
        }
        assert!(written(&edited, DxfVersion::AC1015).object_xdata.get(&handle).is_none());
        let mut up = Cow::Borrowed(&edited);
        restore_book_colors(&mut up, DxfVersion::AC1018);
        assert!(matches!(up, Cow::Borrowed(_)));
    }
}
