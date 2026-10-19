// SPDX-License-Identifier: MPL-2.0
//! Template descriptions and native measurement survive versioned DWG writes.

use acadrust::types::DxfVersion;
use acadrust::{CadDocument, DwgReader, DwgWriter};
use std::io::Cursor;

#[test]
fn template_description_roundtrips_and_transcodes_without_changing_measurement() {
    for source in [DxfVersion::AC1018, DxfVersion::AC1032] {
        for target in [DxfVersion::AC1018, DxfVersion::AC1021, DxfVersion::AC1032] {
            let mut doc = CadDocument::with_version(target);
            doc.dwg_source_version = Some(source);
            doc.header.measurement = 1;
            let source_bytes: Vec<u8> = if source >= DxfVersion::AC1021 {
                vec![2, 0, b'A', 0, 0xE9, 0, 0, 0]
            } else {
                vec![2, 0, b'A', 0xE9, 0, 0]
            };
            doc.header.dwg_template_data = Some(source_bytes.into());
            let bytes = DwgWriter::write_to_vec(&doc).unwrap();
            let loaded = DwgReader::from_stream(Cursor::new(bytes)).read().unwrap();
            assert_eq!(loaded.header.measurement, 1, "{source:?} -> {target:?}");
            let expected: &[u8] = if target >= DxfVersion::AC1021 {
                &[2, 0, b'A', 0, 0xE9, 0, 1, 0]
            } else {
                &[2, 0, b'A', 0xE9, 1, 0]
            };
            assert_eq!(loaded.header.dwg_template_data.as_deref(), Some(expected));
        }
    }
}
