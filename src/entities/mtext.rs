//! Multi-line text entity

use super::{Entity, EntityCommon, LineSpacingStyle};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

/// Attachment point for MText
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AttachmentPoint {
    /// Top left
    TopLeft = 1,
    /// Top center
    TopCenter = 2,
    /// Top right
    TopRight = 3,
    /// Middle left
    MiddleLeft = 4,
    /// Middle center
    MiddleCenter = 5,
    /// Middle right
    MiddleRight = 6,
    /// Bottom left
    BottomLeft = 7,
    /// Bottom center
    BottomCenter = 8,
    /// Bottom right
    BottomRight = 9,
}

/// Drawing direction for MText
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DrawingDirection {
    /// Left to right
    LeftToRight = 1,
    /// Top to bottom (DXF 72 = 3 — the TEXT-era code set skips 2 and 4)
    TopToBottom = 3,
    /// By style (DXF 72 = 5)
    ByStyle = 5,
}

/// Column layout for an [`MText`] entity (stored in R2018+ DWG, non-annotative).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MTextColumnData {
    /// Column type: 0 = no columns, 1 = static columns, 2 = dynamic columns.
    pub column_type: i16,
    /// Number of columns. For dynamic, non-auto-height columns this is the
    /// number of per-column [`heights`](Self::heights); the DWG writer derives
    /// the on-disk count from `heights.len()` in that case to keep the object
    /// stream in sync, so keep the two consistent for dynamic columns.
    pub column_count: i32,
    /// Whether the column flow is reversed.
    pub flow_reversed: bool,
    /// Whether the column height is computed automatically.
    pub auto_height: bool,
    /// Column width.
    pub width: f64,
    /// Gutter width between columns.
    pub gutter: f64,
    /// Per-column heights. Only stored for dynamic, non-auto-height columns.
    pub heights: Vec<f64>,
}

impl MTextColumnData {
    /// Create empty (no-columns) column data.
    pub fn new() -> Self {
        MTextColumnData {
            column_type: 0,
            column_count: 0,
            flow_reversed: false,
            auto_height: false,
            width: 0.0,
            gutter: 0.0,
            heights: Vec::new(),
        }
    }
}

impl Default for MTextColumnData {
    fn default() -> Self {
        Self::new()
    }
}

/// DXF source fields whose presence/order is not recoverable from normalized state.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MTextDxfSource {
    pub alignment_point: Vector3,
    pub line_spacing_style: Option<i16>,
    pub has_background_color: bool,
    pub has_background_transparency: bool,
    pub has_embedded_columns: bool,
    /// Native text snapshot, only when source spelling differs. This lets
    /// projections recognize subsequent edits without decoding across chunks.
    pub decoded_value: Option<String>,
}

/// A multi-line text entity
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MText {
    /// Source lexical text, retained when decoding changed it or when the source spelled a line break itself
    /// (a caret escape or, in binary DXF, a literal break). DXF caret escapes have already been interpreted
    /// independently for each chunk.
    #[cfg_attr(feature = "serde", serde(default))]
    pub source_value: Option<String>,
    /// Source DXF presence and ordered alignment assignments.
    #[cfg_attr(feature = "serde", serde(default))]
    pub dxf_source: Option<MTextDxfSource>,
    /// Background fill was recovered from legacy round-trip XData.
    #[cfg_attr(feature = "serde", serde(default))]
    pub background_from_xdata: bool,
    /// Common entity data
    pub common: EntityCommon,
    /// Text content (may contain formatting codes)
    pub value: String,
    /// Insertion point
    pub insertion_point: Vector3,
    /// Text height
    pub height: f64,
    /// Reference rectangle width
    pub rectangle_width: f64,
    /// Reference rectangle height (optional)
    pub rectangle_height: Option<f64>,
    /// Rotation angle in radians
    pub rotation: f64,
    /// Exact DWG X-axis direction used to derive `rotation`. Kept only while
    /// it still describes the public rotation, avoiding needless last-bit
    /// drift from an `atan2` followed by `sin`/`cos`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub dwg_x_direction: Option<Vector3>,
    /// Text style name
    pub style: String,
    /// Attachment point
    pub attachment_point: AttachmentPoint,
    /// Drawing direction
    pub drawing_direction: DrawingDirection,
    /// Line spacing factor
    pub line_spacing_factor: f64,
    /// Line spacing style (DXF 73): AtLeast (1) or Exactly (2).
    pub line_spacing_style: LineSpacingStyle,
    /// The line spacing style word of a DWG record when it is neither 1 nor 2 (`line_spacing_style` holds the
    /// default for it). Provenance: never part of equality.
    #[doc(hidden)]
    #[cfg_attr(feature = "serde", serde(default, skip))]
    pub source_line_spacing_style: crate::objects::SourceWord,
    /// Normal vector
    pub normal: Vector3,
    /// Background fill flags (BL 90): bit 0x01 = use background fill color,
    /// 0x02 = use drawing window color, 0x10 = text frame (R2018+).
    pub background_fill_flags: i32,
    /// Background fill scale factor (BD 45). Default 1.5.
    pub background_scale: f64,
    /// Background fill color (CMC 63).
    pub background_color: Color,
    /// Background fill transparency (BL 441).
    pub background_transparency: i32,
    /// Whether this MTEXT is annotative (R2018+). When `false`, the DWG stores
    /// a block of redundant fields followed by column data.
    pub is_annotative: bool,
    /// Column layout data (R2018+).
    pub column_data: MTextColumnData,
    /// Horizontal extent of the laid-out text (DXF 42, output-only; DWG
    /// "extents width"). 0 when never laid out.
    #[cfg_attr(feature = "serde", serde(default))]
    pub extents_width: f64,
    /// Vertical extent of the laid-out text (DXF 43, output-only; DWG
    /// "extents height"). 0 when never laid out.
    #[cfg_attr(feature = "serde", serde(default))]
    pub extents_height: f64,
}

/// Compare native drawing state; source spelling/presence is provenance only.
impl PartialEq for MText {
    fn eq(&self, other: &Self) -> bool {
        self.common == other.common
            && self.value == other.value
            && self.insertion_point == other.insertion_point
            && self.height == other.height
            && self.rectangle_width == other.rectangle_width
            && self.rectangle_height == other.rectangle_height
            && self.rotation == other.rotation
            && self.dwg_x_direction == other.dwg_x_direction
            && self.style == other.style
            && self.attachment_point == other.attachment_point
            && self.drawing_direction == other.drawing_direction
            && self.line_spacing_factor == other.line_spacing_factor
            && self.line_spacing_style == other.line_spacing_style
            && self.normal == other.normal
            && self.background_fill_flags == other.background_fill_flags
            && self.background_scale == other.background_scale
            && self.background_color == other.background_color
            && self.background_transparency == other.background_transparency
            && self.is_annotative == other.is_annotative
            && self.column_data == other.column_data
            && self.extents_width == other.extents_width
            && self.extents_height == other.extents_height
    }
}

impl MText {
    /// The text as the source file spelled it (`\U+00B0` escapes rather than the decoded character), while the
    /// public value is still the decoded form of that spelling; `None` once the value has been edited or when
    /// the source spelling was not altered by decoding.
    pub(crate) fn unchanged_source_value(&self) -> Option<&str> {
        let raw = self.source_value.as_deref()?;
        let unchanged = if let Some(dxf) = &self.dxf_source {
            dxf.decoded_value.as_ref() == Some(&self.value)
        } else {
            crate::io::dxf::code_page::decode_legacy_escapes(raw) == self.value
        };
        unchanged.then_some(raw)
    }

    /// Create a new MText entity
    pub fn new() -> Self {
        MText {
            source_value: None,
            dxf_source: None,
            background_from_xdata: false,
            common: EntityCommon::new(),
            value: String::new(),
            insertion_point: Vector3::ZERO,
            height: 1.0,
            rectangle_width: 10.0,
            rectangle_height: None,
            rotation: 0.0,
            dwg_x_direction: None,
            style: "Standard".to_string(),
            attachment_point: AttachmentPoint::TopLeft,
            drawing_direction: DrawingDirection::LeftToRight,
            line_spacing_factor: 1.0,
            line_spacing_style: LineSpacingStyle::AtLeast,
            source_line_spacing_style: crate::objects::SourceWord::default(),
            normal: Vector3::UNIT_Z,
            background_fill_flags: 0,
            background_scale: 1.5,
            background_color: Color::ByLayer,
            background_transparency: 0,
            // Non-annotative by default. Real annotativeness is carried by the
            // annotation context / text style (and, from R2018 DWG on, an inline
            // entity bit the reader sets explicitly) — not by every fresh MTEXT.
            // DXF has no entity-level annotative flag at all, so without this
            // default it would mark every imported MTEXT annotative.
            is_annotative: false,
            column_data: MTextColumnData::new(),
            extents_width: 0.0,
            extents_height: 0.0,
        }
    }

    /// Create a new MText with value and position
    pub fn with_value(value: impl Into<String>, position: Vector3) -> Self {
        MText {
            value: value.into(),
            insertion_point: position,
            ..Self::new()
        }
    }

    /// Set the text height
    pub fn with_height(mut self, height: f64) -> Self {
        self.height = height;
        self
    }

    /// Set the rectangle width
    pub fn with_width(mut self, width: f64) -> Self {
        self.rectangle_width = width;
        self
    }
}

impl Default for MText {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for MText {
    fn handle(&self) -> Handle {
        self.common.handle
    }

    fn set_handle(&mut self, handle: Handle) {
        self.common.handle = handle;
    }

    fn layer(&self) -> &str {
        &self.common.layer
    }

    fn set_layer(&mut self, layer: String) {
        self.common.layer = layer;
    }

    fn color(&self) -> Color {
        self.common.color
    }

    fn set_color(&mut self, color: Color) {
        self.common.color = color;
    }

    fn line_weight(&self) -> LineWeight {
        self.common.line_weight
    }

    fn set_line_weight(&mut self, weight: LineWeight) {
        self.common.line_weight = weight;
    }

    fn transparency(&self) -> Transparency {
        self.common.transparency
    }

    fn set_transparency(&mut self, transparency: Transparency) {
        self.common.transparency = transparency;
    }

    fn is_invisible(&self) -> bool {
        self.common.invisible
    }

    fn set_invisible(&mut self, invisible: bool) {
        self.common.invisible = invisible;
    }

    fn bounding_box(&self) -> BoundingBox3D {
        let height = self.rectangle_height.unwrap_or(self.height * 2.0);
        BoundingBox3D::new(
            self.insertion_point,
            Vector3::new(
                self.insertion_point.x + self.rectangle_width,
                self.insertion_point.y + height,
                self.insertion_point.z,
            ),
        )
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_mtext(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "MTEXT"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_mtext(self, transform);
    }

    fn apply_mirror(&mut self, transform: &crate::types::Transform) {
        super::mirror::mirror_mtext(self, transform);
    }
}
