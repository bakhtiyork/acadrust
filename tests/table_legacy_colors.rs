// SPDX-License-Identifier: MPL-2.0
use acadrust::entities::table::{CellStyle, Table, TableCell};
use acadrust::entities::EntityType;
use acadrust::io::dwg::dwg_reference_type::DwgReferenceType::HardPointer;
use acadrust::io::dwg::dwg_stream_readers::merged_reader::DwgMergedReader;
use acadrust::io::dwg::dwg_stream_readers::object_reader::entities::read_table;
use acadrust::io::dwg::dwg_stream_writers::merged_writer::DwgMergedWriter;
use acadrust::io::dwg::dwg_version::DwgVersion;
use acadrust::types::{Color, Vector3};
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfVersion};
use std::io::Cursor;

#[test]
fn down_saved_table_full_color_payload_keeps_following_cells_aligned() {
    for (version, dxf) in [
        (DwgVersion::AC12, DxfVersion::AC1014),
        (DwgVersion::AC15, DxfVersion::AC1015),
    ] {
        let mut w = DwgMergedWriter::new(version, dxf);
        w.write_3bit_double(Vector3::ZERO);
        if version.r13_14_only() {
            for _ in 0..3 {
                w.write_bit_double(1.0);
            }
        } else {
            w.write_2bits(3);
        }
        w.write_bit_double(0.0);
        w.write_3bit_double(Vector3::UNIT_Z);
        w.write_bit(false);
        w.write_handle(HardPointer, 0);
        w.write_bit_short(22);
        w.write_3bit_double(Vector3::UNIT_X);
        w.write_bit_long(2);
        w.write_bit_long(1);
        w.write_bit_double(10.0);
        w.write_bit_double(20.0);
        w.write_bit_double(5.0);
        w.write_handle(HardPointer, 0);
        for text in ["colored", "following"] {
            w.write_bit_short(1);
            w.write_byte(0);
            w.write_bit(false);
            w.write_bit(false);
            w.write_bit_long(1);
            w.write_bit_long(1);
            w.write_bit_double(0.0);
            w.write_handle(HardPointer, 0);
            w.write_variable_text(text);
            w.write_bit(text == "colored");
            if text == "colored" {
                w.write_bit_long(0x04 | 0x08 | 0x20 | 0x40 | 0x400);
                w.write_byte(0);
                // Explicit CMC wire fields, independent of the table writer.
                for rgb in [0xC200_1122u32, 0xC300_0007] {
                    w.write_bit_short(0);
                    w.write_bit_long(rgb as i32);
                    w.write_byte(0);
                }
                w.write_bit_double(4.0);
                w.write_bit_short(0);
                w.write_bit_long(0xC300_003F_u32 as i32);
                w.write_byte(0);
                w.write_bit_short(40);
                w.write_bit_short(0);
            }
        }
        for _ in 0..4 {
            w.write_bit(false);
        }
        let bytes = w.merge();
        let mut reader = DwgMergedReader::new(bytes, dxf, w.handle_start_bits());
        let table = read_table(&mut reader, version, dxf);
        assert_eq!(table.rows.len(), 1, "{dxf:?}");
        let cells = &table.rows[0].cells;
        let style = cells[0].style.as_ref().unwrap();
        assert_eq!(style.background_color, Color::from_rgb(0, 17, 34));
        assert_eq!(style.content_color, Color::from_index(7));
        assert_eq!(style.text_height, 4.0);
        assert_eq!(style.top_border.color, Color::from_index(63));
        assert_eq!(style.top_border.line_weight.value(), 40);
        assert!(style.top_border.invisible);
        assert_eq!(cells[1].text_value(), "following");
        assert_eq!(cells[1].merge_width, 1);
        assert!(cells[1].style.is_none());
    }
}

#[test]
fn down_saved_table_writer_preserves_true_color_in_legacy_cells() {
    for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018] {
        let mut table = Table::new(Vector3::ZERO, 1, 2);
        table.rows[0].cells[0] = TableCell::text("colored");
        table.rows[0].cells[1] = TableCell::text("following");
        let mut style = CellStyle::new();
        style.legacy_override_bits = true;
        style.override_flags = 0x04 | 0x08;
        style.background_color = Color::from_rgb(1, 23, 45);
        style.content_color = Color::from_rgb(67, 89, 101);
        table.rows[0].cells[0].style = Some(style);
        let mut doc = CadDocument::with_version(version);
        let h = doc.add_entity(EntityType::Table(Box::new(table))).unwrap();
        let bytes = DwgWriter::write_to_vec(&doc).unwrap();
        let loaded = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
        let EntityType::Table(t) = loaded.get_entity(h).unwrap() else {
            panic!()
        };
        let style = t.rows[0].cells[0].style.as_ref().unwrap();
        assert_eq!(style.background_color, Color::from_rgb(1, 23, 45));
        assert_eq!(style.content_color, Color::from_rgb(67, 89, 101));
        assert_eq!(t.rows[0].cells[1].text_value(), "following");
    }
}
