//! Dimension style table entry

use super::TableEntry;
use crate::types::{Color, Handle};

/// A dimension style table entry (analogous to the reference DimensionStyle)
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DimStyle {
    /// Whether this record came from the source DIMSTYLE table.
    ///
    /// A decoded document starts with native Standard defaults before source
    /// records are merged.  The compatibility reader view must be able to
    /// omit that synthetic entry when the source table did not contain it,
    /// while retaining a real source record named Standard.
    #[cfg_attr(feature = "serde", serde(skip))]
    #[doc(hidden)]
    pub source_present: bool,
    /// Group codes present in the source DXF record.
    ///
    /// The native reader starts with format defaults, while ACadSharp starts
    /// with its object defaults and only changes values that occur in the
    /// record.  Keeping this small source mask lets the compatibility view
    /// reproduce that distinction without changing the native table value.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub dxf_present_codes: Option<[u64; 6]>,
    /// Last DXF unit-format group encountered (270 or 277).
    ///
    /// ACadSharp assigns both groups to `LinearUnitFormat` as they are read,
    /// so their source order matters when a record contains both.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub dxf_last_linear_unit_code: Option<u16>,

    /// Unique handle
    pub handle: Handle,
    /// Style name
    pub name: String,
    #[cfg_attr(feature = "serde", serde(default))]
    pub xref_reference: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub xref_resolved: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub xref_dependent: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub xref_handle: Handle,

    // ─── Dimension line ───
    /// Dimension line color (DIMCLRD, code 176)
    pub dimclrd: i16,
    /// DWG true-color override for DIMCLRD; DXF exposes only the ACI field.
    #[cfg_attr(feature = "serde", serde(default))]
    pub dimclrd_true_color: Option<Color>,
    /// Dimension line extension (DIMDLE, code 46)
    pub dimdle: f64,
    /// Dimension line increment for continuation (DIMDLI, code 43)
    pub dimdli: f64,
    /// Dimension line gap (DIMGAP, code 147)
    pub dimgap: f64,
    /// Dimension line weight (DIMLWD, code 371)
    pub dimlwd: i16,
    /// Suppress first dimension line (DIMSD1, code 281)
    pub dimsd1: bool,
    /// Suppress second dimension line (DIMSD2, code 282)
    pub dimsd2: bool,

    // ─── Extension line ───
    /// Extension line color (DIMCLRE, code 177)
    pub dimclre: i16,
    /// DWG true-color override for DIMCLRE.
    #[cfg_attr(feature = "serde", serde(default))]
    pub dimclre_true_color: Option<Color>,
    /// Extension line extension (DIMEXE, code 44)
    pub dimexe: f64,
    /// Extension line offset (DIMEXO, code 42)
    pub dimexo: f64,
    /// Extension line weight (DIMLWE, code 372)
    pub dimlwe: i16,
    /// Suppress first extension line (DIMSE1, code 75)
    pub dimse1: bool,
    /// Suppress second extension line (DIMSE2, code 76)
    pub dimse2: bool,
    /// Fixed extension line length (DIMFXL, code 49)
    pub dimfxl: f64,
    /// Fixed extension line length on (DIMFXLON, code 290)
    pub dimfxlon: bool,

    // ─── Arrows ───
    /// Arrow size (DIMASZ, code 41)
    pub dimasz: f64,
    /// Arrow block handle (DIMBLK, code 342)
    pub dimblk: Handle,
    /// First arrow block handle (DIMBLK1, code 343)
    pub dimblk1: Handle,
    /// Second arrow block handle (DIMBLK2, code 344)
    pub dimblk2: Handle,
    /// Leader arrow block handle (DIMLDRBLK, code 341)
    pub dimldrblk: Handle,
    /// R13/R14 arrow block names, before these references became handles.
    #[cfg_attr(feature = "serde", serde(default))]
    pub dimblk_name: String,
    #[cfg_attr(feature = "serde", serde(default))]
    pub dimblk1_name: String,
    #[cfg_attr(feature = "serde", serde(default))]
    pub dimblk2_name: String,
    /// Separate arrow blocks (DIMSAH, code 173)
    pub dimsah: bool,
    /// Center mark size (DIMCEN, code 141)
    pub dimcen: f64,
    /// Tick size (DIMTSZ, code 142)
    pub dimtsz: f64,

    // ─── Text ───
    /// Dimension text color (DIMCLRT, code 178)
    pub dimclrt: i16,
    /// DWG true-color override for DIMCLRT.
    #[cfg_attr(feature = "serde", serde(default))]
    pub dimclrt_true_color: Option<Color>,
    /// Text height (DIMTXT, code 140)
    pub dimtxt: f64,
    /// Text horizontal alignment / justification (DIMJUST, code 280)
    pub dimjust: i16,
    /// Text vertical alignment (DIMTAD, code 77)
    pub dimtad: i16,
    /// Text vertical position (DIMTVP, code 145)
    pub dimtvp: f64,
    /// Text inside horizontal (DIMTIH, code 73)
    pub dimtih: bool,
    /// Text outside horizontal (DIMTOH, code 74)
    pub dimtoh: bool,
    /// Text inside extensions (DIMTIX, code 174)
    pub dimtix: bool,
    /// Suppress outside extensions (DIMSOXD, code 175)
    pub dimsoxd: bool,
    /// Text background fill mode (DIMTFILL, code 69)
    pub dimtfill: i16,
    /// Text background fill color (DIMTFILLCLR, code 70)
    pub dimtfillclr: i16,
    /// DWG true-color override for DIMTFILLCLR.
    #[cfg_attr(feature = "serde", serde(default))]
    pub dimtfillclr_true_color: Option<Color>,
    /// Text movement (DIMTMOVE, code 279)
    pub dimtmove: i16,
    /// Text direction (DIMTXTDIRECTION, code 295)
    pub dimtxtdirection: bool,
    /// R2010+ alternate measurement zero-factor and suffix.
    #[cfg_attr(feature = "serde", serde(default))]
    pub dimaltmzf: f64,
    #[cfg_attr(feature = "serde", serde(default))]
    pub dimaltmzs: String,
    /// R2010+ primary measurement zero-factor and suffix.
    #[cfg_attr(feature = "serde", serde(default))]
    pub dimmzf: f64,
    #[cfg_attr(feature = "serde", serde(default))]
    pub dimmzs: String,
    /// Text style handle (DIMTXSTY, code 340)
    pub dimtxsty_handle: Handle,
    /// Text style name
    pub dimtxsty: String,

    // ─── Scale / units ───
    /// Dimension scale factor (DIMSCALE, code 40)
    pub dimscale: f64,
    /// Linear scale factor (DIMLFAC, code 144)
    pub dimlfac: f64,
    /// Linear unit format (DIMLUNIT, code 277)
    pub dimlunit: i16,
    /// Decimal places (DIMDEC, code 271)
    pub dimdec: i16,
    /// Rounding (DIMRND, code 45)
    pub dimrnd: f64,
    /// Decimal separator (DIMDSEP, code 278)
    pub dimdsep: i16,
    /// Zero handling (DIMZIN, code 78)
    pub dimzin: i16,

    // ─── Angular ───
    /// Angular unit format (DIMAUNIT, code 275)
    pub dimaunit: i16,
    /// Angular decimal places (DIMADEC, code 179)
    pub dimadec: i16,
    /// Angular zero handling (DIMAZIN, code 79)
    pub dimazin: i16,

    // ─── Alternate units ───
    /// Alternate unit dimensioning on (DIMALT, code 170)
    pub dimalt: bool,
    /// Alternate unit scale factor (DIMALTF, code 143)
    pub dimaltf: f64,
    /// Alternate unit decimal places (DIMALTD, code 171)
    pub dimaltd: i16,
    /// Alternate unit format (DIMALTU, code 273)
    pub dimaltu: i16,
    /// Alternate unit tolerance decimal places (DIMALTTD, code 274)
    pub dimalttd: i16,
    /// Alternate unit rounding (DIMALTRND, code 148)
    pub dimaltrnd: f64,
    /// Alternate unit suffix (DIMAPOST, code 4)
    pub dimapost: String,
    /// Alternate unit zero handling (DIMALTZ, code 285)
    pub dimaltz: i16,
    /// Alternate unit tolerance zero handling (DIMALTTZ, code 286)
    pub dimalttz: i16,

    // ─── Tolerances ───
    /// Generate tolerances (DIMTOL, code 71)
    pub dimtol: bool,
    /// Limits generation (DIMLIM, code 72)
    pub dimlim: bool,
    /// Plus tolerance (DIMTP, code 47)
    pub dimtp: f64,
    /// Minus tolerance (DIMTM, code 48)
    pub dimtm: f64,
    /// Tolerance decimal places (DIMTDEC, code 272)
    pub dimtdec: i16,
    /// Tolerance scale factor (DIMTFAC, code 146)
    pub dimtfac: f64,
    /// Tolerance alignment (DIMTOLJ, code 283)
    pub dimtolj: i16,
    /// Tolerance zero handling (DIMTZIN, code 284)
    pub dimtzin: i16,

    // ─── Fit ───
    /// Text/arrow fit (DIMATFIT, code 289)
    pub dimatfit: i16,
    /// Text outside flow line (DIMTOFL, code 172)
    pub dimtofl: bool,
    /// Cursor update (DIMUPT, code 288)
    pub dimupt: bool,
    /// Dimension fit (DIMFIT, code 287) — obsolete
    pub dimfit: i16,

    // ─── Formatting ───
    /// Postfix (DIMPOST, code 3)
    pub dimpost: String,
    /// Fraction format (DIMFRAC, code 276)
    pub dimfrac: i16,
    /// Arc length symbol position (DIMARCSYM, code 90)
    pub dimarcsym: i16,
    /// Jogged radius angle (DIMJOGANG, code 50)
    pub dimjogang: f64,

    // ─── Linetype handles ───
    /// Dimension line linetype handle (code 345)
    pub dimltex_handle: Handle,
    /// Extension line 1 linetype handle (code 346)
    pub dimltex1_handle: Handle,
    /// Extension line 2 linetype handle (code 347)
    pub dimltex2_handle: Handle,

    /// Obsolete DIMUNIT (code 270)
    pub dimunit: i16,
    /// Annotative: dimensions using this style scale with the annotation scale.
    /// Persisted as XDATA under the `AcadAnnotative` application:
    /// `AnnotativeData { 1 <flag> }`.
    pub annotative: bool,
}

// Source-presence metadata describes how a record was read and is not part of
// the drawing value.  Keep equality stable for callers comparing a native
// style with a cloned/read style.
impl PartialEq for DimStyle {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
            && self.name == other.name
            && self.xref_reference == other.xref_reference
            && self.xref_resolved == other.xref_resolved
            && self.xref_dependent == other.xref_dependent
            && self.xref_handle == other.xref_handle
            && self.dimclrd == other.dimclrd
            && self.dimclrd_true_color == other.dimclrd_true_color
            && self.dimdle == other.dimdle
            && self.dimdli == other.dimdli
            && self.dimgap == other.dimgap
            && self.dimlwd == other.dimlwd
            && self.dimsd1 == other.dimsd1
            && self.dimsd2 == other.dimsd2
            && self.dimclre == other.dimclre
            && self.dimclre_true_color == other.dimclre_true_color
            && self.dimexe == other.dimexe
            && self.dimexo == other.dimexo
            && self.dimlwe == other.dimlwe
            && self.dimse1 == other.dimse1
            && self.dimse2 == other.dimse2
            && self.dimfxl == other.dimfxl
            && self.dimfxlon == other.dimfxlon
            && self.dimasz == other.dimasz
            && self.dimblk == other.dimblk
            && self.dimblk1 == other.dimblk1
            && self.dimblk2 == other.dimblk2
            && self.dimldrblk == other.dimldrblk
            && self.dimblk_name == other.dimblk_name
            && self.dimblk1_name == other.dimblk1_name
            && self.dimblk2_name == other.dimblk2_name
            && self.dimsah == other.dimsah
            && self.dimcen == other.dimcen
            && self.dimtsz == other.dimtsz
            && self.dimclrt == other.dimclrt
            && self.dimclrt_true_color == other.dimclrt_true_color
            && self.dimtxt == other.dimtxt
            && self.dimjust == other.dimjust
            && self.dimtad == other.dimtad
            && self.dimtvp == other.dimtvp
            && self.dimtih == other.dimtih
            && self.dimtoh == other.dimtoh
            && self.dimtix == other.dimtix
            && self.dimsoxd == other.dimsoxd
            && self.dimtfill == other.dimtfill
            && self.dimtfillclr == other.dimtfillclr
            && self.dimtfillclr_true_color == other.dimtfillclr_true_color
            && self.dimtmove == other.dimtmove
            && self.dimtxtdirection == other.dimtxtdirection
            && self.dimaltmzf == other.dimaltmzf
            && self.dimaltmzs == other.dimaltmzs
            && self.dimmzf == other.dimmzf
            && self.dimmzs == other.dimmzs
            && self.dimtxsty_handle == other.dimtxsty_handle
            && self.dimtxsty == other.dimtxsty
            && self.dimscale == other.dimscale
            && self.dimlfac == other.dimlfac
            && self.dimlunit == other.dimlunit
            && self.dimdec == other.dimdec
            && self.dimrnd == other.dimrnd
            && self.dimdsep == other.dimdsep
            && self.dimzin == other.dimzin
            && self.dimaunit == other.dimaunit
            && self.dimadec == other.dimadec
            && self.dimazin == other.dimazin
            && self.dimalt == other.dimalt
            && self.dimaltf == other.dimaltf
            && self.dimaltd == other.dimaltd
            && self.dimaltu == other.dimaltu
            && self.dimalttd == other.dimalttd
            && self.dimaltrnd == other.dimaltrnd
            && self.dimapost == other.dimapost
            && self.dimaltz == other.dimaltz
            && self.dimalttz == other.dimalttz
            && self.dimtol == other.dimtol
            && self.dimlim == other.dimlim
            && self.dimtp == other.dimtp
            && self.dimtm == other.dimtm
            && self.dimtdec == other.dimtdec
            && self.dimtfac == other.dimtfac
            && self.dimtolj == other.dimtolj
            && self.dimtzin == other.dimtzin
            && self.dimatfit == other.dimatfit
            && self.dimtofl == other.dimtofl
            && self.dimupt == other.dimupt
            && self.dimfit == other.dimfit
            && self.dimpost == other.dimpost
            && self.dimfrac == other.dimfrac
            && self.dimarcsym == other.dimarcsym
            && self.dimjogang == other.dimjogang
            && self.dimltex_handle == other.dimltex_handle
            && self.dimltex1_handle == other.dimltex1_handle
            && self.dimltex2_handle == other.dimltex2_handle
            && self.dimunit == other.dimunit
            && self.annotative == other.annotative
    }
}

impl DimStyle {
    /// Create a new dimension style
    pub fn new(name: impl Into<String>) -> Self {
        DimStyle {
            source_present: false,
            dxf_present_codes: None,
            dxf_last_linear_unit_code: None,
            handle: Handle::NULL,
            name: name.into(),
            xref_reference: false,
            xref_resolved: false,
            xref_dependent: false,
            xref_handle: Handle::NULL,
            // Dimension line
            dimclrd: 0,
            dimclrd_true_color: None,
            dimdle: 0.0,
            dimdli: 3.75,
            dimgap: 0.625,
            dimlwd: -2, // ByBlock
            dimsd1: false,
            dimsd2: false,
            // Extension line
            dimclre: 0,
            dimclre_true_color: None,
            dimexe: 1.25,
            dimexo: 0.625,
            dimlwe: -2, // ByBlock
            dimse1: false,
            dimse2: false,
            dimfxl: 1.0,
            dimfxlon: false,
            // Arrows
            dimasz: 0.18,
            dimblk: Handle::NULL,
            dimblk1: Handle::NULL,
            dimblk2: Handle::NULL,
            dimldrblk: Handle::NULL,
            dimblk_name: String::new(),
            dimblk1_name: String::new(),
            dimblk2_name: String::new(),
            dimsah: true,
            dimcen: 0.09,
            dimtsz: 0.0,
            // Text
            dimclrt: 0,
            dimclrt_true_color: None,
            dimtxt: 0.18,
            dimjust: 0,
            dimtad: 1, // Above
            dimtvp: 0.0,
            dimtih: false,
            dimtoh: false,
            dimtix: false,
            dimsoxd: false,
            dimtfill: 0,
            dimtfillclr: 0,
            dimtfillclr_true_color: None,
            dimtmove: 0,
            dimtxtdirection: false,
            dimaltmzf: 0.0,
            dimaltmzs: String::new(),
            dimmzf: 0.0,
            dimmzs: String::new(),
            dimtxsty_handle: Handle::NULL,
            dimtxsty: "Standard".to_string(),
            // Scale/units
            dimscale: 1.0,
            dimlfac: 1.0,
            dimlunit: 2,
            dimdec: 2,
            dimrnd: 0.0,
            dimdsep: 46, // '.'
            dimzin: 8,
            // Angular
            dimaunit: 0,
            dimadec: 0,
            dimazin: 0,
            // Alternate units
            dimalt: false,
            dimaltf: 25.4,
            dimaltd: 3,
            dimaltu: 2,
            dimalttd: 3,
            dimaltrnd: 0.0,
            dimapost: String::new(),
            dimaltz: 0,
            dimalttz: 0,
            // Tolerances
            dimtol: false,
            dimlim: false,
            dimtp: 0.0,
            dimtm: 0.0,
            dimtdec: 2,
            dimtfac: 1.0,
            dimtolj: 0,
            dimtzin: 8,
            // Fit
            dimatfit: 3,
            dimtofl: false,
            dimupt: false,
            dimfit: 0,
            // Formatting
            dimpost: "<>".to_string(),
            dimfrac: 0,
            dimarcsym: 0,
            dimjogang: std::f64::consts::FRAC_PI_4,
            // Linetype handles
            dimltex_handle: Handle::NULL,
            dimltex1_handle: Handle::NULL,
            dimltex2_handle: Handle::NULL,
            // Obsolete
            dimunit: 2,
            annotative: false,
        }
    }

    /// Create the standard dimension style
    pub fn standard() -> Self {
        Self::new("Standard")
    }

    /// The built-in values used for DIMSTYLE groups that a DXF leaves out.
    ///
    /// Writers may omit groups equal to the format's imperial defaults, so a
    /// reader must start from those, not from the metric values [`Self::new`]
    /// uses for freshly created styles. The differences affect zero
    /// suppression, text orientation, precision, placement and line offsets.
    pub fn dxf_defaults(name: impl Into<String>) -> Self {
        DimStyle {
            dimdli: 0.38,
            dimgap: 0.09,
            dimexe: 0.18,
            dimexo: 0.0625,
            dimsah: false,
            dimtad: 0,
            dimtih: true,
            dimtoh: true,
            dimdec: 4,
            dimtdec: 4,
            dimzin: 0,
            dimtzin: 0,
            dimaltd: 2,
            dimalttd: 2,
            dimtolj: 1,
            ..Self::new(name)
        }
    }

    /// Record a DXF group code while reading the source record.
    pub(crate) fn record_dxf_code(&mut self, code: i32) {
        if let Ok(code) = usize::try_from(code) {
            if let Some(bits) = self
                .dxf_present_codes
                .as_mut()
                .and_then(|codes| codes.get_mut(code / 64))
            {
                *bits |= 1 << (code % 64);
            }
        }
    }

    /// Record the last DXF unit-format group encountered in the source.
    pub(crate) fn record_dxf_linear_unit_code(&mut self, code: i32) {
        if matches!(code, 270 | 277) {
            self.dxf_last_linear_unit_code = Some(code as u16);
        }
    }

    /// Return whether the source DXF record contained `code`.
    pub fn dxf_code_present(&self, code: i32) -> bool {
        usize::try_from(code)
            .ok()
            .and_then(|code| {
                self.dxf_present_codes
                    .as_ref()
                    .and_then(|codes| codes.get(code / 64))
                    .map(|bits| bits & (1 << (code % 64)) != 0)
            })
            .unwrap_or(false)
    }
}

impl TableEntry for DimStyle {
    fn handle(&self) -> Handle {
        self.handle
    }

    fn set_handle(&mut self, handle: Handle) {
        self.handle = handle;
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn set_name(&mut self, name: String) {
        self.name = name;
    }

    fn is_standard(&self) -> bool {
        self.name == "Standard"
    }
}
