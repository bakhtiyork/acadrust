// SPDX-License-Identifier: MPL-2.0
//! AutoCAD's modeler data in the ACIS save version of another release (R14, R2000 and the binary releases).
//!
//! AutoCAD stores the modeler data of a drawing in the save version of its release. sample_AC1014.dwg to sample_AC1032.dwg
//! (and the DXF twins) are down-saves of one drawing, so the three bodies (the 3DSOLIDs #D65 and #D6A and the REGION #D69)
//! show, record by record, what AutoCAD writes at every release:
//!
//! | release | form | header | records |
//! |---|---|---|---|
//! | R14 | SAT text, version 106 | `106 <records> 1 0` only (no product, units, tolerances or date) | numbers for senses and flags (`0`), no `wire` in a shell |
//! | R2000 | SAT text, version 400 | the version line, the product line (`16 Autodesk AutoCAD 20 ASM 230.7.0.65535 NT 24 <date>`) and the units/tolerance line | words for senses and flags (`forward`, `no_rotate`), the shell's `wire` pointer |
//! | R2004 and later | SAB, 20800 to 22300 (and the same as SAT text in R2004 to R2010 DXF) | the product, version and date strings and the three doubles in the first record | an `asmheader` record, so every pointer is one more; each record adds `-1`, `$-1` after its attribute pointer (the transform only the `-1`), an edge states the curve parameters of its two vertices and `unknown`, a vertex from 21200 and a coedge from 21800 state one more integer |
//!
//! The pointers count from the first record (the body) at R14 and R2000 and from the `asmheader` at R2004 and later. Every other
//! token of the three bodies is the same in all releases, and every double prints as the C format `%.19g`.
//!
//! What a conversion needs that the older release does not state is derived only where the samples show the rule:
//! * the curve parameters of an edge: the range of the curve when it is finite (all 12 finite curves of the samples), else the
//!   signed distance of the vertex from the root of the curve (the sign: the side the direction points to; 44 of 44
//!   parameters of the samples equal the double AutoCAD states, where the dot product with the direction differs from 3 in the
//!   last digit);
//! * the integer of a vertex (0 when it is the start vertex of its edge, 1 when it is the end vertex: 25 of 25) and of a
//!   coedge (0: 61 of 61);
//! * the units, tolerances and product strings of a body the R14 text does not state: the product and tolerances of every
//!   body of the samples, the units 25.4 of the drawing's inches (the samples are inch drawings, INSUNITS 1; AutoCAD keeps
//!   INSUNITS in a round-trip record of its R14 file).
//!
//! It does not invent anything else: a record kind, a field value or a shape the samples do not show (the kinds of `read_rec`; a cone,
//! sphere, torus or spline surface, an elliptic or interpolated curve, an attribute other than the solid history, a reversed
//! edge, finite plane bounds, a pointer to another kind of record than the samples show) refuses the whole body, and the writer
//! reports it.

use std::borrow::Cow;

use super::reversion::{
    token_end, ACIS_MAGIC, ASM_MAGIC, END, FALSE, IDENT, INTEGER, POINTER, POSITION, SAVES, STRING_1, SUB_IDENT, TRUE, VECTOR, DOUBLE,
};
use crate::document::CadDocument;
use crate::entities::solid3d::{AcisData, AcisVersion};
use crate::entities::EntityType;
use crate::error::{DxfError, Result};
use crate::types::{DxfVersion, Handle};

type R<T> = std::result::Result<T, String>;

/// The ACIS form of a release: SAT text 106 (R14) or 400 (R2000), or the save version of a later release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    Sat106,
    Sat400,
    /// 20800, 21200, 21500, 21800 or 22300 (binary in a DWG; the first three also as text in a DXF).
    Modern(i32),
}

/// What the conversion needs from the drawing.
pub(crate) struct Context {
    /// INSUNITS of the drawing.
    pub insertion_units: i16,
    /// The date of the drawing's last update as AutoCAD's modeler states it (`Tue Dec  2 10:27:20 2025`), when known.
    pub date: Option<String>,
}

/// The converted data: the text for R14 and R2000, the binary data for later releases.
#[derive(Debug)]
pub(crate) struct Converted {
    pub sat_data: String,
    pub sab_data: Vec<u8>,
}

const AUTOCAD: &str = "Autodesk AutoCAD";
const ASM: &str = "ASM 230.7.0.65535 NT";
const ASM_ID: &str = "230.7.0.65535";
/// The modeler units of an inch drawing (INSUNITS 1), as in the header of every body of the samples.
const INCH_UNITS: f64 = 25.4;

/// The date AutoCAD's modeler states for a drawing updated at `julian`.
pub(crate) fn drawing_date(julian: f64) -> Option<String> {
    if julian <= 0.0 {
        return None;
    }
    // Drawing dates count the fraction from midnight, not noon.
    let (year, month, day, hour, minute, second) = crate::fields::julian_parts(julian - 0.5);
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let weekday = DAYS[crate::fields::weekday(julian - 0.5) as usize % 7];
    Some(format!("{weekday} {} {day:>2} {hour:02}:{minute:02}:{second:02} {year}", MONTHS[(month as usize).clamp(1, 12) - 1]))
}

// ---------------------------------------------------------------------------------------------------------------------
// The model: the records of one body with the fields all releases state
// ---------------------------------------------------------------------------------------------------------------------

/// A range bound of a surface or curve: infinite (`I`, a false logical) or a value (`F <value>`, a true logical and a double).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Bound {
    Infinite,
    At(f64),
}

#[derive(Debug, Clone, PartialEq)]
enum Rec {
    Body { attr: i32, lump: i32, wire: i32, transform: i32 },
    Lump { attr: i32, next: i32, shell: i32, body: i32 },
    Shell { attr: i32, next: i32, subshell: i32, face: i32, wire: i32, lump: i32 },
    Face { attr: i32, next: i32, first_loop: i32, shell: i32, subshell: i32, surface: i32, reversed: bool, double: bool, inside: bool },
    Loop { attr: i32, next: i32, coedge: i32, face: i32 },
    Coedge { attr: i32, next: i32, prev: i32, partner: i32, edge: i32, reversed: bool, owner: i32, pcurve: i32 },
    /// The curve parameters of the two vertices exist from 20800 only.
    Edge { attr: i32, start: i32, start_param: Option<f64>, end: i32, end_param: Option<f64>, coedge: i32, curve: i32, reversed: bool },
    /// The integer exists from 21200 only.
    Vertex { attr: i32, edge: i32, state: Option<i32>, point: i32 },
    Point { attr: i32, position: [f64; 3] },
    Transform { attr: i32, rows: [[f64; 3]; 3], translation: [f64; 3], scale: f64, rotate: bool, reflect: bool, shear: bool },
    Plane { attr: i32, root: [f64; 3], normal: [f64; 3], u_direction: [f64; 3], reverse_v: bool, bounds: [Bound; 4] },
    Straight { attr: i32, root: [f64; 3], direction: [f64; 3], bounds: [Bound; 2] },
    History { attr: i32, owner: i32, numbers: [i32; 4] },
}

impl Rec {
    fn name(&self) -> &'static str {
        match self {
            Rec::Body { .. } => "body",
            Rec::Lump { .. } => "lump",
            Rec::Shell { .. } => "shell",
            Rec::Face { .. } => "face",
            Rec::Loop { .. } => "loop",
            Rec::Coedge { .. } => "coedge",
            Rec::Edge { .. } => "edge",
            Rec::Vertex { .. } => "vertex",
            Rec::Point { .. } => "point",
            Rec::Transform { .. } => "transform",
            Rec::Plane { .. } => "plane-surface",
            Rec::Straight { .. } => "straight-curve",
            Rec::History { .. } => "persubent-acadSolidHistory-attrib",
        }
    }

    /// Every pointer the record states.
    fn pointers(&self) -> Vec<i32> {
        match *self {
            Rec::Body { attr, lump, wire, transform } => vec![attr, lump, wire, transform],
            Rec::Lump { attr, next, shell, body } => vec![attr, next, shell, body],
            Rec::Shell { attr, next, subshell, face, wire, lump } => vec![attr, next, subshell, face, wire, lump],
            Rec::Face { attr, next, first_loop, shell, subshell, surface, .. } => vec![attr, next, first_loop, shell, subshell, surface],
            Rec::Loop { attr, next, coedge, face } => vec![attr, next, coedge, face],
            Rec::Coedge { attr, next, prev, partner, edge, owner, pcurve, .. } => vec![attr, next, prev, partner, edge, owner, pcurve],
            Rec::Edge { attr, start, end, coedge, curve, .. } => vec![attr, start, end, coedge, curve],
            Rec::Vertex { attr, edge, point, .. } => vec![attr, edge, point],
            Rec::Point { attr, .. } | Rec::Transform { attr, .. } | Rec::Plane { attr, .. } | Rec::Straight { attr, .. } => vec![attr],
            Rec::History { attr, owner, .. } => vec![attr, owner],
        }
    }
}

/// The strings and numbers of the head of the body: the first lines of the text, the first record of the binary data.
#[derive(Debug, Clone, PartialEq)]
struct Header {
    product: String,
    asm: String,
    date: String,
    units: f64,
    resabs: f64,
    resnor: f64,
}

#[derive(Debug, Clone, PartialEq)]
struct Model {
    /// None for the text of R14, which states none.
    header: Option<Header>,
    recs: Vec<Rec>,
}

/// The layout of the records of a release.
#[derive(Debug, Clone, Copy)]
struct Layout {
    /// 20800 and later: the `-1`, `$-1` after the attribute, the edge parameters, `unknown`.
    modern: bool,
    /// 106: numbers where 400 has words (senses of coedges and edges, the flags of a transform, `reverse_v`), no `wire` in a shell.
    v106: bool,
    /// 21200 and later: the integer of a vertex.
    vertex_state: bool,
    /// 21800 and later: the integer of a coedge.
    coedge_state: bool,
}

impl Layout {
    fn of(form: Form) -> Layout {
        match form {
            Form::Sat106 => Layout { modern: false, v106: true, vertex_state: false, coedge_state: false },
            Form::Sat400 => Layout { modern: false, v106: false, vertex_state: false, coedge_state: false },
            Form::Modern(save) => Layout { modern: true, v106: false, vertex_state: save >= 21200, coedge_state: save >= 21800 },
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Reading and writing the typed values of a record
// ---------------------------------------------------------------------------------------------------------------------

trait Reader {
    fn ptr(&mut self) -> R<i32>;
    fn int(&mut self) -> R<i32>;
    fn dbl(&mut self) -> R<f64>;
    /// A position (`position`) or a vector: three doubles.
    fn triple(&mut self, position: bool) -> R<[f64; 3]>;
    /// A two-valued word of the text (`names.0` false, `names.1` true), a number where `numeric` and the release is 106, a
    /// logical in the binary data.
    fn flag(&mut self, names: [&str; 2], numeric: bool) -> R<bool>;
    fn bound(&mut self) -> R<Bound>;
    fn string(&mut self) -> R<String>;
    /// All tokens of the record consumed.
    fn finished(&self) -> R<()>;
}

trait Sink {
    fn ptr(&mut self, value: i32);
    fn int(&mut self, value: i32);
    fn dbl(&mut self, value: f64) -> R<()>;
    fn triple(&mut self, value: [f64; 3], position: bool) -> R<()>;
    fn flag(&mut self, names: [&str; 2], numeric: bool, value: bool);
    fn bound(&mut self, value: Bound) -> R<()>;
    fn string(&mut self, value: &str);
}

fn finite(value: f64) -> R<f64> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err("a number that is not finite".to_string())
    }
}

/// `%.19g`, the way the modeler prints a double.
fn g19(value: f64) -> R<String> {
    finite(value)?;
    if value == 0.0 {
        return Ok(if value.is_sign_negative() { "-0" } else { "0" }.to_string());
    }
    let text = format!("{value:.18e}");
    let (mantissa, exponent) = text.split_once('e').ok_or("a number that cannot be printed")?;
    let exponent: i32 = exponent.parse().map_err(|_| "a number that cannot be printed".to_string())?;
    let sign = if mantissa.starts_with('-') { "-" } else { "" };
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let trim = |text: &str| text.trim_end_matches('0').to_string();
    Ok(if !(-4..19).contains(&exponent) {
        let fraction = trim(&digits[1..]);
        let point = if fraction.is_empty() { String::new() } else { format!(".{fraction}") };
        format!("{sign}{}{point}e{}{:02}", &digits[..1], if exponent < 0 { '-' } else { '+' }, exponent.abs())
    } else if exponent >= 0 {
        let whole = exponent as usize + 1;
        let fraction = trim(&digits[whole..]);
        let point = if fraction.is_empty() { String::new() } else { format!(".{fraction}") };
        format!("{sign}{}{point}", &digits[..whole])
    } else {
        format!("{sign}0.{}{}", "0".repeat((-exponent - 1) as usize), trim(&digits))
    })
}

struct TextReader<'a> {
    tokens: Vec<&'a str>,
    at: usize,
    /// 1 when the pointers count from an `asmheader`.
    shift: i32,
    v106: bool,
}

impl<'a> TextReader<'a> {
    fn next(&mut self) -> R<&'a str> {
        let token = *self.tokens.get(self.at).ok_or("a record that ends early")?;
        self.at += 1;
        Ok(token)
    }
}

impl Reader for TextReader<'_> {
    fn ptr(&mut self) -> R<i32> {
        let token = self.next()?;
        let value: i32 = token.strip_prefix('$').and_then(|n| n.parse().ok()).ok_or_else(|| format!("{token} where a pointer belongs"))?;
        shift_in(value, self.shift)
    }
    fn int(&mut self) -> R<i32> {
        let token = self.next()?;
        token.parse().map_err(|_| format!("{token} where an integer belongs"))
    }
    fn dbl(&mut self) -> R<f64> {
        let token = self.next()?;
        finite(token.parse().map_err(|_| format!("{token} where a number belongs"))?)
    }
    fn triple(&mut self, _position: bool) -> R<[f64; 3]> {
        Ok([self.dbl()?, self.dbl()?, self.dbl()?])
    }
    fn flag(&mut self, names: [&str; 2], numeric: bool) -> R<bool> {
        let token = self.next()?;
        match token {
            _ if token == names[0] => Ok(false),
            _ if token == names[1] => Ok(true),
            "0" if numeric && self.v106 => Ok(false),
            "1" if numeric && self.v106 => Ok(true),
            _ => Err(format!("{token} where {} or {} belongs", names[0], names[1])),
        }
    }
    fn bound(&mut self) -> R<Bound> {
        match self.next()? {
            "I" => Ok(Bound::Infinite),
            "F" => Ok(Bound::At(self.dbl()?)),
            other => Err(format!("{other} where a range bound belongs")),
        }
    }
    fn string(&mut self) -> R<String> {
        let count = self.next()?;
        let length: usize = count.strip_prefix('@').and_then(|n| n.parse().ok()).ok_or_else(|| format!("{count} where a counted string belongs"))?;
        let text = self.next()?;
        if text.len() != length {
            return Err("a counted string of another length".to_string());
        }
        Ok(text.to_string())
    }
    fn finished(&self) -> R<()> {
        if self.at == self.tokens.len() {
            Ok(())
        } else {
            Err("a record with more tokens than the samples show".to_string())
        }
    }
}

/// A pointer of a file that counts from an `asmheader` (`shift` 1) as the index of the model, which does not count it.
fn shift_in(value: i32, shift: i32) -> R<i32> {
    match value {
        -1 => Ok(-1),
        _ if value < -1 => Err("a pointer below -1".to_string()),
        0 if shift == 1 => Err("a pointer to the asmheader".to_string()),
        _ => Ok(value - shift),
    }
}

struct BinReader<'a> {
    data: &'a [u8],
    tokens: Vec<(u8, usize, usize)>,
    at: usize,
}

impl BinReader<'_> {
    fn next(&mut self, tag: u8) -> R<(usize, usize)> {
        let &(found, from, to) = self.tokens.get(self.at).ok_or("a record that ends early")?;
        if found != tag {
            return Err(format!("a token of kind {found:#04x} where {tag:#04x} belongs"));
        }
        self.at += 1;
        Ok((from, to))
    }
    fn int_at(&self, from: usize) -> i32 {
        i32::from_le_bytes([self.data[from + 1], self.data[from + 2], self.data[from + 3], self.data[from + 4]])
    }
    fn dbl_at(&self, from: usize) -> R<f64> {
        let bytes: [u8; 8] = self.data[from..from + 8].try_into().map_err(|_| "a truncated number".to_string())?;
        finite(f64::from_le_bytes(bytes))
    }
}

impl Reader for BinReader<'_> {
    fn ptr(&mut self) -> R<i32> {
        let (from, _) = self.next(POINTER)?;
        shift_in(self.int_at(from), 1)
    }
    fn int(&mut self) -> R<i32> {
        let (from, _) = self.next(INTEGER)?;
        Ok(self.int_at(from))
    }
    fn dbl(&mut self) -> R<f64> {
        let (from, _) = self.next(DOUBLE)?;
        self.dbl_at(from + 1)
    }
    fn triple(&mut self, position: bool) -> R<[f64; 3]> {
        let &(tag, from, _) = self.tokens.get(self.at).ok_or("a record that ends early")?;
        if tag != if position { POSITION } else { VECTOR } {
            return Err("a token that is no position or vector where the samples show one".to_string());
        }
        self.at += 1;
        Ok([self.dbl_at(from + 1)?, self.dbl_at(from + 9)?, self.dbl_at(from + 17)?])
    }
    fn flag(&mut self, _names: [&str; 2], _numeric: bool) -> R<bool> {
        let &(tag, ..) = self.tokens.get(self.at).ok_or("a record that ends early")?;
        self.at += 1;
        match tag {
            TRUE => Ok(true),
            FALSE => Ok(false),
            _ => Err("a token that is no logical".to_string()),
        }
    }
    fn bound(&mut self) -> R<Bound> {
        if self.flag(["", ""], false)? {
            Ok(Bound::At(self.dbl()?))
        } else {
            Ok(Bound::Infinite)
        }
    }
    fn string(&mut self) -> R<String> {
        let (from, to) = self.next(STRING_1)?;
        String::from_utf8(self.data[from + 2..to].to_vec()).map_err(|_| "a string that is no text".to_string())
    }
    fn finished(&self) -> R<()> {
        if self.at == self.tokens.len() {
            Ok(())
        } else {
            Err("a record with more tokens than the samples show".to_string())
        }
    }
}

struct TextSink {
    out: String,
}

impl TextSink {
    fn token(&mut self, text: &str) {
        self.out.push(' ');
        self.out.push_str(text);
    }
}

impl Sink for TextSink {
    fn ptr(&mut self, value: i32) {
        self.token(&format!("${value}"));
    }
    fn int(&mut self, value: i32) {
        self.token(&value.to_string());
    }
    fn dbl(&mut self, value: f64) -> R<()> {
        let text = g19(value)?;
        self.token(&text);
        Ok(())
    }
    fn triple(&mut self, value: [f64; 3], _position: bool) -> R<()> {
        value.into_iter().try_for_each(|part| self.dbl(part))
    }
    fn flag(&mut self, names: [&str; 2], numeric: bool, value: bool) {
        self.token(if numeric { if value { "1" } else { "0" } } else { names[usize::from(value)] });
    }
    fn bound(&mut self, value: Bound) -> R<()> {
        match value {
            Bound::Infinite => self.token("I"),
            Bound::At(at) => {
                self.token("F");
                self.dbl(at)?;
            }
        }
        Ok(())
    }
    fn string(&mut self, value: &str) {
        self.token(&format!("@{}", value.len()));
        self.token(value);
    }
}

struct BinSink {
    out: Vec<u8>,
}

impl Sink for BinSink {
    fn ptr(&mut self, value: i32) {
        self.out.push(POINTER);
        self.out.extend(value.to_le_bytes());
    }
    fn int(&mut self, value: i32) {
        self.out.push(INTEGER);
        self.out.extend(value.to_le_bytes());
    }
    fn dbl(&mut self, value: f64) -> R<()> {
        finite(value)?;
        self.out.push(DOUBLE);
        self.out.extend(value.to_le_bytes());
        Ok(())
    }
    fn triple(&mut self, value: [f64; 3], position: bool) -> R<()> {
        self.out.push(if position { POSITION } else { VECTOR });
        for part in value {
            self.out.extend(finite(part)?.to_le_bytes());
        }
        Ok(())
    }
    fn flag(&mut self, _names: [&str; 2], _numeric: bool, value: bool) {
        self.out.push(if value { TRUE } else { FALSE });
    }
    fn bound(&mut self, value: Bound) -> R<()> {
        match value {
            Bound::Infinite => self.flag(["", ""], false, false),
            Bound::At(at) => {
                self.flag(["", ""], false, true);
                self.dbl(at)?;
            }
        }
        Ok(())
    }
    fn string(&mut self, value: &str) {
        self.out.push(STRING_1);
        self.out.push(value.len() as u8);
        self.out.extend(value.as_bytes());
    }
}

/// The value a field has in every record of the samples; any other value is refused.
fn expect(found: i32, wanted: i32, what: &str) -> R<()> {
    if found == wanted {
        Ok(())
    } else {
        Err(format!("{what} {found}, which the samples do not show (they state {wanted})"))
    }
}

/// The attribute pointer and, from 20800, the `-1` and `$-1` after it.
fn base(r: &mut dyn Reader, l: Layout) -> R<i32> {
    let attr = r.ptr()?;
    if l.modern {
        expect(r.int()?, -1, "an entity identifier")?;
        expect(r.ptr()?, -1, "a history pointer")?;
    }
    Ok(attr)
}

fn put_base(w: &mut dyn Sink, l: Layout, attr: i32) {
    w.ptr(attr);
    if l.modern {
        w.int(-1);
        w.ptr(-1);
    }
}

const SENSE: [&str; 2] = ["forward", "reversed"];

fn read_rec(name: &str, r: &mut dyn Reader, l: Layout) -> R<Rec> {
    let rec = match name {
        "body" => Rec::Body { attr: base(r, l)?, lump: r.ptr()?, wire: r.ptr()?, transform: r.ptr()? },
        "lump" => Rec::Lump { attr: base(r, l)?, next: r.ptr()?, shell: r.ptr()?, body: r.ptr()? },
        "shell" => {
            let (attr, next, subshell, face) = (base(r, l)?, r.ptr()?, r.ptr()?, r.ptr()?);
            // R14's shell has no `wire`
            let wire = if l.v106 { -1 } else { r.ptr()? };
            Rec::Shell { attr, next, subshell, face, wire, lump: r.ptr()? }
        }
        "face" => {
            let (attr, next, first_loop, shell, subshell, surface) = (base(r, l)?, r.ptr()?, r.ptr()?, r.ptr()?, r.ptr()?, r.ptr()?);
            let reversed = r.flag(SENSE, false)?;
            let double = r.flag(["single", "double"], false)?;
            let inside = if double { r.flag(["out", "in"], false)? } else { false };
            if inside {
                return Err("a double-sided face inside, which the samples do not show".to_string());
            }
            Rec::Face { attr, next, first_loop, shell, subshell, surface, reversed, double, inside }
        }
        "loop" => Rec::Loop { attr: base(r, l)?, next: r.ptr()?, coedge: r.ptr()?, face: r.ptr()? },
        "coedge" => {
            let (attr, next, prev, partner, edge) = (base(r, l)?, r.ptr()?, r.ptr()?, r.ptr()?, r.ptr()?);
            let reversed = r.flag(SENSE, true)?;
            let owner = r.ptr()?;
            if l.coedge_state {
                expect(r.int()?, 0, "a coedge integer")?;
            }
            Rec::Coedge { attr, next, prev, partner, edge, reversed, owner, pcurve: r.ptr()? }
        }
        "edge" => {
            let attr = base(r, l)?;
            let start = r.ptr()?;
            let start_param = if l.modern { Some(r.dbl()?) } else { None };
            let end = r.ptr()?;
            let end_param = if l.modern { Some(r.dbl()?) } else { None };
            let (coedge, curve) = (r.ptr()?, r.ptr()?);
            let reversed = r.flag(SENSE, true)?;
            if l.modern && r.string()? != "unknown" {
                return Err("an edge string other than unknown".to_string());
            }
            Rec::Edge { attr, start, start_param, end, end_param, coedge, curve, reversed }
        }
        "vertex" => {
            let (attr, edge) = (base(r, l)?, r.ptr()?);
            let state = if l.vertex_state { Some(r.int()?) } else { None };
            Rec::Vertex { attr, edge, state, point: r.ptr()? }
        }
        "point" => Rec::Point { attr: base(r, l)?, position: r.triple(true)? },
        "transform" => {
            let attr = r.ptr()?;
            if l.modern {
                expect(r.int()?, -1, "an entity identifier")?;
            }
            let rows = [r.triple(false)?, r.triple(false)?, r.triple(false)?];
            let (translation, scale) = (r.triple(false)?, r.dbl()?);
            let rotate = r.flag(["no_rotate", "rotate"], true)?;
            let reflect = r.flag(["no_reflect", "reflect"], true)?;
            let shear = r.flag(["no_shear", "shear"], true)?;
            Rec::Transform { attr, rows, translation, scale, rotate, reflect, shear }
        }
        "plane-surface" => {
            let attr = base(r, l)?;
            let (root, normal, u_direction) = (r.triple(true)?, r.triple(false)?, r.triple(false)?);
            let reverse_v = r.flag(["forward_v", "reverse_v"], true)?;
            let bounds = [r.bound()?, r.bound()?, r.bound()?, r.bound()?];
            if bounds != [Bound::Infinite; 4] {
                return Err("a plane with finite bounds, which the samples do not show".to_string());
            }
            Rec::Plane { attr, root, normal, u_direction, reverse_v, bounds }
        }
        "straight-curve" => {
            let attr = base(r, l)?;
            let (root, direction) = (r.triple(true)?, r.triple(false)?);
            Rec::Straight { attr, root, direction, bounds: [r.bound()?, r.bound()?] }
        }
        "persubent-acadSolidHistory-attrib" => {
            let attr = base(r, l)?;
            // The text has two pointers between the attribute and the owner, the later releases one (the samples state $-1 in all).
            expect(r.ptr()?, -1, "an attribute pointer")?;
            if !l.modern {
                expect(r.ptr()?, -1, "an attribute pointer")?;
            }
            let owner = r.ptr()?;
            Rec::History { attr, owner, numbers: [r.int()?, r.int()?, r.int()?, r.int()?] }
        }
        other => return Err(format!("a record of the kind {other}, which the samples do not show")),
    };
    r.finished()?;
    Ok(rec)
}

fn write_rec(rec: &Rec, w: &mut dyn Sink, l: Layout) -> R<()> {
    match *rec {
        Rec::Body { attr, lump, wire, transform } => {
            put_base(w, l, attr);
            [lump, wire, transform].into_iter().for_each(|p| w.ptr(p));
        }
        Rec::Lump { attr, next, shell, body } => {
            put_base(w, l, attr);
            [next, shell, body].into_iter().for_each(|p| w.ptr(p));
        }
        Rec::Shell { attr, next, subshell, face, wire, lump } => {
            put_base(w, l, attr);
            [next, subshell, face].into_iter().for_each(|p| w.ptr(p));
            if l.v106 {
                expect(wire, -1, "a shell wire")?;
            } else {
                w.ptr(wire);
            }
            w.ptr(lump);
        }
        Rec::Face { attr, next, first_loop, shell, subshell, surface, reversed, double, inside } => {
            put_base(w, l, attr);
            [next, first_loop, shell, subshell, surface].into_iter().for_each(|p| w.ptr(p));
            w.flag(SENSE, false, reversed);
            w.flag(["single", "double"], false, double);
            if double {
                w.flag(["out", "in"], false, inside);
            }
        }
        Rec::Loop { attr, next, coedge, face } => {
            put_base(w, l, attr);
            [next, coedge, face].into_iter().for_each(|p| w.ptr(p));
        }
        Rec::Coedge { attr, next, prev, partner, edge, reversed, owner, pcurve } => {
            put_base(w, l, attr);
            [next, prev, partner, edge].into_iter().for_each(|p| w.ptr(p));
            w.flag(SENSE, l.v106, reversed);
            w.ptr(owner);
            if l.coedge_state {
                w.int(0);
            }
            w.ptr(pcurve);
        }
        Rec::Edge { attr, start, start_param, end, end_param, coedge, curve, reversed } => {
            put_base(w, l, attr);
            w.ptr(start);
            if l.modern {
                w.dbl(start_param.ok_or("an edge without curve parameters")?)?;
            }
            w.ptr(end);
            if l.modern {
                w.dbl(end_param.ok_or("an edge without curve parameters")?)?;
            }
            w.ptr(coedge);
            w.ptr(curve);
            w.flag(SENSE, l.v106, reversed);
            if l.modern {
                w.string("unknown");
            }
        }
        Rec::Vertex { attr, edge, state, point } => {
            put_base(w, l, attr);
            w.ptr(edge);
            if l.vertex_state {
                w.int(state.ok_or("a vertex without its integer")?);
            }
            w.ptr(point);
        }
        Rec::Point { attr, position } => {
            put_base(w, l, attr);
            w.triple(position, true)?;
        }
        Rec::Transform { attr, rows, translation, scale, rotate, reflect, shear } => {
            w.ptr(attr);
            if l.modern {
                w.int(-1);
            }
            for row in rows {
                w.triple(row, false)?;
            }
            w.triple(translation, false)?;
            w.dbl(scale)?;
            w.flag(["no_rotate", "rotate"], l.v106, rotate);
            w.flag(["no_reflect", "reflect"], l.v106, reflect);
            w.flag(["no_shear", "shear"], l.v106, shear);
        }
        Rec::Plane { attr, root, normal, u_direction, reverse_v, bounds } => {
            put_base(w, l, attr);
            w.triple(root, true)?;
            w.triple(normal, false)?;
            w.triple(u_direction, false)?;
            w.flag(["forward_v", "reverse_v"], l.v106, reverse_v);
            bounds.into_iter().try_for_each(|b| w.bound(b))?;
        }
        Rec::Straight { attr, root, direction, bounds } => {
            put_base(w, l, attr);
            w.triple(root, true)?;
            w.triple(direction, false)?;
            bounds.into_iter().try_for_each(|b| w.bound(b))?;
        }
        Rec::History { attr, owner, numbers } => {
            put_base(w, l, attr);
            w.ptr(-1);
            if !l.modern {
                w.ptr(-1);
            }
            w.ptr(owner);
            numbers.into_iter().for_each(|n| w.int(n));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// Reading a whole body
// ---------------------------------------------------------------------------------------------------------------------

/// The first line of the text: version, number of records, number of bodies and the flag word.
fn version_line(line: &str) -> R<[i32; 4]> {
    let numbers: Vec<i32> = line.split_whitespace().map(|part| part.parse().map_err(|_| "a first line that is no header".to_string())).collect::<R<_>>()?;
    numbers.try_into().map_err(|_| "a first line that is no header".to_string())
}

/// Three strings, each after its length, then a space: the product line.
fn counted_strings(line: &str) -> R<[String; 3]> {
    if !line.is_ascii() {
        return Err("a product line that is no ASCII".to_string());
    }
    let mut at = 0;
    let mut out: [String; 3] = Default::default();
    for item in &mut out {
        let space = line[at..].find(' ').ok_or("a product line that ends early")?;
        let length: usize = line[at..at + space].parse().map_err(|_| "a product line without lengths".to_string())?;
        at += space + 1;
        *item = line.get(at..at + length).ok_or("a product line that ends early")?.to_string();
        at += length;
        if line.as_bytes().get(at) != Some(&b' ') {
            return Err("a product line without its separators".to_string());
        }
        at += 1;
    }
    if at != line.len() {
        return Err("a product line with more than the samples show".to_string());
    }
    Ok(out)
}

fn flags_of(save: i32) -> Option<i32> {
    SAVES.iter().find(|(_, version, _)| *version == save).map(|(.., flags)| *flags)
}

fn header_from_text(lines: &[&str]) -> R<Header> {
    let [product, asm, date] = counted_strings(lines.get(1).ok_or("a header without a product line")?)?;
    if product != AUTOCAD {
        return Err(format!("data of the producer {product}"));
    }
    let numbers: Vec<f64> = lines
        .get(2)
        .ok_or("a header without the units line")?
        .split_whitespace()
        .map(|part| part.parse().ok().filter(|n: &f64| n.is_finite()).ok_or_else(|| "a units line that is no numbers".to_string()))
        .collect::<R<_>>()?;
    let [units, resabs, resnor]: [f64; 3] = numbers.try_into().map_err(|_| "a units line without three numbers".to_string())?;
    Ok(Header { product, asm, date, units, resabs, resnor })
}

fn parse_text(text: &str) -> R<Model> {
    let text = AcisData::strip_sat_terminator(text);
    let lines: Vec<&str> = text.lines().collect();
    let [version, count, bodies, flags] = version_line(lines.first().ok_or("empty data")?)?;
    let form = match version {
        106 => Form::Sat106,
        400 => Form::Sat400,
        20800 | 21200 | 21500 => Form::Modern(version),
        other => return Err(format!("SAT text of the version {other}, which the samples do not show")),
    };
    let layout = Layout::of(form);
    let (wanted_bodies, wanted_flags) = if layout.modern { (2, flags_of(version).unwrap_or(-1)) } else { (1, 0) };
    if bodies != wanted_bodies || flags != wanted_flags {
        return Err("a header that is not AutoCAD's".to_string());
    }
    let (header, first) = if form == Form::Sat106 { (None, 1) } else { (Some(header_from_text(&lines)?), 3) };
    let mut records: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for token in lines[first..].iter().flat_map(|line| line.split_whitespace()) {
        if token == "#" {
            records.push(std::mem::take(&mut current));
        } else {
            current.push(token);
        }
    }
    if !current.is_empty() {
        return Err("a record without its end".to_string());
    }
    let mut records = records.into_iter();
    if layout.modern {
        // asmheader $-1 -1 @13 230.7.0.65535
        let head = records.next().ok_or("no asmheader")?;
        let mut reader = TextReader { tokens: head.get(1..).unwrap_or(&[]).to_vec(), at: 0, shift: 0, v106: false };
        if head.first() != Some(&"asmheader") || reader.ptr()? != -1 || reader.int()? != -1 || reader.string()? != ASM_ID {
            return Err("an asmheader other than AutoCAD's".to_string());
        }
        reader.finished()?;
    }
    let expected = usize::try_from(count).ok().and_then(|count| count.checked_sub(usize::from(layout.modern))).ok_or("a header with a negative number of records")?;
    let mut recs = Vec::new();
    for record in records {
        let name = record.first().ok_or("an empty record")?;
        let mut reader = TextReader { tokens: record[1..].to_vec(), at: 0, shift: i32::from(layout.modern), v106: layout.v106 };
        recs.push(read_rec(name, &mut reader, layout)?);
    }
    if recs.len() != expected {
        return Err("a header that states another number of records".to_string());
    }
    let model = Model { header, recs };
    model.check(layout)?;
    Ok(model)
}

fn parse_binary(data: &[u8]) -> R<Model> {
    if data.len() < 31 || !(data.starts_with(ACIS_MAGIC) || data.starts_with(ASM_MAGIC)) {
        return Err("data that is no AutoCAD's binary modeler data".to_string());
    }
    let word = |at: usize| i32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
    let (save, bodies, flags) = (word(15), word(23), word(27));
    if flags_of(save) != Some(flags) || data.starts_with(ASM_MAGIC) != (save == 22300) || bodies != 2 {
        return Err("a header that is not AutoCAD's".to_string());
    }
    let layout = Layout::of(Form::Modern(save));
    // The tokens, grouped in records; the file ends with `End of ACIS data`, whose last token has no `#`.
    let mut records: Vec<Vec<(u8, usize, usize)>> = Vec::new();
    let mut current: Vec<(u8, usize, usize)> = Vec::new();
    let mut at = 31;
    while at < data.len() {
        let end = token_end(data, at).ok_or("a token the samples do not show")?;
        current.push((data[at], at, end));
        at = end;
        let last = current[current.len() - 1];
        if last.0 == END || (last.0 == IDENT && &data[last.1 + 2..last.2] == b"data" && current.len() == 4) {
            records.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        return Err("a record without its end".to_string());
    }
    let ending = records.pop().ok_or("no records")?;
    let words: Vec<&[u8]> = ending.iter().map(|&(tag, from, to)| if tag == SUB_IDENT || tag == IDENT { &data[from + 2..to] } else { &[][..] }).collect();
    let want: [&[u8]; 4] = [b"End", b"of", if save >= 21800 { b"ASM" } else { b"ACIS" }, b"data"];
    if words != want {
        return Err("an end record other than AutoCAD's".to_string());
    }
    let mut records = records.into_iter();
    // product, version, date, units, tolerances, `asmheader $-1 -1 <version>`
    let mut head = records.next().ok_or("no asmheader")?;
    head.pop();
    let mut reader = BinReader { data, tokens: head, at: 0 };
    let (product, asm, date) = (reader.string()?, reader.string()?, reader.string()?);
    let (units, resabs, resnor) = (reader.dbl()?, reader.dbl()?, reader.dbl()?);
    let (name_from, name_to) = reader.next(IDENT)?;
    if &data[name_from + 2..name_to] != b"asmheader" || reader.ptr_raw()? != -1 || reader.int()? != -1 || reader.string()? != ASM_ID {
        return Err("an asmheader other than AutoCAD's".to_string());
    }
    reader.finished()?;
    if product != AUTOCAD {
        return Err(format!("data of the producer {product}"));
    }
    let mut recs = Vec::new();
    for record in records {
        let mut name = String::new();
        let mut used = 0;
        for &(tag, from, to) in &record {
            if tag != SUB_IDENT && tag != IDENT {
                break;
            }
            if !name.is_empty() {
                name.push('-');
            }
            name.push_str(std::str::from_utf8(&data[from + 2..to]).map_err(|_| "a name that is no text".to_string())?);
            used += 1;
            if tag == IDENT {
                break;
            }
        }
        if record.last().map(|token| token.0) != Some(END) || used >= record.len() {
            return Err("a record without its end".to_string());
        }
        let mut reader = BinReader { data, tokens: record[used..record.len() - 1].to_vec(), at: 0 };
        recs.push(read_rec(&name, &mut reader, layout)?);
    }
    let model = Model { header: Some(Header { product, asm, date, units, resabs, resnor }), recs };
    model.check(layout)?;
    Ok(model)
}

impl BinReader<'_> {
    /// A pointer as the file states it.
    fn ptr_raw(&mut self) -> R<i32> {
        let (from, _) = self.next(POINTER)?;
        Ok(self.int_at(from))
    }
}

impl Model {
    fn rec(&self, pointer: i32) -> R<&Rec> {
        usize::try_from(pointer).ok().and_then(|index| self.recs.get(index)).ok_or_else(|| "a pointer to a record that is not there".to_string())
    }

    /// Every pointer reaches a record, and the integer of a vertex is the one the samples show.
    fn check(&self, layout: Layout) -> R<()> {
        for rec in &self.recs {
            for pointer in rec.pointers() {
                if pointer != -1 {
                    self.rec(pointer)?;
                }
            }
        }
        if layout.vertex_state {
            for (index, rec) in self.recs.iter().enumerate() {
                if let Rec::Vertex { state, .. } = rec {
                    if *state != Some(self.vertex_state(index as i32)?) {
                        return Err("a vertex integer other than the samples show (0 at the start of its edge, 1 at the end)".to_string());
                    }
                }
            }
        }
        Ok(())
    }

    /// 0 when the vertex is the start vertex of its edge, 1 when it is the end vertex.
    fn vertex_state(&self, vertex: i32) -> R<i32> {
        let Rec::Vertex { edge, .. } = self.rec(vertex)? else { return Err("a vertex pointer to another record".to_string()) };
        match self.rec(*edge)? {
            Rec::Edge { start, .. } if *start == vertex => Ok(0),
            Rec::Edge { end, .. } if *end == vertex => Ok(1),
            _ => Err("a vertex that is neither end of its edge".to_string()),
        }
    }

    fn vertex_position(&self, vertex: i32) -> R<[f64; 3]> {
        let Rec::Vertex { point, .. } = self.rec(vertex)? else { return Err("an edge end that is no vertex".to_string()) };
        let Rec::Point { position, .. } = self.rec(*point)? else { return Err("a vertex without a point".to_string()) };
        Ok(*position)
    }

    /// The curve parameters of the two vertices of an edge: the range of its curve when finite, else the vertex position
    /// distance from the root of the curve.
    fn edge_params(&self, edge: &Rec) -> R<(f64, f64)> {
        let Rec::Edge { start, end, curve, reversed, .. } = edge else { return Err("not an edge".to_string()) };
        if *reversed {
            return Err("a reversed edge, which the samples do not show".to_string());
        }
        let Rec::Straight { root, direction, bounds, .. } = self.rec(*curve)? else {
            return Err("an edge on a curve other than a straight one, which the samples do not show".to_string());
        };
        match bounds {
            [Bound::At(from), Bound::At(to)] => Ok((*from, *to)),
            [Bound::Infinite, Bound::Infinite] => {
                let along = |vertex: i32| -> R<f64> {
                    let position = self.vertex_position(vertex)?;
                    let d = [position[0] - root[0], position[1] - root[1], position[2] - root[2]];
                    let distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
                    // the sign: the side of the root the direction points to
                    Ok(if d[0] * direction[0] + d[1] * direction[1] + d[2] * direction[2] < 0.0 { -distance } else { distance })
                };
                Ok((along(*start)?, along(*end)?))
            }
            _ => Err("a curve with one finite bound, which the samples do not show".to_string()),
        }
    }

    /// The values a release states that the model lacks: the curve parameters of an edge, the integer of a vertex.
    fn completed(&self) -> R<Model> {
        let mut recs = Vec::with_capacity(self.recs.len());
        for (index, rec) in self.recs.iter().enumerate() {
            recs.push(match rec {
                Rec::Edge { attr, start, start_param, end, end_param, coedge, curve, reversed } => {
                    let (from, to) = match (start_param, end_param) {
                        (Some(from), Some(to)) => (*from, *to),
                        _ => self.edge_params(rec)?,
                    };
                    Rec::Edge { attr: *attr, start: *start, start_param: Some(from), end: *end, end_param: Some(to), coedge: *coedge, curve: *curve, reversed: *reversed }
                }
                Rec::Vertex { attr, edge, point, .. } => Rec::Vertex { attr: *attr, edge: *edge, state: Some(self.vertex_state(index as i32)?), point: *point },
                other => other.clone(),
            });
        }
        Ok(Model { header: self.header.clone(), recs })
    }

    /// The head of a body in a release that states it, from the model or (R14 has none) from what every body of the samples states.
    fn header_for(&self, context: &Context) -> R<Header> {
        let mut header = match &self.header {
            Some(header) => header.clone(),
            None => {
                if context.insertion_units != 1 {
                    return Err(format!("a body of R14 without units, in a drawing with INSUNITS {} (the samples show inches only)", context.insertion_units));
                }
                Header { product: AUTOCAD.to_string(), asm: ASM.to_string(), date: String::new(), units: INCH_UNITS, resabs: 1e-6, resnor: 1e-10 }
            }
        };
        if header.date.is_empty() {
            // AutoCAD states a date at every release (the DXF has none).
            header.date = context.date.clone().unwrap_or_default();
        }
        Ok(header)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Writing a whole body
// ---------------------------------------------------------------------------------------------------------------------

fn emit_text(model: &Model, form: Form, context: &Context) -> R<String> {
    let layout = Layout::of(form);
    let count = model.recs.len();
    let version = if form == Form::Sat106 { 106 } else { 400 };
    let mut out = format!("{version} {count} 1 0{}\n", " ".repeat(12usize.saturating_sub(count.to_string().len()).max(1)));
    if form == Form::Sat400 {
        let header = model.header_for(context)?;
        out.push_str(&format!(
            "{} {} {} {} {} {} \n{} {} {} \n",
            header.product.len(),
            header.product,
            header.asm.len(),
            header.asm,
            header.date.len(),
            header.date,
            g19(header.units)?,
            g19(header.resabs)?,
            g19(header.resnor)?
        ));
    }
    for rec in &model.recs {
        let mut sink = TextSink { out: String::new() };
        write_rec(rec, &mut sink, layout)?;
        out.push_str(rec.name());
        out.push_str(&sink.out);
        out.push_str(" #\n");
    }
    Ok(out)
}

fn emit_binary(model: &Model, save: i32, context: &Context) -> R<Vec<u8>> {
    let layout = Layout::of(Form::Modern(save));
    let model = model.completed()?;
    let header = model.header_for(context)?;
    let flags = flags_of(save).ok_or("a save version the samples do not show")?;
    let mut out = if save == 22300 { ASM_MAGIC.to_vec() } else { ACIS_MAGIC.to_vec() };
    for word in [save, 0, 2, flags] {
        out.extend(word.to_le_bytes());
    }
    let mut sink = BinSink { out };
    for text in [&header.product, &header.asm, &header.date] {
        sink.string(text);
    }
    for value in [header.units, header.resabs, header.resnor] {
        sink.dbl(value)?;
    }
    sink.out.extend([IDENT, "asmheader".len() as u8]);
    sink.out.extend(b"asmheader");
    sink.ptr(-1);
    sink.int(-1);
    sink.string(ASM_ID);
    sink.out.push(END);
    for rec in &model.recs {
        let parts: Vec<&str> = rec.name().split('-').collect();
        for (index, part) in parts.iter().enumerate() {
            sink.out.extend([if index + 1 == parts.len() { IDENT } else { SUB_IDENT }, part.len() as u8]);
            sink.out.extend(part.as_bytes());
        }
        // pointers count from the asmheader
        let shifted = shifted_up(rec);
        write_rec(&shifted, &mut sink, layout)?;
        sink.out.push(END);
    }
    for part in ["End", "of", if save >= 21800 { "ASM" } else { "ACIS" }] {
        sink.out.extend([SUB_IDENT, part.len() as u8]);
        sink.out.extend(part.as_bytes());
    }
    sink.out.extend([IDENT, 4]);
    sink.out.extend(b"data");
    Ok(sink.out)
}

/// The record with every pointer one higher (the binary data counts the `asmheader`).
fn shifted_up(rec: &Rec) -> Rec {
    let up = |p: i32| if p >= 0 { p + 1 } else { p };
    match *rec {
        Rec::Body { attr, lump, wire, transform } => Rec::Body { attr: up(attr), lump: up(lump), wire: up(wire), transform: up(transform) },
        Rec::Lump { attr, next, shell, body } => Rec::Lump { attr: up(attr), next: up(next), shell: up(shell), body: up(body) },
        Rec::Shell { attr, next, subshell, face, wire, lump } => {
            Rec::Shell { attr: up(attr), next: up(next), subshell: up(subshell), face: up(face), wire: up(wire), lump: up(lump) }
        }
        Rec::Face { attr, next, first_loop, shell, subshell, surface, reversed, double, inside } => Rec::Face {
            attr: up(attr),
            next: up(next),
            first_loop: up(first_loop),
            shell: up(shell),
            subshell: up(subshell),
            surface: up(surface),
            reversed,
            double,
            inside,
        },
        Rec::Loop { attr, next, coedge, face } => Rec::Loop { attr: up(attr), next: up(next), coedge: up(coedge), face: up(face) },
        Rec::Coedge { attr, next, prev, partner, edge, reversed, owner, pcurve } => Rec::Coedge {
            attr: up(attr),
            next: up(next),
            prev: up(prev),
            partner: up(partner),
            edge: up(edge),
            reversed,
            owner: up(owner),
            pcurve: up(pcurve),
        },
        Rec::Edge { attr, start, start_param, end, end_param, coedge, curve, reversed } => Rec::Edge {
            attr: up(attr),
            start: up(start),
            start_param,
            end: up(end),
            end_param,
            coedge: up(coedge),
            curve: up(curve),
            reversed,
        },
        Rec::Vertex { attr, edge, state, point } => Rec::Vertex { attr: up(attr), edge: up(edge), state, point: up(point) },
        Rec::Point { attr, position } => Rec::Point { attr: up(attr), position },
        Rec::Transform { attr, rows, translation, scale, rotate, reflect, shear } => Rec::Transform { attr: up(attr), rows, translation, scale, rotate, reflect, shear },
        Rec::Plane { attr, root, normal, u_direction, reverse_v, bounds } => Rec::Plane { attr: up(attr), root, normal, u_direction, reverse_v, bounds },
        Rec::Straight { attr, root, direction, bounds } => Rec::Straight { attr: up(attr), root, direction, bounds },
        Rec::History { attr, owner, numbers } => Rec::History { attr: up(attr), owner: up(owner), numbers },
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------------------------------------------------

fn target_form(target: DxfVersion) -> Option<Form> {
    match target {
        DxfVersion::AC1014 => Some(Form::Sat106),
        DxfVersion::AC1015 => Some(Form::Sat400),
        other => SAVES.iter().find(|(version, ..)| *version == other).map(|(_, save, _)| Form::Modern(*save)),
    }
}

/// The form AutoCAD's data is in, or None for another producer's data (which is left as it is).
fn source_form(acis: &AcisData) -> Option<Form> {
    if acis.is_binary {
        let data = &acis.sab_data;
        if data.len() < 31 || !(data.starts_with(ACIS_MAGIC) || data.starts_with(ASM_MAGIC)) {
            return None;
        }
        let (save, flags) = (i32::from_le_bytes(data[15..19].try_into().ok()?), i32::from_le_bytes(data[27..31].try_into().ok()?));
        return (flags_of(save) == Some(flags)).then_some(Form::Modern(save));
    }
    let mut lines = acis.sat_data.lines();
    let version = lines.next()?.split_whitespace().next()?;
    let mut autocad = || lines.next().is_some_and(|line| line.starts_with("16 Autodesk AutoCAD "));
    match version {
        "106" => Some(Form::Sat106),
        "400" => autocad().then_some(Form::Sat400),
        "20800" | "21200" | "21500" => autocad().then(|| Form::Modern(version.parse().unwrap_or(0))),
        _ => None,
    }
}

/// AutoCAD's data of `acis` as the release `target` states it: None when the data is already in that form or is another
/// producer's (or a binary release's, which the writer re-versions itself), an error naming the first thing the samples do not show.
pub(crate) fn convert(acis: &AcisData, target: DxfVersion, context: &Context) -> R<Option<Converted>> {
    let Some(goal) = target_form(target) else { return Ok(None) };
    let Some(source) = source_form(acis) else { return Ok(None) };
    let legacy = |form: Form| matches!(form, Form::Sat106 | Form::Sat400);
    if source == goal || (!legacy(source) && !legacy(goal)) {
        return Ok(None);
    }
    let model = if acis.is_binary { parse_binary(&acis.sab_data)? } else { parse_text(&acis.sat_data)? };
    Ok(Some(match goal {
        Form::Modern(save) => Converted { sat_data: String::new(), sab_data: emit_binary(&model, save, context)? },
        form => Converted { sat_data: emit_text(&model, form, context)?, sab_data: Vec::new() },
    }))
}

/// Brings the modeler data of every 3DSOLID, REGION and BODY into the form of the release the drawing is written as. A body
/// that cannot be converted exactly as the samples show refuses the write.
pub(crate) fn prepare_for_write(document: &mut Cow<'_, CadDocument>) -> Result<()> {
    let target = document.version;
    if target_form(target).is_none() {
        return Ok(());
    }
    let context = Context { insertion_units: document.header.insertion_units, date: drawing_date(document.header.update_date_julian) };
    let mut converted: Vec<(Handle, Converted)> = Vec::new();
    for entity in document.entities() {
        let (acis, label) = match entity {
            EntityType::Solid3D(solid) => (&solid.acis_data, "3DSOLID"),
            EntityType::Region(region) => (&region.acis_data, "REGION"),
            EntityType::Body(body) => (&body.acis_data, "BODY"),
            _ => continue,
        };
        if !acis.has_data() {
            continue;
        }
        match convert(acis, target, &context) {
            Ok(Some(data)) => converted.push((entity.common().handle, data)),
            Ok(None) => {}
            Err(reason) => {
                return Err(DxfError::NotImplemented(format!(
                    "Cannot state the modeler data of {label} {} as DWG {target:?}: {reason}",
                    entity.common().handle
                )))
            }
        }
    }
    if converted.is_empty() {
        return Ok(());
    }
    let document = document.to_mut();
    for (handle, data) in converted {
        let Some(entity) = document.get_entity_mut(handle) else { continue };
        let acis = match entity {
            EntityType::Solid3D(solid) => &mut solid.acis_data,
            EntityType::Region(region) => &mut region.acis_data,
            EntityType::Body(body) => &mut body.acis_data,
            _ => continue,
        };
        let binary = !data.sab_data.is_empty();
        acis.version = if binary { AcisVersion::Version2 } else { AcisVersion::Version1 };
        acis.is_binary = binary;
        acis.sat_data = data.sat_data;
        acis.sab_data = data.sab_data;
        acis.source = None;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::conversion_samples as s;
    use super::*;
    use crate::entities::solid3d::{Region, Solid3D};

    /// The date of the 400 text and the binary data of R2004 and R2007 in the samples.
    const DATE: &str = "Tue Dec  2 10:27:20 2025";
    /// The dates of the binary data of R2013 and R2018 (another run of the drawing, same length).
    const OTHER_DATES: [&str; 2] = ["Tue Dec  2 10:27:19 2025", "Wed Sep 24 12:29:03 2025"];

    fn bytes(hex: &str) -> Vec<u8> {
        (0..hex.len()).step_by(2).map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap()).collect()
    }

    /// The data with the date of any release of the samples replaced by `DATE` (the tests compare everything else).
    fn dated(mut data: Vec<u8>) -> Vec<u8> {
        for other in OTHER_DATES {
            if let Some(at) = data.windows(24).position(|window| window == other.as_bytes()) {
                data[at..at + 24].copy_from_slice(DATE.as_bytes());
            }
        }
        data
    }

    fn context() -> Context {
        Context { insertion_units: 1, date: Some(DATE.to_string()) }
    }

    fn converted(acis: &AcisData, target: DxfVersion) -> Converted {
        convert(acis, target, &context()).unwrap().unwrap_or_else(|| panic!("{target:?} keeps the data"))
    }

    /// (name, SAT 106, SAT 400, [(save version, binary data)])
    fn bodies() -> Vec<(&'static str, &'static str, &'static str, Vec<(i32, Vec<u8>)>)> {
        vec![
            (
                "REGION #D69",
                s::REGION_106,
                s::REGION_400,
                vec![(20800, bytes(s::REGION_20800)), (21200, bytes(s::REGION_21200)), (21800, bytes(s::REGION_21800)), (22300, bytes(s::REGION_22300))],
            ),
            ("3DSOLID #D65", s::BOX_106, s::BOX_400, vec![(22300, bytes(s::BOX_22300))]),
        ]
    }

    fn target_of(save: i32) -> DxfVersion {
        SAVES.iter().find(|(_, version, _)| *version == save).unwrap().0
    }

    #[test]
    fn doubles_print_as_the_modeler_prints_them() {
        for (value, text) in [
            (25.4, "25.39999999999999858"),
            (1e-6, "9.999999999999999547e-07"),
            (1e-10, "1.000000000000000036e-10"),
            (0.0, "0"),
            (-0.0, "-0"),
            (2.5, "2.5"),
            (0.1, "0.1000000000000000056"),
            (1.0 / 3.0, "0.3333333333333333148"),
            (100.0, "100"),
            (1e15, "1000000000000000"),
            (1e18, "1000000000000000000"),
            (1e19, "1e+19"),
            (123.456, "123.4560000000000031"),
            (-220.8501226607159253, "-220.8501226607159253"),
            (1e-5, "1.000000000000000082e-05"),
            (0.0001, "0.0001000000000000000048"),
            (1.2345678901234568e20, "1.23456789012345684e+20"),
            (-2.5e-7, "-2.499999999999999887e-07"),
            (4.380771426791327, "4.380771426791326917"),
        ] {
            assert_eq!(g19(value).unwrap(), text, "{value:e}");
        }
        assert!(g19(f64::NAN).is_err() && g19(f64::INFINITY).is_err());
    }

    /// AutoCAD's own R14 and R2000 text of the three bodies is what the data of every later release converts to.
    #[test]
    fn down_saves_state_the_text_of_the_release() {
        for (name, sat106, sat400, binaries) in bodies() {
            for (save, data) in binaries {
                let source = AcisData::from_sab(data);
                let text = converted(&source, DxfVersion::AC1015).sat_data;
                assert_eq!(String::from_utf8(dated(text.into_bytes())).unwrap(), sat400, "{name} {save} to R2000");
                let text = converted(&source, DxfVersion::AC1014).sat_data;
                assert_eq!(text, sat106, "{name} {save} to R14");
            }
        }
        // the text of a DXF of R2004 (no date) states the date of the drawing
        let dxf = AcisData::from_sat(s::REGION_DXF_20800);
        assert_eq!(converted(&dxf, DxfVersion::AC1015).sat_data, s::REGION_400);
        assert_eq!(converted(&dxf, DxfVersion::AC1014).sat_data, s::REGION_106);
    }

    /// The data of R14 and R2000 converts to the binary data AutoCAD's own down-saves of the drawing have in every later release
    /// (the date aside), the curve parameters, vertex and coedge integers derived: the REGION has finite curves, the 3DSOLID
    /// infinite ones and an attribute.
    #[test]
    fn up_saves_state_the_binary_data_of_the_release() {
        for (name, sat106, sat400, binaries) in bodies() {
            for (save, data) in binaries {
                let target = target_of(save);
                for (from, text) in [("R2000", sat400), ("R14", sat106)] {
                    let source = AcisData::from_sat(text);
                    let made = converted(&source, target);
                    assert!(made.sat_data.is_empty());
                    assert_eq!(dated(made.sab_data), dated(data.clone()), "{name} {from} to {save}");
                }
            }
        }
    }

    #[test]
    fn between_the_two_text_releases() {
        for (_, sat106, sat400, _) in bodies() {
            assert_eq!(converted(&AcisData::from_sat(sat106), DxfVersion::AC1015).sat_data, sat400);
            assert_eq!(converted(&AcisData::from_sat(sat400), DxfVersion::AC1014).sat_data, sat106);
        }
    }

    #[test]
    fn what_is_in_the_form_or_not_autocads_stays() {
        let sat400 = AcisData::from_sat(s::REGION_400);
        let binary = AcisData::from_sab(bytes(s::REGION_20800));
        // the form of the release; the binary releases among themselves (the writer re-versions those); the text of a DXF written as a binary release
        assert!(convert(&sat400, DxfVersion::AC1015, &context()).unwrap().is_none());
        assert!(convert(&AcisData::from_sat(s::REGION_106), DxfVersion::AC1014, &context()).unwrap().is_none());
        assert!(convert(&binary, DxfVersion::AC1018, &context()).unwrap().is_none());
        assert!(convert(&binary, DxfVersion::AC1032, &context()).unwrap().is_none());
        assert!(convert(&AcisData::from_sat(s::REGION_DXF_20800), DxfVersion::AC1027, &context()).unwrap().is_none());
        // releases without a form
        assert!(convert(&sat400, DxfVersion::AC1009, &context()).unwrap().is_none());
        assert!(convert(&sat400, DxfVersion::AC1012, &context()).unwrap().is_none());
        // another producer's data: its text, its header words, its binary data
        let other = s::REGION_400.replace("Autodesk AutoCAD", "Other Product ACAD");
        assert!(convert(&AcisData::from_sat(&other), DxfVersion::AC1014, &context()).unwrap().is_none());
        assert!(convert(&AcisData::from_sat("700 0 1 0\n"), DxfVersion::AC1015, &context()).unwrap().is_none());
        let mut flags = bytes(s::REGION_20800);
        flags[27] = 9;
        assert!(convert(&AcisData::from_sab(flags), DxfVersion::AC1015, &context()).unwrap().is_none());
        assert!(convert(&AcisData::from_sab(b"ACIS BinaryFile".to_vec()), DxfVersion::AC1015, &context()).unwrap().is_none());
    }

    fn refusal(acis: &AcisData, target: DxfVersion) -> String {
        match convert(acis, target, &context()) {
            Err(reason) => reason,
            Ok(_) => panic!("{target:?} converts what the samples do not show"),
        }
    }

    #[test]
    fn what_the_samples_do_not_show_is_refused() {
        let with = |from: &str, to: &str| AcisData::from_sat(&s::BOX_400.replacen(from, to, 1));
        // a record kind
        let cone = format!("{}cone-surface $-1 0 0 0 0 0 1 1 0 0 forward_v I I I I #\n", s::REGION_400);
        assert!(refusal(&AcisData::from_sat(&cone), DxfVersion::AC1014).contains("cone-surface"));
        assert!(refusal(&AcisData::from_sat(&cone), DxfVersion::AC1018).contains("cone-surface"));
        // a field value: the attribute pointers, a double-sided face inside, a plane with finite bounds
        assert!(refusal(&with("persubent-acadSolidHistory-attrib $-1 $-1 $-1 $4", "persubent-acadSolidHistory-attrib $-1 $3 $-1 $4"), DxfVersion::AC1014).contains("attribute pointer"));
        assert!(refusal(&AcisData::from_sat(&s::REGION_400.replacen("forward double out", "forward double in", 1)), DxfVersion::AC1014).contains("inside"));
        assert!(refusal(&with("forward_v I I I I", "forward_v F 0 I I I"), DxfVersion::AC1014).contains("finite bounds"));
        // a shape: one token more, one less, a number where a pointer belongs, a pointer to a record that is not there
        assert!(refusal(&with("point $-1 2.5 -2.5 2.5 #", "point $-1 2.5 -2.5 2.5 1 #"), DxfVersion::AC1014).contains("more tokens"));
        assert!(refusal(&with("point $-1 2.5 -2.5 2.5 #", "point $-1 2.5 -2.5 #"), DxfVersion::AC1014).contains("ends early"));
        assert!(refusal(&with("lump $-1 $-1 $3 $0 #", "lump 7 $-1 $3 $0 #"), DxfVersion::AC1014).contains("pointer"));
        assert!(refusal(&with("lump $-1 $-1 $3 $0 #", "lump $-1 $-1 $3 $900 #"), DxfVersion::AC1014).contains("not there"));
        // a double that is not a number
        assert!(refusal(&with("point $-1 2.5 -2.5 2.5 #", "point $-1 2.5 nan 2.5 #"), DxfVersion::AC1014).contains("number"));
        // a header that is not AutoCAD's
        assert!(refusal(&AcisData::from_sat(&s::BOX_400.replacen("400 112 1 0", "400 112 3 0", 1)), DxfVersion::AC1014).contains("header"));
        assert!(refusal(&AcisData::from_sat(&s::BOX_400.replacen("400 112 1 0", "400 113 1 0", 1)), DxfVersion::AC1014).contains("number of records"));
        // binary data cut short, or with a token the samples do not show
        let data = bytes(s::REGION_22300);
        assert!(convert(&AcisData::from_sab(data[..data.len() - 3].to_vec()), DxfVersion::AC1014, &context()).is_err());
        let mut odd = data.clone();
        let at = odd.windows(8).position(|window| window == b"\x0d\x06vertex").unwrap();
        odd[at + 8] = 0x0A; // the first pointer of the first vertex becomes a logical
        assert!(convert(&AcisData::from_sab(odd), DxfVersion::AC1015, &context()).is_err());
    }

    #[test]
    fn what_the_older_release_does_not_state_is_derived_only_where_the_samples_show_it() {
        // R14 states no units: the drawing's inches are the samples'; any other units are not known
        let units = |units| Context { insertion_units: units, date: None };
        let sat106 = AcisData::from_sat(s::REGION_106);
        assert!(convert(&sat106, DxfVersion::AC1018, &units(1)).unwrap().is_some());
        for other in [0, 4, 6] {
            assert!(convert(&sat106, DxfVersion::AC1018, &units(other)).unwrap_err().contains("INSUNITS"));
            assert!(convert(&sat106, DxfVersion::AC1015, &units(other)).is_err());
        }
        // R14 down is no problem in any drawing
        assert!(convert(&AcisData::from_sab(bytes(s::REGION_20800)), DxfVersion::AC1014, &units(4)).unwrap().is_some());
        // an edge that is reversed, or on a curve other than a straight one, has no parameters the samples show
        let reversed = s::REGION_400.replacen("edge $", "edge $", 1);
        let first_edge = reversed.find("\nedge ").unwrap() + 1;
        let end = first_edge + reversed[first_edge..].find(" #").unwrap();
        let flipped = format!("{}{}{}", &reversed[..first_edge], reversed[first_edge..end].replace("forward", "reversed"), &reversed[end..]);
        assert!(convert(&AcisData::from_sat(&flipped), DxfVersion::AC1014, &context()).unwrap().is_some());
        assert!(refusal(&AcisData::from_sat(&flipped), DxfVersion::AC1018).contains("reversed edge"));
        // a vertex integer that is not the end of its edge
        let mut model = parse_binary(&bytes(s::REGION_21200)).unwrap();
        let Some(Rec::Vertex { state, .. }) = model.recs.iter_mut().find(|rec| matches!(rec, Rec::Vertex { .. })) else { panic!("no vertex") };
        *state = state.map(|state| 1 - state);
        assert!(model.check(Layout::of(Form::Modern(21200))).unwrap_err().contains("vertex integer"));
        // a coedge integer other than 0
        let mut data = bytes(s::REGION_21800);
        let at = data.windows(8).position(|window| window == b"\x0d\x06coedge").unwrap();
        let state = data[at..].windows(5).position(|window| window == [INTEGER, 0, 0, 0, 0]).unwrap() + at;
        data[state + 1] = 1;
        assert!(convert(&AcisData::from_sab(data), DxfVersion::AC1015, &context()).unwrap_err().contains("coedge integer"));
    }

    fn document(version: DxfVersion, acis: AcisData) -> CadDocument {
        let mut document = CadDocument::new();
        document.version = version;
        document.header.update_date_julian = 2_461_012.9;
        let mut region = Region::new();
        region.acis_data = acis;
        document.add_entity(EntityType::Region(region)).unwrap();
        let mut solid = Solid3D::new();
        solid.acis_data = AcisData::from_sat(s::BOX_400);
        document.add_entity(EntityType::Solid3D(solid)).unwrap();
        document
    }

    fn acis_of(document: &CadDocument, region: bool) -> AcisData {
        document
            .entities()
            .find_map(|entity| match entity {
                EntityType::Region(r) if region => Some(r.acis_data.clone()),
                EntityType::Solid3D(s) if !region => Some(s.acis_data.clone()),
                _ => None,
            })
            .expect("the entity")
    }

    #[test]
    fn the_writer_states_the_data_of_the_release() {
        let source = document(DxfVersion::AC1015, AcisData::from_sab(bytes(s::REGION_20800)));
        let mut cow = std::borrow::Cow::Borrowed(&source);
        prepare_for_write(&mut cow).unwrap();
        let made = acis_of(&cow, true);
        assert!(!made.is_binary && made.sab_data.is_empty() && made.version == AcisVersion::Version1);
        assert_eq!(made.sat_data, s::REGION_400);
        // the box was R2000 text already
        assert_eq!(acis_of(&cow, false).sat_data, s::BOX_400);
        assert_eq!(acis_of(&source, true).sab_data, bytes(s::REGION_20800), "the document that is written is not changed");
        // R14, and the binary releases
        let mut cow: Cow<'_, CadDocument> = Cow::Owned(source.clone());
        cow.to_mut().version = DxfVersion::AC1014;
        prepare_for_write(&mut cow).unwrap();
        assert_eq!(acis_of(&cow, true).sat_data, s::REGION_106);
        assert_eq!(acis_of(&cow, false).sat_data, converted(&AcisData::from_sat(s::BOX_400), DxfVersion::AC1014).sat_data);
        let mut cow: Cow<'_, CadDocument> = Cow::Owned(document(DxfVersion::AC1032, AcisData::from_sat(s::REGION_400)));
        prepare_for_write(&mut cow).unwrap();
        let made = acis_of(&cow, true);
        assert!(made.is_binary && made.sat_data.is_empty() && made.version == AcisVersion::Version2);
        assert!(made.sab_data.starts_with(ASM_MAGIC));
        // a document that needs no conversion is borrowed
        let same = document(DxfVersion::AC1015, AcisData::from_sat(s::REGION_400));
        let mut cow = std::borrow::Cow::Borrowed(&same);
        prepare_for_write(&mut cow).unwrap();
        assert!(matches!(cow, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn a_body_the_samples_do_not_show_refuses_the_write() {
        let cone = format!("{}cone-surface $-1 0 0 0 0 0 1 1 0 0 forward_v I I I I #\n", s::REGION_400);
        for target in [DxfVersion::AC1014, DxfVersion::AC1018] {
            let error = crate::io::dwg::DwgWriter::write_to_vec(&document(target, AcisData::from_sat(&cone))).unwrap_err().to_string();
            assert!(error.contains("REGION") && error.contains("cone-surface"), "{error}");
        }
        // the form of the release is written as it is
        crate::io::dwg::DwgWriter::write_to_vec(&document(DxfVersion::AC1015, AcisData::from_sat(&cone))).expect("R2000 text as R2000");
    }

    #[test]
    fn dwg_files_of_the_release_hold_the_converted_data() {
        use crate::io::dwg::{DwgReader, DwgWriter};
        let read = |version: DxfVersion, acis: AcisData| {
            let bytes = DwgWriter::write_to_vec(&document(version, acis)).expect("write");
            DwgReader::from_stream(std::io::Cursor::new(bytes)).read().expect("read")
        };
        // binary data of R2004 in a DWG of R2000 and of R14: the text of those releases
        let r2000 = read(DxfVersion::AC1015, AcisData::from_sab(bytes(s::REGION_20800)));
        let region = acis_of(&r2000, true);
        assert!(!region.is_binary);
        assert_eq!(region.sat_data, s::REGION_400);
        let r14 = read(DxfVersion::AC1014, AcisData::from_sab(bytes(s::REGION_20800)));
        assert_eq!(acis_of(&r14, true).sat_data, s::REGION_106);
        // and up again, from the files that were read: the data of the R2004 sample, the date aside
        let r2004 = read(DxfVersion::AC1018, region);
        let up = acis_of(&r2004, true);
        assert!(up.is_binary);
        assert_eq!(dated(up.sab_data), bytes(s::REGION_20800));
    }
}
