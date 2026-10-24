//! ACIS/SAT solid modeler data parser and writer.
//!
//! This module provides support for parsing and generating ACIS SAT (Save And
//! Restore) format data, which is the text-based geometry representation used
//! by 3DSOLID, BODY, and REGION entities in DXF/DWG files.
//!
//! # Supported Versions
//!
//! - **ACIS 1.5–6.0** (SAT versions 400–600): Legacy format without explicit
//!   record indices.
//! - **ACIS 7.0** (SAT version 700): Adds explicit negative record indices,
//!   `asmheader` entity, and `@`-prefixed counted strings.
//! - **ACIS 21.0+** (SAT version 21800): Modern format with extended entity types.
//!
//! # Format Overview
//!
//! A SAT file consists of:
//! 1. A **header** (3–4 lines): version, product info, tolerances
//! 2. **Entity records**: each terminated by `#`, representing the B-rep
//!    topology (body → lump → shell → face → loop → coedge → edge → vertex)
//!    and associated geometry (surfaces, curves, points, transforms).
//!
//! # Example
//!
//! ```rust
//! use acadrust::entities::acis::{SatDocument, SatVersion};
//!
//! // Parse SAT text from a 3DSOLID entity
//! let sat_text = "700 0 1 0\n\
//!     @12 Spatial Corp @7 ACIS 7.0 @24 Thu Jan 01 00:00:00 2023\n\
//!     1e-06 9.9999999999999995e-07\n\
//!     -0 asmheader $-1 -1 @12 700 7 0 0 @5 ACIS @3 7.0 @24 Thu Jan 01 00:00:00 2023 #\n\
//!     -1 body $-1 $-1 $-1 $-1 #\n\
//!     End-of-ACIS-data\n";
//!
//! let doc = SatDocument::parse(sat_text).unwrap();
//! assert_eq!(doc.header.version, SatVersion::new(7, 0, 0));
//! assert_eq!(doc.records.len(), 2);
//!
//! // Write back to SAT text
//! let output = doc.to_sat_string();
//! assert!(output.contains("700"));
//! ```

pub mod parser;
pub mod primitives;
pub(crate) mod reversion;
pub mod sab;
pub mod types;
pub mod writer;

pub use parser::SatParser;
pub use sab::{SabReader, SabWriter};
pub use types::*;
pub use writer::SatWriter;

/// Whether binary ACIS data (`sab`) and SAT text (`sat`) state the same model: both parse, and both print the same
/// SAT text. A DXF of R2000-R2010 carries SAT only, so a DWG's binary data is written as text; the round-trip gate
/// (`acad-dump acis-same`) uses this to verify that the conversion lost nothing. `Err` names the first difference.
pub fn sab_matches_sat(sab: &[u8], sat: &str) -> Result<(), String> {
    sab_matches_sat_with(sab, sat, false)
}

/// [`sab_matches_sat`], optionally leaving the date of the header out: a SAB written from a SAT without a date states
/// the drawing's update date (the binary header needs one), which the text does not have.
pub fn sab_matches_sat_with(sab: &[u8], sat: &str, ignore_header_date: bool) -> Result<(), String> {
    let mut binary = SabReader::read(sab).map_err(|error| format!("the binary data does not parse: {error}"))?;
    let mut text = SatParser::parse(sat).map_err(|error| format!("the SAT text does not parse: {error}"))?;
    if ignore_header_date {
        binary.header.date.clear();
        text.header.date.clear();
    }
    if binary.to_sat_string() == text.to_sat_string() {
        Ok(())
    } else {
        Err("the models print different SAT text".to_string())
    }
}

/// Downgrade ACIS v600+ record token layouts to v400 format.
///
/// ACIS 6.0 added extra fields to some entity types (most notably `edge`).
/// When downgrading SAT output for DXF compatibility, these extra fields
/// must be stripped.
///
/// Detection is based on actual token count rather than version header,
/// because some SAT data has a v400 header but v600 record layouts
/// (e.g., "ACIS 6.00" builder with version=400 header).
///
/// **edge** layout change (after sentinel normalization):
/// - v400 (6 tokens): `$sentinel $sv $ev $coedge $curve sense`
/// - v600 (9 tokens): `$sentinel $sv start_param $ev end_param $coedge $curve sense "unknown"`
///
/// The v600 "unknown" string becomes `7 unknown` in legacy SAT (counted string),
/// which BricsCAD's v400 parser misinterprets as extra tokens.
pub fn downgrade_records_to_v400(records: &mut Vec<SatRecord>) {
    for record in records.iter_mut() {
        if record.entity_type == "edge" {
            // v600 edge tokens (after sentinel normalization):
            //   [0] sentinel $-1
            //   [1] $sv
            //   [2] start_param (Float)      ← remove
            //   [3] $ev
            //   [4] end_param (Float)        ← remove
            //   [5] $coedge
            //   [6] $curve
            //   [7] sense (Enum)
            //   [8] "unknown" (String)       ← remove
            // Result: [0]sentinel [1]$sv [2]$ev [3]$coedge [4]$curve [5]sense
            let len = record.tokens.len();
            if len >= 9 {
                // Remove from end first to preserve indices
                record.tokens.truncate(len - 1); // remove "unknown" string
                record.tokens.remove(4); // remove end_param (was index 4)
                record.tokens.remove(2); // remove start_param (was index 2)
            }
        }
    }
}

#[cfg(test)]
mod sab_sat_tests {
    use super::*;

    const SAT: &str = "21200 2 1 26\n0  0  0 \n1 0.000001 0.0000000001\n\
        face $-1 -1 $-1 $-1 $-1 $-1 forward double out #\n\
        transform $-1 -1 1 0 0 0 1 0 0 0 1 5 6 7 1 no_rotate no_reflect no_shear #\n\
        End-of-ACIS-data\n";

    #[test]
    fn binary_data_matches_its_text_and_nothing_else() {
        let document = SatDocument::parse(SAT).unwrap();
        let sab = SabWriter::write(&document);
        let text = document.to_sat_string();
        assert_eq!(sab_matches_sat(&sab, &text), Ok(()));
        // The same model, wrapped between tokens as the DXF writer wraps long records.
        assert_eq!(sab_matches_sat(&sab, &text.replace(" forward ", "\n forward ")), Ok(()));
        // Another model, another file, and nonsense.
        assert!(sab_matches_sat(&sab, &text.replace("forward", "reversed")).is_err());
        assert!(sab_matches_sat(&sab, "not sat").is_err());
        assert!(sab_matches_sat(&sab[..sab.len() / 2], &text).is_err());
    }
}
