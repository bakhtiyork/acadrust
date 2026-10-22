//! Repro for issue #68: an AC1009/R12 DXF round-trip writes `$ACADVER` as
//! `UNKNOWN` and leaves the default MLEADERSTYLE pointing at a text style
//! handle the input reassigned to another record.

use acadrust::objects::ObjectType;
use acadrust::types::DxfVersion;
use acadrust::{DxfReader, DxfWriter};

/// The reporter's minimal R12 file: handle-less STYLE records, no entities.
const R12_REPRO: &str = "tests/issue68/r12_mleaderstyle_repro.dxf";

fn repro_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(R12_REPRO)
}

#[test]
fn ac1009_is_preserved_on_read_and_written_as_r12() {
    assert_eq!(DxfVersion::parse("AC1009"), Some(DxfVersion::AC1009));
    assert_eq!(DxfVersion::AC1009.as_str(), "AC1009");
    assert_eq!(DxfVersion::AC1009.version_code(), 1009);
    assert_eq!(DxfVersion::from_version_code(1009), DxfVersion::AC1009);
    // R12 sorts below R13 so every `>= AC1012` version gate excludes it.
    assert!(DxfVersion::AC1009 < DxfVersion::AC1012);

    let input = repro_path();

    let document = DxfReader::from_file(&input).unwrap().read().unwrap();
    assert_eq!(
        document.version,
        DxfVersion::AC1009,
        "AC1009 input must not degrade to Unknown"
    );

    let output = std::env::temp_dir().join("issue68_roundtrip.dxf");
    DxfWriter::new(&document).write_to_file(&output).unwrap();

    let text = std::fs::read_to_string(&output).unwrap();
    assert!(
        !text.contains("UNKNOWN"),
        "round-tripped file still declares an UNKNOWN version"
    );

    // An AC1009 document is written as an R12 file: no handles on table records, no `330` owner pointers or `100`
    // subclass markers, no CLASSES or OBJECTS section (R5a-6; the writer used to promote it to R13, upstream
    // issue #68).
    for pair in text.lines().collect::<Vec<_>>().chunks_exact(2) {
        assert!(
            !matches!(pair[0].trim(), "100" | "102" | "330" | "360"),
            "R13 group {} in an R12 file",
            pair[0].trim()
        );
    }
    assert!(!text.contains("OBJECTS") && !text.contains("CLASSES"));
    let reloaded = DxfReader::from_file(&output).unwrap().read().unwrap();
    assert_eq!(reloaded.version, DxfVersion::AC1009, "R12 input must be written as R12");
}

#[test]
fn mleaderstyle_text_style_handle_resolves_after_read() {
    let input = repro_path();

    let document = DxfReader::from_file(&input).unwrap().read().unwrap();

    let text_style_handles: std::collections::HashSet<u64> = document
        .text_styles
        .iter()
        .map(|style| style.handle.value())
        .collect();

    let mut checked = 0;
    for object in document.objects.values() {
        if let ObjectType::MultiLeaderStyle(style) = object {
            let Some(handle) = style.text_style_handle else {
                continue;
            };
            if handle.is_null() {
                continue;
            }
            checked += 1;
            assert!(
                text_style_handles.contains(&handle.value()),
                "MLEADERSTYLE {:?} text_style_handle {:?} does not resolve to a STYLE record",
                style.name,
                handle
            );
        }
    }
    assert!(checked > 0, "expected at least one MLEADERSTYLE to check");

    // And the reference must survive the write unchanged.
    let output = std::env::temp_dir().join("issue68_mleaderstyle.dxf");
    DxfWriter::new(&document).write_to_file(&output).unwrap();
    let reloaded = DxfReader::from_file(&output).unwrap().read().unwrap();

    let reloaded_handles: std::collections::HashSet<u64> = reloaded
        .text_styles
        .iter()
        .map(|style| style.handle.value())
        .collect();
    for object in reloaded.objects.values() {
        if let ObjectType::MultiLeaderStyle(style) = object {
            if let Some(handle) = style.text_style_handle.filter(|h| !h.is_null()) {
                assert!(
                    reloaded_handles.contains(&handle.value()),
                    "MLEADERSTYLE {:?} text_style_handle {:?} stale after write",
                    style.name,
                    handle
                );
            }
        }
    }
}
