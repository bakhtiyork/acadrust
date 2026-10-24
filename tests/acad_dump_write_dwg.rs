//! `acad-dump write-dwg`: the fork's side of the DWG round-trip gate (G5). Same contract as AcadDump's `write-dwg`:
//! failsafe read, optional `--version`, optional `--pin` of the volatile header and summary values.
#![cfg(feature = "acadsharp-compat")]

use acadrust::entities::{EntityType, Line};
use acadrust::io::dwg::DwgReader;
use acadrust::types::{DxfVersion, Vector3};
use acadrust::{CadDocument, DwgWriter};
use std::path::PathBuf;
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("write-dwg");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn source(name: &str) -> PathBuf {
    let mut doc = CadDocument::with_version(DxfVersion::AC1032);
    let mut line = Line::new();
    line.start = Vector3::new(1.0, 2.0, 3.0);
    line.end = Vector3::new(4.0, 5.0, 6.0);
    doc.add_entity(EntityType::Line(line)).unwrap();
    // A named (color book) color: before R2004 it goes into AutoCAD's round-trip record of the colour.
    let mut named = Line::new();
    named.common.color = acadrust::types::Color::from_rgb(1, 2, 3);
    named.common.color_name = Some("DIC COLOR GUIDE(R)$DIC 14".into());
    doc.add_entity(EntityType::Line(named)).unwrap();
    // A name without a color value: the library refuses it before R2004; the tool drops the name, as ACadSharp does.
    let mut unvalued = Line::new();
    unvalued.common.color_name = Some("DIC COLOR GUIDE(R)$DIC 15".into());
    doc.add_entity(EntityType::Line(unvalued)).unwrap();
    doc.header.fingerprint_guid = "{11111111-2222-3333-4444-555555555555}".into();
    doc.header.total_editing_time = 0.25;
    let path = scratch(name);
    DwgWriter::write_to_file(&path, &doc).unwrap();
    path
}

fn run(args: &[&std::ffi::OsStr]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_acad-dump"))
        .args(args)
        .output()
        .expect("run acad-dump")
}

fn read(path: &std::path::Path) -> CadDocument {
    DwgReader::from_file(path).unwrap().read().unwrap()
}

#[test]
fn write_dwg_changes_the_version_and_pins_the_volatile_values() {
    let input = source("source-pinned.dwg");
    for (version, name) in [
        (DxfVersion::AC1014, "AC1014"),
        (DxfVersion::AC1015, "AC1015"),
        (DxfVersion::AC1018, "AC1018"),
        (DxfVersion::AC1024, "AC1024"),
        (DxfVersion::AC1032, "AC1032"),
    ] {
        let output = scratch(&format!("{name}.dwg"));
        let _ = std::fs::remove_file(&output);
        let result = run(&[
            "write-dwg".as_ref(),
            input.as_os_str(),
            output.as_os_str(),
            "--version".as_ref(),
            name.as_ref(),
            "--pin".as_ref(),
        ]);
        assert!(result.status.success(), "{name}: {}", String::from_utf8_lossy(&result.stderr));
        let document = read(&output);
        assert_eq!(document.version, version);
        let names: Vec<Option<String>> = document
            .entities()
            .filter(|e| matches!(e, EntityType::Line(_)))
            .map(|e| e.common().color_name.clone())
            .collect();
        assert_eq!(names.len(), 3, "{name}: all three lines, the named colors too");
        assert!(names.contains(&Some("DIC COLOR GUIDE(R)$DIC 14".into())), "{name}: the valued name stays: {names:?}");
        assert_eq!(
            names.contains(&Some("DIC COLOR GUIDE(R)$DIC 15".into())),
            version >= DxfVersion::AC1018,
            "{name}: the name without a value only from R2004: {names:?}"
        );
        // Pinned: 2000-01-01, no editing time, zero GUIDs.
        assert_eq!(document.header.total_editing_time, 0.0, "{name}");
        // R14 headers have no GUIDs.
        let guid = if version == DxfVersion::AC1014 { "" } else { "00000000-0000-0000-0000-000000000000" };
        assert_eq!(document.header.fingerprint_guid, guid, "{name}");
        assert_eq!(document.header.version_guid, guid, "{name}");
        assert_eq!(document.header.create_date_julian, document.header.update_date_julian, "{name}");
    }
}

#[test]
fn write_dwg_without_pin_keeps_the_header_values() {
    let input = source("source-unpinned.dwg");
    let output = scratch("unpinned.dwg");
    let _ = std::fs::remove_file(&output);
    let result = run(&["write-dwg".as_ref(), input.as_os_str(), output.as_os_str()]);
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    let document = read(&output);
    assert_eq!(document.version, DxfVersion::AC1032, "the version of the source");
    assert_eq!(document.header.fingerprint_guid, "{11111111-2222-3333-4444-555555555555}");
    assert_eq!(document.header.total_editing_time, 0.25);
}

#[test]
fn write_dwg_reports_read_failures_and_usage_errors_with_distinct_exit_codes() {
    let missing = scratch("missing.dwg");
    let _ = std::fs::remove_file(&missing);
    let output = scratch("never.dwg");
    let _ = std::fs::remove_file(&output);
    let result = run(&["write-dwg".as_ref(), missing.as_os_str(), output.as_os_str()]);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).starts_with("read failed: "));
    assert!(!output.exists(), "nothing is written when the read failed");

    let input = source("source-usage.dwg");
    for args in [
        vec!["write-dwg".as_ref(), input.as_os_str()],
        vec!["write-dwg".as_ref(), input.as_os_str(), output.as_os_str(), "--version".as_ref(), "AC9999".as_ref()],
        vec!["write-dwg".as_ref(), input.as_os_str(), output.as_os_str(), "--version".as_ref(), "AD1032".as_ref()],
        vec!["write-dwg".as_ref(), input.as_os_str(), output.as_os_str(), "--bogus".as_ref()],
    ] {
        assert_eq!(run(&args).status.code(), Some(2), "{args:?}");
    }
    assert!(!output.exists());
}
