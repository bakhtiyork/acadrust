//! Binary modeler data (SAB) saved by an older ACIS release.
//!
//! AutoCAD stores the modeler data of a drawing in the ACIS save version of its release (sample_AC1018.dwg to
//! sample_AC1032.dwg are down-saves of one drawing; the 3DSOLID #D65, #D6A and the REGION #D69 are the same bodies in each):
//!
//! | DWG | magic | save version | fourth header word | other differences from the release before |
//! |---|---|---|---|---|
//! | R2004 | `ACIS BinaryFile` | 20800 | 0 | |
//! | R2007 | `ACIS BinaryFile` | 21200 | 26 | every `vertex` record has one more integer, before its last pointer |
//! | R2010 | `ACIS BinaryFile` | 21500 | 24 | none |
//! | R2013 | `ACIS BinaryFile` | 21800 | 12 | every `coedge` record has one more integer, before its last pointer; the end record says `ASM` |
//! | R2018 | `ASM BinaryFile4` | 22300 | 4 | none |
//!
//! Every other token of the three bodies (hundreds of `face`, `loop`, `edge`, `point`, `plane-surface`, `straight-curve`,
//! `transform` and `persubent-acadSolidHistory-attrib` records) is the same in all five files, except the date string of the
//! `asmheader` record. A drawing saved to an older release therefore takes the data of a newer one by the header and by dropping
//! those integers, which is what this module does for the record kinds and shapes the samples show ([`KNOWN`]). It does not
//! invent anything else: a body with any other record kind or shape (a cone, sphere, torus or spline surface, an elliptic or
//! interpolated curve, a tolerant edge or vertex, other attributes: whether their layout differs between the releases is not
//! known), other save versions, text SAT data (R2000, R14: different record layouts) and any conversion to a newer release (the
//! added integers depend on the geometry: the vertex integer is 0 or 1) stay as they are.

use crate::types::DxfVersion;

const ACIS_MAGIC: &[u8; 15] = b"ACIS BinaryFile";
const ASM_MAGIC: &[u8; 15] = b"ASM BinaryFile4";

/// The ACIS save version and the fourth header word AutoCAD states for each DWG version.
const SAVES: [(DxfVersion, i32, i32); 5] = [
    (DxfVersion::AC1018, 20800, 0),
    (DxfVersion::AC1021, 21200, 26),
    (DxfVersion::AC1024, 21500, 24),
    (DxfVersion::AC1027, 21800, 12),
    (DxfVersion::AC1032, 22300, 4),
];

/// The record kinds of the three bodies of the samples: the type name (sub-identifiers and identifier joined by `-`), the shape
/// (one letter per token: `I` identifier, `u` sub-identifier, `p` pointer, `i` integer, `e` enumeration, `d` double, `l`
/// logical, `s` string, `P` position, `V` vector, `#` end of record) and the save versions that state that shape (from, to).
const KNOWN: &[(&str, &str, i32, i32)] = &[
    ("asmheader", "sssdddIpis#", 0, i32::MAX),
    ("body", "Ipipppp#", 0, i32::MAX),
    ("lump", "Ipipppp#", 0, i32::MAX),
    ("shell", "Ipipppppp#", 0, i32::MAX),
    ("face", "Ipippppppll#", 0, i32::MAX),
    ("face", "Ipipppppplll#", 0, i32::MAX),
    ("loop", "Ipipppp#", 0, i32::MAX),
    ("edge", "Ipippdpdppls#", 0, i32::MAX),
    ("point", "IpipP#", 0, i32::MAX),
    ("transform", "IpiVVVVdlll#", 0, i32::MAX),
    ("plane-surface", "uIpipPVVlllll#", 0, i32::MAX),
    ("straight-curve", "uIpipPVll#", 0, i32::MAX),
    ("straight-curve", "uIpipPVldld#", 0, i32::MAX),
    ("persubent-acadSolidHistory-attrib", "uuIpipppiiii#", 0, i32::MAX),
    // vertex $edge, int, $attribute, $edge, [int from 21200], $point
    ("vertex", "Ipippp#", 0, 21200),
    ("vertex", "Ipippip#", 21200, i32::MAX),
    // coedge $coedge, int, $partner, $next, $previous, $edge, $owner, sense, $loop, [int from 21800], $curve
    ("coedge", "Ipippppplpp#", 0, 21800),
    ("coedge", "Ipippppplpip#", 21800, i32::MAX),
    ("End-of-ACIS-data", "uuuI", 0, 21800),
    ("End-of-ASM-data", "uuuI", 21800, i32::MAX),
];

const INTEGER: u8 = 0x04;
const DOUBLE: u8 = 0x06;
const STRING_1: u8 = 0x07;
const STRING_2: u8 = 0x08;
const STRING_4: u8 = 0x09;
const TRUE: u8 = 0x0A;
const FALSE: u8 = 0x0B;
const POINTER: u8 = 0x0C;
const IDENT: u8 = 0x0D;
const SUB_IDENT: u8 = 0x0E;
const END: u8 = 0x11;
const POSITION: u8 = 0x13;
const VECTOR: u8 = 0x14;
const ENUM: u8 = 0x15;

/// The letter of a token's kind in the shapes of [`KNOWN`].
fn token_class(tag: u8) -> u8 {
    match tag {
        INTEGER => b'i',
        POINTER => b'p',
        ENUM => b'e',
        DOUBLE => b'd',
        TRUE | FALSE => b'l',
        END => b'#',
        POSITION => b'P',
        VECTOR => b'V',
        STRING_1 | STRING_2 | STRING_4 => b's',
        IDENT => b'I',
        SUB_IDENT => b'u',
        _ => b'?',
    }
}

/// The end of the token at `at`, when it is one of the kinds the samples show.
fn token_end(data: &[u8], at: usize) -> Option<usize> {
    let tag = *data.get(at)?;
    let end = match tag {
        INTEGER | POINTER | ENUM => at + 5,
        DOUBLE => at + 9,
        TRUE | FALSE | END => at + 1,
        POSITION | VECTOR => at + 25,
        STRING_1 | IDENT | SUB_IDENT => at + 2 + usize::from(*data.get(at + 1)?),
        STRING_2 => at + 3 + usize::from(u16::from_le_bytes([*data.get(at + 1)?, *data.get(at + 2)?])),
        STRING_4 => {
            let length = u32::from_le_bytes(data.get(at + 1..at + 5)?.try_into().ok()?) as usize;
            at.checked_add(5)?.checked_add(length)?
        }
        _ => return None,
    };
    (end <= data.len()).then_some(end)
}

/// `data` (AutoCAD's binary modeler data of a drawing written as `from`'s save version) as the save version of `target`, when
/// `target` is older and the data has the record shapes the samples show. `None` otherwise: the caller keeps the data.
pub(crate) fn for_older_drawing(data: &[u8], target: DxfVersion) -> Option<Vec<u8>> {
    if data.len() < 31 || !(data.starts_with(ACIS_MAGIC) || data.starts_with(ASM_MAGIC)) {
        return None;
    }
    let word = |at: usize| i32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
    let (version, flags) = (word(15), word(27));
    // The magic and the save version tell the same release (a file of another producer says something else).
    if SAVES.iter().find(|(_, v, f)| *v == version && *f == flags).is_none() || data.starts_with(ASM_MAGIC) != (version == 22300) {
        return None;
    }
    let (_, goal, goal_flags) = *SAVES.iter().find(|(v, ..)| *v == target)?;
    if goal >= version {
        return None;
    }
    let mut out = Vec::with_capacity(data.len());
    out.extend_from_slice(ACIS_MAGIC);
    out.extend(goal.to_le_bytes());
    out.extend(word(19).to_le_bytes());
    out.extend(word(23).to_le_bytes());
    out.extend(goal_flags.to_le_bytes());
    let mut at = 31;
    let mut record: Vec<(u8, usize, usize)> = Vec::new();
    while at < data.len() {
        let end = token_end(data, at)?;
        record.push((data[at], at, end));
        at = end;
        // A record ends with `#`; the file ends with `End of ACIS data`, whose last token (the identifier `data`) has none.
        let last = *record.last()?;
        let closes = last.0 == END || (last.0 == IDENT && &data[last.1 + 2..last.2] == b"data" && record.len() == 4);
        if !closes {
            continue;
        }
        // The type name and the shape of the record; a kind or shape the samples do not show refuses the whole body.
        let mut name = Vec::new();
        for &(tag, from, to) in &record {
            if tag == SUB_IDENT || tag == IDENT {
                if !name.is_empty() {
                    name.push(b'-');
                }
                name.extend_from_slice(&data[from + 2..to]);
                if tag == IDENT {
                    break;
                }
            }
        }
        let shape: Vec<u8> = record.iter().map(|&(tag, ..)| token_class(tag)).collect();
        if !KNOWN
            .iter()
            .any(|(known, known_shape, since, until)| known.as_bytes() == name && known_shape.as_bytes() == shape && (*since..*until).contains(&version))
        {
            return None;
        }
        // The tokens of the record that go (by index) and the sub-identifier the end record renames.
        let mut dropped: Option<usize> = None;
        let mut renamed: Option<(usize, &[u8])> = None;
        match name.as_slice() {
            b"vertex" if version >= 21200 && goal < 21200 => dropped = Some(5),
            b"coedge" if version >= 21800 && goal < 21800 => dropped = Some(10),
            // End of ASM data
            b"End-of-ASM-data" if goal < 21800 => renamed = Some((2, b"ACIS")),
            _ => {}
        }
        for (index, &(_, from, to)) in record.iter().enumerate() {
            if dropped == Some(index) {
                continue;
            }
            match renamed {
                Some((renamed_at, text)) if renamed_at == index => {
                    out.push(SUB_IDENT);
                    out.push(text.len() as u8);
                    out.extend_from_slice(text);
                }
                _ => out.extend_from_slice(&data[from..to]),
            }
        }
        record.clear();
    }
    record.is_empty().then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(value: i32) -> Vec<u8> {
        let mut out = vec![INTEGER];
        out.extend(value.to_le_bytes());
        out
    }
    fn ptr(value: i32) -> Vec<u8> {
        let mut out = vec![POINTER];
        out.extend(value.to_le_bytes());
        out
    }
    fn text(tag: u8, value: &str) -> Vec<u8> {
        let mut out = vec![tag, value.len() as u8];
        out.extend(value.as_bytes());
        out
    }
    fn record(parts: &[Vec<u8>]) -> Vec<u8> {
        let mut out: Vec<u8> = parts.concat();
        out.push(END);
        out
    }

    /// A body with the record shapes the samples show at the save version `version`: the header, the `asmheader` record with
    /// a date, a vertex, a coedge and the end record.
    fn body(version: i32, date: &str) -> Vec<u8> {
        let flags = SAVES.iter().find(|(_, v, _)| *v == version).unwrap().2;
        let mut out = if version == 22300 { ASM_MAGIC.to_vec() } else { ACIS_MAGIC.to_vec() };
        for word in [version, 0, 2, flags] {
            out.extend(word.to_le_bytes());
        }
        let double = |value: f64| {
            let mut out = vec![DOUBLE];
            out.extend(value.to_le_bytes());
            out
        };
        // the product, its version, the date, the tolerances and the `asmheader` record are one record in AutoCAD's files
        out.extend(record(&[
            text(STRING_1, "Autodesk AutoCAD"),
            text(STRING_1, "ASM 230.7.0.65535 NT"),
            text(STRING_1, date),
            double(25.4),
            double(1e-6),
            double(1e-10),
            text(IDENT, "asmheader"),
            ptr(-1),
            int(-1),
            text(STRING_1, "230.7.0.65535"),
        ]));
        let mut vertex = vec![text(IDENT, "vertex"), ptr(74), int(-1), ptr(-1), ptr(23)];
        if version >= 21200 {
            vertex.push(int(1));
        }
        vertex.push(ptr(75));
        out.extend(record(&vertex));
        for sense in [FALSE, TRUE] {
            let mut coedge = vec![text(IDENT, "coedge"), ptr(3), int(-1), ptr(4), ptr(5), ptr(6), ptr(7), ptr(8), vec![sense], ptr(9)];
            if version >= 21800 {
                coedge.push(int(0));
            }
            coedge.push(ptr(10));
            out.extend(record(&coedge));
        }
        // the end of the file has no `#`
        out.extend([
            text(SUB_IDENT, "End"),
            text(SUB_IDENT, "of"),
            text(SUB_IDENT, if version >= 21800 { "ASM" } else { "ACIS" }),
            text(IDENT, "data"),
        ]
        .concat());
        out
    }

    /// AutoCAD's own down-save is the newer body with the extra integers gone, the older header and the end record of the older
    /// release (sample_AC1032.dwg to sample_AC1018.dwg, bodies #D65, #D69 and #D6A).
    #[test]
    fn a_body_takes_the_shape_of_the_older_release() {
        let date = "Tue Dec  2 10:27:20 2025";
        for (from, to) in [(22300, 21800), (22300, 21500), (22300, 21200), (22300, 20800), (21800, 21500), (21800, 20800), (21500, 21200), (21500, 20800), (21200, 20800)] {
            let target = SAVES.iter().find(|(_, v, _)| *v == to).unwrap().0;
            assert_eq!(for_older_drawing(&body(from, date), target), Some(body(to, date)), "{from} to {to}");
        }
        // 22300 states ASM in its magic; every other release ACIS
        assert!(body(22300, date).starts_with(ASM_MAGIC));
        assert!(for_older_drawing(&body(22300, date), DxfVersion::AC1027).unwrap().starts_with(ACIS_MAGIC));
    }

    #[test]
    fn data_that_is_not_the_samples_shape_stays_as_it_is() {
        let date = "Wed Sep 24 12:29:03 2025";
        let current = body(22300, date);
        // not older, not a drawing version with ACIS data, newer
        assert_eq!(for_older_drawing(&current, DxfVersion::AC1032), None);
        assert_eq!(for_older_drawing(&current, DxfVersion::AC1015), None);
        assert_eq!(for_older_drawing(&body(20800, date), DxfVersion::AC1027), None);
        // another producer's header (the fork's own binary data states its product first), another save version, a truncated token,
        // a record shape that is not the samples'
        let mut other = current.clone();
        other[15..19].copy_from_slice(&700i32.to_le_bytes());
        assert_eq!(for_older_drawing(&other, DxfVersion::AC1024), None);
        assert_eq!(for_older_drawing(&current[..current.len() - 3], DxfVersion::AC1024), None);
        assert_eq!(for_older_drawing(b"ACIS BinaryFile", DxfVersion::AC1024), None);
        let mut odd = current.clone();
        let vertex = odd.windows(8).position(|window| window == b"\x0d\x06vertex").unwrap();
        odd[vertex + 8 + 5] = FALSE; // the first pointer of the vertex becomes a logical: the record is not a vertex of the samples
        assert_eq!(for_older_drawing(&odd, DxfVersion::AC1024), None);
        // a record kind the samples do not show (a cone surface): whether its layout differs between the releases is not known, so
        // the body is not converted; likewise a known kind in another shape (a face with another field)
        for extra in [
            record(&[text(SUB_IDENT, "cone"), text(IDENT, "surface"), ptr(-1), int(-1), ptr(-1), vec![POSITION; 25], vec![FALSE]]),
            record(&[text(IDENT, "face"), ptr(-1), int(-1), ptr(-1), ptr(1), ptr(2), ptr(3), ptr(4), ptr(5), ptr(6), vec![FALSE], vec![FALSE], int(3)]),
        ] {
            let mut unknown = current.clone();
            let end = unknown.windows(5).position(|window| window == b"\x0e\x03End").unwrap();
            unknown.splice(end..end, extra);
            assert_eq!(for_older_drawing(&unknown, DxfVersion::AC1024), None);
        }
        // the magic and the save version say the same release
        let mut mixed = body(21800, date);
        mixed[..15].copy_from_slice(ASM_MAGIC);
        assert_eq!(for_older_drawing(&mixed, DxfVersion::AC1021), None);
    }
}

