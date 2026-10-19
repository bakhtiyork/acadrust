//! View table entry

use super::TableEntry;
use crate::entities::ViewportRenderMode;
use crate::types::{Color, Handle, Vector3};

/// A view table entry
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct View {
    /// Unique handle
    pub handle: Handle,
    /// View name
    pub name: String,
    /// View center point
    pub center: Vector3,
    /// View height
    pub height: f64,
    /// View width
    pub width: f64,
    /// View direction (from target)
    pub direction: Vector3,
    /// View target point
    pub target: Vector3,
    /// Lens length
    pub lens_length: f64,
    /// Front clipping plane offset
    pub front_clip: f64,
    /// Back clipping plane offset
    pub back_clip: f64,
    /// Twist angle
    pub twist_angle: f64,
    /// Perspective projection flag (VIEWMODE bit 0). True for views created by
    /// the CAMERA command; used to draw the camera display glyph.
    pub perspective: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub front_clipping: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub back_clipping: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub front_clip_at_eye: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub render_mode: ViewportRenderMode,
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
    pub paper_space: bool,
    #[cfg_attr(feature = "serde", serde(default))]
    pub ucs_associated: bool,
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
    pub camera_plottable: bool,
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
    pub live_section_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub visual_style_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub sun_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub named_ucs_handle: Handle,
    #[cfg_attr(feature = "serde", serde(default))]
    pub base_ucs_handle: Handle,
    /// DXF fields observed while reading this entry.  This is compatibility
    /// provenance only: the native model keeps its constructor defaults, while
    /// the ACadSharp view can distinguish an omitted code from an explicit
    /// zero value.
    #[doc(hidden)]
    #[cfg_attr(feature = "serde", serde(skip))]
    pub source_field_mask: u16,
}

impl View {
    /// Source group 71 (VIEWMODE) was present in a DXF record.
    pub(crate) const SOURCE_VIEW_MODE: u16 = 1 << 0;
    /// UCS X-axis X coordinate (group 111) was present.
    pub(crate) const SOURCE_UCS_X_AXIS_X: u16 = 1 << 1;
    /// UCS X-axis Y coordinate (group 121) was present.
    pub(crate) const SOURCE_UCS_X_AXIS_Y: u16 = 1 << 2;
    /// UCS X-axis Z coordinate (group 131) was present.
    pub(crate) const SOURCE_UCS_X_AXIS_Z: u16 = 1 << 3;
    /// UCS Y-axis X coordinate (group 112) was present.
    pub(crate) const SOURCE_UCS_Y_AXIS_X: u16 = 1 << 4;
    /// UCS Y-axis Y coordinate (group 122) was present.
    pub(crate) const SOURCE_UCS_Y_AXIS_Y: u16 = 1 << 5;
    /// UCS Y-axis Z coordinate (group 132) was present.
    pub(crate) const SOURCE_UCS_Y_AXIS_Z: u16 = 1 << 6;

    /// Create a new view
    pub fn new(name: impl Into<String>) -> Self {
        View {
            handle: Handle::NULL,
            name: name.into(),
            center: Vector3::ZERO,
            height: 1.0,
            width: 1.0,
            direction: Vector3::UNIT_Z,
            target: Vector3::ZERO,
            lens_length: 50.0,
            front_clip: 0.0,
            back_clip: 0.0,
            twist_angle: 0.0,
            perspective: false,
            front_clipping: false,
            back_clipping: false,
            front_clip_at_eye: false,
            render_mode: ViewportRenderMode::Wireframe2D,
            use_default_lights: true,
            default_lighting_type: 1,
            brightness: 0.0,
            contrast: 0.0,
            ambient_color: Color::from_index(250),
            paper_space: false,
            ucs_associated: false,
            ucs_origin: Vector3::ZERO,
            ucs_x_axis: Vector3::UNIT_X,
            ucs_y_axis: Vector3::UNIT_Y,
            ucs_elevation: 0.0,
            ucs_ortho_type: 0,
            camera_plottable: false,
            xref_reference: false,
            xref_resolved: false,
            xref_dependent: false,
            xref_handle: Handle::NULL,
            background_handle: Handle::NULL,
            live_section_handle: Handle::NULL,
            visual_style_handle: Handle::NULL,
            sun_handle: Handle::NULL,
            named_ucs_handle: Handle::NULL,
            base_ucs_handle: Handle::NULL,
            source_field_mask: 0,
        }
    }
}

// Source-presence metadata describes how a record was read and is not part of
// the native drawing value. Keep equality stable for callers comparing a
// native entry with a cloned/read entry.
impl PartialEq for View {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
            && self.name == other.name
            && self.center == other.center
            && self.height == other.height
            && self.width == other.width
            && self.direction == other.direction
            && self.target == other.target
            && self.lens_length == other.lens_length
            && self.front_clip == other.front_clip
            && self.back_clip == other.back_clip
            && self.twist_angle == other.twist_angle
            && self.perspective == other.perspective
            && self.front_clipping == other.front_clipping
            && self.back_clipping == other.back_clipping
            && self.front_clip_at_eye == other.front_clip_at_eye
            && self.render_mode == other.render_mode
            && self.use_default_lights == other.use_default_lights
            && self.default_lighting_type == other.default_lighting_type
            && self.brightness == other.brightness
            && self.contrast == other.contrast
            && self.ambient_color == other.ambient_color
            && self.paper_space == other.paper_space
            && self.ucs_associated == other.ucs_associated
            && self.ucs_origin == other.ucs_origin
            && self.ucs_x_axis == other.ucs_x_axis
            && self.ucs_y_axis == other.ucs_y_axis
            && self.ucs_elevation == other.ucs_elevation
            && self.ucs_ortho_type == other.ucs_ortho_type
            && self.camera_plottable == other.camera_plottable
            && self.xref_reference == other.xref_reference
            && self.xref_resolved == other.xref_resolved
            && self.xref_dependent == other.xref_dependent
            && self.xref_handle == other.xref_handle
            && self.background_handle == other.background_handle
            && self.live_section_handle == other.live_section_handle
            && self.visual_style_handle == other.visual_style_handle
            && self.sun_handle == other.sun_handle
            && self.named_ucs_handle == other.named_ucs_handle
            && self.base_ucs_handle == other.base_ucs_handle
    }
}

impl TableEntry for View {
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
}
