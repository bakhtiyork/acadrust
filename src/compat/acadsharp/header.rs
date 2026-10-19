use super::*;

pub(super) fn header(doc: &CadDocument) -> Value {
    let h = &doc.header;
    props(
        "CadHeader",
        [
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
                if doc
                    .dwg_source_version
                    .is_some_and(|v| v >= DxfVersion::AC1015)
                {
                    json!(h.dim_fit)
                } else {
                    json!(UNMAPPED)
                },
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
            (
                "FingerPrintGuid",
                json!(h.fingerprint_guid.to_ascii_lowercase()),
            ),
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
            ("MaintenanceVersion", json!(doc.maintenance_version)),
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
            ("VersionGuid", json!(h.version_guid.to_ascii_lowercase())),
            ("VersionString", json!(doc.version.as_str())),
            (
                "ViewportDefaultViewScaleFactor",
                json!(number(h.viewport_scale_factor)),
            ),
            ("WorldView", json!(h.world_view)),
            ("XEdit", json!(h.xedit)),
        ],
    )
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
            ("Author", json!(s.author)),
            ("Comments", json!(s.comments)),
            ("HyperlinkBase", json!(s.hyperlink_base)),
            ("Keywords", json!(s.keywords)),
            ("LastSavedBy", json!(s.last_saved_by)),
            ("Properties", json!(s.custom_properties)),
            ("RevisionNumber", json!(s.revision_number)),
            ("Subject", json!(s.subject)),
            ("Title", json!(s.title)),
        ],
    )
}
