// SPDX-License-Identifier: MPL-2.0
//
// Unit tests of the SVG writer. They build their documents in memory. The port of ACadSharp's
// `SvgWriterTests` and the replay of the C# outputs of svg/export_sample.dwg are in tests/svg_writer.rs.

use super::converter::{double_to_string, to_pixel_size, to_svg_units, UnitsType};
use super::csmath::{BoundingBox, Extent, Matrix4, Transform, Xyz};
use super::xml_text_writer::{Formatting, XmlTextWriter};
use super::{SvgConfiguration, SvgError, SvgNotificationKind, SvgWriter};
use crate::entities::{
    BoundaryEdge, BoundaryPath, Circle, CircularArcEdge, EllipticArcEdge, EntityType, Hatch, Insert, Line,
    LwPolyline, LwVertex, Text, UnknownEntity,
};
use crate::tables::BlockRecord;
use crate::types::{Vector2, Vector3};
use crate::CadDocument;

fn xml(formatting: Formatting, f: impl FnOnce(&mut XmlTextWriter<&mut Vec<u8>>) -> Result<(), SvgError>) -> String {
    let mut out = Vec::new();
    {
        let mut w = XmlTextWriter::new(&mut out);
        w.formatting = formatting;
        f(&mut w).expect("xml");
    }
    String::from_utf8(out).unwrap()
}

// --- XmlTextWriter (as ACadSharp's SvgWriterXmlTests, which the C# side has as .NET behaviour) ---------

#[test]
fn xml_declaration_has_no_encoding_and_no_byte_order_mark() {
    let s = xml(Formatting::None, |w| {
        w.write_start_document()?;
        w.write_start_element("a")?;
        w.write_end_element()?;
        w.write_end_document()
    });
    assert_eq!(s, "<?xml version=\"1.0\"?><a />");
}

#[test]
fn xml_indents_with_two_spaces_and_lf() {
    let s = xml(Formatting::Indented, |w| {
        w.write_start_document()?;
        w.write_start_element("svg")?;
        w.write_attribute_string("a", "1")?;
        w.write_comment("c")?;
        w.write_start_element("g")?;
        w.write_start_element("line")?;
        w.write_end_element()?;
        w.write_end_element()?;
        w.write_end_element()
    });
    assert_eq!(
        s,
        "<?xml version=\"1.0\"?>\n<svg a=\"1\">\n  <!--c-->\n  <g>\n    <line />\n  </g>\n</svg>"
    );
}

#[test]
fn xml_text_keeps_its_subtree_inline() {
    let s = xml(Formatting::Indented, |w| {
        w.write_start_element("text")?;
        w.write_start_element("tspan")?;
        w.write_string("a")?;
        w.write_end_element()?;
        w.write_start_element("tspan")?;
        w.write_string("b")?;
        w.write_end_element()?;
        w.write_end_element()
    });
    assert_eq!(s, "<text>\n  <tspan>a</tspan>\n  <tspan>b</tspan>\n</text>");
}

#[test]
fn xml_escapes_text_and_attribute_values() {
    let s = xml(Formatting::None, |w| {
        w.write_start_element("a")?;
        w.write_attribute_string("v", "<&>\"\t\n\r'")?;
        w.write_string("<&>\"\u{1}\t\n\r'")?;
        w.write_end_element()
    });
    // As .NET 10's XmlTextWriter (checked with `dotnet fsi`): a tab stays raw in an attribute value, LF and CR become
    // character references; text keeps all three raw; neither escapes the apostrophe.
    assert_eq!(s, "<a v=\"&lt;&amp;&gt;&quot;\t&#xA;&#xD;'\">&lt;&amp;&gt;\"&#x1;\t\n\r'</a>");
}

#[test]
fn xml_empty_string_keeps_an_element_self_closing() {
    let s = xml(Formatting::None, |w| {
        w.write_start_element("a")?;
        w.write_string("")?;
        w.write_end_element()
    });
    assert_eq!(s, "<a />");
}

#[test]
fn xml_values_use_xmlconvert_spellings() {
    let s = xml(Formatting::None, |w| {
        w.write_start_element("v")?;
        w.write_start_attribute("p")?;
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0, 1e20, 1e-5] {
            w.write_value_f64(v)?;
            w.write_value_str(" ")?;
        }
        w.write_end_attribute()?;
        w.write_end_element()
    });
    assert_eq!(s, "<v p=\"NaN INF -INF -0 1E+20 1E-05 \" />");
}

#[test]
fn xml_comment_rejects_double_hyphen_and_trailing_hyphen() {
    let mut out = Vec::new();
    let mut w = XmlTextWriter::new(&mut out);
    w.write_start_element("a").unwrap();
    assert!(matches!(w.write_comment("a--b"), Err(SvgError::Argument(_))));
    assert!(matches!(w.write_comment("a-"), Err(SvgError::Argument(_))));
}

#[test]
fn xml_misuse_is_reported_not_panicked() {
    let mut out = Vec::new();
    let mut w = XmlTextWriter::new(&mut out);
    assert!(matches!(w.write_end_element(), Err(SvgError::InvalidOperation(_))));
    assert!(matches!(w.write_string("x"), Err(SvgError::InvalidOperation(_))));
    w.write_start_element("a").unwrap();
    w.write_string("x").unwrap();
    assert!(matches!(w.write_start_attribute("late"), Err(SvgError::InvalidOperation(_))));
    w.write_end_element().unwrap();
    w.write_end_document().unwrap();
    assert!(matches!(w.write_start_element("b"), Err(SvgError::InvalidOperation(_))));
}

// --- numbers and units ---------------------------------------------------------------------------------

#[test]
fn doubles_print_like_net_core() {
    for (value, expected) in [
        (0.0, "0"),
        (-0.0, "-0"),
        (1.0, "1"),
        (94.48818897637796, "94.48818897637796"),
        (0.4285448570897142, "0.4285448570897142"),
        (0.0001, "0.0001"),
        (0.00001, "1E-05"),
        (123456789012345680.0, "1.2345678901234568E+17"),
        (10000000000000000.0, "10000000000000000"),
        (1e17, "1E+17"),
        (-2.5, "-2.5"),
        (f64::NAN, "NaN"),
        (f64::INFINITY, "Infinity"),
        (f64::NEG_INFINITY, "-Infinity"),
        (5e-324, "5E-324"),
        // .NET 10 `double.ToString(CultureInfo.InvariantCulture)`, checked with `dotnet fsi`: the fixed form up to
        // 17 integer digits whatever the number of significant digits, then the exponent form.
        (1e15, "1000000000000000"),
        (1.5e16, "15000000000000000"),
        (1234567890123456.0, "1234567890123456"),
        (12345678901234567.0, "12345678901234568"),
        (9999999999999998.0, "9999999999999998"),
        (0.00012345, "0.00012345"),
        (1e-300, "1E-300"),
        (f64::MAX, "1.7976931348623157E+308"),
        (1.0 / 3.0, "0.3333333333333333"),
    ] {
        assert_eq!(double_to_string(value), expected, "{value:e}");
    }
}

#[test]
fn unit_suffixes_and_pixel_sizes() {
    assert_eq!(to_svg_units(1.5, UnitsType::MILLIMETERS), "1.5mm");
    assert_eq!(to_svg_units(1.5, UnitsType::CENTIMETERS), "1.5cm");
    assert_eq!(to_svg_units(1.5, UnitsType::INCHES), "1.5in");
    assert_eq!(to_svg_units(1.5, UnitsType::UNITLESS), "1.5");
    assert_eq!(to_svg_units(1.5, UnitsType(6)), "1.5");
    assert_eq!(to_pixel_size(1.0, UnitsType::INCHES).unwrap(), 96.0);
    assert_eq!(to_pixel_size(25.4, UnitsType::MILLIMETERS).unwrap(), 25.4 * 96.0 / 25.4);
    let error = to_pixel_size(1.0, UnitsType(3)).unwrap_err();
    assert_eq!(error.csharp_exception(), "InvalidEnumArgumentException");
}

#[test]
fn configuration_defaults_and_line_weights() {
    let c = SvgConfiguration::default();
    assert_eq!((c.line_weight_ratio, c.default_line_weight, c.point_radius, c.arc_points), (100.0, 0.01, 0.1, 256));
    #[cfg(feature = "acadsharp-compat")]
    assert!(!c.acadsharp_view);
    let t = SvgConfiguration::test();
    assert_eq!((t.line_weight_ratio, t.default_line_weight), (50.0, 0.15));
    // Unitless: the weight divided by the ratio; otherwise the weight in mm (-3 default, 0 the thinnest).
    assert_eq!(t.line_weight_value(30, UnitsType::UNITLESS), 0.6);
    assert_eq!(t.line_weight_value(-3, UnitsType::MILLIMETERS), 0.15);
    assert_eq!(t.line_weight_value(0, UnitsType::MILLIMETERS), 0.001);
    assert_eq!(t.line_weight_value(25, UnitsType::MILLIMETERS), 0.25);
}

// --- CSMath ----------------------------------------------------------------------------------------------

#[test]
fn bounding_boxes_merge_like_csmath() {
    let a = BoundingBox::from_point(Xyz::new(1.0, 2.0, 0.0));
    let b = BoundingBox::from_point(Xyz::new(-1.0, 5.0, 0.0));
    let m = BoundingBox::NULL.merge(a).merge(b);
    assert_eq!((m.min.x, m.min.y, m.max.x, m.max.y), (-1.0, 2.0, 1.0, 5.0));
    assert_eq!(m.extent, Extent::Finite);
    assert_eq!(a.merge(BoundingBox::INFINITE).extent, Extent::Infinite);
    assert!(BoundingBox::NULL.min.x.is_nan());
    // Math.Min propagates NaN.
    assert!(a.merge(BoundingBox::from_point(Xyz::new(f64::NAN, 0.0, 0.0))).min.x.is_nan());
}

#[test]
fn transform_decomposes_a_composed_matrix() {
    let t = Transform::from_trs(Xyz::new(1.0, 2.0, 0.0), Xyz::new(2.0, 3.0, 1.0), Xyz::ZERO).unwrap();
    let back = Transform::from_matrix(*t.matrix());
    assert_eq!(back.translation(), Xyz::new(1.0, 2.0, 0.0));
    assert_eq!(back.scale(), Xyz::new(2.0, 3.0, 1.0));
    assert_eq!(back.euler_rotation().z, 0.0);
    assert_eq!(t.apply_transform(Xyz::new(1.0, 1.0, 0.0)), Xyz::new(3.0, 5.0, 0.0));
}

#[test]
fn a_zero_scale_is_rejected_and_a_singular_matrix_decomposes_to_zero() {
    let error = Transform::from_trs(Xyz::ZERO, Xyz::new(0.0, 1.0, 1.0), Xyz::ZERO).unwrap_err();
    assert_eq!(error.csharp_exception(), "ArgumentException");
    let singular = Transform::from_matrix(Matrix4::create_scale(Xyz::new(0.0, 0.0, 0.0)));
    assert_eq!(singular.scale(), Xyz::ZERO);
    assert!(singular.euler_rotation().x.is_nan());
}

#[test]
fn arbitrary_axis_matches_the_ocs_convention() {
    let m = Matrix4::arbitrary_axis(Xyz::new(0.0, 0.0, -1.0));
    let v = m.mul_xyz(Xyz::new(1.0, 2.0, 3.0));
    assert_eq!(v, Xyz::new(-1.0, 2.0, -3.0));
    assert_eq!(Matrix4::arbitrary_axis(Xyz::AXIS_Z), Matrix4::IDENTITY);
}

// --- the writer over an in-memory document ---------------------------------------------------------------

fn write_model(doc: &CadDocument) -> Result<String, SvgError> {
    let mut out = Vec::new();
    SvgWriter::new(&mut out, doc).with_configuration(SvgConfiguration::test()).write()?;
    Ok(String::from_utf8(out).unwrap())
}

fn model_error(doc: &CadDocument) -> &'static str {
    write_model(doc).expect_err("the write must fail").csharp_exception()
}

#[test]
fn a_line_and_a_circle_are_written_with_their_bounding_box() {
    let mut doc = CadDocument::new();
    let mut line = Line::new();
    line.start = Vector3::new(0.0, 0.0, 0.0);
    line.end = Vector3::new(10.0, 5.0, 0.0);
    let line_handle = doc.add_entity(EntityType::Line(line)).unwrap().value();
    let circle = Circle::from_coords(20.0, 18.0, 0.0, 5.0);
    let circle_handle = doc.add_entity(EntityType::Circle(circle)).unwrap().value();
    let svg = write_model(&doc).unwrap();
    let expected = [
        "<?xml version=\"1.0\"?>".to_owned(),
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"25\" height=\"23\" viewBox=\"0 0 25 23\" transform=\"scale(1,-1)\">".to_owned(),
        format!("  <!--LINE | {line_handle}-->"),
        "  <line vector-effect=\"non-scaling-stroke\" stroke=\"rgb(255,255,255)\" stroke-width=\"0.06mm\" x1=\"0\" y1=\"0\" x2=\"10\" y2=\"5\" />".to_owned(),
        format!("  <!--CIRCLE | {circle_handle}-->"),
        "  <circle vector-effect=\"non-scaling-stroke\" stroke=\"rgb(255,255,255)\" stroke-width=\"0.06mm\" r=\"5\" cx=\"20\" cy=\"18\" fill=\"none\" />".to_owned(),
        "</svg>".to_owned(),
    ]
    .join("\n");
    assert_eq!(svg, expected);
}

#[test]
fn text_is_written_inside_a_group() {
    let mut doc = CadDocument::new();
    let mut text = Text::new();
    text.value = "a<b".into();
    text.height = 2.5;
    text.insertion_point = Vector3::new(1.0, 2.0, 0.0);
    doc.add_entity(EntityType::Text(text)).unwrap();
    let svg = write_model(&doc).unwrap();
    assert!(svg.contains("<g>\n    <text transform=\"translate(1,2)scale(1,-1)\" fill=\"rgb(255,255,255)\" style=\"font:2.5px"));
    assert!(svg.contains(">a&lt;b</text>"));
}

#[test]
fn entities_without_a_writer_raise_a_notification() {
    let mut doc = CadDocument::new();
    doc.add_entity(EntityType::Ray(crate::entities::Ray::default())).unwrap();
    doc.add_entity(EntityType::Line(Line::new())).unwrap();
    let mut out = Vec::new();
    let mut writer = SvgWriter::new(&mut out, &doc);
    writer.write().unwrap();
    assert_eq!(writer.notifications().len(), 1);
    assert_eq!(writer.notifications()[0].kind, SvgNotificationKind::NotImplemented);
    assert_eq!(writer.notifications()[0].message, "[RAY] Entity not implemented.");
}

#[test]
fn an_empty_model_has_a_null_box() {
    let svg = write_model(&CadDocument::new()).unwrap();
    assert!(svg.contains("width=\"NaN\" height=\"NaN\" viewBox=\"NaN NaN NaN NaN\""));
}

// --- the C# exception cases the Swift review found --------------------------------------------------------

#[test]
fn the_bounding_box_of_an_aec_wall_is_not_implemented() {
    let mut doc = CadDocument::new();
    doc.add_entity(EntityType::Unknown(UnknownEntity::new("AEC_WALL"))).unwrap();
    assert_eq!(model_error(&doc), "NotImplementedException");
}

#[test]
fn an_insert_of_an_unknown_block_raises_a_null_reference() {
    let mut doc = CadDocument::new();
    doc.add_entity(EntityType::Insert(Insert::new("NOPE", Vector3::ZERO))).unwrap();
    assert_eq!(model_error(&doc), "NullReferenceException");
}

fn hatch_with(edge: BoundaryEdge) -> CadDocument {
    let mut doc = CadDocument::new();
    let mut hatch = Hatch::new();
    hatch.is_solid = true;
    let mut path = BoundaryPath::new();
    path.edges.push(edge);
    hatch.paths.push(path);
    doc.add_entity(EntityType::Hatch(Box::new(hatch))).unwrap();
    doc
}

fn arc_edge(radius: f64) -> BoundaryEdge {
    BoundaryEdge::CircularArc(CircularArcEdge {
        center: Vector2::new(0.0, 0.0),
        radius,
        start_angle: 0.0,
        end_angle: 1.0,
        counter_clockwise: true,
    })
}

fn ellipse_edge(ratio: f64) -> BoundaryEdge {
    BoundaryEdge::EllipticArc(EllipticArcEdge {
        center: Vector2::new(0.0, 0.0),
        major_axis_endpoint: Vector2::new(2.0, 0.0),
        minor_axis_ratio: ratio,
        start_angle: 0.0,
        end_angle: 1.0,
        counter_clockwise: true,
    })
}

#[test]
fn a_malformed_hatch_arc_or_ellipse_raises_an_argument_out_of_range() {
    assert_eq!(model_error(&hatch_with(arc_edge(0.0))), "ArgumentOutOfRangeException");
    assert_eq!(model_error(&hatch_with(arc_edge(-1.0))), "ArgumentOutOfRangeException");
    assert_eq!(model_error(&hatch_with(ellipse_edge(0.0))), "ArgumentOutOfRangeException");
    assert_eq!(model_error(&hatch_with(ellipse_edge(1.5))), "ArgumentOutOfRangeException");
    // A well-formed one is written.
    let svg = write_model(&hatch_with(arc_edge(2.0))).unwrap();
    assert!(svg.contains("<pattern id=\""));
}

#[test]
fn a_bulge_between_equal_points_raises_an_argument_out_of_range() {
    let mut doc = CadDocument::new();
    let mut pl = LwPolyline::new();
    let mut a = LwVertex::from_coords(1.0, 1.0);
    a.bulge = 0.5;
    pl.vertices.push(a);
    pl.vertices.push(LwVertex::from_coords(1.0, 1.0));
    doc.add_entity(EntityType::LwPolyline(pl)).unwrap();
    assert_eq!(model_error(&doc), "ArgumentOutOfRangeException");
}

#[test]
fn a_bulge_arc_is_sampled_and_written() {
    let mut doc = CadDocument::new();
    let mut pl = LwPolyline::new();
    let mut a = LwVertex::from_coords(0.0, 0.0);
    a.bulge = 1.0;
    pl.vertices.push(a);
    pl.vertices.push(LwVertex::from_coords(2.0, 0.0));
    doc.add_entity(EntityType::LwPolyline(pl)).unwrap();
    let svg = write_model(&doc).unwrap();
    assert!(svg.contains("<polyline "));
    let points = svg.split("points=\"").nth(1).unwrap().split('"').next().unwrap();
    assert_eq!(points.split(' ').count(), 255);
}

#[test]
fn a_block_with_unsupported_units_raises_an_invalid_enum_argument() {
    let mut doc = CadDocument::new();
    let mut record = BlockRecord::new("MILES");
    record.units = 3;
    doc.block_records.add(record).unwrap();
    let record = doc.block_records.get("MILES").unwrap();
    let mut out = Vec::new();
    let error = SvgWriter::new(&mut out, &doc).write_block_record(record).unwrap_err();
    assert_eq!(error.csharp_exception(), "InvalidEnumArgumentException");
}

#[test]
fn an_unscaled_layout_raises_an_argument_exception() {
    let mut doc = CadDocument::new();
    doc.add_layout("L").unwrap();
    let layout = SvgWriter::new(std::io::sink(), &doc).paper_space_layouts().into_iter().find(|l| l.name == "L").unwrap();
    let mut zero = layout.clone();
    zero.plot_scale_numerator = 0.0;
    zero.plot_scale_denominator = 1.0;
    // PrintScale 0 makes the layout transform's scale setter throw.
    let view = super::LayoutView::of(&zero);
    assert_eq!(view.print_scale, 0.0);
    let block = doc.block_records.iter().find(|b| b.handle == layout.block_record).unwrap();
    let mut writer = super::SvgXmlWriter::new(
        Vec::new(),
        super::model::Model::new(&doc),
        SvgConfiguration::default(),
    );
    assert_eq!(writer.write_layout(view, block).unwrap_err().csharp_exception(), "ArgumentException");
}

#[test]
fn a_failing_stream_is_an_io_error() {
    struct Failing;
    impl std::io::Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("closed"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut doc = CadDocument::new();
    doc.add_entity(EntityType::Line(Line::new())).unwrap();
    let error = SvgWriter::new(Failing, &doc).write().unwrap_err();
    assert_eq!(error.csharp_exception(), "IOException");
}

#[test]
fn the_document_builder_throws_a_null_reference_until_its_layout_is_set() {
    let mut doc = CadDocument::new();
    doc.add_layout("L").unwrap();
    let layout = SvgWriter::new(std::io::sink(), &doc).paper_space_layouts().into_iter().find(|l| l.name == "L").unwrap();
    let block = doc.block_records.iter().find(|b| b.handle == layout.block_record).unwrap();
    let view = super::LayoutView::of(layout);
    let mut builder = super::SvgDocumentBuilder::new(Vec::new(), &doc, SvgConfiguration::default());
    assert_eq!(builder.write_layout(view, block).unwrap_err().csharp_exception(), "NullReferenceException");
    builder.set_layout(Some(view));
    builder.write_layout(view, block).unwrap();
    builder.write_line_type("DASHED").unwrap();
    assert!(builder.line_type_writers.contains_key("DASHED"));
}

// --- review R6: identity ids, nesting, native and ACadSharp-view readings --------------------------------------

fn add_block(doc: &mut CadDocument, name: &str) -> crate::types::Handle {
    let mut record = BlockRecord::new(name);
    record.handle = doc.allocate_handle();
    let handle = record.handle;
    doc.block_records.add(record).unwrap();
    handle
}

fn add_to(doc: &mut CadDocument, owner: crate::types::Handle, mut entity: EntityType) {
    entity.common_mut().owner_handle = owner;
    doc.add_entity(entity).unwrap();
}

fn pattern_ids(svg: &str) -> Vec<&str> {
    svg.split("<pattern id=\"").skip(1).map(|s| s.split('"').next().unwrap()).collect()
}

#[test]
fn a_hatch_written_twice_repeats_its_pattern_ids() {
    // C# prints `GetHashCode()` of the pattern and of each pattern line: the same objects give the same ids when a
    // block is inserted twice.
    let mut doc = CadDocument::new();
    let block = add_block(&mut doc, "H");
    let mut hatch = Hatch::new();
    hatch.is_solid = false;
    hatch.pattern = crate::entities::HatchPattern::new("ANSI31");
    hatch.pattern.lines.push(crate::entities::HatchPatternLine {
        angle: 0.5,
        base_point: Vector2::new(0.0, 0.0),
        offset: Vector2::new(0.0, 1.0),
        dash_lengths: vec![],
    });
    let mut path = BoundaryPath::new();
    path.edges.push(arc_edge(2.0));
    hatch.paths.push(path);
    add_to(&mut doc, block, EntityType::Hatch(Box::new(hatch)));
    doc.add_entity(EntityType::Insert(Insert::new("H", Vector3::ZERO))).unwrap();
    doc.add_entity(EntityType::Insert(Insert::new("H", Vector3::new(5.0, 0.0, 0.0)))).unwrap();
    let svg = write_model(&doc).unwrap();
    assert_eq!(pattern_ids(&svg), ["1_line", "2_ANSI31", "1_line", "2_ANSI31"]);
    assert_eq!(svg.matches("fill=\"url(#2_ANSI31)\"").count(), 2);
    assert_eq!(write_model(&doc).unwrap(), svg, "the output is the same from run to run");
}

#[test]
fn a_block_that_inserts_itself_is_an_error_not_a_stack_overflow() {
    // ACadSharp recurses until the process dies; the port stops at a nesting limit.
    let mut doc = CadDocument::new();
    let block = add_block(&mut doc, "SELF");
    add_to(&mut doc, block, EntityType::Insert(Insert::new("SELF", Vector3::ZERO)));
    doc.add_entity(EntityType::Insert(Insert::new("SELF", Vector3::ZERO))).unwrap();
    assert_eq!(model_error(&doc), "InvalidOperationException");
    let record = doc.block_records.get("SELF").unwrap();
    let error = SvgWriter::new(Vec::new(), &doc).write_block_record(record).unwrap_err();
    assert_eq!(error.csharp_exception(), "InvalidOperationException");
}

#[test]
fn the_native_three_dimensional_polyline_is_drawn() {
    let mut doc = CadDocument::new();
    let mut polyline = crate::entities::Polyline::from_points(vec![Vector3::new(0.0, 0.0, 0.0), Vector3::new(3.0, 4.0, 1.0)]);
    polyline.flags.set_closed(true);
    let handle = doc.add_entity(EntityType::Polyline(polyline)).unwrap().value();
    let mut out = Vec::new();
    let mut writer = SvgWriter::new(&mut out, &doc).with_configuration(SvgConfiguration::test());
    writer.write().unwrap();
    assert!(writer.notifications().is_empty());
    let svg = String::from_utf8(out).unwrap();
    assert!(svg.contains(&format!("<!--POLYLINE | {handle}-->\n  <polygon ")), "{svg}");
    assert!(svg.contains("points=\"0,0 3,4 0,0\""), "{svg}");
    assert!(svg.contains("width=\"3\" height=\"4\""), "{svg}");
}

/// An R12 DXF with every R12 rule of ACadSharp's reader: a diameter dimension with user-positioned text (#31), an
/// ownerless paper-space VIEWPORT (#40), an INSERT (#50) with an ATTRIB (#51) and its SEQEND (#52), a polyface mesh
/// (#70) with two vertices and one face record.
fn r12_document() -> CadDocument {
    let header = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n9\n$HANDSEED\n5\n100\n0\nENDSEC\n";
    let tables = "0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLTYPE\n0\nLTYPE\n2\nCONTINUOUS\n70\n0\n0\nENDTAB\n0\nTABLE\n2\nLAYER\n0\nLAYER\n2\n0\n70\n0\n62\n7\n6\nCONTINUOUS\n0\nENDTAB\n0\nTABLE\n2\nSTYLE\n0\nSTYLE\n2\nSTANDARD\n70\n0\n0\nENDTAB\n0\nENDSEC\n";
    let blocks = "0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n8\n0\n2\nB\n70\n2\n10\n0\n20\n0\n30\n0\n3\nB\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n1\n21\n1\n0\nENDBLK\n8\n0\n0\nBLOCK\n8\n0\n2\n*D1\n70\n1\n10\n0\n20\n0\n30\n0\n3\n*D1\n0\nLINE\n8\n0\n10\n0\n20\n0\n11\n2\n21\n0\n0\nENDBLK\n8\n0\n0\nENDSEC\n";
    let entities = "0\nSECTION\n2\nENTITIES\n\
        0\nDIMENSION\n5\n31\n8\n0\n2\n*D1\n70\n131\n10\n0\n20\n0\n15\n1\n25\n0\n\
        0\nVIEWPORT\n5\n40\n67\n1\n8\n0\n10\n0\n20\n0\n40\n1\n41\n1\n69\n2\n\
        0\nINSERT\n5\n50\n8\n0\n66\n1\n2\nB\n10\n0\n20\n0\n\
        0\nATTRIB\n5\n51\n8\n0\n10\n0\n20\n0\n40\n1\n1\nlabel\n2\nTAG\n70\n0\n\
        0\nSEQEND\n5\n52\n8\n0\n\
        0\nPOLYLINE\n5\n70\n8\n0\n66\n1\n10\n0\n20\n0\n30\n0\n70\n64\n71\n2\n72\n1\n\
        0\nVERTEX\n5\n71\n8\n0\n10\n1\n20\n2\n30\n0\n70\n192\n\
        0\nVERTEX\n5\n72\n8\n0\n10\n3\n20\n4\n30\n0\n70\n192\n\
        0\nVERTEX\n5\n73\n8\n0\n10\n0\n20\n0\n30\n0\n70\n128\n71\n1\n72\n2\n73\n1\n\
        0\nSEQEND\n5\n74\n8\n0\n\
        0\nENDSEC\n";
    let text = format!("{header}{tables}{blocks}{entities}0\nEOF\n");
    crate::io::dxf::DxfReader::from_reader(std::io::Cursor::new(text.into_bytes()))
        .unwrap()
        .read()
        .unwrap()
}

fn polyface_points(svg: &str) -> &str {
    let at = svg.find("<!--POLYLINE | 112-->").expect("the polyface mesh is written");
    svg[at..].split("points=\"").nth(1).unwrap().split('"').next().unwrap()
}

#[test]
fn the_default_reads_an_r12_dxf_natively() {
    let doc = r12_document();
    let svg = write_model(&doc).unwrap();
    // The diameter dimension stays, drawn through its block.
    assert!(svg.contains("<!--DIMENSION | 49-->"), "{svg}");
    // The ATTRIB and SEQEND children belong to the INSERT, not to the model space collection.
    assert!(svg.contains("<!--INSERT | 80-->"));
    assert!(!svg.contains("<!--ATTRIB | 81-->") && !svg.contains("<!--SEQEND | 82-->"));
    // The face record of the polyface mesh is a face, not a vertex at the origin.
    assert_eq!(polyface_points(&svg), "1,2 3,4");
    // The paper-space viewport stays in paper space.
    assert!(!svg.contains("<!--VIEWPORT | 64-->"));
    let probe = SvgWriter::new(std::io::sink(), &doc);
    let paper = doc.block_records.iter().find(|b| b.handle == doc.header.paper_space_block_handle).unwrap();
    assert!(paper.entity_handles.contains(&crate::types::Handle::new(0x40)));
    // The layouts of the native document are written.
    assert!(!probe.paper_space_layouts().is_empty());
}

#[cfg(feature = "acadsharp-compat")]
#[test]
fn the_acadsharp_view_reads_an_r12_dxf_as_acadsharp_does() {
    let doc = r12_document();
    let mut configuration = SvgConfiguration::test();
    configuration.acadsharp_view = true;
    let mut out = Vec::new();
    let mut writer = SvgWriter::new(&mut out, &doc).with_configuration(configuration);
    writer.write().unwrap();
    assert!(writer.paper_space_layouts().is_empty(), "ACadSharp reads no layouts from an R12 DXF");
    let svg = String::from_utf8(out).unwrap();
    assert!(!svg.contains("<!--DIMENSION | 49-->"), "{svg}");
    assert!(svg.contains("<!--INSERT | 80-->"));
    let attrib = svg.find("<!--ATTRIB | 81-->").expect("the ATTRIB is promoted");
    let seqend = svg.find("<!--SEQEND | 82-->").expect("the SEQEND is promoted");
    assert!(attrib < seqend);
    assert_eq!(polyface_points(&svg), "1,2 3,4 0,0");
    assert!(svg.contains("<!--VIEWPORT | 64-->"));
}

/// An R14 DWG source as AutoCAD saves it from a later version: the entity holds its true colour and line weight
/// natively, the file's entity record the nearest index and no line weight. The layer is ByLayer, which the DWG layer
/// reader of ACadSharp reads as index 7.
fn old_dwg_document() -> CadDocument {
    let mut doc = CadDocument::new();
    doc.version = crate::types::DxfVersion::AC1014;
    doc.dwg_source_version = Some(crate::types::DxfVersion::AC1014);
    doc.header.source_handle_seed = Some(0x100);
    doc.layers.get_mut("0").unwrap().color = crate::types::Color::ByLayer;
    let mut line = Line::new();
    line.end = Vector3::new(1.0, 1.0, 0.0);
    line.common.color = crate::types::Color::from_rgb(10, 200, 30);
    line.common.line_weight = crate::types::LineWeight::Value(50);
    doc.add_entity(EntityType::Line(line)).unwrap();
    let mut circle = Circle::from_coords(0.0, 0.0, 0.0, 1.0);
    circle.common.color = crate::types::Color::ByLayer;
    doc.add_entity(EntityType::Circle(circle)).unwrap();
    let mut point = crate::entities::Point::default();
    point.common.color = crate::types::Color::Index(12);
    doc.add_entity(EntityType::Point(point)).unwrap();
    doc
}

#[test]
fn the_default_writes_the_native_colours_of_an_old_dwg() {
    let svg = write_model(&old_dwg_document()).unwrap();
    // The true colour and the line weight of the entity (50 / LineWeightRatio 50 in a unitless block), not the index
    // and the weight of the file's record.
    assert!(svg.contains("<line vector-effect=\"non-scaling-stroke\" stroke=\"rgb(10,200,30)\" stroke-width=\"1mm\""), "{svg}");
    // A ByLayer layer stays unresolved (black), and index 12 is the fork's ACI colour.
    assert!(svg.contains("<circle vector-effect=\"non-scaling-stroke\" stroke=\"rgb(0,0,0)\""), "{svg}");
    let (r, g, b) = crate::types::Color::Index(12).rgb().unwrap();
    assert!(svg.contains(&format!("fill=\"rgb({r},{g},{b})\"")), "{svg}");
}

#[cfg(feature = "acadsharp-compat")]
#[test]
fn the_acadsharp_view_writes_the_colours_acadsharp_reads_from_an_old_dwg() {
    let doc = old_dwg_document();
    let mut configuration = SvgConfiguration::test();
    configuration.acadsharp_view = true;
    let mut out = Vec::new();
    SvgWriter::new(&mut out, &doc).with_configuration(configuration).write().unwrap();
    let svg = String::from_utf8(out).unwrap();
    let rgb = |index: u8| {
        let [r, g, b] = crate::compat::acadsharp::view::INDEX_RGB[usize::from(index)];
        format!("rgb({r},{g},{b})")
    };
    let nearest = crate::types::aci_table::nearest_aci(10, 200, 30);
    // The nearest index, and ByLayer: the layer's Default weight (|-3| / LineWeightRatio 50).
    assert!(
        svg.contains(&format!("<line vector-effect=\"non-scaling-stroke\" stroke=\"{}\" stroke-width=\"0.06mm\"", rgb(nearest))),
        "{svg}"
    );
    assert!(svg.contains(&format!("<circle vector-effect=\"non-scaling-stroke\" stroke=\"{}\"", rgb(7))), "{svg}");
    assert!(svg.contains(&format!("fill=\"{}\"", rgb(12))), "{svg}");
    assert_ne!(rgb(12), {
        let (r, g, b) = crate::types::Color::Index(12).rgb().unwrap();
        format!("rgb({r},{g},{b})")
    }, "ACadSharp's palette differs from the fork's at index 12");
}

#[test]
fn mtext_lines_come_from_the_native_parser_by_default() {
    let mut doc = CadDocument::new();
    let mut mtext = crate::entities::MText::new();
    mtext.value = r"a\Pb{\fArial|b0;c}".into();
    doc.add_entity(EntityType::MText(Box::new(mtext))).unwrap();
    let svg = write_model(&doc).unwrap();
    assert!(svg.contains("dy=\"1em\">a</tspan>"), "{svg}");
    assert!(svg.contains("dy=\"1em\">bc</tspan>"), "{svg}");
    #[cfg(feature = "acadsharp-compat")]
    {
        // ACadSharp's text processor throws on a font control after the first character that has no `;`.
        let mut doc = CadDocument::new();
        let mut mtext = crate::entities::MText::new();
        mtext.value = r"a\fArial|b0".into();
        doc.add_entity(EntityType::MText(Box::new(mtext))).unwrap();
        assert!(write_model(&doc).is_ok());
        let mut configuration = SvgConfiguration::test();
        configuration.acadsharp_view = true;
        let error = SvgWriter::new(Vec::new(), &doc).with_configuration(configuration).write().unwrap_err();
        assert_eq!(error.csharp_exception(), "ArgumentOutOfRangeException");
    }
}

#[cfg(feature = "acadsharp-compat")]
#[test]
fn the_configuration_field_and_with_configuration_agree() {
    let doc = r12_document();
    let mut writer = SvgWriter::new(Vec::new(), &doc);
    assert!(!writer.paper_space_layouts().is_empty());
    writer.configuration.acadsharp_view = true;
    assert!(writer.paper_space_layouts().is_empty());
}
