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
fn write_dxf_writes_the_version_it_is_given() {
    let input = source();
    for (name, version, header) in [("v2000.dxf", "AC1015", "AC1015"), ("v2004.dxf", "AC1018", "AC1018"), ("v14.dxf", "AC1014", "AC1014")] {
        let output = scratch(name);
        let _ = std::fs::remove_file(&output);
        let result = run(&["write-dxf".as_ref(), input.as_os_str(), output.as_os_str(), "--version".as_ref(), version.as_ref()]);
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        let text = String::from_utf8_lossy(&std::fs::read(&output).unwrap()).replace("\r\n", "\n");
        assert!(text.contains(&format!("$ACADVER\n  1\n{header}\n")), "{version}");
        assert_eq!(lines_in(&output), 1, "{version}");
    }
    // an unknown version or a missing one is a usage error and writes nothing
    let output = scratch("never-version.dxf");
    let _ = std::fs::remove_file(&output);
    for args in [["--version", "ACXXXX"], ["--version", "2000"]] {
        let result = run(&["write-dxf".as_ref(), input.as_os_str(), output.as_os_str(), args[0].as_ref(), args[1].as_ref()]);
        assert_eq!(result.status.code(), Some(2), "{args:?}");
    }
    assert!(!output.exists());
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

#[test]
fn acis_same_judges_binary_acis_data_against_sat_text() {
    use acadrust::entities::acis::{SabWriter, SatDocument};
    let sat = "21200 2 1 26\n0  0  0 \n1 0.000001 0.0000000001\n\
        face $-1 -1 $-1 $-1 $-1 $-1 forward double out #\n\
        transform $-1 -1 1 0 0 0 1 0 0 0 1 5 6 7 1 no_rotate no_reflect no_shear #\n\
        End-of-ACIS-data\n";
    let document = SatDocument::parse(sat).unwrap();
    let hex: String = SabWriter::write(&document).iter().map(|byte| format!("{byte:02x}")).collect();
    let hex_file = scratch("acis.hex");
    let sat_file = scratch("acis.sat");
    std::fs::write(&hex_file, &hex).unwrap();
    // The same model, and the same model with a record wrapped between tokens, are the same.
    for text in [document.to_sat_string(), document.to_sat_string().replace(" forward ", "\n forward ")] {
        std::fs::write(&sat_file, text).unwrap();
        let result = run(&["acis-same".as_ref(), hex_file.as_os_str(), sat_file.as_os_str()]);
        assert_eq!(result.status.code(), Some(0), "{}", String::from_utf8_lossy(&result.stdout));
        assert_eq!(String::from_utf8_lossy(&result.stdout).trim(), "same");
    }
    // Another model, and text that is not SAT, are not.
    for text in [document.to_sat_string().replace("forward", "reversed"), "not sat".to_string()] {
        std::fs::write(&sat_file, text).unwrap();
        let result = run(&["acis-same".as_ref(), hex_file.as_os_str(), sat_file.as_os_str()]);
        assert_eq!(result.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&result.stdout).starts_with("differ:"));
    }
    // Hex that is not hex is a usage error.
    std::fs::write(&hex_file, "zz").unwrap();
    assert_eq!(run(&["acis-same".as_ref(), hex_file.as_os_str(), sat_file.as_os_str()]).status.code(), Some(2));
}
