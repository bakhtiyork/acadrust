use super::*;

// Defaults below follow CadHeader and its unattached DimensionStyle override.
// A missing source variable is different from an explicitly stored zero/false.
fn source_missing(doc: &CadDocument, variable: &str, first: DxfVersion, last: DxfVersion) -> bool {
    if let Some(vars) = &doc.header.dxf_source_variables {
        !vars.contains(variable)
    } else if let Some(version) = doc.dwg_source_version {
        version < first || version > last
    } else {
        false
    }
}

fn apply_defaults(
    h: &mut crate::document::HeaderVariables,
    missing: impl Fn(&str, DxfVersion, DxfVersion) -> bool,
) {
    use DxfVersion::*;
    macro_rules! default {
        ($field:ident, $var:literal, $first:ident, $last:ident, $value:expr) => {
            if missing($var, $first, $last) {
                h.$field = $value;
            }
        };
        ($field:ident, $var:literal, $first:ident, $value:expr) => {
            default!($field, $var, $first, AC1032, $value);
        };
    }
    default!(angle_direction, "$ANGDIR", AC1012, 1);
    default!(multiline_scale, "$CMLSCALE", AC1012, 20.0);
    default!(dim_alt_decimal_places, "$DIMALTD", AC1012, 3);
    default!(dim_alt_tolerance_decimal_places, "$DIMALTTD", AC1012, 3);
    default!(dim_fit, "$DIMFIT", AC1012, AC1014, 0);
    default!(dim_decimal_places, "$DIMDEC", AC1012, 2);
    default!(dim_tolerance_decimal_places, "$DIMTDEC", AC1012, 2);
    default!(dim_tolerance_justification, "$DIMTOLJ", AC1012, 0);
    default!(dim_tolerance_zero_suppression, "$DIMTZIN", AC1012, 8);
    default!(dim_zero_suppression, "$DIMZIN", AC1012, 8);
    default!(dim_ext_line_extension, "$DIMEXE", AC1012, 1.25);
    default!(dim_ext_line_offset, "$DIMEXO", AC1012, 0.625);
    default!(dim_line_gap, "$DIMGAP", AC1012, 0.625);
    default!(dim_line_increment, "$DIMDLI", AC1012, 3.75);
    default!(dim_text_inside_horizontal, "$DIMTIH", AC1012, false);
    default!(dim_text_outside_horizontal, "$DIMTOH", AC1012, false);
    default!(dim_text_above, "$DIMTAD", AC1012, 1);
    default!(light_glyph_display, "$LIGHTGLYPHDISPLAY", AC1021, 0);
    default!(display_silhouette, "$DISPSILH", AC1012, false);
    default!(loft_angle1, "$LOFTANG1", AC1021, 0.0);
    default!(loft_angle2, "$LOFTANG2", AC1021, 0.0);
    default!(dwf_3d_precision, "$3DDWFPREC", AC1021, 0.0);
    default!(dwf_frame, "$DWFFRAME", AC1021, 0);
    default!(sort_entities, "$SORTENTS", AC1018, 0);
    default!(hide_text, "$HIDETEXT", AC1018, 0);
    default!(lens_length, "$LENSLENGTH", AC1021, 0.0);
    default!(loft_normals, "$LOFTNORMALS", AC1021, 0);
    default!(loft_param, "$LOFTPARAM", AC1021, 0);
    default!(polysolid_height, "$PSOLHEIGHT", AC1021, 0.0);
    default!(polysolid_width, "$PSOLWIDTH", AC1021, 0.0);
    default!(timezone, "$TIMEZONE", AC1021, 0);
    default!(show_solid_history, "$SHOWHIST", AC1021, 0);
    default!(plotstyle_mode, "$PSTYLEMODE", AC1015, false);
    default!(xedit, "$XEDIT", AC1015, false);
    default!(object_snap_mode, "$OSMODE", AC1012, AC1014, 4133);
    default!(sketch_increment, "$SKETCHINC", AC1012, 1.0);
    default!(trace_width, "$TRACEWID", AC1012, 0.0);
    default!(user_timer, "$USRTIMER", AC1012, false);
    default!(menu_name, "$MENU", AC1012, ".".into());
    default!(
        last_saved_by,
        "$LASTSAVEDBY",
        Unknown,
        Unknown,
        "ACadSharp".into()
    );
    if missing("$MEASUREMENT", AC1018, AC1032) {
        h.measurement = 1;
    }
    if missing("$OBSCUREDCOLOR", AC1018, AC1032) {
        h.obscured_color = 0;
    }
    default!(model_space_extents_min, "$EXTMIN", AC1012, Vector3::ZERO);
    default!(model_space_extents_max, "$EXTMAX", AC1012, Vector3::ZERO);
    default!(paper_space_extents_min, "$PEXTMIN", AC1012, Vector3::ZERO);
    default!(paper_space_extents_max, "$PEXTMAX", AC1012, Vector3::ZERO);
}

pub(super) fn new_header() -> crate::document::HeaderVariables {
    let mut h = crate::document::HeaderVariables::default();
    apply_defaults(&mut h, |_, _, _| true);
    h
}

fn maintenance(doc: &CadDocument) -> i32 {
    if doc.header.maintenance_version != 0
        || doc
            .header
            .dxf_source_variables
            .as_ref()
            .is_some_and(|v| v.contains("$ACADMAINTVER"))
    {
        return doc.header.maintenance_version;
    }
    match doc.version {
        DxfVersion::AC1015 => 20,
        DxfVersion::AC1018 => 104,
        DxfVersion::AC1021 => 50,
        DxfVersion::AC1024 => 226,
        DxfVersion::AC1027 => 125,
        DxfVersion::AC1032 => 228,
        _ => 0,
    }
}

// DwgStreamReaderBase.julianToDate followed by CadUtils.ToJulianCalendar:
// the latter uses civil midnight as the integer boundary, adding half a day.
fn dwg_date(day: i32, milliseconds: i32) -> f64 {
    let date = if (1_721_426..=5_373_484).contains(&day) {
        day as f64 + 0.5
    } else {
        1_721_426.0
    };
    date + milliseconds.max(0) as f64 / 86_400_000.0
}

fn editing_time(value: f64) -> f64 {
    // CadUtils.EditingTime truncates each component, including milliseconds.
    let days = value.trunc();
    let mut fraction = value - days;
    let hours = (fraction * 24.0).trunc();
    fraction -= hours / 24.0;
    let minutes = (fraction * 1440.0).trunc();
    fraction -= minutes / 1440.0;
    let seconds = (fraction * 86400.0).trunc();
    let milliseconds = ((fraction * 86400.0 - seconds) * 1000.0).trunc();
    days + (hours * 3_600_000.0 + minutes * 60_000.0 + seconds * 1000.0 + milliseconds)
        / 86_400_000.0
}

pub(super) fn header(doc: &CadDocument) -> Value {
    let mut projected = doc.header.clone();
    apply_defaults(&mut projected, |var, first, last| {
        source_missing(doc, var, first, last)
    });
    resolve_references(doc, &mut projected);
    // DwgHeaderReader's R2000+ block assigns the DIMSD1/DIMSD2 bits to the
    // extension-line properties, overwriting DIMSE1/DIMSE2.
    if doc.dwg_source_version.is_some_and(|version| version >= DxfVersion::AC1015) {
        projected.dim_suppress_ext1 = projected.dim_suppress_line1;
        projected.dim_suppress_ext2 = projected.dim_suppress_line2;
    }
    let h = &projected;
    let mut value = props(
        "CadHeader",
        [
            (
                "CreateEllipseAsPolyline",
                json!(h.create_ellipse_as_polyline),
            ),
            ("DimensionAltMzf", number(h.dim_alt_mzf)),
            ("DimensionAltMzs", json!(h.dim_alt_mzs)),
            (
                "DimensionArcLengthSymbolPosition",
                json!(h.dim_arc_length_symbol_position),
            ),
            ("DimensionFit", json!(h.dim_fit)),
            (
                "DimensionIsExtensionLineLengthFixed",
                json!(h.dim_extension_line_length_fixed),
            ),
            ("DimensionLineType", json!(h.dim_linetype_name)),
            ("DimensionMzf", number(h.dim_mzf)),
            ("DimensionMzs", json!(h.dim_mzs)),
            ("DimensionTex1", json!(h.dim_linetype1_name)),
            ("DimensionTex2", json!(h.dim_linetype2_name)),
            ("DimensionTextDirection", json!(h.dim_text_direction)),
            ("DimensionTextStyleName", json!(h.dim_text_style_name)),
            ("DimensionUnit", json!(h.dim_unit)),
            ("DimensionstyleOverrides", handle(Handle::NULL)),
            ("HideText", json!(h.hide_text)),
            ("IntersectionDisplay", json!(h.intersection_display)),
            ("LastSavedBy", json!(h.last_saved_by)),
            ("ObscuredType", json!(h.obscured_linetype)),
            ("PaperSpaceBaseName", json!(h.paper_space_ucs_base_name)),
            ("PaperSpaceName", json!(h.paper_space_ucs_name)),
            ("RequiredVersions", json!(h.required_versions)),
            ("ShadowMode", json!(h.shadow_mode)),
            ("ShowSolidsHistory", json!(h.show_solid_history)),
            ("SketchPolylines", json!(h.sketch_type != 0)),
            (
                "SolidsRetainHistory",
                json!(i16::from(h.record_solid_history)),
            ),
            ("StackedTextAlignment", json!(h.stacked_text_alignment)),
            (
                "StackedTextSizePercentage",
                json!(h.stacked_text_size_percentage),
            ),
            ("SurfaceIsolineCount", json!(h.isolines)),
            ("TextQuality", json!(h.text_quality)),
            ("UnitMode", json!(h.unit_mode)),
            (
                "ModelSpaceOrthographicBackDOrigin",
                vec3(h.model_space_orthographic_back_origin),
            ),
            (
                "ModelSpaceOrthographicBottomDOrigin",
                vec3(h.model_space_orthographic_bottom_origin),
            ),
            (
                "ModelSpaceOrthographicFrontDOrigin",
                vec3(h.model_space_orthographic_front_origin),
            ),
            (
                "ModelSpaceOrthographicLeftDOrigin",
                vec3(h.model_space_orthographic_left_origin),
            ),
            (
                "ModelSpaceOrthographicRightDOrigin",
                vec3(h.model_space_orthographic_right_origin),
            ),
            (
                "ModelSpaceOrthographicTopDOrigin",
                vec3(h.model_space_orthographic_top_origin),
            ),
            ("ModelSpaceUcs", handle(Handle::NULL)),
            ("ModelSpaceUcsBase", handle(Handle::NULL)),
            (
                "PaperSpaceOrthographicBackDOrigin",
                vec3(h.paper_space_orthographic_back_origin),
            ),
            (
                "PaperSpaceOrthographicBottomDOrigin",
                vec3(h.paper_space_orthographic_bottom_origin),
            ),
            (
                "PaperSpaceOrthographicFrontDOrigin",
                vec3(h.paper_space_orthographic_front_origin),
            ),
            (
                "PaperSpaceOrthographicLeftDOrigin",
                vec3(h.paper_space_orthographic_left_origin),
            ),
            (
                "PaperSpaceOrthographicRightDOrigin",
                vec3(h.paper_space_orthographic_right_origin),
            ),
            (
                "PaperSpaceOrthographicTopDOrigin",
                vec3(h.paper_space_orthographic_top_origin),
            ),
            ("PaperSpaceUcs", handle(Handle::NULL)),
            ("PaperSpaceUcsBase", handle(Handle::NULL)),
            ("AngleBase", json!(number(h.angle_base))),
            ("AngularDirection", json!(h.angle_direction)),
            ("AngularUnit", json!(h.angular_unit_format)),
            ("AngularUnitPrecision", json!(h.angular_unit_precision)),
            ("ArrowBlockName", json!(h.dim_leader_arrow_block)),
            ("AssociatedDimensions", json!(h.associate_dimensions)),
            ("AttributeVisibility", json!(h.attribute_visibility)),
            ("BlipMode", json!(h.blip_mode)),
            ("CameraDisplayObjects", json!(h.camera_display)),
            ("CameraHeight", json!(number(h.camera_height))),
            ("ChamferAngle", json!(number(h.chamfer_angle))),
            ("ChamferDistance1", json!(number(h.chamfer_distance_a))),
            ("ChamferDistance2", json!(number(h.chamfer_distance_b))),
            ("ChamferLength", json!(number(h.chamfer_length))),
            ("CodePage", json!(h.code_page)),
            ("CreateDateTime", json!(number(h.create_date_julian))),
            (
                "CurrentDimensionStyle",
                json!(handle(h.current_dimstyle_handle)),
            ),
            ("CurrentDimensionStyleName", json!(h.current_dimstyle_name)),
            ("CurrentEntityColor", json!(color(h.current_entity_color))),
            ("CurrentEntityLineWeight", json!(h.current_line_weight)),
            (
                "CurrentEntityLinetypeScale",
                json!(number(h.current_entity_linetype_scale)),
            ),
            ("CurrentEntityPlotStyle", json!(h.current_plotstyle_type)),
            ("CurrentLayer", json!(handle(h.current_layer_handle))),
            ("CurrentLayerName", json!(h.current_layer_name)),
            ("CurrentLineType", json!(handle(h.current_linetype_handle))),
            ("CurrentLineTypeName", json!(h.current_linetype_name)),
            ("CurrentMLineStyleName", json!(h.multiline_style)),
            (
                "CurrentMultiLineJustification",
                json!(h.multiline_justification),
            ),
            ("CurrentMultilineScale", json!(number(h.multiline_scale))),
            (
                "CurrentTextStyle",
                json!(handle(h.current_text_style_handle)),
            ),
            ("CurrentTextStyleName", json!(h.current_text_style_name)),
            (
                "DimensionAlternateDimensioningSuffix",
                json!(h.dim_alt_post),
            ),
            (
                "DimensionAlternateUnitDecimalPlaces",
                json!(h.dim_alt_decimal_places),
            ),
            (
                "DimensionAlternateUnitDimensioning",
                json!(h.dim_alternate_units),
            ),
            (
                "DimensionAlternateUnitFormat",
                json!(h.dim_alt_units_format),
            ),
            (
                "DimensionAlternateUnitRounding",
                json!(number(h.dim_alt_rounding)),
            ),
            (
                "DimensionAlternateUnitScaleFactor",
                json!(number(h.dim_alt_scale)),
            ),
            (
                "DimensionAlternateUnitToleranceDecimalPlaces",
                json!(h.dim_alt_tolerance_decimal_places),
            ),
            (
                "DimensionAlternateUnitToleranceZeroHandling",
                json!(h.dim_alt_tolerance_zero_tight),
            ),
            (
                "DimensionAlternateUnitZeroHandling",
                json!(h.dim_alt_tolerance_zero_suppression),
            ),
            (
                "DimensionAngularDimensionDecimalPlaces",
                json!(h.dim_angular_decimal_places),
            ),
            ("DimensionAngularUnit", json!(h.dim_angular_units)),
            (
                "DimensionAngularZeroHandling",
                json!(h.dim_alt_zero_suppression),
            ),
            ("DimensionArrowSize", json!(number(h.dim_arrow_size))),
            ("DimensionAssociativity", json!(h.dimension_associativity)),
            ("DimensionBlockName", json!(h.dim_arrow_block)),
            ("DimensionBlockNameFirst", json!(h.dim_arrow_block1)),
            ("DimensionBlockNameSecond", json!(h.dim_arrow_block2)),
            ("DimensionCenterMarkSize", json!(number(h.dim_center_mark))),
            ("DimensionCursorUpdate", json!(h.dim_user_positioned_text)),
            ("DimensionDecimalPlaces", json!(h.dim_decimal_places)),
            (
                "DimensionDecimalSeparator",
                character(h.dim_decimal_separator),
            ),
            (
                "DimensionDimensionTextArrowFit",
                json!(h.dim_text_arrow_fit),
            ),
            (
                "DimensionExtensionLineColor",
                json!(color(h.dim_ext_line_color)),
            ),
            (
                "DimensionExtensionLineExtension",
                json!(number(h.dim_ext_line_extension)),
            ),
            (
                "DimensionExtensionLineOffset",
                json!(number(h.dim_ext_line_offset)),
            ),
            (
                "DimensionFixedExtensionLineLength",
                json!(number(h.dim_fixed_ext_line_length)),
            ),
            ("DimensionFractionFormat", json!(h.dim_fraction_format)),
            ("DimensionGenerateTolerances", json!(h.dim_tolerance)),
            (
                "DimensionJoggedRadiusDimensionTransverseSegmentAngle",
                json!(number(h.dim_jog_angle)),
            ),
            ("DimensionLimitsGeneration", json!(h.dim_limits)),
            ("DimensionLineColor", json!(color(h.dim_line_color))),
            (
                "DimensionLineExtension",
                json!(number(h.dim_line_extension)),
            ),
            ("DimensionLineGap", json!(number(h.dim_line_gap))),
            (
                "DimensionLineIncrement",
                json!(number(h.dim_line_increment)),
            ),
            ("DimensionLineWeight", json!(h.dim_line_weight)),
            (
                "DimensionLinearScaleFactor",
                json!(number(h.dim_linear_scale)),
            ),
            ("DimensionLinearUnitFormat", json!(h.dim_linear_unit_format)),
            (
                "DimensionMinusTolerance",
                json!(number(h.dim_tolerance_minus)),
            ),
            (
                "DimensionPlusTolerance",
                json!(number(h.dim_tolerance_plus)),
            ),
            ("DimensionPostFix", json!(h.dim_post)),
            ("DimensionRounding", json!(number(h.dim_rounding))),
            ("DimensionScaleFactor", json!(number(h.dim_scale))),
            ("DimensionSeparateArrowBlocks", json!(h.dim_separate_arrows)),
            (
                "DimensionSuppressFirstDimensionLine",
                json!(h.dim_suppress_line1),
            ),
            (
                "DimensionSuppressFirstExtensionLine",
                json!(h.dim_suppress_ext1),
            ),
            (
                "DimensionSuppressOutsideExtensions",
                json!(h.dim_suppress_outside_ext),
            ),
            (
                "DimensionSuppressSecondDimensionLine",
                json!(h.dim_suppress_line2),
            ),
            (
                "DimensionSuppressSecondExtensionLine",
                json!(h.dim_suppress_ext2),
            ),
            ("DimensionTextBackgroundColor", color(h.dim_text_fill_color)),
            ("DimensionTextBackgroundFillMode", json!(h.dim_text_fill)),
            ("DimensionTextColor", json!(color(h.dim_text_color))),
            ("DimensionTextHeight", json!(number(h.dim_text_height))),
            (
                "DimensionTextHorizontalAlignment",
                json!(h.dim_horizontal_justification),
            ),
            (
                "DimensionTextInsideExtensions",
                json!(h.dim_force_text_inside),
            ),
            (
                "DimensionTextInsideHorizontal",
                json!(h.dim_text_inside_horizontal),
            ),
            ("DimensionTextMovement", json!(h.dim_text_movement)),
            (
                "DimensionTextOutsideExtensions",
                json!(h.dim_force_line_inside),
            ),
            (
                "DimensionTextOutsideHorizontal",
                json!(h.dim_text_outside_horizontal),
            ),
            ("DimensionTextStyle", json!(handle(h.dim_text_style_handle))),
            ("DimensionTextVerticalAlignment", json!(h.dim_text_above)),
            (
                "DimensionTextVerticalPosition",
                json!(number(h.dim_text_vertical_pos)),
            ),
            ("DimensionTickSize", json!(number(h.dim_tick_size))),
            (
                "DimensionToleranceAlignment",
                json!(h.dim_tolerance_justification),
            ),
            (
                "DimensionToleranceDecimalPlaces",
                json!(h.dim_tolerance_decimal_places),
            ),
            (
                "DimensionToleranceScaleFactor",
                json!(number(h.dim_tolerance_scale)),
            ),
            (
                "DimensionToleranceZeroHandling",
                json!(h.dim_tolerance_zero_suppression),
            ),
            ("DimensionZeroHandling", json!(h.dim_zero_suppression)),
            ("DisplayLightGlyphs", json!(h.light_glyph_display)),
            ("DisplayLineWeight", json!(h.lineweight_display)),
            ("DisplaySilhouetteCurves", json!(h.display_silhouette)),
            ("DraftAngleFirstCrossSection", json!(number(h.loft_angle1))),
            ("DraftAngleSecondCrossSection", json!(number(h.loft_angle2))),
            (
                "DraftMagnitudeFirstCrossSection",
                json!(number(h.loft_magnitude1)),
            ),
            (
                "DraftMagnitudeSecondCrossSection",
                json!(number(h.loft_magnitude2)),
            ),
            ("Dw3DPrecision", number(h.dwf_3d_precision)),
            ("DgnUnderlayFramesVisibility", json!(h.dgn_frame)),
            ("DwgUnderlayFramesVisibility", json!(h.dwf_frame)),
            ("ObscuredColor", color(Color::from_index(h.obscured_color))),
            ("Elevation", json!(number(h.elevation))),
            ("EndCaps", json!(h.end_caps)),
            ("EntitySortingFlags", json!(h.sort_entities)),
            ("ExtendedNames", json!(h.extended_names)),
            ("ExtensionLineWeight", json!(h.dim_ext_line_weight)),
            (
                "ExternalReferenceClippingBoundaryType",
                json!(h.xclip_frame),
            ),
            ("FacetResolution", json!(number(h.facet_resolution))),
            ("FillMode", json!(h.fill_mode)),
            ("FilletRadius", json!(number(h.fillet_radius))),
            ("FingerPrintGuid", json!(h.fingerprint_guid)),
            ("HaloGapPercentage", json!(h.halo_gap)),
            ("HandleSeed", json!(h.handle_seed)),
            ("HyperLinkBase", json!(h.hyperlink_base)),
            ("IndexCreationFlags", json!(h.index_control)),
            ("InsUnits", json!(h.insertion_units)),
            ("InterfereColor", json!(color(h.interference_color))),
            ("JoinStyle", json!(h.join_style)),
            ("Latitude", json!(number(h.latitude))),
            ("LensLength", json!(number(h.lens_length))),
            ("LimitCheckingOn", json!(h.limit_check)),
            ("LineTypeScale", json!(number(h.linetype_scale))),
            ("LinearUnitFormat", json!(h.linear_unit_format)),
            ("LinearUnitPrecision", json!(h.linear_unit_precision)),
            ("LoadOLEObject", json!(h.ole_startup)),
            ("LoftedObjectNormals", json!(h.loft_normals)),
            ("Longitude", json!(number(h.longitude))),
            ("MaintenanceVersion", json!(maintenance(doc))),
            ("MaxViewportCount", json!(h.max_active_viewports)),
            ("MeasurementUnits", json!(h.measurement)),
            ("MenuFileName", json!(h.menu_name)),
            ("MirrorText", json!(h.mirror_text)),
            ("ModelSpaceExtMax", json!(vec3(h.model_space_extents_max))),
            ("ModelSpaceExtMin", json!(vec3(h.model_space_extents_min))),
            (
                "ModelSpaceInsertionBase",
                json!(vec3(h.model_space_insertion_base)),
            ),
            ("ModelSpaceLimitsMax", json!(vec2(h.model_space_limits_max))),
            ("ModelSpaceLimitsMin", json!(vec2(h.model_space_limits_min))),
            ("ModelSpaceOrigin", json!(vec3(h.model_space_ucs_origin))),
            ("ModelSpaceXAxis", json!(vec3(h.model_space_ucs_x_axis))),
            ("ModelSpaceYAxis", json!(vec3(h.model_space_ucs_y_axis))),
            ("NorthDirection", json!(number(h.north_direction))),
            ("NumberOfSplineSegments", json!(h.spline_segments)),
            ("ObjectSnapMode", json!(h.object_snap_mode)),
            ("OrthoMode", json!(h.ortho_mode)),
            ("PaperSpaceElevation", json!(number(h.paper_elevation))),
            ("PaperSpaceExtMax", json!(vec3(h.paper_space_extents_max))),
            ("PaperSpaceExtMin", json!(vec3(h.paper_space_extents_min))),
            (
                "PaperSpaceInsertionBase",
                json!(vec3(h.paper_space_insertion_base)),
            ),
            ("PaperSpaceLimitsChecking", json!(h.paper_space_limit_check)),
            ("PaperSpaceLimitsMax", json!(vec2(h.paper_space_limits_max))),
            ("PaperSpaceLimitsMin", json!(vec2(h.paper_space_limits_min))),
            (
                "PaperSpaceLineTypeScaling",
                json!(h.paper_space_linetype_scaling as i16),
            ),
            ("PaperSpaceUcsOrigin", json!(vec3(h.paper_space_ucs_origin))),
            ("PaperSpaceUcsXAxis", json!(vec3(h.paper_space_ucs_x_axis))),
            ("PaperSpaceUcsYAxis", json!(vec3(h.paper_space_ucs_y_axis))),
            ("PlotStyleMode", json!(h.plotstyle_mode as i16)),
            ("PointDisplayMode", json!(h.point_display_mode)),
            ("PointDisplaySize", json!(number(h.point_display_size))),
            (
                "PolylineLineTypeGeneration",
                json!(h.polyline_linetype_generation),
            ),
            ("PolylineWidthDefault", json!(number(h.polyline_width))),
            ("ProjectName", json!(h.project_name)),
            ("ProxyGraphics", json!(h.proxy_graphics != 0)),
            ("QuickTextMode", json!(h.quick_text_mode)),
            ("RegenerationMode", json!(h.regen_mode)),
            (
                "RetainXRefDependentVisibilitySettings",
                json!(h.retain_xref_visibility),
            ),
            ("ShadeDiffuseToAmbientPercentage", json!(h.shade_diffuse)),
            ("ShadeEdge", json!(h.shade_edge)),
            (
                "ShadowPlaneLocation",
                json!(number(h.shadow_plane_location)),
            ),
            ("ShowModelSpace", json!(h.show_model_space)),
            ("ShowSplineControlPoints", json!(h.spline_frame)),
            ("SketchIncrement", json!(number(h.sketch_increment))),
            ("SolidLoftedShape", json!(h.loft_param)),
            ("SpatialIndexMaxTreeDepth", json!(h.tree_depth)),
            ("SplineType", json!(h.spline_type)),
            ("StepSize", json!(number(h.step_size))),
            ("StepsPerSecond", json!(number(h.steps_per_second))),
            ("StyleSheetName", json!(h.stylesheet)),
            ("SurfaceDensityU", json!(h.surface_u_density)),
            ("SurfaceDensityV", json!(h.surface_v_density)),
            ("SurfaceMeshTabulationCount1", json!(h.surface_tab1)),
            ("SurfaceMeshTabulationCount2", json!(h.surface_tab2)),
            ("SurfaceType", json!(h.surface_type)),
            ("SweptSolidHeight", json!(number(h.polysolid_height))),
            ("SweptSolidWidth", json!(number(h.polysolid_width))),
            ("TextHeightDefault", json!(number(h.text_height))),
            ("ThicknessDefault", json!(number(h.thickness))),
            ("TimeZone", json!(h.timezone)),
            ("TotalEditingTime", json!(number(h.total_editing_time))),
            ("TraceWidthDefault", json!(number(h.trace_width))),
            ("UcsBaseName", json!(h.ucs_base)),
            ("UcsName", json!(h.model_space_ucs_name)),
            (
                "UniversalCreateDateTime",
                json!(number(h.universal_create_date_julian)),
            ),
            (
                "UniversalUpdateDateTime",
                json!(number(h.universal_update_date_julian)),
            ),
            ("UpdateDateTime", json!(number(h.update_date_julian))),
            (
                "UpdateDimensionsWhileDragging",
                json!(h.update_dimensions_while_dragging),
            ),
            ("UserDouble1", json!(number(h.user_real1))),
            ("UserDouble2", json!(number(h.user_real2))),
            ("UserDouble3", json!(number(h.user_real3))),
            ("UserDouble4", json!(number(h.user_real4))),
            ("UserDouble5", json!(number(h.user_real5))),
            ("UserElapsedTimeSpan", json!(number(h.user_elapsed_time))),
            ("UserShort1", json!(h.user_int1)),
            ("UserShort2", json!(h.user_int2)),
            ("UserShort3", json!(h.user_int3)),
            ("UserShort4", json!(h.user_int4)),
            ("UserShort5", json!(h.user_int5)),
            ("UserTimer", json!(h.user_timer)),
            ("Version", json!(version(doc.version))),
            ("VersionGuid", json!(h.version_guid)),
            ("VersionString", json!(doc.version.as_str())),
            (
                "ViewportDefaultViewScaleFactor",
                json!(number(h.viewport_scale_factor)),
            ),
            ("WorldView", json!(h.world_view)),
            ("XEdit", json!(h.xedit)),
        ],
    );
    project_source_values(doc, h, &mut value);
    value
}

pub(super) fn classes(doc: &CadDocument) -> Value {
    fn class_value(c: &crate::classes::DxfClass, source: bool) -> Value {
        props(
            "DxfClass",
            [
                ("ApplicationName", json!(c.application_name)),
                // ACadSharp's DXF DxfClass starts at 500 and the DXF reader
                // leaves that value unchanged. Native DWG/DXF registries keep
                // their actual positional numbers.
                (
                    "ClassNumber",
                    json!(if source { 500 } else { c.class_number }),
                ),
                ("CppClassName", json!(c.cpp_class_name)),
                ("DwgVersion", json!(c.dwg_version)),
                ("DxfName", json!(c.dxf_name)),
                ("InstanceCount", json!(c.instance_count)),
                ("ProxyFlags", json!(c.proxy_flags.0)),
                ("WasZombie", json!(c.was_zombie)),
            ],
        )
    }

    // ACadSharp uses a case-insensitive, name-keyed collection for both
    // formats. Keep the first entry without altering native positional data.
    let mut seen = std::collections::HashSet::new();
    if let Some(source) = doc.classes.source_classes() {
        Value::Array(
            source
                .iter()
                .filter(|class| seen.insert(class.dxf_name.to_ascii_uppercase()))
                .map(|c| class_value(c, true))
                .collect(),
        )
    } else {
        // ACadSharp stores DWG classes in a name-keyed collection.  The DWG
        // stream itself is positional, so the native collection deliberately
        // retains duplicate names for object decoding and writing.  Deduplicate
        // only this compatibility projection, keeping the first source entry
        // and its original class number.
        Value::Array(
            doc.classes
                .iter()
                .filter(|class| seen.insert(class.dxf_name.to_ascii_uppercase()))
                .map(|c| class_value(c, false))
                .collect(),
        )
    }
}
pub(super) fn summary(doc: &CadDocument) -> Value {
    let s = &doc.summary_info;
    props(
        "CadSummaryInfo",
        [
            (
                "CreatedDate",
                summary_date(
                    s.created_date_julian,
                    doc.dwg_summary_timestamps.map(|t| t[0]),
                ),
            ),
            (
                "ModifiedDate",
                summary_date(
                    s.modified_date_julian,
                    doc.dwg_summary_timestamps.map(|t| t[1]),
                ),
            ),
            ("Author", json!(s.author)),
            ("Comments", json!(s.comments)),
            ("HyperlinkBase", json!(s.hyperlink_base)),
            ("Keywords", json!(s.keywords)),
            (
                "LastSavedBy",
                json!(if doc.header.dxf_source_variables.is_some() {
                    ""
                } else {
                    &s.last_saved_by
                }),
            ),
            ("Properties", json!(s.custom_properties)),
            ("RevisionNumber", json!(s.revision_number)),
            ("Subject", json!(s.subject)),
            ("Title", json!(s.title)),
        ],
    )
}

fn resolve_references(doc: &CadDocument, h: &mut crate::document::HeaderVariables) {
    // DXF stores names; its native handle slots can still hold constructor
    // defaults. DWG stores handles and ACadSharp resolves them to names first.
    macro_rules! resolve {
        ($table:ident, $handle:ident, $name:ident) => {
            let by_handle = doc.dwg_source_version.and_then(|_| {
                doc.$table
                    .iter()
                    .find(|entry| !h.$handle.is_null() && entry.handle == h.$handle)
            });
            if let Some(entry) = by_handle.or_else(|| doc.$table.get(&h.$name)) {
                h.$handle = entry.handle;
                if by_handle.is_some() {
                    h.$name = entry.name.clone();
                }
            } else {
                h.$handle = Handle::NULL;
            }
        };
    }
    resolve!(layers, current_layer_handle, current_layer_name);
    resolve!(line_types, current_linetype_handle, current_linetype_name);
    resolve!(
        text_styles,
        current_text_style_handle,
        current_text_style_name
    );
    resolve!(dim_styles, current_dimstyle_handle, current_dimstyle_name);
    resolve!(text_styles, dim_text_style_handle, dim_text_style_name);
}

fn project_source_values(
    doc: &CadDocument,
    h: &crate::document::HeaderVariables,
    value: &mut Value,
) {
    let dwg = doc.dwg_source_version;
    let dxf = h.dxf_source_variables.is_some();
    let nullable = |variable, since, text: &str| {
        if source_missing(doc, variable, since, DxfVersion::AC1032)
            || (!dxf && dwg.is_none() && text.is_empty())
        {
            Value::Null
        } else {
            json!(text)
        }
    };
    value["HyperLinkBase"] = nullable("$HYPERLINKBASE", DxfVersion::AC1015, &h.hyperlink_base);
    value["StyleSheetName"] = nullable("$STYLESHEET", DxfVersion::AC1015, &h.stylesheet);
    value["ProjectName"] = nullable("$PROJECTNAME", DxfVersion::AC1018, &h.project_name);
    for (key, variable, text) in [
        (
            "DimensionBlockNameFirst",
            "$DIMBLK1",
            h.dim_arrow_block1.as_str(),
        ),
        (
            "DimensionBlockNameSecond",
            "$DIMBLK2",
            h.dim_arrow_block2.as_str(),
        ),
    ] {
        value[key] = nullable(variable, DxfVersion::AC1012, text);
    }
    if dxf {
        value["TotalEditingTime"] = number(editing_time(h.total_editing_time));
        value["UserElapsedTimeSpan"] = number(editing_time(h.user_elapsed_time));
        // These properties lack CadSystemVariable attributes in ACadSharp.
        value["ObscuredColor"] = color(Color::ByBlock);
        value["ObscuredType"] = json!(0);
        value["IntersectionDisplay"] = json!(0);
    }
    if let Some(version) = dwg {
        value["CodePage"] = json!(h.code_page.to_ascii_lowercase());
        for (index, key, native) in [
            (0, "CreateDateTime", h.create_date_julian),
            (1, "UpdateDateTime", h.update_date_julian),
        ] {
            let (day, ms) = timestamp_parts(native, h.dwg_timestamps.map(|t| t[index]));
            value[key] = number(dwg_date(day, ms));
        }
        for (index, key, native) in [
            (2, "TotalEditingTime", h.total_editing_time),
            (3, "UserElapsedTimeSpan", h.user_elapsed_time),
        ] {
            let (hours, ms) = timestamp_parts(native, h.dwg_timestamps.map(|t| t[index]));
            value[key] = number(if hours < 0 || ms < 0 {
                0.0
            } else {
                hours as f64 / 24.0 + ms as f64 / 86_400_000.0
            });
        }
        if version >= DxfVersion::AC1015 {
            // DwgHeaderReader exposes masked flag values rather than native
            // enums, and compares multi-bit masks with 1 for these booleans.
            value["CurrentEntityLineWeight"] =
                json!(crate::types::LineWeight::from_value(h.current_line_weight).to_dwg_index());
            value["EndCaps"] = json!(h.end_caps << 5);
            value["JoinStyle"] = json!(h.join_style << 7);
            value["PlotStyleMode"] = json!(if h.plotstyle_mode { 0x2000 } else { 0 });
            for key in [
                "DisplayLineWeight",
                "XEdit",
                "ExtendedNames",
                "LoadOLEObject",
            ] {
                value[key] = json!(false);
            }
            // UpdateHeader assigns DIMBLK, then (quirk) overwrites the same
            // property with DIMLDRBLK; the separate ArrowBlockName stays empty.
            for handle in [h.dim_arrow_block_handle, h.dim_leader_arrow_block_handle] {
                if let Some(block) = doc
                    .block_records
                    .iter()
                    .find(|b| !handle.is_null() && b.handle == handle)
                {
                    value["DimensionBlockName"] = json!(block.name);
                }
            }
            for (key, handle) in [
                ("DimensionBlockNameFirst", h.dim_arrow_block1_handle),
                ("DimensionBlockNameSecond", h.dim_arrow_block2_handle),
            ] {
                value[key] = doc
                    .block_records
                    .iter()
                    .find(|b| !handle.is_null() && b.handle == handle)
                    .map(|b| json!(b.name))
                    .unwrap_or(Value::Null);
            }
        }
        value["MeasurementUnits"] = json!(template_measurement(doc));
        if version >= DxfVersion::AC1018 && version < DxfVersion::AC1021 {
            // The older INTERSECTIONCOLOR slot is assigned to InterfereColor.
            value["InterfereColor"] = color(Color::from_index(h.intersection_color));
        }
        // The header-handle collection retains these pointers but never binds
        // them to the header's standalone UCS instances or linetype names.
        for key in [
            "UcsName",
            "UcsBaseName",
            "PaperSpaceName",
            "PaperSpaceBaseName",
        ] {
            value[key] = json!("");
        }
        for key in ["DimensionLineType", "DimensionTex1", "DimensionTex2"] {
            value[key] = json!("ByBlock");
        }
        value["CurrentMLineStyleName"] = json!("Standard");
    }
    // Color.TrueColor is the little-endian packed RGB integer in ACadSharp.
    // Keep this local to header projection; native Color is unchanged.
    for key in [
        "CurrentEntityColor",
        "DimensionExtensionLineColor",
        "DimensionLineColor",
        "DimensionTextColor",
        "DimensionTextBackgroundColor",
        "InterfereColor",
        "ObscuredColor",
    ] {
        if value[key]["IsTrueColor"] == true {
            let c = &value[key];
            let packed = c["R"].as_u64().unwrap()
                | c["G"].as_u64().unwrap() << 8
                | c["B"].as_u64().unwrap() << 16;
            value[key]["TrueColor"] = json!(packed);
        }
    }
}

fn summary_date(value: Option<f64>, source: Option<(i32, i32)>) -> Value {
    if value.is_none() && source == Some((0, 0)) {
        // A present zero pair takes the oracle's DateTime.MinValue path;
        // a missing SummaryInfo section retains the nondeterministic default.
        return number(dwg_date(0, 0));
    }
    value
        .map(|date| {
            let (day, ms) = timestamp_parts(date, source);
            number(dwg_date(day, ms))
        })
        .unwrap_or_else(|| json!(UNMAPPED))
}

// Preserve source pairs (including negative components) while the native value
// is unchanged. A caller editing the native date or duration takes precedence.
fn timestamp_parts(value: f64, source: Option<(i32, i32)>) -> (i32, i32) {
    if let Some((day, ms)) = source {
        if value == day as f64 + ms as f64 / 86_400_000.0 {
            return (day, ms);
        }
    }
    (
        value.floor() as i32,
        ((value - value.floor()) * 86_400_000.0).round() as i32,
    )
}

fn template_measurement(doc: &CadDocument) -> i16 {
    if doc
        .dwg_source_version
        .is_some_and(|v| v < DxfVersion::AC1018)
    {
        return 1;
    }
    let Some(data) = &doc.header.dwg_template_data else {
        return 1;
    };
    if data.len() < 4 {
        return 1;
    }
    let length = i16::from_le_bytes([data[0], data[1]]).max(0) as usize;
    let width = if doc
        .dwg_source_version
        .is_some_and(|v| v >= DxfVersion::AC1021)
    {
        2
    } else {
        1
    };
    if let Some(bytes) = data.get(2 + length * width..4 + length * width) {
        let native = i16::from_le_bytes([bytes[0], bytes[1]]);
        if matches!(native, 0 | 1) && native != doc.header.measurement {
            return doc.header.measurement;
        }
    }
    // DwgReader.readTemplate treats the description length as bytes even for
    // UTF-16 versions. Native parse_template correctly counts code units.
    let Some(bytes) = data.get(2 + length..4 + length) else {
        return 1;
    };
    i16::from_le_bytes([bytes[0], bytes[1]])
}
