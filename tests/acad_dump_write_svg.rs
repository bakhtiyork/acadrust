//! `acad-dump write-svg`: the fork's side of the SVG gate (G6). The tool contract is that of CadKit's
//! plans/P10-svg-writer.md: `model.svg`, `layout-NN.svg`, `block-NNNN.svg` and `index.tsv`, a status of `ok` or
//! `!<C# exception type>`, no file for a failed write, exit 0 when the read worked, 1 when it failed, 2 on usage.
#![cfg(feature = "acadsharp-compat")]

use acadrust::entities::{EntityType, Line, LwPolyline, LwVertex};
use acadrust::types::Vector3;
use acadrust::{CadDocument, DxfWriter};
use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("write-svg").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(args: &[&std::ffi::OsStr]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_acad-dump")).args(args).output().expect("run acad-dump")
}

fn dxf(dir: &Path, doc: &CadDocument) -> PathBuf {
    let path = dir.join("source.dxf");
    DxfWriter::new(doc).write_to_file(&path).unwrap();
    path
}

fn index(dir: &Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(dir.join("out/index.tsv"))
        .unwrap()
        .lines()
        .map(|l| l.split('\t').map(str::to_owned).collect())
        .collect()
}

#[test]
fn writes_the_model_the_layouts_and_every_block_record() {
    let dir = scratch("ok");
    let mut doc = CadDocument::new();
    let mut line = Line::new();
    line.end = Vector3::new(10.0, 5.0, 0.0);
    doc.add_entity(EntityType::Line(line)).unwrap();
    let input = dxf(&dir, &doc);
    let out = dir.join("out");
    let output = run(&[std::ffi::OsStr::new("write-svg"), input.as_os_str(), out.as_os_str()]);
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    let rows = index(&dir);
    assert_eq!(rows[0], ["model.svg", "model", "*Model_Space", "ok"]);
    assert!(rows.iter().any(|r| r[1] == "layout" && r[3] == "ok"));
    assert!(rows.iter().any(|r| r[0] == "block-0000.svg" && r[1] == "block"));
    for row in &rows {
        assert_eq!(row[3], "ok", "{row:?}");
        assert!(dir.join("out").join(&row[0]).is_file(), "{}", row[0]);
    }
    let model = std::fs::read_to_string(dir.join("out/model.svg")).unwrap();
    assert!(model.starts_with("<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\""));
    assert!(model.contains("<line "));
}

#[test]
fn a_failed_write_has_a_status_and_no_file() {
    let dir = scratch("failed");
    let mut doc = CadDocument::new();
    let mut polyline = LwPolyline::new();
    let mut first = LwVertex::from_coords(1.0, 1.0);
    first.bulge = 0.5;
    polyline.vertices.push(first);
    polyline.vertices.push(LwVertex::from_coords(1.0, 1.0));
    doc.add_entity(EntityType::LwPolyline(polyline)).unwrap();
    let input = dxf(&dir, &doc);
    let out = dir.join("out");
    let output = run(&[
        std::ffi::OsStr::new("write-svg"),
        input.as_os_str(),
        out.as_os_str(),
        std::ffi::OsStr::new("--config"),
        std::ffi::OsStr::new("test"),
    ]);
    assert_eq!(output.status.code(), Some(0));
    let rows = index(&dir);
    assert_eq!(rows[0], ["model.svg", "model", "*Model_Space", "!ArgumentOutOfRangeException"]);
    assert!(!out.join("model.svg").exists());
    let block = rows.iter().find(|r| r[2] == "*Model_Space" && r[1] == "block").expect("model space block row");
    assert_eq!(block[3], "!ArgumentOutOfRangeException");
    assert!(!out.join(&block[0]).exists());
}

#[test]
fn exit_codes_follow_the_contract() {
    let dir = scratch("exit");
    let out = dir.join("out");
    // Usage errors: 2.
    assert_eq!(run(&[std::ffi::OsStr::new("write-svg")]).status.code(), Some(2));
    assert_eq!(
        run(&[
            std::ffi::OsStr::new("write-svg"),
            std::ffi::OsStr::new("a.dxf"),
            out.as_os_str(),
            std::ffi::OsStr::new("--config"),
            std::ffi::OsStr::new("other"),
        ])
        .status
        .code(),
        Some(2)
    );
    // A file that cannot be read: 1, with the line the round-trip scripts classify.
    let missing = dir.join("missing.dxf");
    let output = run(&[std::ffi::OsStr::new("write-svg"), missing.as_os_str(), out.as_os_str()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("read failed: "));
}
