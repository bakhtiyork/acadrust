//! The `DSTYLE` extended data of dimensions and leaders in the R14 form (R5b-19).
//!
//! A dimension or leader that overrides variables of its dimension style keeps them in the `ACAD` application's `DSTYLE` block
//! (`1000 DSTYLE 1002 { (1070 <group code of the variable> <value>)... 1002 }`). AutoCAD's own files of one drawing
//! (sample_AC1014.dwg and sample_AC1015.dwg to sample_AC1032.dwg, entities #512 to #527) state the block in two forms:
//!
//! | | R2000 and later | R14 |
//! |---|---|---|
//! | arrowhead blocks (DIMBLK1, DIMBLK2) | `343` / `344` `1005` the block record's handle (aligned dimension #527) | `6` / `7` `1000` the R14 name of the block (`ARCHTICK` for `_ArchTick`) |
//! | DIMUNIT | none | `270` first in the block when the entity states DIMLUNIT: 4 for DIMLUNIT 4 with DIMFRAC 1 (#527; its style ISO-25 has DIMUNIT 2) |
//! | angular precision | the style's DIMADEC (0 for ISO-25, DIMDEC 2): no block (#515, #516) | `271` `1070 0`: R14 has no DIMADEC, so the dimension states its DIMDEC (#515, #516) |
//! | leader arrowhead | the style's DIMLDRBLK (none for ISO-25): no extended data (#512) | `5` `1000 ""`: R14 has no DIMLDRBLK, the leader states its arrowhead as DIMBLK (#512) |
//!
//! The native model holds the R2000 form (the form of the format that has the fields). A reader of an R13 / R14 file restores it
//! when [`to_r14`] states the block it read exactly; the writers of R13 / R14 and the ACadSharp view of such a file state the R14
//! form again from the model, so a file read and written at R14 is unchanged, an R14 file written for a later version carries no
//! stale R14 record and a down-save to R14 carries what AutoCAD's does. Anything the samples do not show is left as the file had
//! it: a name no block record has, a 271 on a dimension that is not angular, a 270 that is not the derived unit. Such a block is
//! remembered with the pairs read (`CadDocument::r14_dimension_overrides_as_read`) and stated as read while the entity holds them;
//! an entity edited after the read, or added, gets the R14 form of what it holds (an edit after the read wins).
//!
//! Evidence: the only AutoCAD files of R14 or earlier with `DSTYLE` data on entities are sample_AC1014.dwg and AutoCAD's R12 DXF
//! twins sample_AC1009_ascii.dxf / sample_AC1009_binary.dxf (the six entities #512 #515 #516 #51F #522 #527 of one drawing, one
//! dimension style ISO-25); the corpus's R12 to R14 files (nextgis arc_R14.dwg, line_R14.dwg, arc_R12.dxf; cadviewer ce.dxf) have
//! no dimension or leader. Every derivation below is from that one drawing: the R14 name of arrowheads other than `_BoxBlank` and
//! `_ArchTick`, DIMUNIT for units other than decimal and architectural stacked, the leader's `5` for a style with a DIMLDRBLK, and
//! the position of `5` / `271` among other overrides (appended) are UNVERIFIED.
#[cfg(feature = "acadsharp-compat")]
use std::borrow::Cow;

use crate::document::CadDocument;
use crate::entities::{Dimension, EntityType};
use crate::tables::DimStyle;
use crate::types::{DxfVersion, Handle};
use crate::xdata::{ExtendedData, ExtendedDataRecord, XDataValue as X};

const APPLICATION: &str = "ACAD";
const DSTYLE: &str = "DSTYLE";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Leader,
    Angular,
    Other,
}

type Pairs = Vec<(i16, X)>;

fn text(value: &X) -> Option<&str> {
    match value {
        X::String(text) | X::ControlString(text) => Some(text.trim_end_matches('\0')),
        _ => None,
    }
}

/// The range of the `DSTYLE` block of `values`, from its name to after its closing brace: `Ok(None)` for no block, `Err` for a block
/// that does not have the shape AutoCAD writes (the entity is left as it is).
fn block_range(values: &[X]) -> Result<Option<(usize, usize)>, ()> {
    let Some(start) = values.iter().position(|value| matches!(value, X::String(name) if name.trim_end_matches('\0') == DSTYLE)) else {
        return Ok(None);
    };
    if values.get(start + 1).and_then(text) != Some("{") {
        return Err(());
    }
    let close = values[start + 2..].iter().position(|value| matches!(value, X::ControlString(_))).ok_or(())? + start + 2;
    if text(&values[close]) == Some("}") { Ok(Some((start, close + 1))) } else { Err(()) }
}

/// The `(group code, value)` pairs of a `DSTYLE` block.
fn block_pairs(values: &[X]) -> Option<Pairs> {
    let inner = &values[2..values.len() - 1];
    if inner.len() % 2 != 0 {
        return None;
    }
    inner
        .chunks(2)
        .map(|pair| match (&pair[0], &pair[1]) {
            (X::Integer16(code), value) => Some((*code, value.clone())),
            _ => None,
        })
        .collect()
}

fn block_values(pairs: &Pairs) -> Vec<X> {
    let mut values = vec![X::String(DSTYLE.to_string()), X::ControlString("{".to_string())];
    for (code, value) in pairs {
        values.push(X::Integer16(*code));
        values.push(value.clone());
    }
    values.push(X::ControlString("}".to_string()));
    values
}

/// The name R14 states for the arrowhead block `handle` (the empty name for no block); `None` for a handle no block record has.
fn arrow_name(document: &CadDocument, handle: Handle) -> Option<String> {
    if handle.is_null() {
        return Some(String::new());
    }
    let block = document.block_records.iter().find(|block| block.handle == handle)?;
    Some(super::autocad_records::r14_arrow_name(&block.name).map_or_else(|| block.name.clone(), str::to_string))
}

/// The arrowhead block an R14 name stands for (none for the empty name); `None` when no block record has it.
fn arrow_handle(document: &CadDocument, name: &str) -> Option<Handle> {
    if name.is_empty() {
        return Some(Handle::NULL);
    }
    document
        .block_records
        .iter()
        .find(|block| super::autocad_records::r14_arrow_name(&block.name).map_or(block.name == name, |r14| r14.eq_ignore_ascii_case(name)))
        .map(|block| block.handle)
}

/// The R14 DIMUNIT of a linear unit format and fraction format: only the pairs AutoCAD's files show (decimal; architectural, stacked).
fn unit(lunit: i16, frac: i16) -> Option<i16> {
    match lunit {
        2 => Some(2),
        4 if frac != 2 => Some(4),
        _ => None,
    }
}

fn find(pairs: &Pairs, code: i16) -> Option<&X> {
    pairs.iter().find(|(found, _)| *found == code).map(|(_, value)| value)
}

fn integer(pairs: &Pairs, code: i16) -> Option<i16> {
    match find(pairs, code) {
        Some(X::Integer16(value)) => Some(*value),
        _ => None,
    }
}

/// The unit the block's DIMLUNIT / DIMFRAC give (the style's where the block does not state one).
fn block_unit(pairs: &Pairs, style: &DimStyle) -> Option<i16> {
    unit(integer(pairs, 277).unwrap_or(style.dimlunit), integer(pairs, 276).unwrap_or(style.dimfrac))
}

/// The block in the R14 form, from the block in the R2000 form (`None` when a handle names no block record).
fn to_r14(document: &CadDocument, kind: Kind, style: &DimStyle, pairs: &Pairs) -> Option<Pairs> {
    let mut out: Pairs = Vec::with_capacity(pairs.len() + 2);
    for (code, value) in pairs {
        out.push(match (*code, value) {
            (342..=344, X::Handle(handle)) => (*code - 337, X::String(arrow_name(document, *handle)?)),
            (341, X::Handle(handle)) if kind == Kind::Leader => (5, X::String(arrow_name(document, *handle)?)),
            _ => (*code, value.clone()),
        });
    }
    if out.iter().filter(|(code, _)| *code == 5).count() > 1 {
        return None;
    }
    if find(&out, 270).is_none() && (find(&out, 276).is_some() || find(&out, 277).is_some()) {
        if let (Some(stated), Some(held)) = (block_unit(&out, style), unit(style.dimlunit, style.dimfrac)) {
            if stated != held {
                out.insert(0, (270, X::Integer16(stated)));
            }
        }
    }
    if kind == Kind::Angular && find(&out, 271).is_none() && style.dimadec >= 0 && style.dimadec != style.dimdec {
        out.push((271, X::Integer16(style.dimadec)));
    }
    if kind == Kind::Leader && find(&out, 5).is_none() {
        out.push((5, X::String(arrow_name(document, style.dimldrblk)?)));
    }
    Some(out)
}

/// The block in the R2000 form, from the block in the R14 form; `None` when the block is not one [`to_r14`] states exactly.
fn from_r14(document: &CadDocument, kind: Kind, style: &DimStyle, pairs: &Pairs) -> Option<Pairs> {
    let mut out: Pairs = Vec::with_capacity(pairs.len());
    for (code, value) in pairs {
        match (*code, value) {
            (270, X::Integer16(stated)) => {
                if block_unit(pairs, style) != Some(*stated) {
                    return None;
                }
            }
            (271, X::Integer16(stated)) if kind == Kind::Angular => {
                if *stated != style.dimadec {
                    return None;
                }
            }
            (5, X::String(name)) if kind == Kind::Leader => {
                let handle = arrow_handle(document, name.trim_end_matches('\0'))?;
                if handle != style.dimldrblk {
                    out.push((341, X::Handle(handle)));
                }
            }
            (5..=7, X::String(name)) => out.push((*code + 337, X::Handle(arrow_handle(document, name.trim_end_matches('\0'))?))),
            _ => out.push((*code, value.clone())),
        }
    }
    (to_r14(document, kind, style, &out)? == *pairs).then_some(out)
}

fn kind_and_style<'a>(document: &'a CadDocument, entity: &EntityType) -> Option<(Kind, &'a DimStyle)> {
    let (kind, style) = match entity {
        EntityType::Leader(leader) => (Kind::Leader, &leader.dimension_style),
        EntityType::Dimension(dimension) => {
            let kind = match &**dimension {
                Dimension::Angular2Ln(_) | Dimension::Angular3Pt(_) => Kind::Angular,
                _ => Kind::Other,
            };
            (kind, &dimension.base().style_name)
        }
        _ => return None,
    };
    Some((kind, document.dim_styles.get(style)?))
}

/// The `ACAD` record of `data` and the range of its `DSTYLE` block, with the block's pairs (none for no block).
fn current(data: &ExtendedData) -> Option<(Option<(usize, Option<(usize, usize)>)>, Pairs)> {
    let Some(record) = data.records().iter().position(|record| record.application_name.eq_ignore_ascii_case(APPLICATION)) else {
        return Some((None, Pairs::new()));
    };
    match block_range(&data.records()[record].values).ok()? {
        Some((start, end)) => {
            let pairs = block_pairs(&data.records()[record].values[start..end])?;
            Some((Some((record, Some((start, end)))), pairs))
        }
        None => Some((Some((record, None)), Pairs::new())),
    }
}

/// `data` with its `DSTYLE` block replaced by `pairs` (removed when `pairs` is empty, and the `ACAD` record with it when nothing
/// else is left; added at the end of the record, or in a record in front of the others, when there is none). The verbatim block of
/// the application is dropped so the writer states the record.
fn with_pairs(data: &ExtendedData, application: Option<u64>, position: Option<(usize, Option<(usize, usize)>)>, pairs: &Pairs) -> ExtendedData {
    let mut result = data.clone();
    match (position, pairs.is_empty()) {
        (None, true) => {}
        (None, false) => {
            let mut record = ExtendedDataRecord::new(APPLICATION);
            record.values = block_values(pairs);
            result.insert_record(0, record);
        }
        (Some((record, range)), empty) => {
            let values = &mut result.records_mut()[record].values;
            match (range, empty) {
                (Some((start, end)), true) => {
                    values.drain(start..end);
                }
                (Some((start, end)), false) => {
                    values.splice(start..end, block_values(pairs));
                }
                (None, false) => values.extend(block_values(pairs)),
                (None, true) => {}
            }
            if result.records()[record].values.is_empty() {
                result.remove_record_ignore_case(APPLICATION);
            }
        }
    }
    if let Some(application) = application {
        result.raw_dwg_eed.retain(|(app, _)| *app != application);
    }
    result
}

fn acad_handle(document: &CadDocument) -> Option<u64> {
    document.app_ids.get(APPLICATION).map(|app| app.handle.value())
}

/// Restore the R2000 form of the `DSTYLE` block of every dimension and leader of a drawing read from R13 / R14; a block [`from_r14`]
/// does not restore is remembered with the pairs read. Run once per read: a second run would take the restored blocks for blocks the
/// samples do not explain (the DWG reader states its post-read passes twice; this one is called from the first only).
pub(crate) fn restore_document(document: &mut CadDocument) {
    if document.version > DxfVersion::AC1014 {
        return;
    }
    let application = acad_handle(document);
    let mut changes: Vec<(usize, ExtendedData)> = Vec::new();
    let mut as_read: Vec<(Handle, Pairs)> = Vec::new();
    for (index, entity) in document.entities.iter().enumerate() {
        let Some((kind, style)) = kind_and_style(document, entity) else { continue };
        let data = &entity.common().extended_data;
        let Some((position, pairs)) = current(data) else { continue };
        match from_r14(document, kind, style, &pairs) {
            Some(restored) if restored != pairs => changes.push((index, with_pairs(data, application, position, &restored))),
            Some(_) => {}
            None => as_read.push((entity.common().handle, pairs)),
        }
    }
    for (index, data) in changes {
        std::sync::Arc::make_mut(&mut document.entities[index]).common_mut().extended_data = data;
    }
    document.r14_dimension_overrides_as_read.extend(as_read);
}

/// Whether the drawing was read from a file of R14 or earlier (a DWG or a DXF: AutoCAD's R12 DXF holds the R14 form).
#[cfg(feature = "acadsharp-compat")]
fn read_at_r14(document: &CadDocument) -> bool {
    document.dwg_source_version.or(document.dxf_source_version).is_some_and(|version| version <= DxfVersion::AC1014)
}

/// The extended data of `entity` as an R13 / R14 file states it, when that differs from `data` (its extended data in the model):
/// none for a block the reader of an R13 / R14 file left as the file had it, while the entity holds it unchanged.
pub(crate) fn r14_form(document: &CadDocument, entity: &EntityType, data: &ExtendedData) -> Option<ExtendedData> {
    let (kind, style) = kind_and_style(document, entity)?;
    let (position, pairs) = current(data)?;
    if document.r14_dimension_overrides_as_read.get(&entity.common().handle).is_some_and(|read| *read == pairs) {
        return None;
    }
    let stated = to_r14(document, kind, style, &pairs)?;
    (stated != pairs).then(|| with_pairs(data, acad_handle(document), position, &stated))
}

/// [`r14_form`] for the ACadSharp view of a drawing read from R13 / R14.
#[cfg(feature = "acadsharp-compat")]
pub(crate) fn view_data<'a>(document: &CadDocument, handle: Handle, data: &'a ExtendedData) -> Cow<'a, ExtendedData> {
    if !read_at_r14(document) {
        return Cow::Borrowed(data);
    }
    match document.get_entity(handle).and_then(|entity| r14_form(document, entity, data)) {
        Some(stated) => Cow::Owned(stated),
        None => Cow::Borrowed(data),
    }
}

/// A document as a DXF of `version` states it: the R14 form of the `DSTYLE` data of its dimensions and leaders in a DXF of R14 or
/// earlier (AutoCAD's R12 DXF holds it: sample_AC1009_ascii.dxf), the R2000 form of the model in a later one.
pub(crate) fn prepare_for_dxf(document: &mut std::borrow::Cow<'_, CadDocument>, version: DxfVersion) {
    if version > DxfVersion::AC1014 {
        return;
    }
    let changes: Vec<(usize, ExtendedData)> = document
        .entities
        .iter()
        .enumerate()
        .filter_map(|(index, entity)| Some((index, r14_form(document, entity, &entity.common().extended_data)?)))
        .collect();
    if changes.is_empty() {
        return;
    }
    let document = document.to_mut();
    for (index, data) in changes {
        std::sync::Arc::make_mut(&mut document.entities[index]).common_mut().extended_data = data;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::BlockRecord;

    fn document() -> (CadDocument, Handle) {
        let mut document = CadDocument::with_version(DxfVersion::AC1014);
        let mut tick = BlockRecord::new("_ArchTick");
        tick.handle = document.allocate_handle();
        let handle = tick.handle;
        document.block_records.add(tick).unwrap();
        (document, handle)
    }

    fn style() -> DimStyle {
        let mut style = DimStyle::new("ISO-25");
        style.dimdec = 2;
        style.dimadec = 0;
        style.dimlunit = 2;
        style
    }

    /// AutoCAD's block of the aligned dimension #527 in the R2000 form and in the R14 form (sample_AC1015.dwg, sample_AC1014.dwg).
    #[test]
    fn the_aligned_dimension_of_the_samples_converts_both_ways() {
        let (document, tick) = document();
        let style = style();
        let r2000: Pairs = vec![
            (276, X::Integer16(1)),
            (78, X::Integer16(8)),
            (277, X::Integer16(4)),
            (44, X::Real(2.0)),
            (176, X::Integer16(1)),
            (343, X::Handle(tick)),
            (173, X::Integer16(1)),
            (344, X::Handle(tick)),
        ];
        let r14 = to_r14(&document, Kind::Other, &style, &r2000).unwrap();
        let codes: Vec<i16> = r14.iter().map(|(code, _)| *code).collect();
        assert_eq!(codes, [270, 276, 78, 277, 44, 176, 6, 173, 7]);
        assert_eq!((r14[0].1.clone(), r14[6].1.clone()), (X::Integer16(4), X::String("ARCHTICK".into())));
        assert_eq!(from_r14(&document, Kind::Other, &style, &r14), Some(r2000));
    }

    /// The pairs R14 states that [`to_r14`] would not state from what the block restores to are not restored.
    #[test]
    fn a_block_to_r14_does_not_state_exactly_is_not_restored() {
        let (document, _) = document();
        let style = style();
        // 270 that is not the unit the block's DIMLUNIT gives
        let wrong_unit: Pairs = vec![(270, X::Integer16(5)), (277, X::Integer16(4))];
        assert_eq!(from_r14(&document, Kind::Other, &style, &wrong_unit), None);
        // 271 other than the style's DIMADEC, and on a dimension that is not angular
        assert_eq!(from_r14(&document, Kind::Angular, &style, &vec![(271, X::Integer16(3))]), None);
        assert_eq!(from_r14(&document, Kind::Other, &style, &vec![(271, X::Integer16(0))]), Some(vec![(271, X::Integer16(0))]));
        // a block name no block record has
        assert_eq!(from_r14(&document, Kind::Other, &style, &vec![(6, X::String("NOSUCH".into()))]), None);
        // a leader without the 5 that to_r14 always adds
        assert_eq!(from_r14(&document, Kind::Leader, &style, &Pairs::new()), None);
        assert_eq!(from_r14(&document, Kind::Leader, &style, &vec![(5, X::String(String::new()))]), Some(Pairs::new()));
    }

    /// DIMADEC equal to DIMDEC, or a DIMLUNIT the style has already, needs no record.
    #[test]
    fn nothing_is_added_that_the_style_already_states() {
        let (document, _) = document();
        let mut equal = style();
        equal.dimadec = 2;
        assert_eq!(to_r14(&document, Kind::Angular, &equal, &Pairs::new()), Some(Pairs::new()));
        let same_unit: Pairs = vec![(277, X::Integer16(2))];
        assert_eq!(to_r14(&document, Kind::Other, &style(), &same_unit), Some(same_unit));
        // a dimension is not given the leader's arrowhead
        assert_eq!(to_r14(&document, Kind::Other, &style(), &Pairs::new()), Some(Pairs::new()));
    }

    #[test]
    fn a_block_that_is_not_the_shape_autocad_writes_is_not_touched() {
        let open = vec![X::String("DSTYLE".into()), X::ControlString("{".into()), X::Integer16(271)];
        assert!(block_range(&open).is_err());
        let nested = vec![X::String("DSTYLE".into()), X::ControlString("{".into()), X::Integer16(271), X::ControlString("{".into())];
        assert!(block_range(&nested).is_err(), "a block whose next control string is not the closing brace");
        let odd = vec![X::String("DSTYLE".into()), X::ControlString("{".into()), X::Integer16(271), X::ControlString("}".into())];
        assert!(block_pairs(&odd).is_none());
        assert_eq!(block_range(&[X::String("OTHER".into())]), Ok(None));
    }
}
