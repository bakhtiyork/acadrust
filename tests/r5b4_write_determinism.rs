// SPDX-License-Identifier: MPL-2.0
//! R5b-4 review (gate G5): a DWG write of one document is the same bytes every time. The object map's iteration order
//! changes with each map the reader builds (and from one process to the next), and the classes the writer appended for a
//! DXF source written at another version below R2013 followed it (`acad-dump write-dwg --pin` of
//! dynamic-blocks/BLOCKFLIPPARAMETER.dxf at AC1018 wrote 32,844 or 32,876 bytes). Embedded fixtures only.

use std::io::Cursor;

use acadrust::types::DxfVersion;
use acadrust::{CadDocument, DwgReader, DwgWriter, DxfReader, DxfWriter};

#[path = "support/dynamic_property_fixtures.rs"]
mod fixture_data;

/// AutoCAD's DWG of a dynamic block, written as DXF: the DXF text that the repeated reads start from.
fn dxf_text(name: &str) -> Vec<u8> {
    let document = DwgReader::from_stream(Cursor::new(fixture_data::fixture(name))).read().expect("DWG read");
    DxfWriter::new(&document).write_to_vec().expect("DXF write")
}

/// A fresh read of the DXF (a new object map, so a new iteration order) written as DWG at `version`.
fn write_fresh_read(dxf: &[u8], version: DxfVersion) -> Vec<u8> {
    let mut document: CadDocument =
        DxfReader::from_reader(Cursor::new(dxf.to_vec())).expect("DXF reader").read().expect("DXF read");
    document.version = version;
    DwgWriter::write_to_vec(&document).unwrap_or_else(|error| panic!("{version:?}: DWG write failed: {error}"))
}

#[test]
fn a_dxf_source_written_at_another_version_is_the_same_bytes_every_time() {
    for name in ["BLOCKFLIPPARAMETER", "BLOCKVISIBILITYPARAMETER"] {
        let dxf = dxf_text(name);
        for version in [DxfVersion::AC1014, DxfVersion::AC1015, DxfVersion::AC1018, DxfVersion::AC1024] {
            let first = write_fresh_read(&dxf, version);
            for run in 1..8 {
                let again = write_fresh_read(&dxf, version);
                assert!(
                    again == first,
                    "{name} at {version:?}: write {run} differs from the first ({} against {} bytes)",
                    again.len(),
                    first.len()
                );
            }
        }
    }
}
