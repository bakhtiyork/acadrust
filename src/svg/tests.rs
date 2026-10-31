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
        w.write_attribute_string("v", "<&>\"\t\n")?;
        w.write_string("<&>\"\u{1}")?;
        w.write_end_element()
    });
    assert_eq!(s, "<a v=\"&lt;&amp;&gt;&quot;&#x9;&#xA;\"><&amp;&gt;\"&#x1;</a>".replace("<&amp;", "&lt;&amp;"));
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
    let circle = Circle::from_coords(20.0, 20.0, 0.0, 5.0);
    let circle_handle = doc.add_entity(EntityType::Circle(circle)).unwrap().value();
    let svg = write_model(&doc).unwrap();
    let expected = [
        "<?xml version=\"1.0\"?>".to_owned(),
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"25\" height=\"25\" viewBox=\"0 0 25 25\" transform=\"scale(1,-1)\">".to_owned(),
        format!("  <!--LINE | {line_handle}-->"),
        "  <line vector-effect=\"non-scaling-stroke\" stroke=\"rgb(255,255,255)\" stroke-width=\"0.06mm\" x1=\"0\" y1=\"0\" x2=\"10\" y2=\"5\" />".to_owned(),
        format!("  <!--CIRCLE | {circle_handle}-->"),
        "  <circle vector-effect=\"non-scaling-stroke\" stroke=\"rgb(255,255,255)\" stroke-width=\"0.06mm\" r=\"5\" cx=\"20\" cy=\"20\" fill=\"none\" />".to_owned(),
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
        super::model::Model::new(&doc, false),
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
