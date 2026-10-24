//! The round-trip extended data of AutoCAD's own down-saves.
//!
//! An R14, R2000 or R2004 drawing that AutoCAD saved from a later version keeps what its entity records cannot store as
//! extended data of the `ACAD` application, in front of the entity's other `ACAD` data (sample_AC1014.dwg,
//! sample_AC1015.dwg and sample_AC1018.dwg are down-saves of one drawing; their six samples hold these three kinds and no
//! other `RT*` record):
//!
//! * `RTTcAl` (before R2004: no true colour): `1000 "RTTcAl"`, `1070` the R2004 entity colour word, `1071` the R2004 colour
//!   value, `1000` the colour-book identity. The word is `0x8000` (a colour that is not an index), `0x4000` (a colour-book
//!   colour), `0x2000` (the entity has a transparency, whose value is not kept) and the nearest ACI index in its low byte,
//!   which is also the colour the entity record states. The value is the packed colour of an R2004 `CMC` (`0xC2`, red,
//!   green, blue; the RGB of the book colour for a book colour). The identity is `"<book>$<name>"`, or `"$$NoName$$"`.
//! * `RTLwPs` (R14: no line weight): `1000 "RTLwPs"`, `1070` `0x80` | the DWG line-weight index (29 is ByLayer and has no
//!   record).
//! * `RTMaterial` (before R2007: no material): `1000 "RTMaterial"`, `1005` the handle of the entity's material; the entity
//!   whose material is ByBlock names the built-in ByBlock material.
//!
//! The native model holds the values themselves: the reader restores them from the records and removes the records, the
//! writer states them again for the versions that cannot store them. The records appear in this order in every entity of
//! the samples that has more than one (`RTLwPs` first); no sample entity holds `RTTcAl` and `RTMaterial` together.
use crate::document::CadDocument;
use crate::entities::EntityProperties;
use crate::objects::ObjectType;
use crate::types::{Color, DxfVersion, Handle, LineWeight};
use crate::xdata::{ExtendedData, ExtendedDataRecord, XDataValue as X};

const LINE_WEIGHT: &str = "RTLwPs";
const TRUE_COLOR: &str = "RTTcAl";
const MATERIAL: &str = "RTMaterial";
const NO_NAME: &str = "$$NoName$$";
const APPLICATION: &str = "ACAD";

/// The colour word's flags of a colour that is not an index, a colour-book colour and a transparency.
const WORD_COMPLEX: u16 = 0x8000;
const WORD_BOOK: u16 = 0x4000;
const WORD_TRANSPARENCY: u16 = 0x2000;

pub(crate) fn stores_true_color(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1018
}

pub(crate) fn stores_line_weight(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1015
}

pub(crate) fn stores_material(version: DxfVersion) -> bool {
    version >= DxfVersion::AC1021
}

/// True when a drawing of `version` holds the records for some value (it lacks a field for one of the three).
pub(crate) fn applies(version: DxfVersion) -> bool {
    !stores_material(version)
}

/// The colour-book colour an entity names by handle.
fn book_color<'a>(document: &'a CadDocument, properties: &EntityProperties) -> Option<&'a crate::objects::BookColor> {
    match document.objects.get(&properties.color_book_handle?)? {
        ObjectType::BookColor(color) => Some(color),
        _ => None,
    }
}

/// The handle of the built-in ByBlock material (the `ACAD_MATERIAL` dictionary of the named objects dictionary).
fn by_block_material(document: &CadDocument) -> Option<Handle> {
    let entry = |dictionary: Handle, name: &str| match document.objects.get(&dictionary)? {
        ObjectType::Dictionary(dictionary) => dictionary.get(name),
        _ => None,
    };
    entry(document.header.acad_material_dict_handle, "ByBlock").or_else(|| {
        entry(entry(document.header.named_objects_dict_handle, "ACAD_MATERIAL")?, "ByBlock")
    })
}

/// The ACI index an R14 or R2000 entity record states for `properties` (what AutoCAD's own record states beside an
/// `RTTcAl` record), when the entity holds one: the nearest index of a colour that is not an index.
#[cfg(feature = "acadsharp-compat")]
pub(crate) fn aci_index(properties: &EntityProperties) -> Option<u8> {
    match properties.color {
        Color::Rgb { r, g, b } => Some(crate::types::aci_table::nearest_aci(r, g, b)),
        _ => None,
    }
}

/// The RGB value of an entity's true colour or colour-book colour. The entity's own colour comes first: a colour-book object
/// read from a DWG before R2004 holds an index only (its record has no colour value), the entity the restored RGB.
fn true_color(document: &CadDocument, properties: &EntityProperties) -> Option<(u8, u8, u8)> {
    match (properties.color, book_color(document, properties)) {
        (Color::Rgb { r, g, b }, _) => Some((r, g, b)),
        (_, Some(book)) => match book.color {
            Color::Rgb { r, g, b } => Some((r, g, b)),
            color => color.rgb(),
        },
        _ => None,
    }
}

/// Whether a drawing before R2004 can state the colour-book name of an entity: its `RTTcAl` record needs a colour value.
pub(crate) fn can_state_color_name(properties: &EntityProperties, document: &CadDocument) -> bool {
    true_color(document, properties).is_some()
}

/// The values of the records a drawing of `version` states for `properties` (empty when it can store them all, or the
/// entity holds none of them).
pub(crate) fn groups(properties: &EntityProperties, document: &CadDocument, version: DxfVersion) -> Vec<X> {
    let mut values = Vec::new();
    if !stores_line_weight(version) && properties.line_weight != LineWeight::ByLayer {
        values.extend([
            X::String(LINE_WEIGHT.into()),
            X::Integer16(0x80 | properties.line_weight.to_dwg_index() as i16),
        ]);
    }
    if !stores_true_color(version) {
        let book = book_color(document, properties);
        let rgb = true_color(document, properties);
        if let Some((r, g, b)) = rgb {
            let index = match properties.color {
                Color::Rgb { r, g, b } => crate::types::aci_table::nearest_aci(r, g, b),
                color => color.approximate_index().clamp(0, 255) as u8,
            };
            let named = properties.color_name.as_deref().is_some_and(|name| !name.is_empty());
            let mut word = WORD_COMPLEX | index as u16;
            if book.is_some() || named {
                word |= WORD_BOOK;
            }
            if !properties.transparency.is_by_layer() || properties.dwg_transparency_dropped {
                word |= WORD_TRANSPARENCY;
            }
            let name = match (book, properties.color_name.as_deref()) {
                (_, Some(name)) => Some(name.to_string()),
                (Some(book), None) => crate::io::dxf::join_color_book_name(Some(&book.book_name), Some(&book.color_name)),
                _ => None,
            };
            values.extend([
                X::String(TRUE_COLOR.into()),
                X::Integer16(word as i16),
                X::Integer32(((b as u32) | ((g as u32) << 8) | ((r as u32) << 16) | (0xC2u32 << 24)) as i32),
                X::String(name.filter(|name| !name.is_empty()).unwrap_or_else(|| NO_NAME.into())),
            ]);
        }
    }
    if !stores_material(version) {
        let handle = match (properties.material_flags, properties.material_handle) {
            (0b11, Some(handle)) if !handle.is_null() && document.objects.contains_key(&handle) => Some(handle),
            (0b01, _) => by_block_material(document),
            _ => None,
        };
        if let Some(handle) = handle {
            values.extend([X::String(MATERIAL.into()), X::Handle(handle)]);
        }
    }
    values
}

/// The number of values at the start of `values` that belong to the records (a record is complete or not counted).
fn record_len(values: &[X]) -> Option<usize> {
    let shape: &[fn(&X) -> bool] = match values.first()? {
        X::String(name) if name == LINE_WEIGHT => &[|v| matches!(v, X::Integer16(_))],
        X::String(name) if name == TRUE_COLOR => &[
            |v| matches!(v, X::Integer16(_)),
            |v| matches!(v, X::Integer32(_)),
            |v| matches!(v, X::String(_)),
        ],
        X::String(name) if name == MATERIAL => &[|v| matches!(v, X::Handle(_))],
        _ => return None,
    };
    (values.len() > shape.len() && shape.iter().zip(&values[1..]).all(|(check, value)| check(value))).then_some(shape.len() + 1)
}

/// The length of the run of records at the start of `values`.
fn leading_records(values: &[X]) -> usize {
    let mut end = 0;
    while let Some(length) = record_len(&values[end..]) {
        end += length;
    }
    end
}

/// Whether the `ACAD` record `values` start with a round-trip record.
pub(crate) fn has_records(values: &[X]) -> bool {
    record_len(values).is_some()
}

/// What reading the records of a drawing needs from its objects, found once before the entities are visited.
pub(crate) struct Restorer {
    version: DxfVersion,
    by_block_material: Option<Handle>,
    book_colors: Vec<(String, String, Handle)>,
    application: Option<u64>,
}

impl Restorer {
    /// `None` when the drawing's version stores everything the records keep.
    pub(crate) fn new(document: &CadDocument) -> Option<Self> {
        if !applies(document.version) {
            return None;
        }
        let mut by_block_material = None;
        let mut book_colors = Vec::new();
        for (handle, object) in &document.objects {
            match object {
                ObjectType::Material(material) if material.name.eq_ignore_ascii_case("ByBlock") => by_block_material = Some(*handle),
                ObjectType::BookColor(color) if color.book_name.is_empty() && color.color_name.is_empty() => {
                    // The DBCOLOR record before R2004 holds an index only; its names are the key of its entry in the colour
                    // dictionary (sample_AC1014.dwg: "RAL_CLASSIC$RAL_1006", sample_AC1015.dwg: "RAL CLASSIC$RAL 1006").
                    let key = match document.objects.get(&color.owner) {
                        Some(ObjectType::Dictionary(dictionary)) => {
                            dictionary.entries.iter().find(|(_, entry)| entry == handle).map(|(key, _)| key.as_str())
                        }
                        _ => None,
                    };
                    if let Some(key) = key {
                        let (book, name) = crate::io::dxf::split_color_book_name(key);
                        book_colors.push((book.unwrap_or_default(), name.unwrap_or_default(), *handle));
                    }
                }
                ObjectType::BookColor(color) => book_colors.push((color.book_name.clone(), color.color_name.clone(), *handle)),
                _ => {}
            }
        }
        book_colors.sort_by_key(|(_, _, handle)| *handle);
        Some(Self {
            version: document.version,
            by_block_material,
            book_colors,
            application: document.app_ids.get(APPLICATION).map(|app| app.handle.value()),
        })
    }

    /// Whether the `ACAD` record of `properties` starts with a round-trip record.
    pub(crate) fn wants(&self, properties: &EntityProperties) -> bool {
        !properties.extended_data.records().is_empty()
            && properties
                .extended_data
                .records()
                .iter()
                .any(|record| record.application_name.eq_ignore_ascii_case(APPLICATION) && has_records(&record.values))
    }

    /// Restore the values of the round-trip records of `properties` from its `ACAD` record and remove the records.
    pub(crate) fn restore(&self, properties: &mut EntityProperties) -> bool {
        let Some(index) = properties
            .extended_data
            .records()
            .iter()
            .position(|record| record.application_name.eq_ignore_ascii_case(APPLICATION))
        else {
            return false;
        };
        if !has_records(&properties.extended_data.records()[index].values) {
            return false;
        }
        let mut values = std::mem::take(&mut properties.extended_data.records_mut()[index].values);
        let removed = self.apply(properties, &mut values);
        let records = properties.extended_data.records_mut();
        records[index].values = values;
        if removed {
            if records[index].values.is_empty() {
                properties.extended_data.remove_record_ignore_case(APPLICATION);
            }
            if let Some(application) = self.application {
                properties.extended_data.raw_dwg_eed.retain(|(app, _)| *app != application);
            }
        }
        removed
    }

    /// Apply the records at the start of `values` to `properties` and remove those the drawing's version cannot hold.
    fn apply(&self, properties: &mut EntityProperties, values: &mut Vec<X>) -> bool {
        let version = self.version;
        let end = leading_records(values);
        let mut kept: Vec<X> = Vec::new();
        let mut at = 0;
        let mut removed = false;
        while at < end {
            let length = record_len(&values[at..]).unwrap_or(end - at);
            let record = &values[at..at + length];
            let applied = match (&record[0], &record[1..]) {
                (X::String(name), [X::Integer16(value)])
                    if name == LINE_WEIGHT && !stores_line_weight(version) && value & 0x80 != 0 =>
                {
                    properties.line_weight = LineWeight::from_dwg_index((*value & 0x1F) as u8);
                    true
                }
                (X::String(name), [X::Integer16(word), X::Integer32(value), X::String(identity)])
                    if name == TRUE_COLOR && !stores_true_color(version) =>
                {
                    self.restore_color(properties, *word as u16, *value as u32, identity);
                    true
                }
                (X::String(name), [X::Handle(handle)]) if name == MATERIAL && !stores_material(version) => {
                    if Some(*handle) == self.by_block_material {
                        properties.material_flags = 0b01;
                        properties.material_handle = None;
                    } else {
                        properties.material_flags = 0b11;
                        properties.material_handle = Some(*handle);
                    }
                    true
                }
                _ => false,
            };
            if applied {
                removed = true;
            } else {
                kept.extend_from_slice(record);
            }
            at += length;
        }
        if removed {
            kept.extend(values.drain(end..));
            *values = kept;
        }
        removed
    }

    fn restore_color(&self, properties: &mut EntityProperties, word: u16, value: u32, identity: &str) {
        let [blue, green, red, method] = value.to_le_bytes();
        if word & WORD_BOOK != 0 {
            // As an R2004 record of the colour states it (sample_AC1018.dwg, circle #99F): the RGB value, the name and the
            // colour-book object of that name; the low byte of the word is the nearest index the entity record states.
            properties.color = if method == 0xC2 { Color::from_rgb(red, green, blue) } else { Color::from_index((word & 0xFF) as i16) };
            properties.color_name = Some(identity.to_string());
            let (book, name) = crate::io::dxf::split_color_book_name(identity);
            let (book, name) = (book.unwrap_or_default(), name.unwrap_or_default());
            // The names of an R14 colour-book object (its dictionary key) are symbol names, spaces as underscores; the identity
            // of the record is the colour's own name.
            let same = |a: &str, b: &str| a.chars().map(|c| if c == ' ' { '_' } else { c.to_ascii_uppercase() }).eq(b.chars().map(|c| if c == ' ' { '_' } else { c.to_ascii_uppercase() }));
            if let Some((_, _, handle)) = self.book_colors.iter().find(|(b, n, _)| same(b, &book) && same(n, &name)) {
                properties.color_book_handle = Some(*handle);
            }
        } else {
            properties.color = match method {
                0xC2 => Color::from_rgb(red, green, blue),
                0xC3 => Color::from_index(blue as i16),
                0xC0 => Color::ByLayer,
                0xC1 => Color::ByBlock,
                _ => properties.color,
            };
            if identity != NO_NAME && !identity.is_empty() {
                properties.color_name = Some(identity.to_string());
            }
        }
        properties.dwg_transparency_dropped = word & WORD_TRANSPARENCY != 0;
    }
}

/// `xdata` with the `ACAD` record's round-trip records replaced by `values` (the record is made when the entity has none, in
/// front of the other applications). The verbatim block of the application is dropped so the writer states the record;
/// `None` when the application has a block that was not decoded into a record (it cannot be merged).
pub(crate) fn with_groups(xdata: &ExtendedData, values: Vec<X>, application: Option<u64>) -> Option<ExtendedData> {
    let mut result = xdata.clone();
    let existing = result
        .records()
        .iter()
        .position(|record| record.application_name.eq_ignore_ascii_case(APPLICATION));
    match existing {
        Some(index) => {
            let record = &mut result.records_mut()[index];
            let old = leading_records(&record.values);
            record.values.splice(..old, values);
        }
        None => {
            if application.is_some_and(|application| result.raw_dwg_eed.iter().any(|(app, _)| *app == application)) {
                return None;
            }
            let mut record = ExtendedDataRecord::new(APPLICATION);
            record.values = values;
            result.insert_record(0, record);
        }
    }
    if let Some(application) = application {
        result.raw_dwg_eed.retain(|(app, _)| *app != application);
    }
    Some(result)
}

// ── MTEXT ────────────────────────────────────────────────────────────────────────────────────────────────────────

const MTEXT_BEGIN: &str = "MTEXTBEGIN";
const MTEXT_END: &str = "MTEXTEND";
const BACKGROUND_BEGIN: &str = "ACAD_MTEXT_BBRT";
const BACKGROUND_END: &str = "ACAD_MTEXT_BERT";
const COLUMN_BEGIN: &str = "ACAD_MTEXT_COLUMN_INFO_BEGIN";
const COLUMN_END: &str = "ACAD_MTEXT_COLUMN_INFO_END";

/// Whether a drawing of `version` holds the MTEXT line spacing in a record of the `ACAD` data (every MTEXT of AutoCAD's R14
/// and R2000 files: sample_AC1014.dwg, sample_AC1015.dwg; its R2004 file has none).
pub(crate) fn states_mtext_spacing(version: DxfVersion) -> bool {
    version < DxfVersion::AC1018
}

/// The packed colour of an R2004 `CMC`.
fn packed_color(color: Color) -> i32 {
    (match color {
        Color::Rgb { r, g, b } => (b as u32) | ((g as u32) << 8) | ((r as u32) << 16) | (0xC2u32 << 24),
        Color::ByLayer => 0xC0u32 << 24,
        Color::ByBlock => 0xC1u32 << 24,
        Color::None => 0xC8u32 << 24,
        Color::Index(index) => index as u32 | (0xC3u32 << 24),
    }) as i32
}

/// Whether a drawing of `version` holds the columns of an MTEXT in a record of the `ACAD` data: R2018 states them in the MTEXT
/// record, every version before it in `ACAD_MTEXT_COLUMN_INFO` (sample_AC1015.dwg to sample_AC1027.dwg, and their DXF twins).
pub(crate) fn states_mtext_columns(version: DxfVersion) -> bool {
    version < DxfVersion::AC1032
}

/// The `ACAD_MTEXT_COLUMN_INFO` block of AutoCAD's files before R2018 for dynamic columns with heights of their own (the
/// four MTEXT of sample_AC1015.dwg to sample_AC1027.dwg that sample_AC1032.dwg states as column type 2): `75` column type, `79`
/// auto height, `76` the number of columns, `78` flow reversed, `48` column width, `49` gutter, `50` the number of heights and
/// the heights. A height AutoCAD's R2018 record states as a negative number (-2.94, -23.73, -3, -204.35 for the four MTEXT: the
/// height of the text it laid out) is 0 in all of the older files; the corpus files that state a height of their own state it
/// positive (ezdxf columns_R2007.dxf: 23.85, 52.56). One column only: in that file every record of two or more columns, of
/// static columns or of automatic heights (11 of 24) is followed by `ACAD_MTEXT_COLUMNS_BEGIN 47 <n> <handles>
/// ACAD_MTEXT_COLUMNS_END`, the MTEXT entities that hold the text of the other columns, which a drawing of R2018 does not have
/// and the writer does not make: such columns are not stated.
fn column_block(columns: &crate::entities::MTextColumnData) -> Vec<X> {
    if columns.column_type != 2 || columns.auto_height || columns.heights.len() != 1 {
        return Vec::new();
    }
    let mut values = vec![
        X::String(COLUMN_BEGIN.into()),
        X::Integer16(75),
        X::Integer16(columns.column_type),
        X::Integer16(79),
        X::Integer16(0),
        X::Integer16(76),
        X::Integer16(columns.heights.len() as i16),
        X::Integer16(78),
        X::Integer16(columns.flow_reversed as i16),
        X::Integer16(48),
        X::Real(columns.width),
        X::Integer16(49),
        X::Real(columns.gutter),
        X::Integer16(50),
        X::Integer16(columns.heights.len() as i16),
    ];
    values.extend(columns.heights.iter().map(|height| X::Real(if *height < 0.0 { 0.0 } else { *height })));
    values.push(X::String(COLUMN_END.into()));
    values
}

/// The blocks of the `ACAD` data that a drawing before R2018 holds for an MTEXT, as AutoCAD's files state them. Before R2018 the
/// columns (`ACAD_MTEXT_COLUMN_INFO`, `column_block`); before R2004 (sample_AC1014.dwg, sample_AC1015.dwg) also
/// `MTEXTBEGIN 73 <line spacing style> 44 <line spacing factor> 74 0 MTEXTEND` for every MTEXT, and for one with a background
/// fill `ACAD_MTEXT_BBRT 91 <fill flags> 46 <scale> 64 <colour> 65 "" 66 "" 442 <transparency> ACAD_MTEXT_BERT` (the fields those
/// versions predate), in this order (the column block first in sample_AC1015.dwg).
pub(crate) fn mtext_blocks(text: &crate::entities::MText, version: DxfVersion) -> Vec<X> {
    let mut values = Vec::new();
    if states_mtext_columns(version) {
        values.extend(column_block(&text.column_data));
    }
    if !states_mtext_spacing(version) {
        return values;
    }
    if !text.dwg_without_spacing_block {
        values.extend([
            X::String(MTEXT_BEGIN.into()),
            X::Integer16(73),
            X::Integer16(text.line_spacing_style as i16),
            X::Integer16(44),
            X::Real(text.line_spacing_factor),
            X::Integer16(74),
            X::Integer16(0),
            X::String(MTEXT_END.into()),
        ]);
    }
    if text.background_fill_flags != 0 {
        values.extend([
            X::String(BACKGROUND_BEGIN.into()),
            X::Integer16(91),
            X::Integer32(text.background_fill_flags),
            X::Integer16(46),
            X::Real(text.background_scale),
            X::Integer16(64),
            X::Integer32(packed_color(text.background_color)),
            X::Integer16(65),
            X::String(String::new()),
            X::Integer16(66),
            X::String(String::new()),
            X::Integer16(442),
            X::Integer32(text.background_transparency),
            X::String(BACKGROUND_END.into()),
        ]);
    }
    values
}

/// The range of the block from the string `begin` to the string `end` (both included).
fn block_range(values: &[X], begin: &str, end: &str) -> Option<std::ops::Range<usize>> {
    let start = values.iter().position(|value| matches!(value, X::String(text) if text == begin))?;
    let length = values[start..].iter().position(|value| matches!(value, X::String(text) if text == end))?;
    Some(start..start + length + 1)
}

/// `xdata` with the MTEXT blocks of its `ACAD` record replaced by `blocks` (added at the end when it has none); `None` when
/// the application has a block that was not decoded into a record.
pub(crate) fn with_mtext_blocks(xdata: &ExtendedData, blocks: Vec<X>, application: Option<u64>) -> Option<ExtendedData> {
    let mut result = xdata.clone();
    let existing = result
        .records()
        .iter()
        .position(|record| record.application_name.eq_ignore_ascii_case(APPLICATION));
    match existing {
        Some(index) => {
            let values = &mut result.records_mut()[index].values;
            // The columns of a record a source states stay unless the MTEXT states columns of its own.
            let states_columns = blocks.iter().any(|value| matches!(value, X::String(text) if text == COLUMN_BEGIN));
            for (begin, end) in [(MTEXT_BEGIN, MTEXT_END), (BACKGROUND_BEGIN, BACKGROUND_END), (COLUMN_BEGIN, COLUMN_END)] {
                if begin == COLUMN_BEGIN && !states_columns {
                    continue;
                }
                if let Some(range) = block_range(values, begin, end) {
                    values.drain(range);
                }
            }
            values.extend(blocks);
        }
        None => {
            if application.is_some_and(|application| result.raw_dwg_eed.iter().any(|(app, _)| *app == application)) {
                return None;
            }
            let mut record = ExtendedDataRecord::new(APPLICATION);
            record.values = blocks;
            result.add_record(record);
        }
    }
    if let Some(application) = application {
        result.raw_dwg_eed.retain(|(app, _)| *app != application);
    }
    Some(result)
}

/// Remove the MTEXT blocks from the `ACAD` data of `properties` (the record, when nothing else is left, and the verbatim
/// block); returns the line spacing the `MTEXTBEGIN` block stated.
pub(crate) fn take_mtext_blocks(properties: &mut EntityProperties, application: Option<u64>) -> (bool, Option<(i16, f64)>) {
    let index = properties
        .extended_data
        .records()
        .iter()
        .position(|record| record.application_name.eq_ignore_ascii_case(APPLICATION));
    let Some(index) = index else {
        return (false, None);
    };
    let values = &mut properties.extended_data.records_mut()[index].values;
    let mut spacing = None;
    let mut removed = false;
    let mut had_spacing = false;
    for (begin, end) in [(MTEXT_BEGIN, MTEXT_END), (BACKGROUND_BEGIN, BACKGROUND_END)] {
        if let Some(range) = block_range(values, begin, end) {
            if begin == MTEXT_BEGIN {
                had_spacing = true;
                if let Some([X::Integer16(73), X::Integer16(style), X::Integer16(44), X::Real(factor)]) =
                    values.get(range.start + 1..range.start + 5)
                {
                    spacing = Some((*style, *factor));
                }
            }
            values.drain(range);
            removed = true;
        }
    }
    if removed {
        if properties.extended_data.records()[index].values.is_empty() {
            properties.extended_data.remove_record_ignore_case(APPLICATION);
        }
        if let Some(application) = application {
            properties.extended_data.raw_dwg_eed.retain(|(app, _)| *app != application);
        }
    }
    (had_spacing, spacing)
}

/// Restore the values of the round-trip records of every entity of a document read from a DXF (a DWG reader does it
/// entity by entity while it decodes the extended data).
pub(crate) fn restore_document(document: &mut CadDocument) {
    use crate::entities::EntityType;
    let Some(restorer) = Restorer::new(document) else {
        return;
    };
    for record in document.block_records.iter_mut() {
        for common in [record.block_entity_common.as_deref_mut(), record.block_end_common.as_deref_mut()].into_iter().flatten() {
            if restorer.wants(common) {
                restorer.restore(common);
            }
        }
    }
    for entity in document.entities_mut() {
        if restorer.wants(entity.common()) {
            restorer.restore(&mut entity.common_mut());
        }
        if let EntityType::Insert(insert) = entity {
            for attribute in &mut insert.attributes {
                if restorer.wants(&attribute.common) {
                    restorer.restore(&mut attribute.common);
                }
            }
        }
    }
}

/// A document as a DXF of `version` states it: the MTEXT blocks of the versions that hold them. The `RT*` records are not
/// stated: AutoCAD adds them when it saves a DWG for an older release, and its DXF files of R2000 and R2004 hold none
/// (sample_AC1015_ascii.dxf: the point #29A states colour 230 and nothing else; neither that file nor
/// sample_AC1018_ascii.dxf has `RTTcAl`, `RTLwPs` or `RTMaterial`, while the R2000 file states `MTEXTBEGIN` in all 36 MTEXT,
/// as its DWG does), so a DXF carries the nearest index for the colour and no line weight or material where its version
/// has no group for them. The DXF reader still restores the values from a file that has the records (`restore_document`).
pub(crate) fn prepare_for_dxf(document: &mut std::borrow::Cow<'_, CadDocument>, version: DxfVersion) {
    use crate::entities::EntityType;
    if !states_mtext_columns(version) {
        return;
    }
    let application = document.app_ids.get(APPLICATION).map(|app| app.handle.value());
    let mut changes: Vec<(crate::types::Handle, ExtendedData)> = Vec::new();
    for entity in document.entities() {
        if let EntityType::MText(text) = entity {
            let blocks = mtext_blocks(text, version);
            if !blocks.is_empty() {
                if let Some(merged) = with_mtext_blocks(&text.common.extended_data, blocks, application) {
                    changes.push((text.common.handle, merged));
                }
            }
        }
    }
    if changes.is_empty() {
        return;
    }
    let document = document.to_mut();
    for (handle, data) in changes {
        if let Some(entity) = document.get_entity_mut(handle) {
            entity.common_mut().extended_data = data;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{Circle, Line, Point};
    use crate::objects::{Material, ObjectType};
    use crate::types::{Transparency, Vector3};

    fn document() -> (CadDocument, Handle, Handle) {
        let mut document = CadDocument::with_version(DxfVersion::AC1032);
        let materials = document.header.acad_material_dict_handle;
        let add = |document: &mut CadDocument, name: &str| {
            let mut material = Material::new();
            material.name = name.into();
            material.handle = document.allocate_handle();
            material.owner = materials;
            let handle = material.handle;
            document.objects.insert(handle, ObjectType::Material(material));
            if let Some(ObjectType::Dictionary(entries)) = document.objects.get_mut(&materials) {
                entries.add_entry(name, handle);
            }
            handle
        };
        let by_block = add(&mut document, "ByBlock");
        let own = add(&mut document, "Aluminum Cast");
        (document, by_block, own)
    }

    fn line(color: Color, weight: LineWeight) -> EntityProperties {
        let mut line = Line::from_points(Vector3::ZERO, Vector3::UNIT_X);
        line.common.color = color;
        line.common.line_weight = weight;
        (*line.common.properties).clone()
    }

    /// AutoCAD's records, value by value (sample_AC1014.dwg: the line #9D4 states `RTLwPs 158, RTTcAl -24393 -1031782974
    /// $$NoName$$`, the point #29A `RTTcAl -32538 -1024128365 $$NoName$$`, the lines #2CF `RTLwPs 131`, the point #298
    /// `RTLwPs 158, RTMaterial 151` with 151 the handle of the ByBlock material).
    #[test]
    fn the_records_hold_autocads_values() {
        let (document, by_block, own) = document();
        let colored = line(Color::from_rgb(245, 10, 147), LineWeight::Value(13));
        assert_eq!(
            groups(&colored, &document, DxfVersion::AC1014),
            vec![
                X::String("RTLwPs".into()),
                X::Integer16(131),
                X::String("RTTcAl".into()),
                X::Integer16(-32538),
                X::Integer32(-1024128365),
                X::String("$$NoName$$".into()),
            ]
        );
        // R2000 holds the line weight: no RTLwPs; R2004 holds the true colour: no RTTcAl.
        assert_eq!(groups(&colored, &document, DxfVersion::AC1015)[0], X::String("RTTcAl".into()));
        assert!(groups(&colored, &document, DxfVersion::AC1018).is_empty());
        assert_eq!(
            groups(&line(Color::ByLayer, LineWeight::ByBlock), &document, DxfVersion::AC1014),
            vec![X::String("RTLwPs".into()), X::Integer16(158)]
        );
        assert!(groups(&line(Color::Index(3), LineWeight::ByLayer), &document, DxfVersion::AC1014).is_empty());
        let mut circle = Circle::new();
        circle.common.material_flags = 0b11;
        circle.common.material_handle = Some(own);
        let mut point = Point::new();
        point.common.material_flags = 0b01;
        assert_eq!(
            groups(&circle.common.properties, &document, DxfVersion::AC1018),
            vec![X::String("RTMaterial".into()), X::Handle(own)]
        );
        assert_eq!(
            groups(&point.common.properties, &document, DxfVersion::AC1018),
            vec![X::String("RTMaterial".into()), X::Handle(by_block)]
        );
        assert!(groups(&point.common.properties, &document, DxfVersion::AC1021).is_empty());
        // The word keeps that the entity has a transparency (AutoCAD keeps no value): 0x80E6 | 0x2000.
        let mut transparent = colored;
        transparent.transparency = Transparency::from_alpha_value(0x0200_0099);
        assert_eq!(groups(&transparent, &document, DxfVersion::AC1015)[1], X::Integer16(0xA0E6u16 as i16));
    }

    #[test]
    fn restoring_removes_exactly_the_records_it_applied() {
        let (document, by_block, own) = document();
        let mut old = CadDocument::with_version(DxfVersion::AC1014);
        old.objects = document.objects.clone();
        let restorer = Restorer::new(&old).expect("R14 holds the records");
        let mut properties = line(Color::ByLayer, LineWeight::ByLayer);
        let mut values = vec![
            X::String("RTLwPs".into()),
            X::Integer16(158),
            X::String("RTTcAl".into()),
            X::Integer16(-32538),
            X::Integer32(-1024128365),
            X::String("$$NoName$$".into()),
            X::String("RTMaterial".into()),
            X::Handle(by_block),
            X::String("PLUGIN".into()),
        ];
        assert!(restorer.apply(&mut properties, &mut values));
        assert_eq!(values, vec![X::String("PLUGIN".into())]);
        assert_eq!(properties.line_weight, LineWeight::ByBlock);
        assert_eq!(properties.color, Color::from_rgb(245, 10, 147));
        assert_eq!((properties.material_flags, properties.material_handle), (0b01, None));
        // A record of a value the version holds natively is plain data: R2000 has line weights.
        let mut r2000 = CadDocument::with_version(DxfVersion::AC1015);
        r2000.objects = document.objects.clone();
        let restorer = Restorer::new(&r2000).unwrap();
        let mut values = vec![X::String("RTLwPs".into()), X::Integer16(131), X::String("RTMaterial".into()), X::Handle(own)];
        let mut properties = line(Color::ByLayer, LineWeight::ByLayer);
        assert!(restorer.apply(&mut properties, &mut values));
        assert_eq!(values, vec![X::String("RTLwPs".into()), X::Integer16(131)]);
        assert_eq!(properties.line_weight, LineWeight::ByLayer);
        assert_eq!(properties.material_handle, Some(own));
        // The drawing of R2007 holds all three.
        assert!(Restorer::new(&CadDocument::with_version(DxfVersion::AC1021)).is_none());
    }

    /// AutoCAD's R14 file of its drawing: the circle #99F states `RTTcAl -16342 (0xC02A) -1025339392 (0xC2E29000) "RAL CLASSIC$RAL
    /// 1006"` and the DBCOLOR #996 an index only, named by its colour dictionary key "RAL_CLASSIC$RAL_1006". The read gives what
    /// the R2004 record states (sample_AC1018.dwg): the RGB value, the name and the colour-book object.
    #[test]
    fn a_colour_book_record_names_the_colour_book_object_of_its_dictionary_key() {
        use crate::objects::{BookColor, Dictionary};
        let mut old = CadDocument::with_version(DxfVersion::AC1014);
        let mut dictionary = Dictionary::new();
        dictionary.handle = old.allocate_handle();
        let mut book = BookColor::new();
        book.handle = old.allocate_handle();
        book.owner = dictionary.handle;
        book.color = Color::Index(42);
        dictionary.add_entry("RAL_CLASSIC$RAL_1006", book.handle);
        let (book_handle, dictionary_handle) = (book.handle, dictionary.handle);
        old.objects.insert(book_handle, ObjectType::BookColor(book));
        old.objects.insert(dictionary_handle, ObjectType::Dictionary(dictionary));
        let restorer = Restorer::new(&old).unwrap();
        let mut properties = line(Color::Index(42), LineWeight::ByLayer);
        let mut values = vec![
            X::String("RTTcAl".into()),
            X::Integer16(0xC02Au16 as i16),
            X::Integer32(0xC2E2_9000u32 as i32),
            X::String("RAL CLASSIC$RAL 1006".into()),
        ];
        assert!(restorer.apply(&mut properties, &mut values));
        assert!(values.is_empty());
        assert_eq!(properties.color, Color::from_rgb(226, 144, 0));
        assert_eq!(properties.color_name.as_deref(), Some("RAL CLASSIC$RAL 1006"));
        assert_eq!(properties.color_book_handle, Some(book_handle));
        // Written again, the record is AutoCAD's: the RGB of the entity, not that of the index the object holds.
        assert_eq!(groups(&properties, &old, DxfVersion::AC1014), vec![
            X::String("RTTcAl".into()),
            X::Integer16(0xC02Au16 as i16),
            X::Integer32(0xC2E2_9000u32 as i32),
            X::String("RAL CLASSIC$RAL 1006".into()),
        ]);
        // A name without an object of that name keeps the colour and the name only.
        let mut other = line(Color::Index(42), LineWeight::ByLayer);
        let mut values = vec![X::String("RTTcAl".into()), X::Integer16(0xC02Au16 as i16), X::Integer32(0xC2E2_9000u32 as i32), X::String("RAL CLASSIC$RAL 9999".into())];
        assert!(restorer.apply(&mut other, &mut values));
        assert_eq!((other.color, other.color_book_handle), (Color::from_rgb(226, 144, 0), None));
    }

    /// sample_AC1015.dwg to sample_AC1027.dwg state the columns of the MTEXT #3EC (dynamic, width 15.741414244987482, gutter
    /// 110.22316703811953) as `ACAD_MTEXT_COLUMN_INFO` with the height 0; sample_AC1032.dwg states the height -2.9442928603910867.
    #[test]
    fn dynamic_columns_are_stated_in_the_record_before_r2018_with_the_heights_autocad_states() {
        use crate::entities::{MText, MTextColumnData};
        let mut text = MText::new();
        text.column_data = MTextColumnData {
            column_type: 2,
            column_count: 0,
            flow_reversed: false,
            auto_height: false,
            width: 15.741414244987482,
            gutter: 110.22316703811953,
            heights: vec![-2.9442928603910867],
        };
        let expected = vec![
            X::String("ACAD_MTEXT_COLUMN_INFO_BEGIN".into()),
            X::Integer16(75),
            X::Integer16(2),
            X::Integer16(79),
            X::Integer16(0),
            X::Integer16(76),
            X::Integer16(1),
            X::Integer16(78),
            X::Integer16(0),
            X::Integer16(48),
            X::Real(15.741414244987482),
            X::Integer16(49),
            X::Real(110.22316703811953),
            X::Integer16(50),
            X::Integer16(1),
            X::Real(0.0),
            X::String("ACAD_MTEXT_COLUMN_INFO_END".into()),
        ];
        for version in [DxfVersion::AC1021, DxfVersion::AC1024, DxfVersion::AC1027] {
            assert_eq!(mtext_blocks(&text, version), expected, "{version:?}");
        }
        // R2000 states the line spacing after it; R2018 states the columns in the MTEXT record itself
        let r2000 = mtext_blocks(&text, DxfVersion::AC1015);
        assert_eq!(r2000[..expected.len()], expected[..]);
        assert_eq!(r2000[expected.len()], X::String("MTEXTBEGIN".into()));
        assert!(mtext_blocks(&text, DxfVersion::AC1032).is_empty());
        // a height of its own is kept; the flow direction is the record's 78
        text.column_data.heights = vec![52.563114843856];
        text.column_data.flow_reversed = true;
        let own = mtext_blocks(&text, DxfVersion::AC1027);
        assert_eq!(&own[5..9], &[X::Integer16(76), X::Integer16(1), X::Integer16(78), X::Integer16(1)]);
        assert_eq!(&own[13..16], &[X::Integer16(50), X::Integer16(1), X::Real(52.563114843856)]);
        // two columns need the MTEXT of the second column (`ACAD_MTEXT_COLUMNS`, columns_R2007.dxf), which the drawing does not
        // have: no record
        text.column_data.heights = vec![11.069645635849, 0.0];
        assert!(mtext_blocks(&text, DxfVersion::AC1027).is_empty());
        assert_eq!(mtext_blocks(&text, DxfVersion::AC1015)[0], X::String("MTEXTBEGIN".into()));
        text.column_data.heights = vec![0.0];
        // static columns, automatic heights and an MTEXT without columns state no record either
        text.column_data.column_type = 1;
        assert!(mtext_blocks(&text, DxfVersion::AC1027).is_empty());
        text.column_data.column_type = 2;
        text.column_data.auto_height = true;
        assert!(mtext_blocks(&text, DxfVersion::AC1027).is_empty());
        assert!(mtext_blocks(&MText::new(), DxfVersion::AC1027).is_empty());
    }

    /// The columns of a record a source states stay when the MTEXT has none of its own, and are replaced when it has.
    #[test]
    fn a_column_record_of_the_source_is_replaced_only_by_columns_of_the_mtext() {
        use crate::entities::{MText, MTextColumnData};
        let old = vec![
            X::String("ACAD_MTEXT_COLUMN_INFO_BEGIN".into()),
            X::Integer16(75),
            X::Integer16(2),
            X::String("ACAD_MTEXT_COLUMN_INFO_END".into()),
        ];
        let mut xdata = ExtendedData::new();
        let mut record = ExtendedDataRecord::new("ACAD");
        record.values = [X::String("PLUGIN".into())].into_iter().chain(old.clone()).collect();
        xdata.add_record(record);
        // an MTEXT of R2000 without columns of its own: the line spacing is stated, the record stays
        let plain = MText::new();
        let blocks = mtext_blocks(&plain, DxfVersion::AC1015);
        let kept = with_mtext_blocks(&xdata, blocks, None).unwrap();
        assert!(kept.records()[0].values.starts_with(&[X::String("PLUGIN".into())]));
        assert!(kept.records()[0].values.windows(old.len()).any(|window| window == old.as_slice()));
        // an MTEXT with columns: the record is the MTEXT's, the other data stays
        let mut columned = MText::new();
        columned.column_data = MTextColumnData { column_type: 2, column_count: 0, flow_reversed: false, auto_height: false, width: 5.0, gutter: 1.0, heights: vec![0.0] };
        let replaced = with_mtext_blocks(&xdata, mtext_blocks(&columned, DxfVersion::AC1027), None).unwrap();
        let values = &replaced.records()[0].values;
        assert_eq!(values[0], X::String("PLUGIN".into()));
        assert!(!values.windows(old.len()).any(|window| window == old.as_slice()));
        assert_eq!(values.iter().filter(|value| matches!(value, X::String(text) if text == "ACAD_MTEXT_COLUMN_INFO_BEGIN")).count(), 1);
        assert!(values.contains(&X::Real(5.0)));
    }

    #[test]
    fn an_incomplete_record_is_not_a_record() {
        assert!(!has_records(&[X::String("RTTcAl".into()), X::Integer16(1)]));
        assert!(!has_records(&[X::String("RTLwPs".into())]));
        assert!(!has_records(&[X::Integer16(1), X::String("RTLwPs".into()), X::Integer16(131)]));
        assert!(has_records(&[X::String("RTLwPs".into()), X::Integer16(131)]));
    }
}
