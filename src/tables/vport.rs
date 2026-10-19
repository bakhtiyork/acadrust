//! Viewport table entry

use super::TableEntry;
use crate::entities::{GridFlags, ViewportRenderMode};
use crate::types::{Color, Handle, Vector2, Vector3};

/// A viewport table entry
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VPort {
    /// Unique handle
    pub handle: Handle,
    /// Viewport name
    pub name: String,
    /// Lower-left corner
    pub lower_left: Vector2,
    /// Upper-right corner
    pub upper_right: Vector2,
    /// View center point
    pub view_center: Vector2,
    /// Snap base point
    pub snap_base: Vector2,
    /// Snap spacing
    pub snap_spacing: Vector2,
    /// Grid spacing
    pub grid_spacing: Vector2,
    /// View direction
    pub view_direction: Vector3,
    /// View target
    pub view_target: Vector3,
    /// View height
    pub view_height: f64,
    /// Aspect ratio
    pub aspect_ratio: f64,
    /// Lens length
    pub lens_length: f64,
    /// View twist angle
    pub view_twist: f64,
    /// Front clipping plane distance
    pub front_clip: f64,
    /// Back clipping plane distance
    pub back_clip: f64,
    /// UCS follow mode
    pub ucsfollow: bool,
    /// Circle zoom percent
    pub circle_zoom: i16,
    /// Fast zoom enabled
    pub fast_zoom: bool,
    /// Grid on/off
    pub grid_on: bool,
    /// Snap on/off
    pub snap_on: bool,
    /// Snap style (isometric)
    pub snap_style: bool,
    /// Snap isometric pair
    pub snap_isopair: i16,
    /// Snap rotation angle
    pub snap_rotation: f64,
    /// Visual style / render mode (DXF code 281)
    pub render_mode: ViewportRenderMode,
    #[cfg_attr(feature = "serde", serde(default))]
    pub perspective: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub front_clipping: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub back_clipping: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub front_clip_at_eye: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucsicon_lower: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucsicon_origin: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub use_default_lights: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub default_lighting_type: i16,
    #[cfg_attr(feature = "serde", serde(default))]
    pub brightness: f64,
    #[cfg_attr(feature = "serde", serde(default))]
    pub contrast: f64,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ambient_color: Color,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucs_at_origin: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucs_per_viewport: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucs_origin: Vector3,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucs_x_axis: Vector3,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucs_y_axis: Vector3,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucs_elevation: f64,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucs_ortho_type: i16,
    #[cfg_attr(feature = "serde", serde(default))]
    pub grid_flags: GridFlags,
    #[cfg_attr(feature = "serde", serde(default))]
    pub grid_major: i16,
    #[cfg_attr(feature = "serde", serde(default))]
    pub xref_reference: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub xref_resolved: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub xref_dependent: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub xref_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub background_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub visual_style_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub sun_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub named_ucs_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub base_ucs_handle: Handle,
    /// DXF fields observed while reading this entry.  This is compatibility
    /// provenance only; it does not alter native VPORT defaults.
    #[doc(hidden)]
    #[cfg_attr(feature = "serde", serde(skip))]
    pub source_field_mask: u16,
}

impl VPort {
    /// Source group 71 (VIEWMODE) was present in a DXF record.
    pub(crate) const SOURCE_VIEW_MODE: u16 = 1 << 0;
    /// Source group 74 (UCSICON) was present in a DXF record.
    pub(crate) const SOURCE_UCS_ICON: u16 = 1 << 1;
    /// Source group 60 (grid flags) was present in a DXF record.
    pub(crate) const SOURCE_GRID_FLAGS: u16 = 1 << 2;
    /// Source group 72 (circle zoom percent) was present in a DXF record.
    pub(crate) const SOURCE_CIRCLE_ZOOM: u16 = 1 << 3;
    /// Source group 76 (show grid) was present in a DXF record.
    pub(crate) const SOURCE_SHOW_GRID: u16 = 1 << 4;

    /// Create a new viewport
    pub fn new(name: impl Into<String>) -> Self {
        VPort {
            handle: Handle::NULL,
            name: name.into(),
            lower_left: Vector2::ZERO,
            upper_right: Vector2::new(1.0, 1.0),
            view_center: Vector2::ZERO,
            snap_base: Vector2::ZERO,
            snap_spacing: Vector2::new(0.5, 0.5),
            grid_spacing: Vector2::new(10.0, 10.0),
            view_direction: Vector3::UNIT_Z,
            view_target: Vector3::ZERO,
            view_height: 10.0,
            aspect_ratio: 1.0,
            lens_length: 50.0,
            view_twist: 0.0,
            front_clip: 0.0,
            back_clip: 0.0,
            ucsfollow: false,
            circle_zoom: 100,
            fast_zoom: true,
            grid_on: false,
            snap_on: false,
            snap_style: false,
            snap_isopair: 0,
            snap_rotation: 0.0,
            render_mode: ViewportRenderMode::Wireframe2D,
            perspective: false,
            front_clipping: false,
            back_clipping: false,
            front_clip_at_eye: false,
            ucsicon_lower: true,
            ucsicon_origin: true,
            use_default_lights: true,
            default_lighting_type: 1,
            brightness: 0.0,
            contrast: 0.0,
            ambient_color: Color::from_index(250),
            ucs_at_origin: false,
            ucs_per_viewport: true,
            ucs_origin: Vector3::ZERO,
            ucs_x_axis: Vector3::UNIT_X,
            ucs_y_axis: Vector3::UNIT_Y,
            ucs_elevation: 0.0,
            ucs_ortho_type: 0,
            grid_flags: GridFlags::from_bits(2),
            grid_major: 5,
            xref_reference: false,
            xref_resolved: false,
            xref_dependent: false,
            xref_handle: Handle::NULL,
            background_handle: Handle::NULL,
            visual_style_handle: Handle::NULL,
            sun_handle: Handle::NULL,
            named_ucs_handle: Handle::NULL,
            base_ucs_handle: Handle::NULL,
            source_field_mask: 0,
        }
    }

    /// Create the standard "*Active" viewport
    pub fn active() -> Self {
        Self::new("*Active")
    }
}

// Source-presence metadata describes how a record was read and is not part of
// the native drawing value. Keep equality stable for callers comparing a
// native entry with a cloned/read entry.
impl PartialEq for VPort {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
            && self.name == other.name
            && self.lower_left == other.lower_left
            && self.upper_right == other.upper_right
            && self.view_center == other.view_center
            && self.snap_base == other.snap_base
            && self.snap_spacing == other.snap_spacing
            && self.grid_spacing == other.grid_spacing
            && self.view_direction == other.view_direction
            && self.view_target == other.view_target
            && self.view_height == other.view_height
            && self.aspect_ratio == other.aspect_ratio
            && self.lens_length == other.lens_length
            && self.view_twist == other.view_twist
            && self.front_clip == other.front_clip
            && self.back_clip == other.back_clip
            && self.ucsfollow == other.ucsfollow
            && self.circle_zoom == other.circle_zoom
            && self.fast_zoom == other.fast_zoom
            && self.grid_on == other.grid_on
            && self.snap_on == other.snap_on
            && self.snap_style == other.snap_style
            && self.snap_isopair == other.snap_isopair
            && self.snap_rotation == other.snap_rotation
            && self.render_mode == other.render_mode
            && self.perspective == other.perspective
            && self.front_clipping == other.front_clipping
            && self.back_clipping == other.back_clipping
            && self.front_clip_at_eye == other.front_clip_at_eye
            && self.ucsicon_lower == other.ucsicon_lower
            && self.ucsicon_origin == other.ucsicon_origin
            && self.use_default_lights == other.use_default_lights
            && self.default_lighting_type == other.default_lighting_type
            && self.brightness == other.brightness
            && self.contrast == other.contrast
            && self.ambient_color == other.ambient_color
            && self.ucs_at_origin == other.ucs_at_origin
            && self.ucs_per_viewport == other.ucs_per_viewport
            && self.ucs_origin == other.ucs_origin
            && self.ucs_x_axis == other.ucs_x_axis
            && self.ucs_y_axis == other.ucs_y_axis
            && self.ucs_elevation == other.ucs_elevation
            && self.ucs_ortho_type == other.ucs_ortho_type
            && self.grid_flags == other.grid_flags
            && self.grid_major == other.grid_major
            && self.xref_reference == other.xref_reference
            && self.xref_resolved == other.xref_resolved
            && self.xref_dependent == other.xref_dependent
            && self.xref_handle == other.xref_handle
            && self.background_handle == other.background_handle
            && self.visual_style_handle == other.visual_style_handle
            && self.sun_handle == other.sun_handle
            && self.named_ucs_handle == other.named_ucs_handle
            && self.base_ucs_handle == other.base_ucs_handle
    }
}

impl TableEntry for VPort {
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
        self.name == "*Active"
    }
}
