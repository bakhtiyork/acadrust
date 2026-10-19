//! ACadSharp compatibility projections for geographic and spatial objects.
//!
//! These two objects have reader-specific quirks in the reference dumper.  The
//! typed model keeps the values found in the file, while this module applies
//! the small projection needed by the differential-dump contract.

use crate::objects::{GeoData, SpatialFilter};
use crate::types::{Handle, Matrix4};
use serde_json::Value;

use super::{bool_value, common, handle, int, matrix4, number, props, record, text, vec2, vec3};

const UNLISTED: i64 = -999;
const METERS: i32 = 6;

/// Emit a GeoData object using the values exposed by ACadSharp for `reader`.
pub(super) fn dump_geo_data(
    doc: &crate::CadDocument,
    handle_value: Handle,
    value: &GeoData,
    reader: &str,
) -> Value {
    let mut p = common(
        "GEODATA",
        UNLISTED,
        "AcDbGeoData",
        None,
        Some(&value.reactors),
        value.xdictionary_handle,
    );

    let dxf = reader == "dxf";
    let present = |code| {
        value
            .dxf_present_codes
            .as_ref()
            .is_none_or(|codes| codes.contains(&code))
    };
    // C# assigns the DWG vertical unit to HorizontalUnits and never sets
    // VerticalUnits in the R2010/R2013 branch.
    let (horizontal_units, vertical_units) = if reader == "dwg" && matches!(value.version, 2 | 3) {
        (value.vertical_units, METERS)
    } else if dxf {
        (
            if present(91) {
                value.horizontal_units
            } else {
                METERS
            },
            if present(92) {
                value.vertical_units
            } else {
                METERS
            },
        )
    } else {
        (value.horizontal_units, value.vertical_units)
    };
    // GeoData.cs maps DXF 41 to horizontal scale and 40 to vertical scale.
    // Its R2009 callback instead reads the horizontal scale from 46 and
    // leaves the vertical scale at its constructor default.
    let (horizontal_scale, vertical_scale) = if dxf && value.version == 1 {
        (value.horizontal_unit_scale, 1.0)
    } else if dxf {
        (value.vertical_unit_scale, value.horizontal_unit_scale)
    } else {
        (value.horizontal_unit_scale, value.vertical_unit_scale)
    };
    let mut reference = value.reference_point;
    if dxf && value.version == 1 {
        // C# handles 40/41/42, then still maps the obsolete 11/21/31 codes
        // onto ReferencePoint. Native data keeps the two points separate.
        if present(11) {
            reference.x = value.obsolete_observation_point.x;
        }
        if present(21) {
            reference.y = value.obsolete_observation_point.y;
        }
        if present(31) {
            reference.z = value.obsolete_observation_point.z;
        }
    }
    let mut north = value.north_direction;
    let mut up = value.up_direction;
    if dxf {
        if value.version != 1 || !present(52) {
            if !present(12) {
                north.x = 0.0;
            }
            if !present(22) {
                north.y = 1.0;
            }
        }
        if !present(210) {
            up.x = 0.0;
        }
        if !present(220) {
            up.y = 0.0;
        }
        if !present(230) {
            up.z = 1.0;
        }
    }
    let definition = if dxf {
        value
            .raw_dxf_definition
            .as_deref()
            .unwrap_or(&value.coordinate_system_definition)
    } else {
        &value.coordinate_system_definition
    };
    // The DXF callback never assigns CadGeoDataTemplate.HostBlockHandle.
    // DWG does, and the template resolves only a registered BlockRecord.
    let host = if !dxf
        && !value.host_block.is_null()
        && doc
            .block_records
            .iter()
            .any(|block| block.handle == value.host_block)
    {
        handle(value.host_block)
    } else {
        Value::Null
    };

    p.extend([
        (
            "CoordinateProjectionRadius",
            number(value.coordinate_projection_radius),
        ),
        ("CoordinateSystemDefinition", text(definition)),
        (
            "CoordinatesType",
            int(if dxf && !present(70) {
                1
            } else {
                value.coordinate_type as i64
            }),
        ),
        ("DesignPoint", vec3(value.design_point)),
        (
            "EnableSeaLevelCorrection",
            bool_value(value.sea_level_correction),
        ),
        (
            "Faces",
            Value::Array(
                value
                    .mesh_faces
                    .iter()
                    .map(|face| {
                        props(
                            "GeoData.GeoMeshFace",
                            [
                                ("Index1", int(face.first as i64)),
                                ("Index2", int(face.second as i64)),
                                ("Index3", int(face.third as i64)),
                            ],
                        )
                    })
                    .collect(),
            ),
        ),
        ("GeoRssTag", text(&value.geo_rss_tag)),
        ("HorizontalUnitScale", number(horizontal_scale)),
        ("HorizontalUnits", int(horizontal_units as i64)),
        ("HostBlock", host),
        ("NorthDirection", vec2(north)),
        (
            "ObservationCoverageTag",
            text(&value.observation_coverage_tag),
        ),
        ("ObservationFromTag", text(&value.observation_from_tag)),
        ("ObservationToTag", text(&value.observation_to_tag)),
        ("ReferencePoint", vec3(reference)),
        (
            "ScaleEstimationMethod",
            int(value.scale_estimation_method as i64),
        ),
        ("SeaLevelElevation", number(value.sea_level_elevation)),
        ("UpDirection", vec3(up)),
        (
            "UserSpecifiedScaleFactor",
            number(
                if (dxf && !present(141)) || (reader == "dwg" && value.version == 1) {
                    0.0
                } else {
                    value.user_scale_factor
                },
            ),
        ),
        ("Version", int(value.version as i64)),
        ("VerticalUnitScale", number(vertical_scale)),
        ("VerticalUnits", int(vertical_units as i64)),
        (
            "Points",
            Value::Array(
                value
                    .mesh_points
                    .iter()
                    .map(|point| {
                        props(
                            "GeoData.GeoMeshPoint",
                            [
                                ("Destination", vec2(point.destination)),
                                ("Source", vec2(point.source)),
                            ],
                        )
                    })
                    .collect(),
            ),
        ),
    ]);
    record(handle_value, "GeoData", value.owner, props("GeoData", p))
}

/// Emit a SpatialFilter using ACadSharp's DXF matrix parsing quirk.
pub(super) fn dump_spatial_filter(
    handle_value: Handle,
    value: &SpatialFilter,
    reader: &str,
) -> Value {
    let mut p = common(
        "SPATIAL_FILTER",
        UNLISTED,
        "AcDbSpatialFilter",
        None,
        None,
        None,
    );

    let (insert_transform, inverse_insert_transform) = if reader.eq_ignore_ascii_case("dxf") {
        // The reference DXF reader's InsertTransformRead condition is
        // inverted.  Each matrix therefore overwrites the inverse slot;
        // with the standard two matrices the second one is the clip-bound
        // transform and InsertTransform remains identity.
        (Matrix4::identity(), value.clip_bound_transform)
    } else {
        (value.clip_bound_transform, value.inverse_block_transform)
    };

    p.extend([
        ("BackDistance", number(value.back_clip.unwrap_or_default())),
        (
            "BoundaryPoints",
            Value::Array(value.boundary_points.iter().copied().map(vec2).collect()),
        ),
        ("ClipBackPlane", bool_value(value.back_clip.is_some())),
        ("ClipFrontPlane", bool_value(value.front_clip.is_some())),
        ("DisplayBoundary", bool_value(value.display_enabled)),
        (
            "FrontDistance",
            number(value.front_clip.unwrap_or_default()),
        ),
        ("InsertTransform", matrix4(&insert_transform)),
        ("InverseInsertTransform", matrix4(&inverse_insert_transform)),
        ("Normal", vec3(value.normal)),
        ("Origin", vec3(value.origin)),
    ]);
    record(
        handle_value,
        "SpatialFilter",
        value.owner,
        props("SpatialFilter", p),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objects::GeoData;
    use crate::types::{Handle, Matrix4, Vector2};

    fn matrix_with_translation(x: f64) -> Matrix4 {
        let mut value = Matrix4::identity();
        value.m[0][3] = x;
        value
    }

    fn property<'a>(record: &'a Value, name: &str) -> &'a Value {
        &record["props"][name]
    }

    #[test]
    fn dwg_geodata_projects_units_like_acadsharp() {
        let mut value = GeoData::new();
        value.version = 3;
        value.horizontal_units = 6;
        value.vertical_units = 1;

        let dump = dump_geo_data(&crate::CadDocument::new(), Handle::new(0x10), &value, "dwg");
        assert_eq!(property(&dump, "HorizontalUnits"), &Value::from(1));
        assert_eq!(property(&dump, "VerticalUnits"), &Value::from(6));

        let dxf_dump = dump_geo_data(&crate::CadDocument::new(), Handle::new(0x10), &value, "dxf");
        assert_eq!(property(&dxf_dump, "HorizontalUnits"), &Value::from(6));
        assert_eq!(property(&dxf_dump, "VerticalUnits"), &Value::from(1));
    }

    #[test]
    fn r2009_geodata_keeps_native_units_for_dwg_projection() {
        let mut value = GeoData::new();
        value.version = 1;
        value.horizontal_units = 2;
        value.vertical_units = 3;

        let dump = dump_geo_data(&crate::CadDocument::new(), Handle::new(0x10), &value, "dwg");
        assert_eq!(property(&dump, "HorizontalUnits"), &Value::from(2));
        assert_eq!(property(&dump, "VerticalUnits"), &Value::from(3));
    }

    #[test]
    fn dxf_spatial_filter_uses_identity_then_clip_matrix() {
        let mut value = SpatialFilter::new();
        value.boundary_points = vec![Vector2::new(1.0, 2.0)];
        value.inverse_block_transform = matrix_with_translation(3.0);
        value.clip_bound_transform = matrix_with_translation(7.0);

        let dump = dump_spatial_filter(Handle::new(0x20), &value, "dxf");
        assert_eq!(property(&dump, "InsertTransform")["M03"], Value::from(0.0));
        assert_eq!(
            property(&dump, "InverseInsertTransform")["M03"],
            Value::from(7.0)
        );

        let dwg_dump = dump_spatial_filter(Handle::new(0x20), &value, "dwg");
        assert_eq!(
            property(&dwg_dump, "InsertTransform")["M03"],
            Value::from(7.0)
        );
        assert_eq!(
            property(&dwg_dump, "InverseInsertTransform")["M03"],
            Value::from(3.0)
        );
    }
}
