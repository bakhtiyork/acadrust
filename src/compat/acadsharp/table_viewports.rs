// SPDX-License-Identifier: MPL-2.0
//! ACadSharp projections for VIEW and VPORT table records.
//!
//! The native table entries retain acadrust's values and defaults.  This
//! module contains only the source-aware values needed by the ACadSharp view:
//! packed VIEWMODE/UCSICON flags, omitted DXF defaults, and references that
//! ACadSharp's readers actually resolve.

use super::{handle, vec3, CadDocument};
use crate::objects::ObjectType;
use crate::tables::{VPort, View};
use crate::types::{DxfVersion, Handle, Vector3};
use serde_json::{json, Value as JsonValue};

fn source_dxf(doc: &CadDocument) -> bool {
    doc.dwg_source_version.is_none() && doc.header.dxf_source_variables.is_some()
}

fn source_dwg(doc: &CadDocument) -> bool {
    doc.dwg_source_version.is_some()
}

/// ACadSharp's `readXrefDependantBit` never emits the resolved (32) bit for
/// DWG table entries.  DXF's code 70 is assigned directly and retains it.
fn standard_flags(
    xref_dependent: bool,
    xref_resolved: bool,
    xref_reference: bool,
    source_dwg: bool,
) -> JsonValue {
    let resolved = !source_dwg && xref_resolved;
    json!(
        i16::from(xref_dependent) * 16 | i16::from(resolved) * 32 | i16::from(xref_reference) * 64
    )
}

fn view_mode(view: &View, source_dxf: bool) -> JsonValue {
    // VPORT has a non-zero ACadSharp constructor default; VIEW's enum default
    // is zero.  Only a DXF record can omit a group that the native parser
    // represents with booleans, because DWG always carries these bits.
    if source_dxf && view.source_field_mask & View::SOURCE_VIEW_MODE == 0 {
        return json!(0);
    }
    json!(
        i16::from(view.perspective)
            | i16::from(view.front_clipping) * 2
            | i16::from(view.back_clipping) * 4
            | i16::from(view.front_clip_at_eye) * 16
    )
}

fn vport_view_mode(vport: &VPort, source_dxf: bool, source_dwg: bool) -> JsonValue {
    if source_dxf && vport.source_field_mask & VPort::SOURCE_VIEW_MODE == 0 {
        // ACadSharp.VPort initializes ViewModeType.FrontClippingZ.
        return json!(16);
    }
    if source_dwg {
        // The DWG reader starts with FrontClippingZ and ORs the encoded bits.
        // Bit 4 therefore remains set even when all four encoded bits are 0.
        return json!(
            16 | i16::from(vport.perspective)
                | i16::from(vport.front_clipping) * 2
                | i16::from(vport.back_clipping) * 4
                | i16::from(vport.ucsfollow) * 8
        );
    }
    json!(
        i16::from(vport.perspective)
            | i16::from(vport.front_clipping) * 2
            | i16::from(vport.back_clipping) * 4
            | i16::from(vport.ucsfollow) * 8
            | i16::from(vport.front_clip_at_eye) * 16
    )
}

fn source_axis(axis: Vector3, mask: u16, x_bit: u16, y_bit: u16, z_bit: u16) -> Vector3 {
    Vector3::new(
        if mask & x_bit != 0 { axis.x } else { 0.0 },
        if mask & y_bit != 0 { axis.y } else { 0.0 },
        if mask & z_bit != 0 { axis.z } else { 0.0 },
    )
}

fn view_axes(view: &View, source_dxf: bool, source_dwg: bool) -> (Vector3, Vector3) {
    if !source_dxf && !source_dwg {
        return (view.ucs_x_axis, view.ucs_y_axis);
    }
    if source_dwg {
        // ACadSharp only reads the UCS axes when group 72 is set.  Its View
        // constructor leaves both XYZ values at zero otherwise.
        if !view.ucs_associated {
            return (Vector3::ZERO, Vector3::ZERO);
        }
    } else if source_dxf {
        let x = source_axis(
            view.ucs_x_axis,
            view.source_field_mask,
            View::SOURCE_UCS_X_AXIS_X,
            View::SOURCE_UCS_X_AXIS_Y,
            View::SOURCE_UCS_X_AXIS_Z,
        );
        let y = source_axis(
            view.ucs_y_axis,
            view.source_field_mask,
            View::SOURCE_UCS_Y_AXIS_X,
            View::SOURCE_UCS_Y_AXIS_Y,
            View::SOURCE_UCS_Y_AXIS_Z,
        );
        return (x, y);
    }
    (view.ucs_x_axis, view.ucs_y_axis)
}

fn vport_grid_flags(doc: &CadDocument, vport: &VPort, source_dxf: bool, source_dwg: bool) -> i16 {
    if (source_dxf && vport.source_field_mask & VPort::SOURCE_GRID_FLAGS == 0)
        || (source_dwg && doc.version < DxfVersion::AC1021)
    {
        // ACadSharp.VPort initializes GridFlags to _1 | _2.  Older DWG
        // records do not carry group 60 and therefore keep that default.
        3
    } else {
        vport.grid_flags.to_bits()
    }
}

fn vport_circle_zoom(vport: &VPort, source_dxf: bool) -> i16 {
    if source_dxf && vport.source_field_mask & VPort::SOURCE_CIRCLE_ZOOM == 0 {
        // ACadSharp.VPort initializes CircleZoomPercent to 1000.
        1000
    } else {
        vport.circle_zoom
    }
}

fn vport_show_grid(vport: &VPort, source_dxf: bool) -> bool {
    if source_dxf && vport.source_field_mask & VPort::SOURCE_SHOW_GRID == 0 {
        // ACadSharp.VPort initializes ShowGrid to true.
        true
    } else {
        vport.grid_on
    }
}

fn visual_style(doc: &CadDocument, value: Handle, source: bool) -> JsonValue {
    // CadViewTemplate stores code 348 in a side template but never assigns it
    // in build(); CadVPortTemplate has the same empty style branch.  Therefore
    // source-reader compatibility views expose null even when native parsing
    // retains the raw handle.  For constructed documents, expose only a
    // registered VisualStyle object, matching a resolved object reference.
    if source || value.is_null() {
        return JsonValue::Null;
    }
    if matches!(doc.objects.get(&value), Some(ObjectType::VisualStyle(_))) {
        handle(value)
    } else {
        JsonValue::Null
    }
}

/// Values to append after the ordinary VIEW mapping.  Duplicate keys are
/// intentional: `props` keeps the last value, allowing this source-aware
/// adapter to override the native projection without changing it.
pub(super) fn view_overrides(doc: &CadDocument, view: &View) -> Vec<(&'static str, JsonValue)> {
    let source_dxf = source_dxf(doc);
    let source_dwg = source_dwg(doc);
    let source = source_dxf || source_dwg;
    let (x_axis, y_axis) = view_axes(view, source_dxf, source_dwg);
    vec![
        (
            "Flags",
            standard_flags(
                view.xref_dependent,
                view.xref_resolved,
                view.xref_reference,
                source_dwg,
            ),
        ),
        ("UcsXAxis", vec3(x_axis)),
        ("UcsYAxis", vec3(y_axis)),
        ("ViewMode", view_mode(view, source_dxf)),
        (
            "VisualStyle",
            visual_style(doc, view.visual_style_handle, source),
        ),
    ]
}

/// Values to append after the ordinary VPORT mapping.  Duplicate keys are
/// intentional; `props` keeps the last value.
pub(super) fn vport_overrides(doc: &CadDocument, vport: &VPort) -> Vec<(&'static str, JsonValue)> {
    let source_dxf = source_dxf(doc);
    let source_dwg = source_dwg(doc);
    let source = source_dxf || source_dwg;
    let icon = if source_dwg {
        // The DWG reader assigns OnLower for bit 0, then OnOrigin (3, not the
        // OffOrigin value 2) for bit 1; with neither bit the constructor
        // default OnOrigin remains.
        if vport.ucsicon_origin || !vport.ucsicon_lower {
            3
        } else {
            1
        }
    } else {
        i16::from(vport.ucsicon_lower) | i16::from(vport.ucsicon_origin) * 2
    };
    vec![
        (
            "Flags",
            standard_flags(
                vport.xref_dependent,
                vport.xref_resolved,
                vport.xref_reference,
                source_dwg,
            ),
        ),
        (
            "CircleZoomPercent",
            json!(vport_circle_zoom(vport, source_dxf)),
        ),
        (
            "GridFlags",
            json!(vport_grid_flags(doc, vport, source_dxf, source_dwg)),
        ),
        ("ShowGrid", json!(vport_show_grid(vport, source_dxf))),
        ("UcsIconDisplay", json!(icon)),
        ("ViewMode", vport_view_mode(vport, source_dxf, source_dwg)),
        (
            "VisualStyle",
            visual_style(doc, vport.visual_style_handle, source),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::VisualStyle;

    #[test]
    fn packed_modes_and_icon_preserve_explicit_zeroes() {
        let mut doc = CadDocument::new();
        doc.header.source_handle_seed = Some(0x100);

        let mut view = View::new("view");
        view.source_field_mask = View::SOURCE_VIEW_MODE;
        doc.header.dxf_source_variables = Some(Default::default());
        let view_values = view_overrides(&doc, &view);
        assert_eq!(
            view_values
                .iter()
                .find(|(k, _)| *k == "ViewMode")
                .unwrap()
                .1,
            0
        );

        let mut vport = VPort::new("vp");
        vport.source_field_mask = VPort::SOURCE_VIEW_MODE | VPort::SOURCE_UCS_ICON;
        vport.perspective = false;
        vport.front_clipping = false;
        vport.back_clipping = false;
        vport.ucsfollow = false;
        vport.front_clip_at_eye = false;
        vport.ucsicon_lower = false;
        vport.ucsicon_origin = false;
        let values = vport_overrides(&doc, &vport);
        assert_eq!(values.iter().find(|(k, _)| *k == "ViewMode").unwrap().1, 0);
        assert_eq!(
            values
                .iter()
                .find(|(k, _)| *k == "UcsIconDisplay")
                .unwrap()
                .1,
            0
        );
    }

    #[test]
    fn dwg_icon_bits_map_to_the_reader_enum() {
        let mut doc = CadDocument::new();
        doc.header.source_handle_seed = Some(0x100);
        doc.dwg_source_version = Some(crate::types::DxfVersion::AC1015);
        let icon = |lower: bool, origin: bool| {
            let mut vport = VPort::new("vp");
            vport.ucsicon_lower = lower;
            vport.ucsicon_origin = origin;
            vport_overrides(&doc, &vport)
                .iter()
                .find(|(k, _)| *k == "UcsIconDisplay")
                .unwrap()
                .1
                .clone()
        };
        // Bit 1 assigns OnOrigin (3), not the OffOrigin value 2.
        assert_eq!(icon(false, true), 3);
        assert_eq!(icon(true, true), 3);
        assert_eq!(icon(true, false), 1);
        assert_eq!(icon(false, false), 3);
    }

    #[test]
    fn omitted_source_defaults_are_projected_without_mutating_native_values() {
        let mut doc = CadDocument::new();
        doc.header.source_handle_seed = Some(0x100);
        let vport = VPort::new("vp");
        assert_eq!(vport.grid_flags.to_bits(), 2);
        doc.header.dxf_source_variables = Some(Default::default());
        let values = vport_overrides(&doc, &vport);
        assert_eq!(values.iter().find(|(k, _)| *k == "GridFlags").unwrap().1, 3);
        assert_eq!(values.iter().find(|(k, _)| *k == "ViewMode").unwrap().1, 16);
        assert_eq!(
            values
                .iter()
                .find(|(k, _)| *k == "CircleZoomPercent")
                .unwrap()
                .1,
            1000
        );
        assert_eq!(
            values.iter().find(|(k, _)| *k == "ShowGrid").unwrap().1,
            true
        );
    }

    #[test]
    fn source_visual_style_is_null_but_registered_constructed_style_is_kept() {
        let mut doc = CadDocument::new();
        let mut style = VisualStyle::new();
        style.handle = Handle::new(0x123);
        doc.objects
            .insert(style.handle, ObjectType::VisualStyle(style));
        let view = View::new("view");
        assert_eq!(
            view_overrides(&doc, &view)
                .iter()
                .find(|(k, _)| *k == "VisualStyle")
                .unwrap()
                .1,
            JsonValue::Null
        );
        let mut view = view;
        view.visual_style_handle = Handle::new(0x123);
        assert_eq!(
            view_overrides(&doc, &view)
                .iter()
                .find(|(k, _)| *k == "VisualStyle")
                .unwrap()
                .1,
            json!("#123")
        );
    }

    #[test]
    fn dwg_resolved_xref_bit_is_not_emitted() {
        let mut doc = CadDocument::new();
        doc.dwg_source_version = Some(DxfVersion::AC1021);
        let mut view = View::new("view");
        view.xref_dependent = true;
        view.xref_resolved = true;
        view.xref_reference = true;
        let values = view_overrides(&doc, &view);
        assert_eq!(values.iter().find(|(k, _)| *k == "Flags").unwrap().1, 80);
    }
}
