use std::io::Cursor;

use acadrust::{DxfReader, DxfWriter, Handle};

const DXF: &str = "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nVPORT\n5\n8\n102\n{ACAD_XDICTIONARY\n360\nA8\n102\n}\n102\n{ACAD_REACTORS\n330\nB6\n330\nB7\n102\n}\n330\n0\n100\nAcDbSymbolTable\n70\n1\n1001\nACAD\n1000\nDbSaveVer\n1070\n379\n0\nVPORT\n5\n10\n330\n8\n100\nAcDbSymbolTableRecord\n100\nAcDbViewportTableRecord\n2\n*Active\n10\n0\n20\n0\n11\n1\n21\n1\n40\n1\n41\n1\n0\nENDTAB\n0\nENDSEC\n0\nEOF\n";

#[test]
fn table_header_xdata_and_defined_groups_use_control_handle() {
    let document = DxfReader::from_reader(Cursor::new(DXF.as_bytes()))
        .expect("reader")
        .read()
        .expect("document");

    let table = Handle::new(8);
    let xdata = document.object_xdata(table).expect("table-header xdata");
    let record = xdata.get_record("ACAD").expect("ACAD record");
    assert!(record
        .values
        .iter()
        .any(|value| matches!(value, acadrust::xdata::XDataValue::Integer16(379))));
    assert_eq!(
        document.extension_dictionary_handle(table),
        Some(Handle::new(0xA8))
    );

    // Header XDATA must be written back under the table control handle and
    // remain readable after a complete DXF round trip.
    let bytes = DxfWriter::new(&document).write_to_vec().expect("write");
    let restored = DxfReader::from_reader(Cursor::new(bytes))
        .expect("round-trip reader")
        .read()
        .expect("round-trip document");
    assert!(restored
        .object_xdata(table)
        .and_then(|data| data.get_record("ACAD"))
        .is_some());
    assert_eq!(
        restored.extension_dictionary_handle(table),
        Some(Handle::new(0xA8))
    );
}
