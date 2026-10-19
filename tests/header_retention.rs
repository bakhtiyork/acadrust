// SPDX-License-Identifier: MPL-2.0
//! Synthetic header retention checks for the native DXF/DWG readers and writers.

use std::io::Cursor;

use acadrust::types::{DxfVersion, Handle, Vector3};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfWriter};

fn dxf_roundtrip(document: &CadDocument, binary: bool) -> CadDocument {
    let bytes = if binary {
        DxfWriter::new_binary(document)
    } else {
        DxfWriter::new(document)
    }
    .write_to_vec()
    .expect("DXF write");
    DxfReader::from_reader(Cursor::new(bytes))
        .expect("DXF reader")
        .read()
        .expect("DXF read")
}

#[test]
fn dxf_header_retains_discarded_dimension_and_ucs_values() {
    for binary in [false, true] {
        let mut document = CadDocument::with_version(DxfVersion::AC1032);
        let h = &mut document.header;
        h.create_ellipse_as_polyline = true;
        h.unit_mode = 1;
        h.dim_fit = 2;
        h.dim_text_arrow_fit = 4;
        h.dim_arc_length_symbol_position = 2;
        h.dim_unit = 7;
        h.dim_extension_line_length_fixed = true;
        h.dim_text_direction = 1;
        h.dim_alt_mzf = 0.25;
        h.dim_alt_mzs = Some(String::new());
        h.dim_mzf = 0.5;
        h.dim_mzs = Some("primary suffix".into());
        h.dim_text_style_name = "DimText".into();
        h.dim_linetype_name = "DimLine".into();
        h.dim_linetype1_name = "ExtOne".into();
        h.dim_linetype2_name = "ExtTwo".into();
        h.paper_space_ucs_base_name = "PaperBase".into();
        h.model_space_orthographic_top_origin = Vector3::new(1.0, 2.0, 3.0);
        h.model_space_orthographic_back_origin = Vector3::new(4.0, 5.0, 6.0);
        h.paper_space_orthographic_left_origin = Vector3::new(7.0, 8.0, 9.0);
        h.paper_space_orthographic_front_origin = Vector3::new(10.0, 11.0, 12.0);

        let read = dxf_roundtrip(&document, binary);
        let got = &read.header;
        assert!(got.create_ellipse_as_polyline);
        assert_eq!(got.unit_mode, 1);
        assert_eq!(got.dim_fit, 2);
        assert_eq!(got.dim_text_arrow_fit, 4);
        assert_eq!(got.dim_arc_length_symbol_position, 2);
        assert_eq!(got.dim_unit, 7);
        assert!(got.dim_extension_line_length_fixed);
        assert_eq!(got.dim_text_direction, 1);
        assert_eq!(got.dim_alt_mzf, 0.25);
        assert_eq!(got.dim_alt_mzs.as_deref(), Some(""));
        assert_eq!(got.dim_mzf, 0.5);
        assert_eq!(got.dim_mzs.as_deref(), Some("primary suffix"));
        assert_eq!(got.dim_text_style_name, "DimText");
        assert_eq!(got.dim_linetype_name, "DimLine");
        assert_eq!(got.dim_linetype1_name, "ExtOne");
        assert_eq!(got.dim_linetype2_name, "ExtTwo");
        assert_eq!(got.paper_space_ucs_base_name, "PaperBase");
        assert_eq!(
            got.model_space_orthographic_top_origin,
            Vector3::new(1.0, 2.0, 3.0)
        );
        assert_eq!(
            got.model_space_orthographic_back_origin,
            Vector3::new(4.0, 5.0, 6.0)
        );
        assert_eq!(
            got.paper_space_orthographic_left_origin,
            Vector3::new(7.0, 8.0, 9.0)
        );
        assert_eq!(
            got.paper_space_orthographic_front_origin,
            Vector3::new(10.0, 11.0, 12.0)
        );
    }
}

#[test]
fn dwg_header_roundtrip_retains_values_across_generations() {
    for version in [
        DxfVersion::AC1014,
        DxfVersion::AC1015,
        DxfVersion::AC1018,
        DxfVersion::AC1021,
        DxfVersion::AC1024,
        DxfVersion::AC1027,
        DxfVersion::AC1032,
    ] {
        let mut document = CadDocument::with_version(version);
        let h = &mut document.header;
        h.create_date_julian = 2_451_544.5;
        h.update_date_julian = 2_451_545.25;
        h.total_editing_time = 12.5;
        h.user_elapsed_time = 0.125;
        h.create_ellipse_as_polyline = true;
        h.unit_mode = 1;
        h.dim_unit = 7;
        h.paper_space_ucs_handle = Handle::new(0x31);
        h.model_space_ucs_handle = Handle::new(0x32);
        h.model_space_orthographic_top_origin = Vector3::new(1.0, 2.0, 3.0);
        h.model_space_orthographic_bottom_origin = Vector3::new(4.0, 5.0, 6.0);
        h.model_space_orthographic_left_origin = Vector3::new(7.0, 8.0, 9.0);
        h.model_space_orthographic_right_origin = Vector3::new(10.0, 11.0, 12.0);
        h.model_space_orthographic_front_origin = Vector3::new(13.0, 14.0, 15.0);
        h.model_space_orthographic_back_origin = Vector3::new(16.0, 17.0, 18.0);
        h.paper_space_orthographic_top_origin = Vector3::new(19.0, 20.0, 21.0);
        h.paper_space_orthographic_bottom_origin = Vector3::new(22.0, 23.0, 24.0);
        h.paper_space_orthographic_left_origin = Vector3::new(25.0, 26.0, 27.0);
        h.paper_space_orthographic_right_origin = Vector3::new(28.0, 29.0, 30.0);
        h.paper_space_orthographic_front_origin = Vector3::new(31.0, 32.0, 33.0);
        h.paper_space_orthographic_back_origin = Vector3::new(34.0, 35.0, 36.0);
        h.dim_alt_mzs = Some(String::new());
        h.dim_mzs = Some("suffix".into());
        let expected_create_ellipse = h.create_ellipse_as_polyline;
        let expected_unit_mode = h.unit_mode;
        let expected_dim_unit = h.dim_unit;
        let expected_paper_ucs = h.paper_space_ucs_handle;
        let expected_model_ucs = h.model_space_ucs_handle;

        let bytes = DwgWriter::write_to_vec(&document).expect("DWG write");
        let read = DwgReader::from_stream(Cursor::new(bytes))
            .read()
            .expect("DWG read");
        let got = &read.header;
        assert_eq!(
            got.create_ellipse_as_polyline, expected_create_ellipse,
            "{version:?}"
        );
        assert_eq!(got.unit_mode, expected_unit_mode, "{version:?}");
        let expected_dim_unit = if version <= DxfVersion::AC1014 {
            expected_dim_unit
        } else {
            2
        };
        assert_eq!(got.dim_unit, expected_dim_unit, "{version:?}");
        assert_eq!(
            got.paper_space_ucs_handle, expected_paper_ucs,
            "{version:?}"
        );
        assert_eq!(
            got.model_space_ucs_handle, expected_model_ucs,
            "{version:?}"
        );
        assert_eq!(
            got.dwg_timestamps,
            Some([
                (2_451_544, 43_200_000),
                (2_451_545, 21_600_000),
                (12, 43_200_000),
                (0, 10_800_000),
            ]),
            "{version:?}"
        );
        if version >= DxfVersion::AC1015 {
            assert_eq!(
                got.model_space_orthographic_top_origin,
                Vector3::new(1.0, 2.0, 3.0),
                "{version:?}"
            );
            assert_eq!(
                got.model_space_orthographic_bottom_origin,
                Vector3::new(4.0, 5.0, 6.0),
                "{version:?}"
            );
            assert_eq!(
                got.model_space_orthographic_left_origin,
                Vector3::new(7.0, 8.0, 9.0),
                "{version:?}"
            );
            assert_eq!(
                got.model_space_orthographic_right_origin,
                Vector3::new(10.0, 11.0, 12.0),
                "{version:?}"
            );
            assert_eq!(
                got.model_space_orthographic_front_origin,
                Vector3::new(13.0, 14.0, 15.0),
                "{version:?}"
            );
            assert_eq!(
                got.model_space_orthographic_back_origin,
                Vector3::new(16.0, 17.0, 18.0),
                "{version:?}"
            );
            assert_eq!(
                got.paper_space_orthographic_top_origin,
                Vector3::new(19.0, 20.0, 21.0),
                "{version:?}"
            );
            assert_eq!(
                got.paper_space_orthographic_bottom_origin,
                Vector3::new(22.0, 23.0, 24.0),
                "{version:?}"
            );
            assert_eq!(
                got.paper_space_orthographic_left_origin,
                Vector3::new(25.0, 26.0, 27.0),
                "{version:?}"
            );
            assert_eq!(
                got.paper_space_orthographic_right_origin,
                Vector3::new(28.0, 29.0, 30.0),
                "{version:?}"
            );
            assert_eq!(
                got.paper_space_orthographic_front_origin,
                Vector3::new(31.0, 32.0, 33.0),
                "{version:?}"
            );
            assert_eq!(
                got.paper_space_orthographic_back_origin,
                Vector3::new(34.0, 35.0, 36.0),
                "{version:?}"
            );
        }
        if version >= DxfVersion::AC1024 {
            assert_eq!(got.dim_alt_mzs.as_deref(), Some(""), "{version:?}");
            assert_eq!(got.dim_mzs.as_deref(), Some("suffix"), "{version:?}");
        }
    }
}
