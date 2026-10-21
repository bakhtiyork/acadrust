// SPDX-License-Identifier: MPL-2.0
//! The elevation a DWG dimension record stores once is the Z of its text
//! middle point and of its insertion point (points 11 and 12).
use acadrust::entities::dimension::{Dimension, DimensionLinear};
use acadrust::entities::EntityType;
use acadrust::types::{DxfVersion, Vector3};
use acadrust::{CadDocument, DwgReader, DwgWriter};
use std::io::Cursor;

#[test]
fn dwg_dimension_insertion_point_takes_the_record_elevation() {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut dim = DimensionLinear::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
    dim.base.text_middle_point = Vector3::new(5.0, 2.0, -208.0);
    dim.base.insertion_point = Vector3::new(1.5, 2.5, 0.0);
    doc.add_entity(EntityType::Dimension(Box::new(Dimension::Linear(dim))))
        .unwrap();
    let bytes = DwgWriter::write_to_vec(&doc).expect("DWG write");
    let read = DwgReader::from_stream(Cursor::new(bytes)).read().expect("DWG read");
    let Some(EntityType::Dimension(dimension)) = read
        .entities()
        .find(|e| matches!(e, EntityType::Dimension(_)))
    else {
        panic!("dimension survives the round trip");
    };
    let base = dimension.base();
    assert_eq!(base.text_middle_point.z, -208.0);
    assert_eq!(base.insertion_point.x, 1.5);
    assert_eq!(base.insertion_point.y, 2.5);
    assert_eq!(base.insertion_point.z, -208.0);
}
