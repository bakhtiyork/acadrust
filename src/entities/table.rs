//! Table entity implementation.
//!
//! The Table entity represents a grid of cells that can contain text,
//! blocks, or formulas, with extensive styling and formatting options.

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

use bitflags::bitflags;

// ============================================================================
// Enums
// ============================================================================

/// Cell content type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum CellType {
    /// Text content.
    #[default]
    Text = 1,
    /// Block reference content.
    Block = 2,
}

impl From<u8> for CellType {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Text,
            2 => Self::Block,
            _ => Self::Text,
        }
    }
}

/// Cell value data type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u32)]
pub enum CellValueType {
    /// Unknown type.
    #[default]
    Unknown = 0,
    /// Long integer.
    Long = 1,
    /// Double precision float.
    Double = 2,
    /// Text string.
    String = 4,
    /// Date value.
    Date = 8,
    /// 2D point.
    Point2D = 0x10,
    /// 3D point.
    Point3D = 0x20,
    /// Object handle reference.
    Handle = 0x40,
    /// Binary buffer.
    Buffer = 0x80,
    /// Result buffer.
    ResultBuffer = 0x100,
    /// General value.
    General = 0x200,
}

impl From<u32> for CellValueType {
    fn from(value: u32) -> Self {
        match value {
            1 => Self::Long,
            2 => Self::Double,
            4 => Self::String,
            8 => Self::Date,
            0x10 => Self::Point2D,
            0x20 => Self::Point3D,
            0x40 => Self::Handle,
            0x80 => Self::Buffer,
            0x100 => Self::ResultBuffer,
            0x200 => Self::General,
            _ => Self::Unknown,
        }
    }
}

/// Value unit type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u32)]
pub enum ValueUnitType {
    /// No units.
    #[default]
    NoUnits = 0,
    /// Distance units.
    Distance = 1,
    /// Angle units.
    Angle = 2,
    /// Area units.
    Area = 4,
    /// Volume units.
    Volume = 8,
    /// Currency units.
    Currency = 0x10,
    /// Percentage.
    Percentage = 0x20,
}

impl From<u32> for ValueUnitType {
    fn from(value: u32) -> Self {
        match value {
            1 => Self::Distance,
            2 => Self::Angle,
            4 => Self::Area,
            8 => Self::Volume,
            0x10 => Self::Currency,
            0x20 => Self::Percentage,
            _ => Self::NoUnits,
        }
    }
}

/// Border type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum BorderType {
    /// Single line border.
    #[default]
    Single = 1,
    /// Double line border.
    Double = 2,
}

impl From<i16> for BorderType {
    fn from(value: i16) -> Self {
        match value {
            1 => Self::Single,
            2 => Self::Double,
            _ => Self::Single,
        }
    }
}

/// Cell content type for table content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum TableCellContentType {
    /// Unknown content.
    #[default]
    Unknown = 0,
    /// Value content.
    Value = 1,
    /// Field content.
    Field = 2,
    /// Block content.
    Block = 4,
}

impl From<u8> for TableCellContentType {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Value,
            2 => Self::Field,
            4 => Self::Block,
            _ => Self::Unknown,
        }
    }
}

/// Cell style type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum CellStyleType {
    /// Cell style.
    #[default]
    Cell = 1,
    /// Row style.
    Row = 2,
    /// Column style.
    Column = 3,
    /// Formatted table data style.
    FormattedTableData = 4,
    /// Table style.
    Table = 5,
}

impl From<u8> for CellStyleType {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Cell,
            2 => Self::Row,
            3 => Self::Column,
            4 => Self::FormattedTableData,
            5 => Self::Table,
            _ => Self::Cell,
        }
    }
}

/// Break flow direction for table breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum BreakFlowDirection {
    /// Break to the right.
    #[default]
    Right = 1,
    /// Break vertically.
    Vertical = 2,
    /// Break to the left.
    Left = 4,
}

impl From<u8> for BreakFlowDirection {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Right,
            2 => Self::Vertical,
            4 => Self::Left,
            _ => Self::Right,
        }
    }
}

// ============================================================================
// Bitflags
// ============================================================================

bitflags! {
    /// Cell edge flags indicating which borders to affect.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct CellEdgeFlags: u32 {
        /// No edges.
        const NONE = 0;
        /// Top edge.
        const TOP = 1;
        /// Right edge.
        const RIGHT = 2;
        /// Bottom edge.
        const BOTTOM = 4;
        /// Left edge.
        const LEFT = 8;
        /// Inside vertical edges.
        const INSIDE_VERTICAL = 16;
        /// Inside horizontal edges.
        const INSIDE_HORIZONTAL = 32;
        /// All edges.
        const ALL = 63;
    }
}

bitflags! {
    /// Cell state flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct CellStateFlags: u32 {
        /// No state.
        const NONE = 0;
        /// Content is locked.
        const CONTENT_LOCKED = 1;
        /// Content is read-only.
        const CONTENT_READ_ONLY = 2;
        /// Cell is linked.
        const LINKED = 4;
        /// Content modified after update.
        const CONTENT_MODIFIED_AFTER_UPDATE = 8;
        /// Format is locked.
        const FORMAT_LOCKED = 16;
        /// Format is read-only.
        const FORMAT_READ_ONLY = 32;
        /// Format modified after update.
        const FORMAT_MODIFIED_AFTER_UPDATE = 64;
    }
}

bitflags! {
    /// Cell style property override flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct CellStylePropertyFlags: u32 {
        /// No properties.
        const NONE = 0;
        /// Data type.
        const DATA_TYPE = 1;
        /// Data format.
        const DATA_FORMAT = 2;
        /// Rotation.
        const ROTATION = 4;
        /// Block scale.
        const BLOCK_SCALE = 8;
        /// Alignment.
        const ALIGNMENT = 16;
        /// Content color.
        const CONTENT_COLOR = 32;
        /// Text style.
        const TEXT_STYLE = 64;
        /// Text height.
        const TEXT_HEIGHT = 128;
        /// Auto scale.
        const AUTO_SCALE = 256;
        /// Background color.
        const BACKGROUND_COLOR = 512;
        /// Left margin.
        const MARGIN_LEFT = 1024;
        /// Top margin.
        const MARGIN_TOP = 2048;
        /// Right margin.
        const MARGIN_RIGHT = 4096;
        /// Bottom margin.
        const MARGIN_BOTTOM = 8192;
        /// Content layout.
        const CONTENT_LAYOUT = 16384;
        /// Merge all.
        const MERGE_ALL = 32768;
        /// Flow direction bottom to top.
        const FLOW_DIRECTION_BOTTOM_TO_TOP = 65536;
        /// Horizontal spacing.
        const MARGIN_HORIZONTAL_SPACING = 131072;
        /// Vertical spacing.
        const MARGIN_VERTICAL_SPACING = 262144;
    }
}

bitflags! {
    /// Border property override flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct BorderPropertyFlags: u32 {
        /// No properties.
        const NONE = 0;
        /// Border type.
        const BORDER_TYPE = 1;
        /// Line weight.
        const LINE_WEIGHT = 2;
        /// Line type.
        const LINE_TYPE = 4;
        /// Color.
        const COLOR = 8;
        /// Invisibility.
        const INVISIBILITY = 16;
        /// Double line spacing.
        const DOUBLE_LINE_SPACING = 32;
        /// All properties.
        const ALL = 63;
    }
}

bitflags! {
    /// Content layout flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct ContentLayoutFlags: u32 {
        /// No layout.
        const NONE = 0;
        /// Flow layout.
        const FLOW = 1;
        /// Stacked horizontal.
        const STACKED_HORIZONTAL = 2;
        /// Stacked vertical.
        const STACKED_VERTICAL = 4;
    }
}

bitflags! {
    /// Break option flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct BreakOptionFlags: u32 {
        /// No options.
        const NONE = 0;
        /// Enable breaks.
        const ENABLE_BREAKS = 1;
        /// Repeat top labels.
        const REPEAT_TOP_LABELS = 2;
        /// Repeat bottom labels.
        const REPEAT_BOTTOM_LABELS = 4;
        /// Allow manual positions.
        const ALLOW_MANUAL_POSITIONS = 8;
        /// Allow manual heights.
        const ALLOW_MANUAL_HEIGHTS = 16;
    }
}

// ============================================================================
// Cell Border
// ============================================================================

/// Border definition for a cell edge.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CellBorder {
    /// Border type.
    pub border_type: BorderType,
    /// Border color.
    pub color: Color,
    /// Border line weight.
    pub line_weight: LineWeight,
    /// Line type object used for the border.
    pub line_type_handle: Option<Handle>,
    /// Whether the border is invisible.
    pub invisible: bool,
    /// Double line spacing.
    pub double_spacing: f64,
    /// Override flags.
    pub override_flags: BorderPropertyFlags,
}

impl CellBorder {
    /// Creates a default cell border.
    pub fn new() -> Self {
        Self {
            border_type: BorderType::Single,
            color: Color::ByBlock,
            line_weight: LineWeight::ByLayer,
            line_type_handle: None,
            invisible: false,
            double_spacing: 0.0,
            override_flags: BorderPropertyFlags::NONE,
        }
    }

    /// Creates a visible border with the given color.
    pub fn with_color(color: Color) -> Self {
        let mut border = Self::new();
        border.color = color;
        border.override_flags |= BorderPropertyFlags::COLOR;
        border
    }

    /// Creates an invisible border.
    pub fn invisible() -> Self {
        let mut border = Self::new();
        border.invisible = true;
        border
    }
}

impl Default for CellBorder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Cell Value
// ============================================================================

/// Value stored in a table cell.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CellValue {
    /// Value type.
    pub value_type: CellValueType,
    /// Exact DWG/DXF type code. Preserves vendor-defined values that map to
    /// [`CellValueType::Unknown`].
    #[cfg_attr(feature = "serde", serde(default))]
    pub raw_type_code: i32,
    /// Unit type.
    pub unit_type: ValueUnitType,
    /// Exact DWG/DXF unit code, including code 12 whose value-string is absent.
    #[cfg_attr(feature = "serde", serde(default))]
    pub raw_unit_type_code: i32,
    /// Value flags.
    pub flags: i32,
    /// Stored size field used by Date, Point and General values.
    #[cfg_attr(feature = "serde", serde(default))]
    pub data_size: i32,
    /// Text string value.
    pub text: String,
    /// Format string.
    pub format: String,
    /// Formatted display value.
    pub formatted_value: String,
    /// Numeric value (for Long/Double types).
    pub numeric_value: f64,
    /// Handle value (for Handle type).
    pub handle_value: Option<Handle>,
    /// Point value for Point2D/Point3D cell values.
    pub point_value: Vector3,
    /// Date/buffer/result-buffer payload.
    pub binary_value: Vec<u8>,
    /// DXF group 91 exactly as read, kept even when the value flags suppress the body.
    #[cfg_attr(feature = "serde", serde(default))]
    pub dxf_long: Option<i32>,
}

impl CellValue {
    /// Creates an empty cell value.
    pub fn new() -> Self {
        Self {
            value_type: CellValueType::Unknown,
            raw_type_code: 0,
            unit_type: ValueUnitType::NoUnits,
            raw_unit_type_code: 0,
            flags: 0,
            data_size: 0,
            text: String::new(),
            format: String::new(),
            formatted_value: String::new(),
            numeric_value: 0.0,
            handle_value: None,
            point_value: Vector3::ZERO,
            binary_value: Vec::new(),
            dxf_long: None,
        }
    }

    /// Creates a text value.
    pub fn text(s: &str) -> Self {
        Self {
            value_type: CellValueType::String,
            raw_type_code: CellValueType::String as i32,
            unit_type: ValueUnitType::NoUnits,
            raw_unit_type_code: 0,
            flags: 0,
            data_size: 0,
            text: s.to_string(),
            format: String::new(),
            formatted_value: s.to_string(),
            numeric_value: 0.0,
            handle_value: None,
            point_value: Vector3::ZERO,
            binary_value: Vec::new(),
            dxf_long: None,
        }
    }

    /// Creates a numeric value.
    pub fn number(n: f64) -> Self {
        Self {
            value_type: CellValueType::Double,
            raw_type_code: CellValueType::Double as i32,
            unit_type: ValueUnitType::NoUnits,
            raw_unit_type_code: 0,
            flags: 0,
            data_size: 0,
            text: String::new(),
            format: String::new(),
            formatted_value: n.to_string(),
            numeric_value: n,
            handle_value: None,
            point_value: Vector3::ZERO,
            binary_value: Vec::new(),
            dxf_long: None,
        }
    }

    /// Creates an integer value.
    pub fn integer(n: i64) -> Self {
        Self {
            value_type: CellValueType::Long,
            raw_type_code: CellValueType::Long as i32,
            unit_type: ValueUnitType::NoUnits,
            raw_unit_type_code: 0,
            flags: 0,
            data_size: 0,
            text: String::new(),
            format: String::new(),
            formatted_value: n.to_string(),
            numeric_value: n as f64,
            handle_value: None,
            point_value: Vector3::ZERO,
            binary_value: Vec::new(),
            dxf_long: None,
        }
    }

    /// Returns true if the value is empty.
    pub fn is_empty(&self) -> bool {
        self.value_type == CellValueType::Unknown && self.text.is_empty()
    }

    /// Returns the display string.
    pub fn display(&self) -> &str {
        if !self.formatted_value.is_empty() {
            &self.formatted_value
        } else if !self.text.is_empty() {
            &self.text
        } else {
            ""
        }
    }

    /// Type code to serialize after accounting for API-side type changes.
    pub fn type_code(&self) -> i32 {
        if CellValueType::from(self.raw_type_code as u32) == self.value_type {
            self.raw_type_code
        } else {
            self.value_type as i32
        }
    }

    /// Unit code to serialize after accounting for API-side unit changes.
    pub fn unit_type_code(&self) -> i32 {
        if ValueUnitType::from(self.raw_unit_type_code as u32) == self.unit_type {
            self.raw_unit_type_code
        } else {
            self.unit_type as i32
        }
    }
}

impl Default for CellValue {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Cell Content
// ============================================================================

/// Content within a table cell.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CellContent {
    /// Content type.
    pub content_type: TableCellContentType,
    /// Cell value.
    pub value: CellValue,
    /// Block content handle (for block type).
    pub block_handle: Option<Handle>,
    /// Text style handle.
    pub text_style_handle: Option<Handle>,
    /// Text style name used by legacy DXF table cells.
    #[cfg_attr(feature = "serde", serde(default))]
    pub text_style_name: String,
    /// Content color.
    pub color: Color,
    /// Rotation angle in radians.
    pub rotation: f64,
    /// Scale factor (for blocks).
    pub scale: f64,
    /// Text height.
    pub text_height: f64,
    /// Referenced FIELD object for field content.
    pub field_handle: Option<Handle>,
    /// Block attribute values attached to block content.
    pub attributes: Vec<TableAttribute>,
    /// Whether the source record carried a content-format payload.  This is
    /// distinct from the payload's property flags, which may all be zero.
    #[cfg_attr(feature = "serde", serde(default))]
    pub format_has_data: bool,
    /// Content-format override flags.
    pub format_override_flags: i32,
    pub format_property_flags: i32,
    pub format_value_data_type: i32,
    pub format_value_unit_type: i32,
    pub value_format: String,
    pub alignment: i32,
    /// Cached content geometry.
    pub geometry: Option<CellContentGeometry>,
}

impl CellContent {
    /// Whether the item states nothing: no value of any kind, no block, no field, no attribute.
    pub fn is_empty_item(&self) -> bool {
        self.block_handle.is_none()
            && self.field_handle.is_none()
            && self.attributes.is_empty()
            && self.value.value_type == CellValueType::Unknown
            && self.value.text.is_empty()
            && self.value.formatted_value.is_empty()
            && self.value.handle_value.is_none()
            && self.value.binary_value.is_empty()
            && self.value.numeric_value == 0.0
    }

    /// Whether the item is a text without characters, field, block or attribute (what a legacy cell states for an empty cell).
    pub fn is_empty_text(&self) -> bool {
        self.block_handle.is_none()
            && self.field_handle.is_none()
            && self.attributes.is_empty()
            && self.value.value_type == CellValueType::String
            && self.value.text.is_empty()
            && self.value.handle_value.is_none()
            && self.value.binary_value.is_empty()
    }

    pub fn sets(&self, property: CellStylePropertyFlags) -> bool {
        let bits = (self.format_property_flags | self.format_override_flags) as u32;
        bits & property.bits() == property.bits()
    }

    /// Creates empty cell content.
    pub fn new() -> Self {
        Self {
            content_type: TableCellContentType::Unknown,
            value: CellValue::new(),
            block_handle: None,
            text_style_handle: None,
            text_style_name: String::new(),
            color: Color::ByBlock,
            rotation: 0.0,
            scale: 1.0,
            text_height: 0.18,
            field_handle: None,
            attributes: Vec::new(),
            format_has_data: false,
            format_override_flags: 0,
            format_property_flags: 0,
            format_value_data_type: 0,
            format_value_unit_type: 0,
            value_format: String::new(),
            alignment: 0,
            geometry: None,
        }
    }

    /// Creates text content.
    pub fn text(s: &str) -> Self {
        Self {
            content_type: TableCellContentType::Value,
            value: CellValue::text(s),
            block_handle: None,
            text_style_handle: None,
            text_style_name: String::new(),
            color: Color::ByBlock,
            rotation: 0.0,
            scale: 1.0,
            text_height: 0.18,
            field_handle: None,
            attributes: Vec::new(),
            format_has_data: false,
            format_override_flags: 0,
            format_property_flags: 0,
            format_value_data_type: 0,
            format_value_unit_type: 0,
            value_format: String::new(),
            alignment: 0,
            geometry: None,
        }
    }

    /// Creates block content.
    pub fn block(block_handle: Handle) -> Self {
        Self {
            content_type: TableCellContentType::Block,
            value: CellValue::new(),
            block_handle: Some(block_handle),
            text_style_handle: None,
            text_style_name: String::new(),
            color: Color::ByBlock,
            rotation: 0.0,
            scale: 1.0,
            text_height: 0.18,
            field_handle: None,
            attributes: Vec::new(),
            format_has_data: false,
            format_override_flags: 0,
            format_property_flags: 0,
            format_value_data_type: 0,
            format_value_unit_type: 0,
            value_format: String::new(),
            alignment: 0,
            geometry: None,
        }
    }
}

impl Default for CellContent {
    fn default() -> Self {
        Self::new()
    }
}

/// Attribute value attached to block content in a table cell.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TableAttribute {
    pub definition_handle: Handle,
    pub value: String,
    pub index: i32,
}

/// Cached table-cell content geometry.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CellContentGeometry {
    pub distance_to_top_left: Vector3,
    pub distance_to_center: Vector3,
    pub width: f64,
    pub height: f64,
    pub outer_width: f64,
    pub outer_height: f64,
    pub flags: i32,
}

/// Named custom data value used by tables, rows, columns and cells.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TableCustomData {
    pub name: String,
    pub value: CellValue,
}

// ============================================================================
// Cell Style
// ============================================================================

/// Style applied to a table cell.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CellStyle {
    /// Style type.
    pub style_type: CellStyleType,
    /// Property override flags.
    pub property_flags: CellStylePropertyFlags,
    /// Complete style override flag word.
    pub override_flags: i32,
    #[cfg_attr(feature = "serde", serde(default))]
    pub legacy_override_bits: bool,
    /// Nested content-format metadata.
    pub content_format_override_flags: i32,
    pub content_property_flags: i32,
    pub value_data_type: i32,
    pub value_unit_type: i32,
    pub value_format: String,
    /// Background fill color.
    pub background_color: Color,
    /// Content color.
    pub content_color: Color,
    /// Text style handle.
    pub text_style_handle: Option<Handle>,
    /// Text style name used by legacy DXF table cells.
    #[cfg_attr(feature = "serde", serde(default))]
    pub text_style_name: String,
    /// Text height.
    pub text_height: f64,
    /// Rotation angle.
    pub rotation: f64,
    /// Scale factor.
    pub scale: f64,
    /// Alignment value.
    pub alignment: i32,
    /// Background fill enabled.
    pub fill_enabled: bool,
    /// Content layout flags.
    pub layout_flags: ContentLayoutFlags,
    /// Exact margin-override flag word stored by DWG.
    #[cfg_attr(feature = "serde", serde(default))]
    pub margin_override_flags: i16,
    /// Margins.
    pub margin_left: f64,
    pub margin_top: f64,
    pub margin_right: f64,
    pub margin_bottom: f64,
    /// Spacing.
    pub horizontal_spacing: f64,
    pub vertical_spacing: f64,
    /// Borders.
    pub top_border: CellBorder,
    pub right_border: CellBorder,
    pub bottom_border: CellBorder,
    pub left_border: CellBorder,
    /// Border edge records actually present in the DWG cell style.
    #[cfg_attr(feature = "serde", serde(default))]
    pub applied_border_edges: CellEdgeFlags,
    /// Borders not covered by the four outer edges (inside horizontal/vertical
    /// and vendor-defined edge masks).
    pub additional_borders: Vec<(u32, CellBorder)>,
}

impl CellStyle {
    pub fn overridden(&self) -> CellStylePropertyFlags {
        if self.legacy_override_bits {
            self.property_flags
        } else {
            self.property_flags | CellStylePropertyFlags::from_bits_truncate(self.override_flags as u32)
        }
    }

    pub fn sets(&self, property: CellStylePropertyFlags) -> bool {
        self.overridden().contains(property)
    }

    /// Creates a default cell style.
    pub fn new() -> Self {
        Self {
            style_type: CellStyleType::Cell,
            property_flags: CellStylePropertyFlags::NONE,
            override_flags: 0,
            legacy_override_bits: false,
            content_format_override_flags: 0,
            content_property_flags: 0,
            value_data_type: 0,
            value_unit_type: 0,
            value_format: String::new(),
            background_color: Color::ByBlock,
            content_color: Color::ByBlock,
            text_style_handle: None,
            text_style_name: String::new(),
            text_height: 0.18,
            rotation: 0.0,
            scale: 1.0,
            alignment: 0,
            fill_enabled: false,
            layout_flags: ContentLayoutFlags::FLOW,
            margin_override_flags: 0,
            margin_left: 0.06,
            margin_top: 0.06,
            margin_right: 0.06,
            margin_bottom: 0.06,
            horizontal_spacing: 0.0,
            vertical_spacing: 0.0,
            top_border: CellBorder::new(),
            right_border: CellBorder::new(),
            bottom_border: CellBorder::new(),
            left_border: CellBorder::new(),
            applied_border_edges: CellEdgeFlags::NONE,
            additional_borders: Vec::new(),
        }
    }

    /// Sets uniform margins.
    pub fn set_margins(&mut self, margin: f64) {
        self.margin_left = margin;
        self.margin_top = margin;
        self.margin_right = margin;
        self.margin_bottom = margin;
    }

    /// Sets all borders to the same style.
    pub fn set_border_color(&mut self, color: Color) {
        self.top_border.color = color;
        self.right_border.color = color;
        self.bottom_border.color = color;
        self.left_border.color = color;
    }
}

impl Default for CellStyle {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Table Cell
// ============================================================================

#[cfg(feature = "serde")]
fn default_cell_geometry_scale() -> f64 {
    1.0
}

/// A cell in a table.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TableCell {
    /// Cell type (text or block).
    pub cell_type: CellType,
    /// Cell state flags.
    pub state: CellStateFlags,
    /// Cell contents (can have multiple).
    pub contents: Vec<CellContent>,
    /// Cell style override.
    pub style: Option<CellStyle>,
    /// Source cell-style type, retained when the source had no style data.
    #[cfg_attr(feature = "serde", serde(default))]
    pub source_style_type: Option<i32>,
    /// Cell tooltip.
    pub tooltip: String,
    /// Rotation angle.
    pub rotation: f64,
    /// R2010+ cached cell-geometry scale.
    #[cfg_attr(feature = "serde", serde(default = "default_cell_geometry_scale"))]
    pub geometry_scale: f64,
    /// Auto-fit content.
    pub auto_fit: bool,
    /// Merged cell width (number of columns).
    pub merge_width: i32,
    /// Merged cell height (number of rows).
    pub merge_height: i32,
    /// Flag value.
    pub flag: i32,
    /// Merged value (encoding of merge info).
    pub merged: i32,
    /// Virtual edge flag.
    pub virtual_edge: i16,
    /// Has linked data.
    pub has_linked_data: bool,
    /// Custom data value.
    pub custom_data: i32,
    /// Pre-R2010 edge flag byte.
    pub edge_flags: u8,
    /// Pre-R2010 text/block value handle.
    pub value_handle: Option<Handle>,
    /// Pre-R2010 block scale.
    pub block_scale: f64,
    /// Named custom data entries.
    pub custom_data_items: Vec<TableCustomData>,
    /// Linked data object and its stored range dimensions.
    pub data_link_handle: Option<Handle>,
    pub data_link_rows: i32,
    pub data_link_columns: i32,
    pub data_link_unknown: i32,
    /// Cell style identifier.
    pub style_id: i32,
    /// Cached geometry metadata.
    #[cfg_attr(feature = "serde", serde(default))]
    pub geometry_data_flag: i32,
    #[cfg_attr(feature = "serde", serde(default))]
    pub geometry_width_with_gap: f64,
    #[cfg_attr(feature = "serde", serde(default))]
    pub geometry_height_with_gap: f64,
    pub geometry_flags: i32,
    pub geometry_handle: Option<Handle>,
    pub geometry: Option<CellContentGeometry>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub geometries: Vec<CellContentGeometry>,
}

impl TableCell {
    /// Creates an empty cell.
    pub fn new() -> Self {
        Self {
            cell_type: CellType::Text,
            state: CellStateFlags::NONE,
            contents: Vec::new(),
            style: None,
            source_style_type: None,
            tooltip: String::new(),
            rotation: 0.0,
            geometry_scale: 1.0,
            auto_fit: false,
            merge_width: 1,
            merge_height: 1,
            flag: 0,
            merged: 0,
            virtual_edge: 0,
            has_linked_data: false,
            custom_data: 0,
            edge_flags: 0,
            value_handle: None,
            block_scale: 1.0,
            custom_data_items: Vec::new(),
            data_link_handle: None,
            data_link_rows: 0,
            data_link_columns: 0,
            data_link_unknown: 0,
            style_id: 0,
            geometry_data_flag: 0,
            geometry_width_with_gap: 0.0,
            geometry_height_with_gap: 0.0,
            geometry_flags: 0,
            geometry_handle: None,
            geometry: None,
            geometries: Vec::new(),
        }
    }

    /// Creates a cell with text content.
    pub fn text(s: &str) -> Self {
        let mut cell = Self::new();
        cell.contents.push(CellContent::text(s));
        cell
    }

    /// Creates a cell with block content.
    pub fn block(block_handle: Handle) -> Self {
        let mut cell = Self::new();
        cell.cell_type = CellType::Block;
        cell.contents.push(CellContent::block(block_handle));
        cell
    }

    /// Gets the first content (convenience method).
    pub fn content(&self) -> Option<&CellContent> {
        self.contents.first()
    }

    /// Gets the text value of the first content.
    pub fn text_value(&self) -> &str {
        self.contents
            .first()
            .map(|c| c.value.display())
            .unwrap_or("")
    }

    pub fn binary_layout(&self) -> std::borrow::Cow<'_, TableCell> {
        self.binary_layout_with(&[])
    }

    /// [`binary_layout`](Self::binary_layout) of a cell of a table that states `row_heights` (the row heights of its
    /// legacy table style override): a legacy cell states a text height equal to the height of its row where the user set
    /// none, and AutoCAD's own R2010 file of such a table (sample_AC1018.dwg to sample_AC1024.dwg) keeps the style's text
    /// height there and overrides it only for another value.
    pub fn binary_layout_with(&self, row_heights: &[f64]) -> std::borrow::Cow<'_, TableCell> {
        use CellStylePropertyFlags as P;
        const CONTENT: P = P::TEXT_HEIGHT.union(P::TEXT_STYLE).union(P::CONTENT_COLOR);
        let legacy = self.style.as_ref().is_some_and(|s| s.legacy_override_bits);
        if !legacy {
            return std::borrow::Cow::Borrowed(self);
        }
        let own = self.style.clone();
        let untyped = self.contents.iter().any(|c| c.format_value_data_type == 0);
        if own.is_none() && !untyped {
            return std::borrow::Cow::Borrowed(self);
        }
        let mut cell = self.clone();
        for content in &mut cell.contents {
            if content.format_value_data_type == 0 {
                if content.value.text.is_empty() && content.field_handle.is_none() {
                    content.format_value_data_type = 512;
                } else {
                    content.format_value_data_type = 4;
                    content.format_override_flags |= 3;
                    // AutoCAD's R2010 file states the text as the formatted value of a text item (sample_AC1024.dwg).
                    if content.value.value_type == CellValueType::String && content.value.formatted_value.is_empty() {
                        content.value.formatted_value = content.value.text.clone();
                    }
                }
            }
        }
        if let Some(own) = own {
            let p = own.overridden();
            for content in &mut cell.contents {
                if p.contains(P::TEXT_HEIGHT) && !row_heights.contains(&own.text_height) {
                    content.format_override_flags |= P::TEXT_HEIGHT.bits() as i32;
                    content.text_height = own.text_height;
                }
                if p.contains(P::TEXT_STYLE) {
                    content.format_override_flags |= P::TEXT_STYLE.bits() as i32;
                    content.text_style_handle = own.text_style_handle;
                    content.text_style_name = own.text_style_name.clone();
                }
                if p.contains(P::CONTENT_COLOR) {
                    content.format_override_flags |= P::CONTENT_COLOR.bits() as i32;
                    content.color = own.content_color;
                }
            }
            if let Some(style) = cell.style.as_mut() {
                if p.contains(P::BACKGROUND_COLOR) && !style.fill_enabled {
                    style.background_color = Color::ByBlock;
                }
                for (color_bit, weight_bit, edge, border) in [
                    (0x40, 0x400, CellEdgeFlags::TOP, &mut style.top_border),
                    (0x80, 0x800, CellEdgeFlags::RIGHT, &mut style.right_border),
                    (0x100, 0x1000, CellEdgeFlags::BOTTOM, &mut style.bottom_border),
                    (0x200, 0x2000, CellEdgeFlags::LEFT, &mut style.left_border),
                ] {
                    if own.override_flags & color_bit != 0 {
                        border.override_flags |= BorderPropertyFlags::COLOR;
                    }
                    if own.override_flags & weight_bit != 0 {
                        border.override_flags |=
                            BorderPropertyFlags::LINE_WEIGHT | BorderPropertyFlags::INVISIBILITY;
                    }
                    if own.override_flags & (color_bit | weight_bit) != 0 {
                        style.applied_border_edges |= edge;
                    }
                }
                style.override_flags = (p & !CONTENT).bits() as i32;
                style.property_flags = P::NONE;
                style.legacy_override_bits = false;
            }
        }
        std::borrow::Cow::Owned(cell)
    }

    /// The pre-R2010 flat layout of a cell that carries the R2010 layout (the inverse of [`binary_layout`](Self::binary_layout)):
    /// the formats the content and the cell style state go into the one override word of a legacy cell, as AutoCAD's own R2004
    /// file of a drawing states them (sample_AC1018.dwg from sample_AC1032.dwg): a text height, content colour, text style or
    /// alignment the content format overrides (0x20, 0x08, 0x10, 0x01), the cell's background colour (0x04), and per border
    /// edge the colour (0x40, 0x80, 0x100, 0x200) and line weight with visibility (0x400, 0x800, 0x1000, 0x2000) that the
    /// edge's border overrides, which also set the edge in the cell's edge flags (top 1, right 2, bottom 4, left 8). A cell
    /// of the legacy layout, or with nothing overridden, is returned as it is.
    pub fn legacy_layout(&self) -> std::borrow::Cow<'_, TableCell> {
        self.legacy_layout_in(None, false)
    }

    /// [`legacy_layout`](Self::legacy_layout) of a cell of a row with the style `row` and of a table whose last column holds the
    /// cell when `last_column`. In AutoCAD's R2000 and R2004 files of a table whose rows state text heights of their own
    /// (sample_AC1015.dwg, sample_AC1018.dwg: title 0.1425, other rows 0.1026) a cell that overrides no text height states
    /// the row's, and a cell of the last column takes its right edge from the inside horizontal border (edge flags 11 where the
    /// cells to its left have 9).
    pub fn legacy_layout_in(&self, row: Option<&CellStyle>, last_column: bool) -> std::borrow::Cow<'_, TableCell> {
        use CellStylePropertyFlags as P;
        let modern_style = self.style.as_ref().is_some_and(|s| !s.legacy_override_bits);
        let content_format = self.contents.first().map_or(0, |c| c.format_override_flags as u32);
        let content_bits = (P::TEXT_HEIGHT | P::CONTENT_COLOR | P::TEXT_STYLE | P::ALIGNMENT).bits();
        if !modern_style && !(self.style.is_none() && content_format & content_bits != 0) {
            return std::borrow::Cow::Borrowed(self);
        }
        let mut cell = self.clone();
        let mut style = self.style.clone().unwrap_or_else(CellStyle::new);
        let own = if modern_style { style.overridden() } else { P::NONE };
        let set = P::from_bits_truncate(content_format) | own;
        let content = self.contents.first();
        let mut flags = 0;
        if set.contains(P::ALIGNMENT) {
            flags |= 0x01;
            if !own.contains(P::ALIGNMENT) {
                style.alignment = content.map_or(style.alignment, |c| c.alignment);
            }
        }
        if own.contains(P::BACKGROUND_COLOR) {
            flags |= 0x04;
        }
        if let Some(content) = content {
            if set.contains(P::CONTENT_COLOR) {
                flags |= 0x08;
                style.content_color = content.color;
            }
            if set.contains(P::TEXT_STYLE) {
                flags |= 0x10;
                style.text_style_handle = content.text_style_handle;
                style.text_style_name = content.text_style_name.clone();
            }
            if set.contains(P::TEXT_HEIGHT) {
                flags |= 0x20;
                style.text_height = content.text_height;
            }
        }
        // A row that states a text height of its own (override bit 0x80 of its style; AutoCAD's R2010 and later files of the
        // sample drawing: the title row 0.1425, the others 0.1026) gives it to the cells of the row that override none.
        if flags & 0x20 == 0 && modern_style {
            if let Some(row) = row.filter(|row| row.override_flags & 0x80 != 0 && row.text_height > 0.0) {
                flags |= 0x20;
                style.text_height = row.text_height;
            }
        }
        // A value type or format the content overrides sets the one bit above the borders' (AutoCAD's flag word 0x40000).
        if content_format & (P::DATA_TYPE | P::DATA_FORMAT).bits() != 0 {
            flags |= 0x40000;
        }
        let explicit = |border: &CellBorder| {
            border
                .override_flags
                .intersects(BorderPropertyFlags::COLOR | BorderPropertyFlags::LINE_WEIGHT | BorderPropertyFlags::INVISIBILITY)
        };
        if last_column && !explicit(&style.right_border) {
            let inside = style
                .additional_borders
                .iter()
                .find(|(edge, border)| *edge == CellEdgeFlags::INSIDE_HORIZONTAL.bits() as u32 && explicit(border))
                .map(|(_, border)| border.clone());
            if let Some(inside) = inside {
                style.right_border = inside;
                style.applied_border_edges |= CellEdgeFlags::RIGHT;
            }
        }
        let mut edges = 0u8;
        for (color_bit, weight_bit, edge, edge_bit, border) in [
            (0x40, 0x400, CellEdgeFlags::TOP, 1u8, &style.top_border),
            (0x80, 0x800, CellEdgeFlags::RIGHT, 2, &style.right_border),
            (0x100, 0x1000, CellEdgeFlags::BOTTOM, 4, &style.bottom_border),
            (0x200, 0x2000, CellEdgeFlags::LEFT, 8, &style.left_border),
        ] {
            if !style.applied_border_edges.contains(edge) {
                continue;
            }
            let mut explicit = false;
            if border.override_flags.contains(BorderPropertyFlags::COLOR) {
                flags |= color_bit;
                explicit = true;
            }
            if border.override_flags.intersects(BorderPropertyFlags::LINE_WEIGHT | BorderPropertyFlags::INVISIBILITY) {
                flags |= weight_bit;
                explicit = true;
            }
            if explicit {
                edges |= edge_bit;
            }
        }
        if cell.edge_flags == 0 {
            cell.edge_flags = edges;
        }
        if flags == 0 {
            cell.style = None;
        } else {
            style.override_flags = flags;
            style.property_flags = P::NONE;
            style.legacy_override_bits = true;
            cell.style = Some(style);
        }
        std::borrow::Cow::Owned(cell)
    }

    pub fn set_text(&mut self, s: &str) {
        // Typed text keeps the content's format and is stored as a string
        // value with the flags the reference writes for typed text (6).
        let mut content = self
            .contents
            .first()
            .filter(|c| c.content_type == TableCellContentType::Value)
            .cloned()
            .unwrap_or_else(|| CellContent::text(s));
        content.value = CellValue::text(s);
        content.value.flags = 6;
        content.field_handle = None;
        self.contents.clear();
        self.contents.push(content);
        self.cell_type = CellType::Text;
    }

    /// Returns true if this cell spans multiple cells.
    /// Group 91 (177 before R2007) of a legacy cell is the override word of its style: the bits its groups imply
    /// (set as the groups are read) and the ones no group states. Those go onto the style, as a DWG cell stores
    /// them; the word is not kept apart.
    pub(crate) fn state_untied_override_bits(&mut self) {
        let untied = std::mem::take(&mut self.flag) & !LEGACY_TIED_OVERRIDE_BITS;
        if untied != 0 {
            let style = self.style.get_or_insert_with(CellStyle::new);
            style.override_flags |= untied;
            style.legacy_override_bits = true;
        }
    }

    pub fn is_merged(&self) -> bool {
        self.merge_width > 1 || self.merge_height > 1
    }

    /// Returns true if the cell has content.
    pub fn has_content(&self) -> bool {
        !self.contents.is_empty()
    }

    /// Returns true if this cell has multiple contents.
    pub fn has_multiple_contents(&self) -> bool {
        self.contents.len() > 1
    }
}

impl Default for TableCell {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Table Row
// ============================================================================

/// A row in a table.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TableRow {
    /// Row height.
    pub height: f64,
    /// Cells in this row.
    pub cells: Vec<TableCell>,
    /// Row style override.
    pub style: Option<CellStyle>,
    /// Source row-style type, retained when the source had no style data.
    #[cfg_attr(feature = "serde", serde(default))]
    pub source_style_type: Option<i32>,
    /// Custom data value.
    pub custom_data: i32,
    pub custom_data_items: Vec<TableCustomData>,
    pub style_id: i32,
}

impl TableRow {
    /// Creates a new row with the given number of cells.
    pub fn new(num_cells: usize) -> Self {
        let cells = (0..num_cells).map(|_| TableCell::new()).collect();
        Self {
            height: 0.25,
            cells,
            style: None,
            source_style_type: None,
            custom_data: 0,
            custom_data_items: Vec::new(),
            style_id: 0,
        }
    }

    /// Creates a row from cell values.
    pub fn from_texts(texts: &[&str]) -> Self {
        let cells = texts.iter().map(|t| TableCell::text(t)).collect();
        Self {
            height: 0.25,
            cells,
            style: None,
            source_style_type: None,
            custom_data: 0,
            custom_data_items: Vec::new(),
            style_id: 0,
        }
    }

    /// Returns the number of cells.
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// Gets a cell by index.
    pub fn cell(&self, index: usize) -> Option<&TableCell> {
        self.cells.get(index)
    }

    /// Gets a mutable cell by index.
    pub fn cell_mut(&mut self, index: usize) -> Option<&mut TableCell> {
        self.cells.get_mut(index)
    }
}

impl Default for TableRow {
    fn default() -> Self {
        Self::new(0)
    }
}

// ============================================================================
// Table Column
// ============================================================================

/// A column definition in a table.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TableColumn {
    /// Column name.
    pub name: String,
    /// Column width.
    pub width: f64,
    /// Column style override.
    pub style: Option<CellStyle>,
    /// Source column-style type, retained when the source had no style data.
    #[cfg_attr(feature = "serde", serde(default))]
    pub source_style_type: Option<i32>,
    /// Custom data value.
    pub custom_data: i32,
    pub custom_data_items: Vec<TableCustomData>,
    pub style_id: i32,
}

impl TableColumn {
    /// Creates a new column with default width.
    pub fn new() -> Self {
        Self {
            name: String::new(),
            width: 2.5,
            style: None,
            source_style_type: None,
            custom_data: 0,
            custom_data_items: Vec::new(),
            style_id: 0,
        }
    }

    /// Creates a column with the given width.
    pub fn with_width(width: f64) -> Self {
        Self {
            name: String::new(),
            width,
            style: None,
            source_style_type: None,
            custom_data: 0,
            custom_data_items: Vec::new(),
            style_id: 0,
        }
    }

    /// Creates a named column.
    pub fn named(name: &str, width: f64) -> Self {
        Self {
            name: name.to_string(),
            width,
            style: None,
            source_style_type: None,
            custom_data: 0,
            custom_data_items: Vec::new(),
            style_id: 0,
        }
    }
}

impl Default for TableColumn {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Cell Range
// ============================================================================

/// A range of cells in a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CellRange {
    /// Top row index.
    pub top_row: usize,
    /// Left column index.
    pub left_col: usize,
    /// Bottom row index (inclusive).
    pub bottom_row: usize,
    /// Right column index (inclusive).
    pub right_col: usize,
}

impl CellRange {
    /// Creates a range for a single cell.
    pub fn cell(row: usize, col: usize) -> Self {
        Self {
            top_row: row,
            left_col: col,
            bottom_row: row,
            right_col: col,
        }
    }

    /// Creates a range of cells.
    pub fn new(top_row: usize, left_col: usize, bottom_row: usize, right_col: usize) -> Self {
        Self {
            top_row,
            left_col,
            bottom_row,
            right_col,
        }
    }

    /// Returns the number of rows in the range.
    pub fn row_count(&self) -> usize {
        self.bottom_row - self.top_row + 1
    }

    /// Returns the number of columns in the range.
    pub fn col_count(&self) -> usize {
        self.right_col - self.left_col + 1
    }

    /// Returns the total number of cells in the range.
    pub fn cell_count(&self) -> usize {
        self.row_count() * self.col_count()
    }

    /// Returns true if this range contains the given cell.
    pub fn contains(&self, row: usize, col: usize) -> bool {
        row >= self.top_row
            && row <= self.bottom_row
            && col >= self.left_col
            && col <= self.right_col
    }
}

impl Default for CellRange {
    fn default() -> Self {
        Self::cell(0, 0)
    }
}

/// One manual table-break position.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TableBreakData {
    pub position: Vector3,
    pub height: f64,
    pub flags: i32,
}

/// Cached range attached to a table break.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TableBreakRange {
    pub position: Vector3,
    pub start_row: i32,
    pub end_row: i32,
}

/// Pre-R2010 table-wide style overrides.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LegacyTableStyleOverride {
    pub flags: i32,
    pub title_suppressed: Option<bool>,
    /// Header suppression retained from legacy DXF.
    #[cfg_attr(feature = "serde", serde(default))]
    pub header_suppressed: Option<bool>,
    pub flow_direction: Option<i16>,
    pub horizontal_cell_margin: Option<f64>,
    pub vertical_cell_margin: Option<f64>,
    pub row_colors: Vec<Color>,
    pub row_fill_none: Vec<bool>,
    pub row_fill_colors: Vec<Color>,
    pub row_alignments: Vec<i16>,
    /// Text-style names retained from legacy DXF.
    #[cfg_attr(feature = "serde", serde(default))]
    pub text_style_names: Vec<String>,
    pub text_style_handles: Vec<Handle>,
    pub row_heights: Vec<f64>,
}

/// Pre-R2010 border overrides. Values follow set-bit order from bit 0 through 17.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LegacyBorderOverrides<T> {
    pub flags: i32,
    pub values: Vec<T>,
}

impl<T> Default for LegacyBorderOverrides<T> {
    fn default() -> Self {
        Self {
            flags: 0,
            values: Vec::new(),
        }
    }
}

// ============================================================================
// Table Entity
// ============================================================================

#[cfg(feature = "serde")]
fn default_table_scale() -> Vector3 {
    Vector3::new(1.0, 1.0, 1.0)
}

/// Table entity.
///
/// A table is a grid of cells containing text, blocks, or formulas,
/// with extensive styling and formatting options.
///
/// # Example
///
/// ```ignore
/// use acadrust::entities::{Table, TableRow};
/// use acadrust::types::Vector3;
///
/// // Create a 3x4 table
/// let mut table = Table::new(Vector3::new(0.0, 0.0, 0.0), 3, 4);
///
/// // Set header row
/// table.set_cell_text(0, 0, "Name");
/// table.set_cell_text(0, 1, "Value");
/// table.set_cell_text(0, 2, "Unit");
/// table.set_cell_text(0, 3, "Notes");
///
/// // Add data rows
/// table.set_cell_text(1, 0, "Length");
/// table.set_cell_text(1, 1, "100.0");
/// table.set_cell_text(1, 2, "mm");
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Table {
    /// Common entity data.
    pub common: EntityCommon,
    /// Insertion point.
    pub insertion_point: Vector3,
    /// INSERT base scale factors.
    #[cfg_attr(feature = "serde", serde(default = "default_table_scale"))]
    pub insert_scale: Vector3,
    /// INSERT base rotation angle in radians.
    #[cfg_attr(feature = "serde", serde(default))]
    pub insert_rotation: f64,
    /// Last outer DXF INSERT scale codes, including overlapping table codes.
    #[cfg_attr(feature = "serde", serde(default))]
    pub dxf_insert_scale: Option<Vector3>,
    /// Last outer DXF INSERT rotation code, including later subclasses.
    #[cfg_attr(feature = "serde", serde(default))]
    pub dxf_insert_rotation: Option<f64>,
    /// Raw INSERT array dimensions and spacings retained from DXF group codes
    /// 70/71/44/45. Table grid dimensions use separate native fields.
    #[cfg_attr(feature = "serde", serde(default))]
    pub dxf_insert_array: Option<(u16, u16, f64, f64)>,
    /// Horizontal direction vector.
    pub horizontal_direction: Vector3,
    /// Normal vector.
    pub normal: Vector3,
    /// Table style handle.
    pub table_style_handle: Option<Handle>,
    /// Block record handle (table is based on block).
    pub block_record_handle: Option<Handle>,
    /// Anonymous block name stored by the legacy DXF INSERT base.
    #[cfg_attr(feature = "serde", serde(default))]
    pub block_name: String,
    /// Table data version.
    pub data_version: i16,
    /// Table value flags.
    pub value_flags: i32,
    /// Flag overrides.
    pub override_flag: bool,
    pub override_border_color: bool,
    pub override_border_line_weight: bool,
    pub override_border_visibility: bool,
    /// Native pre-R2010 table and border overrides.
    pub legacy_style_override: Option<LegacyTableStyleOverride>,
    pub legacy_border_colors: Option<LegacyBorderOverrides<Color>>,
    pub legacy_border_line_weights: Option<LegacyBorderOverrides<LineWeight>>,
    pub legacy_border_visibility: Option<LegacyBorderOverrides<bool>>,
    /// Rows.
    pub rows: Vec<TableRow>,
    /// Columns.
    pub columns: Vec<TableColumn>,
    /// Break options.
    pub break_options: BreakOptionFlags,
    /// Whether the source DWG carried the optional break-data section.
    #[cfg_attr(feature = "serde", serde(default))]
    pub has_break_data: bool,
    /// Break flow direction.
    pub break_flow_direction: BreakFlowDirection,
    /// Raw break-flow direction value from the source DWG, including zero or
    /// unknown values that the typed enum maps to its default.
    #[cfg_attr(feature = "serde", serde(default))]
    pub source_break_flow_direction: Option<i32>,
    /// Break spacing.
    pub break_spacing: f64,
    /// Linked table-data name and description.
    pub name: String,
    pub description: String,
    /// FIELD objects referenced by the table content.
    pub field_handles: Vec<Handle>,
    /// Base style applied to the table.
    pub base_style: Option<CellStyle>,
    /// Source base-style type, retained when the source had no style data.
    #[cfg_attr(feature = "serde", serde(default))]
    pub base_style_type: Option<i32>,
    /// Merged cell ranges.
    pub merged_ranges: Vec<CellRange>,
    /// Manual break positions and cached break ranges.
    pub break_data: Vec<TableBreakData>,
    pub break_ranges: Vec<TableBreakRange>,
    /// Undocumented header values retained by the native DWG codec.
    pub dwg_unknown_byte: u8,
    pub dwg_unknown_handle: Option<Handle>,
    pub dwg_unknown_long1: i32,
    /// R2010-only header bit. New tables use the native default `true`.
    /// Kept separate from the R2013+ long, whose native default is zero.
    #[cfg_attr(feature = "serde", serde(default))]
    pub dwg_r2010_unknown_bit: Option<bool>,
    pub dwg_unknown_long2: i32,
    pub dwg_unknown_short: i16,
    /// The columns, rows and base style as the legacy DXF entity stated them, kept when a table read from DXF
    /// was completed from its TABLECONTENT and TABLEGEOMETRY objects (`restore_table_roundtrip`). Compatibility
    /// provenance, not CAD data: the view of a reader that knows the legacy entity only (the oracle's DXF reader)
    /// is projected from it, and the DXF writer states the legacy cells from it. Not in the native dump.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub(crate) dxf_legacy: DxfLegacyView,
}

/// The legacy state of a table completed from its objects (see [`Table::dxf_legacy`]). It takes part in equality:
/// the DXF writer states the legacy cells from it, so two tables that differ in it write different files.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct DxfLegacyView(pub Option<Box<TableLegacyView>>);

/// What the legacy `ACAD_TABLE` entity of a DXF stated, before the objects completed it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableLegacyView {
    pub columns: Vec<TableColumn>,
    pub rows: Vec<TableRow>,
    pub base_style: Option<CellStyle>,
    pub base_style_type: Option<i32>,
    /// The fields of the legacy entity record that an R2010 record does not have (see [`Table::as_r2010_record`]).
    pub value_flags: i32,
    pub override_flag: bool,
    pub legacy_style_override: Option<LegacyTableStyleOverride>,
}

fn visit_table_value_handles(value: &mut CellValue, visit: &mut impl FnMut(&mut Handle)) {
    if let Some(handle) = value.handle_value.as_mut() {
        visit(handle);
    }
}

fn visit_table_border_handles(value: &mut CellBorder, visit: &mut impl FnMut(&mut Handle)) {
    if let Some(handle) = value.line_type_handle.as_mut() {
        visit(handle);
    }
}

fn visit_table_style_handles(value: &mut CellStyle, visit: &mut impl FnMut(&mut Handle)) {
    if let Some(handle) = value.text_style_handle.as_mut() {
        visit(handle);
    }
    visit_table_border_handles(&mut value.top_border, visit);
    visit_table_border_handles(&mut value.right_border, visit);
    visit_table_border_handles(&mut value.bottom_border, visit);
    visit_table_border_handles(&mut value.left_border, visit);
    for (_, border) in &mut value.additional_borders {
        visit_table_border_handles(border, visit);
    }
}

fn visit_table_custom_data_handles(
    values: &mut [TableCustomData],
    visit: &mut impl FnMut(&mut Handle),
) {
    for value in values {
        visit_table_value_handles(&mut value.value, visit);
    }
}

fn visit_table_cell_handles(value: &mut TableCell, visit: &mut impl FnMut(&mut Handle)) {
    for content in &mut value.contents {
        visit_table_value_handles(&mut content.value, visit);
        if let Some(handle) = content.block_handle.as_mut() {
            visit(handle);
        }
        if let Some(handle) = content.text_style_handle.as_mut() {
            visit(handle);
        }
        if let Some(handle) = content.field_handle.as_mut() {
            visit(handle);
        }
        for attribute in &mut content.attributes {
            visit(&mut attribute.definition_handle);
        }
    }
    if let Some(style) = value.style.as_mut() {
        visit_table_style_handles(style, visit);
    }
    if let Some(handle) = value.value_handle.as_mut() {
        visit(handle);
    }
    visit_table_custom_data_handles(&mut value.custom_data_items, visit);
    if let Some(handle) = value.data_link_handle.as_mut() {
        visit(handle);
    }
    if let Some(handle) = value.geometry_handle.as_mut() {
        visit(handle);
    }
}

impl Table {
    pub fn sync_merged_ranges_from_cells(&mut self) {
        let ranges = self.canonical_merged_ranges();
        self.merged_ranges = ranges;
    }

    pub(crate) fn sync_cells_from_merged_ranges(&mut self) {
        let ranges = self.canonical_merged_ranges();
        self.set_merged_ranges(ranges);
    }

    pub(crate) fn canonical_merged_ranges(&self) -> Vec<CellRange> {
        let row_count = self.rows.len();
        let column_count = self.columns.len();
        if row_count == 0 || column_count == 0 {
            return Vec::new();
        }

        let mut ranges = Vec::new();
        for range in &self.merged_ranges {
            if range.top_row >= row_count
                || range.left_col >= column_count
                || range.top_row > range.bottom_row
                || range.left_col > range.right_col
            {
                continue;
            }
            let range = CellRange {
                top_row: range.top_row,
                left_col: range.left_col,
                bottom_row: range.bottom_row.min(row_count - 1),
                right_col: range.right_col.min(column_count - 1),
            };
            if range.cell_count() > 1 && !ranges.contains(&range) {
                ranges.push(range);
            }
        }
        for (row_index, row) in self.rows.iter().enumerate() {
            for (column_index, cell) in row.cells.iter().take(column_count).enumerate() {
                let height = cell.merge_height.max(1) as usize;
                let width = cell.merge_width.max(1) as usize;
                if height == 1 && width == 1 {
                    continue;
                }
                let range = CellRange {
                    top_row: row_index,
                    left_col: column_index,
                    bottom_row: row_index.saturating_add(height - 1).min(row_count - 1),
                    right_col: column_index.saturating_add(width - 1).min(column_count - 1),
                };
                if range.cell_count() > 1 && !ranges.contains(&range) {
                    ranges.push(range);
                }
            }
        }
        ranges
    }

    fn set_merged_ranges(&mut self, ranges: Vec<CellRange>) {
        for row in &mut self.rows {
            for cell in &mut row.cells {
                cell.merge_width = 1;
                cell.merge_height = 1;
                cell.merged = 0;
            }
        }
        self.merged_ranges = ranges;
        for range in &self.merged_ranges {
            for row in self
                .rows
                .iter_mut()
                .take(range.bottom_row + 1)
                .skip(range.top_row)
            {
                for cell in row
                    .cells
                    .iter_mut()
                    .take(range.right_col + 1)
                    .skip(range.left_col)
                {
                    cell.merged = 1;
                }
            }
            if let Some(cell) = self
                .rows
                .get_mut(range.top_row)
                .and_then(|row| row.cells.get_mut(range.left_col))
            {
                cell.merged = 0;
                cell.merge_width = range.col_count() as i32;
                cell.merge_height = range.row_count() as i32;
            }
        }
    }

    pub(crate) fn visit_object_handles_mut(&mut self, visit: &mut impl FnMut(&mut Handle)) {
        if let Some(handle) = self.common.linetype_handle.as_mut() {
            visit(handle);
        }
        for handle in &mut self.common.reactors {
            visit(handle);
        }
        if let Some(handle) = self.common.xdictionary_handle.as_mut() {
            visit(handle);
        }
        visit(&mut self.common.owner_handle);
        if let Some(handle) = self.common.color_book_handle.as_mut() {
            visit(handle);
        }
        if let Some(handle) = self.common.full_visual_style_handle.as_mut() {
            visit(handle);
        }
        if let Some(handle) = self.common.face_visual_style_handle.as_mut() {
            visit(handle);
        }
        if let Some(handle) = self.common.edge_visual_style_handle.as_mut() {
            visit(handle);
        }
        if let Some(handle) = self.common.material_handle.as_mut() {
            visit(handle);
        }
        if let Some(handle) = self.common.plotstyle_handle.as_mut() {
            visit(handle);
        }
        if let Some(handle) = self.table_style_handle.as_mut() {
            visit(handle);
        }
        if let Some(handle) = self.block_record_handle.as_mut() {
            visit(handle);
        }
        if let Some(style) = self.legacy_style_override.as_mut() {
            for handle in &mut style.text_style_handles {
                visit(handle);
            }
        }
        for row in &mut self.rows {
            if let Some(style) = row.style.as_mut() {
                visit_table_style_handles(style, visit);
            }
            visit_table_custom_data_handles(&mut row.custom_data_items, visit);
            for cell in &mut row.cells {
                visit_table_cell_handles(cell, visit);
            }
        }
        for column in &mut self.columns {
            if let Some(style) = column.style.as_mut() {
                visit_table_style_handles(style, visit);
            }
            visit_table_custom_data_handles(&mut column.custom_data_items, visit);
        }
        for handle in &mut self.field_handles {
            visit(handle);
        }
        if let Some(style) = self.base_style.as_mut() {
            visit_table_style_handles(style, visit);
        }
        if let Some(handle) = self.dwg_unknown_handle.as_mut() {
            visit(handle);
        }
    }

    /// Creates a new table with the given dimensions.
    pub fn new(insertion_point: Vector3, num_rows: usize, num_cols: usize) -> Self {
        let rows = (0..num_rows).map(|_| TableRow::new(num_cols)).collect();
        let columns = (0..num_cols).map(|_| TableColumn::new()).collect();

        Self {
            common: EntityCommon::default(),
            insertion_point,
            insert_scale: Vector3::new(1.0, 1.0, 1.0),
            insert_rotation: 0.0,
            dxf_insert_scale: None,
            dxf_insert_rotation: None,
            dxf_insert_array: None,
            horizontal_direction: Vector3::new(1.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            table_style_handle: None,
            block_record_handle: None,
            block_name: String::new(),
            data_version: 0,
            value_flags: 0,
            override_flag: false,
            override_border_color: false,
            override_border_line_weight: false,
            override_border_visibility: false,
            legacy_style_override: None,
            legacy_border_colors: None,
            legacy_border_line_weights: None,
            legacy_border_visibility: None,
            rows,
            columns,
            break_options: BreakOptionFlags::NONE,
            has_break_data: false,
            break_flow_direction: BreakFlowDirection::Right,
            source_break_flow_direction: None,
            break_spacing: 0.0,
            name: String::new(),
            description: String::new(),
            field_handles: Vec::new(),
            base_style: None,
            base_style_type: None,
            merged_ranges: Vec::new(),
            break_data: Vec::new(),
            break_ranges: Vec::new(),
            dwg_unknown_byte: 0,
            dwg_unknown_handle: None,
            dwg_unknown_long1: 0,
            dwg_r2010_unknown_bit: None,
            dwg_unknown_long2: 0,
            dwg_unknown_short: 38,
            dxf_legacy: DxfLegacyView(None),
        }
    }

    /// Returns the number of rows.
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// Returns the number of columns.
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Gets a cell at the given position.
    pub fn cell(&self, row: usize, col: usize) -> Option<&TableCell> {
        self.rows.get(row).and_then(|r| r.cells.get(col))
    }

    /// Gets a mutable cell at the given position.
    pub fn cell_mut(&mut self, row: usize, col: usize) -> Option<&mut TableCell> {
        self.rows.get_mut(row).and_then(|r| r.cells.get_mut(col))
    }

    /// Sets the text content of a cell.
    pub fn set_cell_text(&mut self, row: usize, col: usize, text: &str) -> bool {
        if let Some(cell) = self.cell_mut(row, col) {
            cell.set_text(text);
            true
        } else {
            false
        }
    }

    /// Gets the text content of a cell.
    pub fn cell_text(&self, row: usize, col: usize) -> Option<&str> {
        self.cell(row, col).map(|c| c.text_value())
    }

    /// Sets the height of a row.
    pub fn set_row_height(&mut self, row: usize, height: f64) {
        if let Some(r) = self.rows.get_mut(row) {
            r.height = height;
        }
    }

    /// Sets the width of a column.
    pub fn set_column_width(&mut self, col: usize, width: f64) {
        if let Some(c) = self.columns.get_mut(col) {
            c.width = width;
        }
    }

    /// Gets the total table width.
    pub fn total_width(&self) -> f64 {
        self.columns.iter().map(|c| c.width).sum()
    }

    /// Gets the total table height.
    pub fn total_height(&self) -> f64 {
        self.rows.iter().map(|r| r.height).sum()
    }

    /// Adds a new row at the end.
    pub fn add_row(&mut self) -> &mut TableRow {
        let num_cols = self.columns.len();
        self.rows.push(TableRow::new(num_cols));
        self.rows.last_mut().unwrap()
    }

    /// Adds a new column at the end.
    pub fn add_column(&mut self, width: f64) {
        self.columns.push(TableColumn::with_width(width));
        // Add a cell to each row
        for row in &mut self.rows {
            row.cells.push(TableCell::new());
        }
    }

    /// Inserts a row at the given index.
    pub fn insert_row(&mut self, index: usize) {
        let num_cols = self.columns.len();
        if index <= self.rows.len() {
            let ranges = self
                .canonical_merged_ranges()
                .into_iter()
                .map(|mut range| {
                    if index <= range.top_row {
                        range.top_row += 1;
                        range.bottom_row += 1;
                    } else if index <= range.bottom_row {
                        range.bottom_row += 1;
                    }
                    range
                })
                .collect();
            self.rows.insert(index, TableRow::new(num_cols));
            for range in &mut self.break_ranges {
                if index as i32 <= range.start_row {
                    range.start_row += 1;
                    range.end_row += 1;
                } else if index as i32 <= range.end_row {
                    range.end_row += 1;
                }
            }
            self.set_merged_ranges(ranges);
        }
    }

    /// Inserts a column at the given index.
    pub fn insert_column(&mut self, index: usize, width: f64) {
        if index <= self.columns.len() {
            let ranges = self
                .canonical_merged_ranges()
                .into_iter()
                .map(|mut range| {
                    if index <= range.left_col {
                        range.left_col += 1;
                        range.right_col += 1;
                    } else if index <= range.right_col {
                        range.right_col += 1;
                    }
                    range
                })
                .collect();
            self.columns.insert(index, TableColumn::with_width(width));
            for row in &mut self.rows {
                row.cells.insert(index, TableCell::new());
            }
            self.set_merged_ranges(ranges);
        }
    }

    /// Removes a row.
    pub fn remove_row(&mut self, index: usize) -> Option<TableRow> {
        if index < self.rows.len() {
            let ranges = self
                .canonical_merged_ranges()
                .into_iter()
                .filter_map(|mut range| {
                    if index < range.top_row {
                        range.top_row -= 1;
                        range.bottom_row -= 1;
                    } else if index <= range.bottom_row {
                        if range.top_row == range.bottom_row {
                            return None;
                        }
                        range.bottom_row -= 1;
                    }
                    (range.cell_count() > 1).then_some(range)
                })
                .collect();
            let removed = self.rows.remove(index);
            self.break_ranges.retain_mut(|range| {
                if (index as i32) < range.start_row {
                    range.start_row -= 1;
                    range.end_row -= 1;
                } else if index as i32 <= range.end_row {
                    if range.start_row == range.end_row {
                        return false;
                    }
                    range.end_row -= 1;
                }
                true
            });
            self.set_merged_ranges(ranges);
            Some(removed)
        } else {
            None
        }
    }

    /// Removes a column.
    pub fn remove_column(&mut self, index: usize) -> Option<TableColumn> {
        if index < self.columns.len() {
            let ranges = self
                .canonical_merged_ranges()
                .into_iter()
                .filter_map(|mut range| {
                    if index < range.left_col {
                        range.left_col -= 1;
                        range.right_col -= 1;
                    } else if index <= range.right_col {
                        if range.left_col == range.right_col {
                            return None;
                        }
                        range.right_col -= 1;
                    }
                    (range.cell_count() > 1).then_some(range)
                })
                .collect();
            let col = self.columns.remove(index);
            for row in &mut self.rows {
                if index < row.cells.len() {
                    row.cells.remove(index);
                }
            }
            self.set_merged_ranges(ranges);
            Some(col)
        } else {
            None
        }
    }

    /// Merges cells in the given range.
    pub fn merge_cells(&mut self, range: CellRange) {
        if range.top_row > range.bottom_row
            || range.left_col > range.right_col
            || range.bottom_row >= self.rows.len()
            || range.right_col >= self.columns.len()
            || range.cell_count() <= 1
        {
            return;
        }
        let mut ranges = self.canonical_merged_ranges();
        ranges.retain(|current| {
            current.bottom_row < range.top_row
                || current.top_row > range.bottom_row
                || current.right_col < range.left_col
                || current.left_col > range.right_col
        });
        ranges.push(range);
        self.set_merged_ranges(ranges);
    }

    /// Unmerges a cell.
    pub fn unmerge_cell(&mut self, row: usize, col: usize) {
        let ranges = self
            .canonical_merged_ranges()
            .into_iter()
            .filter(|range| !range.contains(row, col))
            .collect();
        self.set_merged_ranges(ranges);
    }

    /// Sets uniform row height for all rows.
    pub fn set_uniform_row_height(&mut self, height: f64) {
        for row in &mut self.rows {
            row.height = height;
        }
    }

    /// Sets uniform column width for all columns.
    pub fn set_uniform_column_width(&mut self, width: f64) {
        for col in &mut self.columns {
            col.width = width;
        }
    }

    /// Clears all cell content but keeps structure.
    pub fn clear_content(&mut self) {
        for row in &mut self.rows {
            for cell in &mut row.cells {
                cell.contents.clear();
            }
        }
    }
}

impl Default for Table {
    fn default() -> Self {
        Self::new(Vector3::ZERO, 3, 3)
    }
}

impl Entity for Table {
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

    fn set_line_weight(&mut self, line_weight: LineWeight) {
        self.common.line_weight = line_weight;
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
        let horizontal = if self.horizontal_direction.length() > 1.0e-12 {
            self.horizontal_direction.normalize()
        } else {
            Vector3::UNIT_X
        };
        let normal = if self.normal.length() > 1.0e-12 {
            self.normal.normalize()
        } else {
            Vector3::UNIT_Z
        };
        let down = {
            let candidate = horizontal.cross(&normal);
            if candidate.length() > 1.0e-12 {
                candidate.normalize()
            } else {
                Vector3::new(0.0, -1.0, 0.0)
            }
        };
        let width = self.total_width();
        let height = self.total_height();
        let p0 = self.insertion_point;
        let p1 = p0 + horizontal * width;
        let p2 = p0 + down * height;
        let p3 = p1 + down * height;
        BoundingBox3D::from_points(&[p0, p1, p2, p3])
            .unwrap_or_else(|| BoundingBox3D::from_point(self.insertion_point))
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_table(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "ACAD_TABLE"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_table(self, transform);
    }
}

// ============================================================================
// Builder
// ============================================================================

/// Builder for Table entities.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TableBuilder {
    table: Table,
}

impl TableBuilder {
    /// Creates a new builder.
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            table: Table::new(Vector3::ZERO, rows, cols),
        }
    }

    /// Sets the insertion point.
    pub fn at(mut self, point: Vector3) -> Self {
        self.table.insertion_point = point;
        self
    }

    /// Sets uniform row height.
    pub fn row_height(mut self, height: f64) -> Self {
        self.table.set_uniform_row_height(height);
        self
    }

    /// Sets uniform column width.
    pub fn column_width(mut self, width: f64) -> Self {
        self.table.set_uniform_column_width(width);
        self
    }

    /// Sets the layer.
    pub fn layer(mut self, layer: &str) -> Self {
        self.table.common.layer = layer.to_string();
        self
    }

    /// Sets text in a cell.
    pub fn cell_text(mut self, row: usize, col: usize, text: &str) -> Self {
        self.table.set_cell_text(row, col, text);
        self
    }

    /// Sets a header row (first row) with texts.
    pub fn header(mut self, headers: &[&str]) -> Self {
        for (col, text) in headers.iter().enumerate() {
            self.table.set_cell_text(0, col, text);
        }
        self
    }

    /// Builds the table.
    pub fn build(self) -> Table {
        self.table
    }
}

impl Default for TableBuilder {
    fn default() -> Self {
        Self::new(3, 3)
    }
}

/// The key of the extension-dictionary record that links a table to its TABLECONTENT and TABLEGEOMETRY objects.
const ROUNDTRIP_RECORD_KEY: &str = "ACAD_XREC_ROUNDTRIP";
/// The value flags AutoCAD states in the legacy record of every table it writes (`Table::legacy_value_flags`).
const LEGACY_TABLE_VALUE_FLAGS: i32 = 22;
/// The first entry (group 102) of that record.
const ROUNDTRIP_TABLE_MARKER: &str = "ACAD_ROUNDTRIP_2008_TABLE_ENTITY";

/// What the round-trip record of one table names: the content object (group 360), the geometry object (361) and
/// the break ranges (count 90, then position 10, first row 90, last row 90 for each).
struct TableRoundtrip {
    content: Handle,
    geometry: Option<Handle>,
    break_ranges: Vec<TableBreakRange>,
}

fn table_roundtrip(document: &crate::document::CadDocument, table: Handle) -> Option<TableRoundtrip> {
    use crate::objects::{ObjectType, XRecordValue};
    let dictionary = document.extension_dictionary_handle(table)?;
    let ObjectType::Dictionary(dictionary) = document.objects.get(&dictionary)? else {
        return None;
    };
    let ObjectType::XRecord(record) = document.objects.get(&dictionary.get(ROUNDTRIP_RECORD_KEY)?)? else {
        return None;
    };
    let mut entries = record.entries.iter();
    if entries.next()?.value.as_string() != Some(ROUNDTRIP_TABLE_MARKER) {
        return None;
    }
    let mut content = None;
    let mut geometry = None;
    let mut breaks = Vec::new();
    let mut counts = Vec::new();
    let mut position = Vector3::ZERO;
    let mut rows = Vec::new();
    for entry in entries {
        match (entry.code, &entry.value) {
            // The record of a drawing saved before R2007 goes on with the markers of the pre-R2007 table and its cells
            // (ACAD_ROUNDTRIP_PRE2007_TABLE, ACAD_ROUNDTRIP_PRE2007_TABLECELL) and their own handles.
            (102, _) => break,
            (360, XRecordValue::Handle(handle)) => content = Some(*handle),
            (361, XRecordValue::Handle(handle)) => geometry = Some(*handle),
            (90, value) => match value.as_i32() {
                Some(value) if counts.is_empty() => counts.push(value),
                Some(value) => rows.push(value),
                None => {}
            },
            (10, value) => {
                if let Some((x, y, z)) = value.as_point3d() {
                    position = Vector3::new(x, y, z);
                }
            }
            _ => {}
        }
        if rows.len() == 2 {
            breaks.push(TableBreakRange {
                position,
                start_row: rows[0],
                end_row: rows[1],
            });
            rows.clear();
        }
    }
    if counts.first().copied() != Some(breaks.len() as i32) {
        return None;
    }
    Some(TableRoundtrip {
        content: content?,
        geometry,
        break_ranges: breaks,
    })
}

/// What completes a table of a DXF of R2010 or later: the cells of its TABLECONTENT object, the cell geometry of its TABLEGEOMETRY
/// object and the break ranges of its round-trip record. `None` when the table has no such objects or another cell grid than the
/// content object (the legacy cells were edited without it).
fn dxf_completion_parts(
    document: &crate::document::CadDocument,
    table: &Table,
) -> Option<(Table, Option<crate::objects::TableGeometry>, Vec<TableBreakRange>)> {
    use crate::objects::{DataObjectData, ObjectType};
    let link = table_roundtrip(document, table.common.handle)?;
    let Some(ObjectType::TableContent(content)) = document.objects.get(&link.content) else {
        return None;
    };
    let same_grid = content.rows.len() == table.rows.len()
        && content.columns.len() == table.columns.len()
        && content.rows.iter().zip(&table.rows).all(|(a, b)| a.cells.len() == b.cells.len());
    if !same_grid {
        return None;
    }
    let geometry = link.geometry.and_then(|handle| match document.objects.get(&handle) {
        Some(ObjectType::DataObject(object)) => match &object.data {
            DataObjectData::TableGeometry(geometry) => Some(geometry.clone()),
            _ => None,
        },
        _ => None,
    });
    Some((content.clone(), geometry, link.break_ranges))
}

/// Complete the tables of a document read from DXF.
///
/// A DXF states a table as the legacy `ACAD_TABLE` entity (cell values and a few overrides) and keeps the rest of
/// what an R2010+ DWG holds in the entity record itself in objects: TABLECONTENT (cell contents with their formats,
/// cell styles, custom data, row and column styles) and TABLEGEOMETRY (cell geometry), linked from the
/// `ACAD_XREC_ROUNDTRIP` record of the entity's extension dictionary, which also states the break ranges. The typed
/// data goes onto the entity here, so a table reads the same from DXF as from DWG; the objects stay, as in a DWG.
/// A table whose cell grid differs from its content object's (the legacy cells were edited without it) is left as
/// read.
pub(crate) fn restore_table_roundtrip(document: &mut crate::document::CadDocument) {
    use crate::entities::EntityType;
    if !document.entities().any(|entity| matches!(entity, EntityType::Table(_))) {
        return;
    }
    // Before R2010 the entity record holds the legacy cells only, as the DXF does.
    let mut work = std::collections::HashMap::new();
    if document.version >= crate::types::DxfVersion::AC1024 {
        for entity in document.entities() {
            let EntityType::Table(table) = entity else {
                continue;
            };
            if let Some(parts) = dxf_completion_parts(document, table) {
                work.insert(table.common.handle, parts);
            }
        }
    }
    for entity in document.entities_mut_keep_raw() {
        let EntityType::Table(table) = entity else {
            continue;
        };
        if let Some((content, geometry, break_ranges)) = work.remove(&table.common.handle) {
            table.restore_modern_content(&content, geometry.as_ref(), break_ranges);
            // The legacy words of the entity (value flags, style override, cell edge flags, block scales) stay in the legacy view; the
            // table holds what an R2010 DWG record of it holds, as a table completed from a file before R2010 does.
            table.as_r2010_record();
        } else {
            // A table of the legacy layout: a block cell also keeps its block pointer in the cell itself, as a
            // pre-R2010 DWG record states it.
            for cell in table.rows.iter_mut().flat_map(|row| row.cells.iter_mut()) {
                if cell.cell_type == CellType::Block {
                    cell.value_handle = cell.contents.first().and_then(|content| content.block_handle);
                }
            }
        }
    }
}

/// State the TABLECONTENT and TABLEGEOMETRY objects and the `ACAD_XREC_ROUNDTRIP` record that link them to the table for the tables of a
/// document that have none, when it is written as a DWG or a DXF of R2004 or earlier: the entity record of those versions holds the
/// legacy flat cells (text, a block, the style overrides), and AutoCAD keeps the typed cells, styles and geometry of the R2010 form in
/// these objects (sample_AC1015.dwg and sample_AC1018.dwg, the DXF twins of R2000 and R2004; the reader `restore_dwg_table_roundtrip`
/// completes the table from them). A table that has its objects (read from a file that held them) keeps them as they are, a table
/// read from a file of R2004 or earlier is the flat form (it has no typed cells to keep), and a table of plain text and block cells
/// needs none (`holds_more_than_flat_cells`). The content object is the table itself; the geometry object holds the geometry the
/// cells cache; the record states the break ranges.
/// Whether a table holds something its legacy flat cells cannot state: a typed or formatted value, a field, a cell with several
/// items, custom data, a data link, a tooltip, a break range. A table of plain text and block cells (what the legacy record
/// holds) has its cells back from the legacy record, and AutoCAD's objects would carry nothing the reader needs.
fn holds_more_than_flat_cells(table: &Table) -> bool {
    !table.break_ranges.is_empty()
        || table.rows.iter().any(|row| {
            !row.custom_data_items.is_empty()
                || row.cells.iter().any(|cell| {
                    cell.contents.len() > 1
                        || !cell.custom_data_items.is_empty()
                        || cell.has_linked_data
                        || !cell.tooltip.is_empty()
                        || cell.contents.iter().any(|content| {
                            content.field_handle.is_some()
                                || !content.value_format.is_empty()
                                || !matches!(content.value.value_type, CellValueType::Unknown | CellValueType::String)
                                || !content.value.format.is_empty()
                                || !matches!(content.content_type, TableCellContentType::Value | TableCellContentType::Block | TableCellContentType::Unknown)
                        })
                })
        })
        || table.columns.iter().any(|column| !column.custom_data_items.is_empty())
}

pub(crate) fn tables_need_roundtrip(document: &crate::document::CadDocument, version: crate::types::DxfVersion) -> bool {
    use crate::entities::EntityType;
    // R2007 states the value of every cell in the entity; a version below it (and a source below it) has the text only.
    version < crate::types::DxfVersion::AC1021
        && document.dwg_source_version.or(document.dxf_source_version).is_none_or(|source| source >= crate::types::DxfVersion::AC1021)
        && document.entities().any(|entity| {
            matches!(entity, EntityType::Table(table) if table.dxf_legacy.0.is_none() && holds_more_than_flat_cells(table) && table_roundtrip(document, table.common.handle).is_none())
        })
}

pub(crate) fn store_table_roundtrip(document: &mut crate::document::CadDocument, version: crate::types::DxfVersion) {
    use crate::entities::EntityType;
    use crate::objects::{DataObject, DataObjectData, ObjectType, TableGeometry, TableGeometryCell, XRecordEntry, XRecordValue};
    if !tables_need_roundtrip(document, version) {
        return;
    }
    let mut missing: Vec<Handle> = document
        .entities()
        .filter_map(|entity| match entity {
            EntityType::Table(table) if table.dxf_legacy.0.is_none() && holds_more_than_flat_cells(table) && table_roundtrip(document, table.common.handle).is_none() => {
                Some(table.common.handle)
            }
            _ => None,
        })
        .collect();
    missing.sort();
    // The classes the objects are declared under.
    for name in ["TABLECONTENT", "TABLEGEOMETRY"] {
        if !document.classes.contains(name) {
            if let Some(class) = crate::classes::default_classes().into_iter().find(|class| class.dxf_name == name) {
                document.classes.add_or_update(class);
            }
        }
    }
    let flags = if version >= crate::types::DxfVersion::AC1015 {
        crate::objects::DictionaryCloningFlags::KeepExisting
    } else {
        crate::objects::DictionaryCloningFlags::NotApplicable
    };
    for handle in missing {
        let Some(EntityType::Table(table)) = document.get_entity(handle) else {
            continue;
        };
        let table = (**table).clone();
        // An extension dictionary of the table that already holds another round-trip record keeps it: the groups are appended.
        let record = document.ensure_xrecord(handle, ROUNDTRIP_RECORD_KEY);
        if let Some(ObjectType::XRecord(existing)) = document.objects.get(&record) {
            if existing.entries.iter().any(|entry| entry.value.as_string() == Some(ROUNDTRIP_TABLE_MARKER)) {
                continue;
            }
        }
        let (content_handle, geometry_handle) = (document.allocate_handle(), document.allocate_handle());
        // The content object: the table itself, as an object of the record.
        let mut content = table.clone();
        content.common = crate::entities::EntityCommon::default();
        content.common.handle = content_handle;
        content.common.owner_handle = record;
        content.dxf_legacy = DxfLegacyView(None);
        document.objects.insert(content_handle, ObjectType::TableContent(content));
        // The geometry object: the geometry the cells cache.
        let geometry = TableGeometry {
            rows: table.rows.len() as i32,
            columns: table.columns.len() as i32,
            cells: table
                .rows
                .iter()
                .flat_map(|row| row.cells.iter())
                .map(|cell| TableGeometryCell {
                    geometry_data_flag: cell.geometry_data_flag,
                    width_with_gap: cell.geometry_width_with_gap,
                    height_with_gap: cell.geometry_height_with_gap,
                    table_geometry: cell.geometry_handle.unwrap_or(Handle::NULL),
                    geometry: cell.geometries.clone(),
                })
                .collect(),
        };
        let mut geometry_object = DataObject::new(DataObjectData::TableGeometry(geometry));
        geometry_object.handle = geometry_handle;
        geometry_object.owner = record;
        document.objects.insert(geometry_handle, ObjectType::DataObject(geometry_object));
        if let Some(ObjectType::XRecord(record)) = document.objects.get_mut(&record) {
            let mut entries = vec![
                XRecordEntry::string(102, ROUNDTRIP_TABLE_MARKER),
                XRecordEntry::handle(360, content_handle),
                XRecordEntry::int16(70, 2),
                XRecordEntry::int32(90, table.break_ranges.len() as i32),
            ];
            for range in &table.break_ranges {
                entries.push(XRecordEntry::new(10, XRecordValue::Point3D(range.position.x, range.position.y, range.position.z)));
                entries.push(XRecordEntry::int32(90, range.start_row));
                entries.push(XRecordEntry::int32(90, range.end_row));
            }
            entries.push(XRecordEntry::handle(361, geometry_handle));
            record.entries.extend(entries);
            record.entries_complete = true;
            record.cloning_flags = flags;
            if record.reactors.is_empty() {
                // as every XRECORD of AutoCAD's extension dictionaries: the dictionary is its reactor
                record.reactors = vec![record.owner];
            }
            record.synchronize_object_references();
        }
    }
}

/// Whether a table without legacy words has the objects the reader completes it from (it was read from a file of R2010 or later, which
/// keeps them, or `store_table_roundtrip` made them) and holds no value flags.
fn table_needs_value_flags(document: &crate::document::CadDocument, table: &Table) -> bool {
    table.value_flags == 0 && table.dxf_legacy.0.is_none() && table_roundtrip(document, table.common.handle).is_some()
}

pub(crate) fn tables_need_value_flags(document: &crate::document::CadDocument) -> bool {
    use crate::entities::EntityType;
    document.entities().any(|entity| matches!(entity, EntityType::Table(table) if table_needs_value_flags(document, table)))
}

/// State the value flags of the legacy entity record (a DWG before R2010, a DXF) of the tables that have none. AutoCAD states 22 in
/// every table it writes there (both tables of sample_AC1014.dwg to sample_AC1021.dwg and the DXF twins of R2000 to R2018, whatever
/// the R2010 record of the table holds: its own short is 22 and 6). A table with the objects the reader completes it from reads back
/// as its R2010 record again (value flags 0, `as_r2010_record`); a flat table, which reads back as it is written, and a table with
/// words of its own keep what they hold. Applied to the copy of the document a writer prepares.
pub(crate) fn state_legacy_value_flags(document: &mut crate::document::CadDocument) {
    use crate::entities::EntityType;
    let handles: Vec<Handle> = document
        .entities()
        .filter_map(|entity| match entity {
            EntityType::Table(table) if table_needs_value_flags(document, table) => Some(table.common.handle),
            _ => None,
        })
        .collect();
    for handle in handles {
        if let Some(EntityType::Table(table)) = document.get_entity_mut(handle) {
            table.value_flags = LEGACY_TABLE_VALUE_FLAGS;
        }
    }
}

/// Whether the document was read from a DWG or a DXF of R2007 or earlier: its tables are the legacy flat form (their typed cells
/// are in the TABLECONTENT objects AutoCAD keeps with them).
fn legacy_source(document: &crate::document::CadDocument) -> bool {
    document.dwg_source_version.or(document.dxf_source_version).is_some_and(|version| version < crate::types::DxfVersion::AC1024)
}

/// Complete the tables of a document read from a DWG or a DXF of R2007 or earlier from the TABLECONTENT and TABLEGEOMETRY objects AutoCAD
/// keeps with them (a legacy table, whose entity record holds the flat cells only): the typed data goes onto the entity as an
/// R2010 DWG holds it (`Table::with_roundtrip_content`, the up-conversion the writer applies to such a table for a later version),
/// and the legacy cells stay as the view of the ACadSharp compatibility layer. A table the objects do not describe (none, another
/// cell grid, a cell edited since) is left as read.
pub(crate) fn restore_dwg_table_roundtrip(document: &mut crate::document::CadDocument) {
    use crate::entities::EntityType;
    if !legacy_source(document) {
        return;
    }
    // One look at the entities; only the restored tables are touched (by handle), and they keep their verbatim record: the
    // legacy record they were read from is what `unrestored_legacy` writes again while nothing changes.
    let restored: Vec<(Handle, Table)> = document
        .entities()
        .filter_map(|entity| match entity {
            EntityType::Table(table) => table.with_roundtrip_content(document).map(|mut restored| {
                restored.as_r2010_record();
                (table.common.handle, restored)
            }),
            _ => None,
        })
        .collect();
    for (handle, restored) in restored {
        if let Some(EntityType::Table(table)) = document.get_entity_mut_keep_raw(handle) {
            **table = restored;
        }
    }
}

/// The custom data item of a table cell that holds the checksum of `cell_text_checksum`.
const CELL_CHECKSUM: &str = "ACAD_ROUNDTRIP_2008_CELL_CHECKSUM";

/// The checksum AutoCAD keeps with a table cell: the sum of every UTF-16 unit of the displayed text times its 1-based position
/// (sample_AC1018.dwg: "Table sample" 7783, "Text" 1110, the number cell shown "0.500000" 1739, an empty cell 0; 19 of 19 cells
/// of its two tables). Non-ASCII text: UTF-16 units assumed (no sample).
pub(crate) fn cell_text_checksum(text: &str) -> f64 {
    text.encode_utf16().enumerate().map(|(index, unit)| (index as f64 + 1.0) * f64::from(unit)).sum()
}

/// The SYSTEMTIME (year, month, day of week, day, hour, minute, second, millisecond; 16 bits each) of the day nearest to
/// `seconds` since 1970-01-01 UTC, with the time of day zero. A date cell holds the local midnight of its day, whose UTC
/// second count is up to twelve hours before or after the midnight of UTC (the sample's cell, 2024-11-15, is
/// 2024-11-14 23:00 UTC).
fn system_time_of_day(seconds: u32) -> [u8; 16] {
    let days = (i64::from(seconds) + 43_200) / 86_400;
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    let weekday = (days + 4).rem_euclid(7);
    let mut bytes = [0u8; 16];
    for (index, value) in [year, month, weekday, day].into_iter().enumerate() {
        bytes[index * 2..index * 2 + 2].copy_from_slice(&(value as u16).to_le_bytes());
    }
    bytes
}

impl Table {
    /// The table as an R2010 record states it, when it is a legacy table (read from a DWG before R2010) whose extension
    /// dictionary links the round-trip TABLECONTENT and TABLEGEOMETRY objects AutoCAD keeps with it: the typed cells, styles
    /// and geometry of those objects, as AutoCAD's own up-conversion takes them (sample_AC1018.dwg to sample_AC1024.dwg: the
    /// date, angle, currency and block cells, the border overrides, the row styles of R14). `None` when the table has no such
    /// objects, when its cell grid differs from theirs or when the text of a legacy cell is no longer the text the objects
    /// state (the cell was edited after the file was saved): the legacy cells are then converted cell by cell.
    pub(crate) fn with_roundtrip_content(&self, document: &crate::document::CadDocument) -> Option<Table> {
        use crate::objects::{DataObjectData, ObjectType};
        if self.dxf_legacy.0.is_some() || !legacy_source(document) {
            return None;
        }
        let link = table_roundtrip(document, self.common.handle)?;
        let Some(ObjectType::TableContent(content)) = document.objects.get(&link.content) else {
            return None;
        };
        let same_grid = content.rows.len() == self.rows.len()
            && content.columns.len() == self.columns.len()
            && content.rows.iter().zip(&self.rows).all(|(a, b)| a.cells.len() == b.cells.len());
        if !same_grid {
            return None;
        }
        let unchanged = self.rows.iter().zip(&content.rows).all(|(row, from)| {
            row.cells.iter().zip(&from.cells).all(|(cell, from)| {
                let legacy = cell.text_value();
                // AutoCAD stores the checksum of the displayed text with every cell of the content object and trusts the value of
                // the cell only while the legacy cell's display still sums to it (19 of 19 cells of sample_AC1018.dwg's two tables:
                // text, numbers, a point, a date, a percentage, a currency). It tells an edit of any cell, also one that no longer
                // parses as the value's type.
                let checksum = from.custom_data_items.iter().find(|data| {
                    data.name == CELL_CHECKSUM && data.value.value_type == CellValueType::Double
                });
                match from.contents.as_slice() {
                    [item] if item.content_type == TableCellContentType::Value && checksum.is_some() => {
                        checksum.is_some_and(|data| (cell_text_checksum(legacy) - data.value.numeric_value).abs() < 0.5)
                    }
                    // A text the objects state must be the text of the legacy cell; a typed value (number, date, point) has
                    // only its display in the legacy cell, which AutoCAD checks by a checksum of its own.
                    [item] if item.content_type == TableCellContentType::Value && item.value.value_type == CellValueType::String => {
                        // A format changes how the text is displayed (%tc1: upper case).
                        !item.value_format.is_empty()
                            || legacy == item.value.formatted_value
                            || legacy.eq_ignore_ascii_case(&item.value.text)
                    }
                    // A plain number states its display exactly (AutoCAD's checksum of the cell would tell the same).
                    [item]
                        if matches!(item.value.value_type, CellValueType::Double | CellValueType::Long)
                            && item.value_format.is_empty()
                            && !legacy.is_empty() =>
                    {
                        legacy.trim().parse::<f64>().map_or(true, |shown| (shown - item.value.numeric_value).abs() < 1e-5)
                    }
                    _ => true,
                }
            })
        });
        if !unchanged {
            return None;
        }
        let geometry = link.geometry.and_then(|handle| match document.objects.get(&handle) {
            Some(ObjectType::DataObject(object)) => match &object.data {
                DataObjectData::TableGeometry(geometry) => Some(geometry.clone()),
                _ => None,
            },
            _ => None,
        });
        let mut table = self.clone();
        table.restore_modern_content(content, geometry.as_ref(), link.break_ranges);
        // The objects state the value of a cell, not what its entity record states about it: the displayed text is the legacy
        // cell's, the value carries the format of its content (and the unit of that format), and the flags are AutoCAD's (the
        // R2010 file of the sample drawing: 6 for a value without a format, 4 with one, 5 for a typed cell without a value, 7
        // for an untyped one). The legacy cell's own custom data (the cell checksum) stays.
        for (row, legacy) in table.rows.iter_mut().zip(&self.rows) {
            for (cell, legacy) in row.cells.iter_mut().zip(&legacy.cells) {
                if !legacy.custom_data_items.is_empty() {
                    cell.custom_data_items = legacy.custom_data_items.clone();
                }
                // The record of a cell in the entity states the checksum value with the flag 2.
                for item in &mut cell.custom_data_items {
                    if item.value.flags == 0 {
                        item.value.flags = 2;
                    }
                }
                let text = legacy.text_value();
                if let [item] = cell.contents.as_mut_slice() {
                    if item.content_type != TableCellContentType::Value {
                        continue;
                    }
                    let unset = text.is_empty();
                    // The objects state a date as 32-bit seconds since 1970 (8 bytes); the entity record, as AutoCAD's
                    // up-conversion writes it, a Windows SYSTEMTIME of the day (16 bytes: sample_AC1024.dwg, 2024-11-15 is
                    // `E8 07 0B 00 05 00 0F 00` and eight zero bytes).
                    if item.value.value_type == CellValueType::Date && item.value.binary_value.len() == 8 {
                        let seconds = u32::from_le_bytes([
                            item.value.binary_value[0],
                            item.value.binary_value[1],
                            item.value.binary_value[2],
                            item.value.binary_value[3],
                        ]);
                        item.value.binary_value = system_time_of_day(seconds).to_vec();
                        item.value.data_size = 16;
                    }
                    item.value.format = item.value_format.clone();
                    item.value.formatted_value = text.to_string();
                    let unit = item.format_value_unit_type;
                    item.value.unit_type = ValueUnitType::from(unit as u32);
                    item.value.raw_unit_type_code = unit;
                    // A cell whose content format is no data type (0, 4 text, 512 none) is untyped.
                    let untyped = matches!(item.format_value_data_type, 0 | 4 | 512);
                    item.value.flags = match (unset, untyped, item.value_format.is_empty()) {
                        (true, true, _) => 7,
                        (true, false, _) => 5,
                        (false, _, true) => 6,
                        (false, _, false) => 4,
                    };
                }
            }
        }
        Some(table)
    }

    /// The table as the DWG before R2010 it was read from stated it, when it is a table `restore_dwg_table_roundtrip` completed from
    /// its objects and nothing has changed since: the legacy cells, which are written again as they were read. `None` for any
    /// other table (a table completed from a DXF, an edited one: the typed cells are converted back cell by cell).
    pub(crate) fn unrestored_legacy(&self, document: &crate::document::CadDocument) -> Option<Table> {
        let legacy = self.dxf_legacy.0.as_deref()?;
        let mut base = self.clone();
        base.dxf_legacy = DxfLegacyView(None);
        base.columns = legacy.columns.clone();
        base.rows = legacy.rows.clone();
        base.base_style = legacy.base_style.clone();
        base.base_style_type = legacy.base_style_type;
        base.value_flags = legacy.value_flags;
        base.override_flag = legacy.override_flag;
        base.legacy_style_override = legacy.legacy_style_override.clone();
        let again = if legacy_source(document) {
            let mut again = base.with_roundtrip_content(document)?;
            again.as_r2010_record();
            again
        } else {
            // A table of a DXF of R2010 or later completed by `restore_table_roundtrip`.
            let (content, geometry, break_ranges) = dxf_completion_parts(document, &base)?;
            let mut again = base.clone();
            again.restore_modern_content(&content, geometry.as_ref(), break_ranges);
            again.as_r2010_record();
            again
        };
        (again == *self).then_some(base)
    }

    /// The fields of the legacy entity record that an R2010 record has no place for take the values an R2010 record reads with
    /// (AutoCAD's own R2010 and later files of a table read back: the value flags 0, no table style override, no edge flags, no
    /// block pointer or scale in the cell, none of the cell's content items for a cell a merge covers or a value that is unset).
    /// A table completed from its objects holds them as an R2010 record does, and the legacy cells it was read with stay in
    /// `dxf_legacy`.
    pub(crate) fn as_r2010_record(&mut self) {
        self.value_flags = 0;
        self.override_flag = false;
        self.legacy_style_override = None;
        let covered: Vec<Vec<bool>> = (0..self.rows.len())
            .map(|row| (0..self.rows[row].cells.len()).map(|column| self.is_merged_away(row, column)).collect())
            .collect();
        for (row_index, row) in self.rows.iter_mut().enumerate() {
            for (column_index, cell) in row.cells.iter_mut().enumerate() {
                cell.edge_flags = 0;
                cell.value_handle = None;
                cell.block_scale = 1.0;
                cell.auto_fit = false;
                if covered[row_index][column_index]
                    && !cell.contents.is_empty()
                    && cell.contents.iter().all(|content| content.is_empty_item() || content.is_empty_text())
                {
                    cell.contents.clear();
                }
                for content in &mut cell.contents {
                    if content.content_type == TableCellContentType::Value && content.value.flags & 1 != 0 {
                        content.value.data_size = 0;
                        content.value.binary_value.clear();
                        if content.value.flags == 7 {
                            content.value.value_type = CellValueType::Unknown;
                            content.value.raw_type_code = 0;
                        }
                    }
                }
            }
        }
    }

    /// Whether cell (`row`, `col`) lies inside a merged range of one row without being its first cell.
    pub fn is_merged_away(&self, row: usize, col: usize) -> bool {
        self.merged_ranges.iter().any(|range| {
            range.top_row == range.bottom_row
                && (range.top_row..=range.bottom_row).contains(&row)
                && (range.left_col..=range.right_col).contains(&col)
                && (row, col) != (range.top_row, range.left_col)
        })
    }

    /// Put the typed data of a TABLECONTENT object (`content`), a TABLEGEOMETRY object and the break ranges of the
    /// round-trip record on a table read from its legacy DXF form, as an R2010+ DWG holds them in the entity.
    /// `content` has the same cell grid as the table.
    pub(crate) fn restore_modern_content(
        &mut self,
        content: &Table,
        geometry: Option<&crate::objects::TableGeometry>,
        break_ranges: Vec<TableBreakRange>,
    ) {
        self.dxf_legacy = DxfLegacyView(Some(Box::new(TableLegacyView {
            columns: self.columns.clone(),
            rows: self.rows.clone(),
            base_style: self.base_style.clone(),
            base_style_type: self.base_style_type,
            value_flags: self.value_flags,
            override_flag: self.override_flag,
            legacy_style_override: self.legacy_style_override.clone(),
        })));
        self.name = content.name.clone();
        self.description = content.description.clone();
        self.base_style = content.base_style.clone();
        self.base_style_type = content.base_style_type;
        self.break_ranges = break_ranges;
        for (column, from) in self.columns.iter_mut().zip(&content.columns) {
            column.name = from.name.clone();
            column.style = from.style.clone();
            column.source_style_type = from.source_style_type;
            column.custom_data = from.custom_data;
            column.custom_data_items = from.custom_data_items.clone();
            column.style_id = from.style_id;
        }
        let columns = self.columns.len();
        for (row_index, (row, from)) in self.rows.iter_mut().zip(&content.rows).enumerate() {
            row.style = from.style.clone();
            row.source_style_type = from.source_style_type;
            row.custom_data = from.custom_data;
            row.custom_data_items = from.custom_data_items.clone();
            row.style_id = from.style_id;
            for (column_index, (cell, from)) in row.cells.iter_mut().zip(&from.cells).enumerate() {
                cell.tooltip = from.tooltip.clone();
                cell.custom_data = from.custom_data;
                cell.custom_data_items = from.custom_data_items.clone();
                cell.has_linked_data = from.has_linked_data;
                cell.data_link_handle = from.data_link_handle;
                cell.data_link_rows = from.data_link_rows;
                cell.data_link_columns = from.data_link_columns;
                cell.data_link_unknown = from.data_link_unknown;
                cell.style = from.style.clone();
                cell.source_style_type = from.source_style_type;
                cell.style_id = from.style_id;
                // The entity states one effective content item per cell (the legacy cell has a block pointer
                // and a value of its own; the pointer is the item). A cell with a single content in the
                // content object has that item. An empty cell, and a cell with several items (the entity states
                // the first), keep the legacy item, whose format is the cell's own content format.
                cell.contents.truncate(1);
                let single = from.contents.len() == 1 && cell.contents.len() == 1;
                if single {
                    cell.contents = from.contents.clone();
                } else if let (Some(item), Some(style)) = (cell.contents.first_mut(), cell.style.as_ref()) {
                    item.format_has_data = true;
                    item.format_override_flags = style.content_format_override_flags;
                    item.format_property_flags = style.content_property_flags;
                    item.format_value_data_type = style.value_data_type;
                    item.format_value_unit_type = style.value_unit_type;
                    item.value_format = style.value_format.clone();
                    item.color = style.content_color;
                    item.text_style_handle = style.text_style_handle;
                    item.text_height = style.text_height;
                    item.rotation = style.rotation;
                    item.scale = style.scale;
                    item.alignment = style.alignment;
                }
                // The geometry object states every item of a cell; the entity's cached geometry is that of the
                // effective item, which the object does not carry for a cell with several.
                if from.contents.len() > 1 {
                    continue;
                }
                let Some(geometry) = geometry.and_then(|g| g.cells.get(row_index * columns + column_index)) else {
                    continue;
                };
                cell.geometry_data_flag = geometry.geometry_data_flag;
                cell.geometry_width_with_gap = geometry.width_with_gap;
                cell.geometry_height_with_gap = geometry.height_with_gap;
                cell.geometry_handle = (!geometry.table_geometry.is_null()).then_some(geometry.table_geometry);
                cell.geometry_flags = geometry.geometry.len() as i32;
                for (index, value) in geometry.geometry.iter().enumerate() {
                    if let Some(item) = cell.contents.get_mut(index) {
                        item.geometry = Some(value.clone());
                    }
                }
                cell.geometries = geometry.geometry.clone();
                cell.geometry = cell.geometries.first().cloned();
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_of_the_round_trip_object_becomes_the_system_time_autocad_states() {
        // The cell of sample_AC1018.dwg (F0 80 36 67 00 00 00 00 = 1731625200, 2024-11-14 23:00 UTC): AutoCAD's R2010 record
        // states E8 07 0B 00 05 00 0F 00 and eight zero bytes.
        assert_eq!(system_time_of_day(0x6736_80F0), [0xE8, 7, 0x0B, 0, 5, 0, 0x0F, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&system_time_of_day(1_731_628_800)[..8], &[0xE8, 7, 0x0B, 0, 5, 0, 0x0F, 0]);
        // 2000-02-29 (a Tuesday) and the epoch (a Thursday).
        assert_eq!(&system_time_of_day(951_782_400)[..8], &[0xD0, 7, 2, 0, 2, 0, 29, 0]);
        assert_eq!(&system_time_of_day(0)[..8], &[0xB2, 7, 1, 0, 4, 0, 1, 0]);
    }

    /// The checksums of sample_AC1018.dwg's table cells (the content object states them as the first custom data item).
    #[test]
    fn the_cell_checksum_sums_the_position_weighted_units_of_the_displayed_text() {
        assert_eq!(cell_text_checksum("Table sample"), 7783.0);
        assert_eq!(cell_text_checksum("Text"), 1110.0);
        assert_eq!(cell_text_checksum("0.500000"), 1739.0);
        assert_eq!(cell_text_checksum(""), 0.0);
        assert_eq!(cell_text_checksum("Hello this is a title"), 21385.0);
    }

    #[test]
    fn test_table_creation() {
        let table = Table::new(Vector3::ZERO, 3, 4);
        assert_eq!(table.row_count(), 3);
        assert_eq!(table.column_count(), 4);
    }

    #[test]
    fn test_table_cell_access() {
        let mut table = Table::new(Vector3::ZERO, 2, 2);
        table.set_cell_text(0, 0, "A1");
        table.set_cell_text(1, 1, "B2");

        assert_eq!(table.cell_text(0, 0), Some("A1"));
        assert_eq!(table.cell_text(1, 1), Some("B2"));
        assert_eq!(table.cell_text(0, 1), Some(""));
    }

    #[test]
    fn test_table_dimensions() {
        let mut table = Table::new(Vector3::ZERO, 2, 3);
        table.set_uniform_row_height(1.0);
        table.set_uniform_column_width(2.0);

        assert_eq!(table.total_height(), 2.0);
        assert_eq!(table.total_width(), 6.0);
    }

    #[test]
    fn test_table_add_row() {
        let mut table = Table::new(Vector3::ZERO, 2, 2);
        table.add_row();
        assert_eq!(table.row_count(), 3);
        assert_eq!(table.rows[2].cell_count(), 2);
    }

    #[test]
    fn test_table_add_column() {
        let mut table = Table::new(Vector3::ZERO, 2, 2);
        table.add_column(3.0);
        assert_eq!(table.column_count(), 3);
        assert_eq!(table.columns[2].width, 3.0);
        assert_eq!(table.rows[0].cell_count(), 3);
    }

    #[test]
    fn test_table_remove_row() {
        let mut table = Table::new(Vector3::ZERO, 3, 2);
        table.remove_row(1);
        assert_eq!(table.row_count(), 2);
    }

    #[test]
    fn test_table_remove_column() {
        let mut table = Table::new(Vector3::ZERO, 2, 3);
        table.remove_column(1);
        assert_eq!(table.column_count(), 2);
        assert_eq!(table.rows[0].cell_count(), 2);
    }

    #[test]
    fn test_cell_merge() {
        let mut table = Table::new(Vector3::ZERO, 3, 3);
        table.merge_cells(CellRange::new(0, 0, 1, 2));

        let cell = table.cell(0, 0).unwrap();
        assert!(cell.is_merged());
        assert_eq!(cell.merge_width, 3);
        assert_eq!(cell.merge_height, 2);
    }

    #[test]
    fn test_cell_value() {
        let text = CellValue::text("Hello");
        assert_eq!(text.display(), "Hello");
        assert!(!text.is_empty());

        let number = CellValue::number(42.5);
        assert_eq!(number.numeric_value, 42.5);
    }

    #[test]
    fn test_cell_range() {
        let range = CellRange::new(1, 2, 3, 4);
        assert_eq!(range.row_count(), 3);
        assert_eq!(range.col_count(), 3);
        assert_eq!(range.cell_count(), 9);
        assert!(range.contains(2, 3));
        assert!(!range.contains(0, 0));
    }

    #[test]
    fn test_table_builder() {
        let table = TableBuilder::new(2, 3)
            .at(Vector3::new(10.0, 20.0, 0.0))
            .row_height(0.5)
            .column_width(2.0)
            .header(&["A", "B", "C"])
            .cell_text(1, 0, "Data")
            .build();

        assert_eq!(table.insertion_point.x, 10.0);
        assert_eq!(table.cell_text(0, 0), Some("A"));
        assert_eq!(table.cell_text(1, 0), Some("Data"));
    }

    #[test]
    fn test_table_bounding_box() {
        let mut table = Table::new(Vector3::new(5.0, 10.0, 0.0), 2, 3);
        table.set_uniform_row_height(1.0);
        table.set_uniform_column_width(2.0);

        // The insertion point is the table's top-left corner: with the
        // default horizontal (+X) and normal (+Z), rows flow downward (-Y),
        // so the box spans y in [insert.y - height, insert.y] and
        // x in [insert.x, insert.x + width].
        let bbox = table.bounding_box();
        assert_eq!(bbox.min.x, 5.0);
        assert_eq!(bbox.max.x, 11.0); // 5 + 3*2
        assert_eq!(bbox.max.y, 10.0); // insertion point = top edge
        assert_eq!(bbox.min.y, 8.0); // 10 - 2*1
    }

    #[test]
    fn test_table_translate() {
        let mut table = Table::new(Vector3::new(0.0, 0.0, 0.0), 2, 2);
        table.translate(Vector3::new(10.0, 20.0, 5.0));
        assert_eq!(table.insertion_point, Vector3::new(10.0, 20.0, 5.0));
    }

    #[test]
    fn test_cell_content() {
        let text = CellContent::text("Hello");
        assert_eq!(text.content_type, TableCellContentType::Value);

        let block = CellContent::block(Handle::new(0x100));
        assert_eq!(block.content_type, TableCellContentType::Block);
        assert!(block.block_handle.is_some());
    }

    #[test]
    fn test_cell_border() {
        let border = CellBorder::with_color(Color::from_index(1));
        assert!(!border.invisible);
        assert!(border.override_flags.contains(BorderPropertyFlags::COLOR));
    }

    #[test]
    fn test_cell_style() {
        let mut style = CellStyle::new();
        style.set_margins(0.1);
        style.set_border_color(Color::from_index(2));

        assert_eq!(style.margin_left, 0.1);
        assert_eq!(style.top_border.color, Color::from_index(2));
    }

    #[test]
    fn test_table_row() {
        let row = TableRow::from_texts(&["A", "B", "C"]);
        assert_eq!(row.cell_count(), 3);
        assert_eq!(row.cell(0).unwrap().text_value(), "A");
    }

    #[test]
    fn test_table_column() {
        let col = TableColumn::named("Width", 5.0);
        assert_eq!(col.name, "Width");
        assert_eq!(col.width, 5.0);
    }

    #[test]
    fn test_insert_operations() {
        let mut table = Table::new(Vector3::ZERO, 2, 2);
        table.insert_row(1);
        assert_eq!(table.row_count(), 3);

        table.insert_column(1, 3.0);
        assert_eq!(table.column_count(), 3);
        assert_eq!(table.rows[0].cell_count(), 3);
    }

    #[test]
    fn test_cell_state_flags() {
        let flags = CellStateFlags::CONTENT_LOCKED | CellStateFlags::FORMAT_LOCKED;
        assert!(flags.contains(CellStateFlags::CONTENT_LOCKED));
        assert!(!flags.contains(CellStateFlags::LINKED));
    }
}

/// The bits of a legacy cell's override word that a group of the cell states (see [`cell_override_bit`]).
const LEGACY_TIED_OVERRIDE_BITS: i32 = 0x3FFF;

pub fn legacy_override_properties(flags: i32) -> CellStylePropertyFlags {
    let mut properties = CellStylePropertyFlags::NONE;
    for (bit, property) in [
        (0x01, CellStylePropertyFlags::ALIGNMENT),
        (0x02, CellStylePropertyFlags::BACKGROUND_COLOR),
        (0x04, CellStylePropertyFlags::BACKGROUND_COLOR),
        (0x08, CellStylePropertyFlags::CONTENT_COLOR),
        (0x10, CellStylePropertyFlags::TEXT_STYLE),
        (0x20, CellStylePropertyFlags::TEXT_HEIGHT),
    ] {
        if flags & bit != 0 {
            properties |= property;
        }
    }
    properties
}

pub fn cell_override_bit(code: i32) -> Option<i32> {
    Some(match code {
        170 => 0x01,
        283 => 0x02,
        63 => 0x04,
        64 => 0x08,
        7 => 0x10,
        140 => 0x20,
        69 => 0x40,
        65 => 0x80,
        66 => 0x100,
        68 => 0x200,
        279 | 289 => 0x400,
        275 | 285 => 0x800,
        276 | 286 => 0x1000,
        278 | 288 => 0x2000,
        _ => return None,
    })
}
