//! `acad-dump write-dxf`: the fork's side of the DXF round-trip gate (G4). Reads with the dump command's
//! failsafe options and writes the document back, ASCII or binary.
#![cfg(feature = "acadsharp-compat")]

use acadrust::entities::{EntityType, Line};
use acadrust::io::dxf::DxfReader;
use acadrust::types::Vector3;
use acadrust::{CadDocument, DxfWriter};
use std::path::PathBuf;
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("write-dxf");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

fn source() -> PathBuf {
    let mut doc = CadDocument::new();
    let mut line = Line::new();
    line.start = Vector3::new(1.0, 2.0, 3.0);
    line.end = Vector3::new(4.0, 5.0, 6.0);
    doc.add_entity(EntityType::Line(line)).unwrap();
    let path = scratch("source.dxf");
    DxfWriter::new(&doc).write_to_file(&path).unwrap();
    path
}

fn run(args: &[&std::ffi::OsStr]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_acad-dump"))
        .args(args)
        .output()
        .expect("run acad-dump")
}

fn lines_in(path: &std::path::Path) -> usize {
    DxfReader::from_file(path)
        .unwrap()
        .read()
        .unwrap()
        .entities()
        .filter(|entity| matches!(entity, EntityType::Line(_)))
        .count()
}

#[test]
fn write_dxf_writes_ascii_and_binary_files_that_read_back() {
    let input = source();
    for (name, binary) in [("ascii.dxf", false), ("binary.dxf", true)] {
        let output = scratch(name);
        let _ = std::fs::remove_file(&output);
        let mut args: Vec<&std::ffi::OsStr> = vec!["write-dxf".as_ref(), input.as_os_str(), output.as_os_str()];
        if binary {
            args.push("--binary".as_ref());
        }
        let result = run(&args);
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        let bytes = std::fs::read(&output).unwrap();
        assert_eq!(bytes.starts_with(b"AutoCAD Binary DXF"), binary);
        assert_eq!(lines_in(&output), 1);
    }
}

#[test]
fn write_dxf_reports_read_failures_and_usage_errors_with_distinct_exit_codes() {
    let garbage = scratch("missing.dxf");
    let _ = std::fs::remove_file(&garbage);
    let output = scratch("never.dxf");
    let _ = std::fs::remove_file(&output);
    let result = run(&["write-dxf".as_ref(), garbage.as_os_str(), output.as_os_str()]);
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).starts_with("read failed: "));
    assert!(!output.exists(), "nothing is written when the read failed");

    let result = run(&["write-dxf".as_ref(), garbage.as_os_str()]);
    assert_eq!(result.status.code(), Some(2));
}
