//! The SVG writer against ACadSharp's: the port of `SvgWriterTests` and a byte-for-byte replay of the
//! outputs of ACadSharp's `SvgWriter` for svg/export_sample.dwg, through `acad-dump write-svg`.
//!
//! The sample and the C# outputs (CadKit `Tests/CadCoreTests/Fixtures`) are not copied into this
//! repository because the licence of the sample is not recorded. Point `CADKIT_FIXTURES` at that
//! directory (default: `../../CadKit/Tests/CadCoreTests/Fixtures` beside the worktrees). Without it
//! the tests print `SKIPPED` on stderr and end; run with `--nocapture` to see it.
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::{read_document_file, view_document};
use acadrust::svg::{SvgConfiguration, SvgWriter};
use flate2::read::DeflateDecoder;
use std::io::Read;
use std::path::PathBuf;
use std::process::Command;

fn fixtures() -> Option<PathBuf> {
    let dir = std::env::var_os("CADKIT_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../CadKit/Tests/CadCoreTests/Fixtures"));
    if dir.join("samples/svg/export_sample.dwg").is_file() && dir.join("svg/export_sample/index.tsv").is_file() {
        Some(dir)
    } else {
        eprintln!(
            "SKIPPED: the SVG fixtures are not at {} (set CADKIT_FIXTURES to CadKit's Tests/CadCoreTests/Fixtures)",
            dir.display()
        );
        None
    }
}

fn test_configuration() -> SvgConfiguration {
    let mut configuration = SvgConfiguration::test();
    configuration.acadsharp_view = true;
    configuration
}

/// `SvgWriterTests.WriteLayouts`: every paper-space layout of export_sample.dwg is written.
#[test]
fn write_layouts() {
    let Some(dir) = fixtures() else { return };
    let native = read_document_file(&dir.join("samples/svg/export_sample.dwg")).expect("read export_sample.dwg");
    let viewed = view_document(&native);
    let document = viewed.as_ref();
    let probe = SvgWriter::new(std::io::sink(), document).with_configuration(test_configuration());
    let layouts = probe.paper_space_layouts();
    let names: Vec<&str> = layouts.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(
        names,
        ["curve-ltypes", "dimensions", "hatch", "Layout1", "Layout2", "layout_size", "line-types", "text_sample"]
    );
    for layout in layouts {
        let mut out = Vec::new();
        let mut writer = SvgWriter::new(&mut out, document).with_configuration(test_configuration());
        writer.write_layout(layout).unwrap_or_else(|e| panic!("layout {}: {e}", layout.name));
        assert!(out.starts_with(b"<?xml version=\"1.0\"?>\n<svg "), "{}", layout.name);
        assert!(out.ends_with(b"</svg>"), "{}", layout.name);
    }
}

/// `SvgWriterTests.WriteModel`.
#[test]
fn write_model() {
    let Some(dir) = fixtures() else { return };
    let native = read_document_file(&dir.join("samples/svg/export_sample.dwg")).expect("read export_sample.dwg");
    let viewed = view_document(&native);
    let mut out = Vec::new();
    let notifications = {
        let mut writer = SvgWriter::new(&mut out, viewed.as_ref()).with_configuration(test_configuration());
        writer.write().expect("model space");
        writer.notifications().to_vec()
    };
    assert!(out.ends_with(b"</svg>"));
    assert!(notifications.iter().all(|n| n.message.ends_with("Entity not implemented.")));
}

fn inflate(path: &std::path::Path) -> Vec<u8> {
    let raw = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut out = Vec::new();
    DeflateDecoder::new(&raw[..]).read_to_end(&mut out).expect("raw deflate");
    out
}

/// ACadSharp prints `object.GetHashCode()` of every hatch pattern and pattern line in the element ids, a
/// per-process number. Replace each distinct number of `<pattern id="N_...">` and of the `fill="url(#N_...)"`
/// of paths and rects by the ordinal of its first use; every other byte stays.
fn canonical(data: &[u8]) -> Vec<u8> {
    let mut ordinals: Vec<Vec<u8>> = Vec::new();
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    let find = |from: usize, needle: &[u8]| data[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from);
    while i < data.len() {
        if data[i] != b'<' {
            out.push(data[i]);
            i += 1;
            continue;
        }
        let skip_to = if data[i..].starts_with(b"<!--") {
            find(i + 4, b"-->").map(|p| p + 3)
        } else if data[i..].starts_with(b"<![CDATA[") {
            find(i + 9, b"]]>").map(|p| p + 3)
        } else if data[i..].starts_with(b"<?") {
            find(i + 2, b"?>").map(|p| p + 2)
        } else if data[i..].starts_with(b"</") || data[i..].starts_with(b"<!") {
            find(i, b">").map(|p| p + 1)
        } else {
            None
        };
        if let Some(end) = skip_to {
            out.extend_from_slice(&data[i..end]);
            i = end;
            continue;
        }
        // An element start tag: copy it, replacing the hash numbers of the eligible attributes.
        let mut j = i + 1;
        let name_start = j;
        while j < data.len() && !matches!(data[j], b' ' | b'\t' | b'\r' | b'\n' | b'/' | b'>') {
            j += 1;
        }
        let name = data[name_start..j].to_vec();
        out.extend_from_slice(&data[i..j]);
        while j < data.len() && data[j] != b'>' {
            if data[j] == b'=' && j + 1 < data.len() && data[j + 1] == b'"' {
                let attr_end = out.len();
                let attr: Vec<u8> = out[..attr_end].iter().rev().take_while(|b| b.is_ascii_alphanumeric() || **b == b'-').copied().collect::<Vec<_>>().into_iter().rev().collect();
                let value_start = j + 2;
                let value_end = data[value_start..].iter().position(|b| *b == b'"').map(|p| p + value_start).unwrap_or(data.len());
                let value = &data[value_start..value_end];
                out.extend_from_slice(b"=\"");
                let (eligible, prefix, rest): (bool, &[u8], &[u8]) = if name == b"pattern" && attr == b"id" {
                    (true, b"", value)
                } else if (name == b"path" || name == b"rect") && attr == b"fill" && value.starts_with(b"url(#") {
                    (true, &value[..5], &value[5..])
                } else {
                    (false, b"", value)
                };
                let digits = rest.iter().position(|b| !(b.is_ascii_digit() || *b == b'-')).unwrap_or(rest.len());
                if eligible && digits > 0 && rest.get(digits) == Some(&b'_') {
                    let number = rest[..digits].to_vec();
                    let ordinal = ordinals.iter().position(|n| *n == number).unwrap_or_else(|| {
                        ordinals.push(number);
                        ordinals.len() - 1
                    });
                    out.extend_from_slice(prefix);
                    out.extend_from_slice(ordinal.to_string().as_bytes());
                    out.extend_from_slice(&rest[digits..]);
                } else {
                    out.extend_from_slice(value);
                }
                out.push(b'"');
                j = value_end + 1;
            } else {
                out.push(data[j]);
                j += 1;
            }
        }
        i = j;
    }
    out
}

#[test]
fn canonicalisation_replaces_only_hatch_ids() {
    let a = b"<g><pattern id=\"123_line\" x=\"1\"><rect fill=\"url(#123_line)\" /></pattern><!-- 5_x --><pattern id=\"9_ANSI31\"/><path fill=\"url(#9_ANSI31)\" d=\"5_\"/></g>";
    let b = b"<g><pattern id=\"77_line\" x=\"1\"><rect fill=\"url(#77_line)\" /></pattern><!-- 5_x --><pattern id=\"4_ANSI31\"/><path fill=\"url(#4_ANSI31)\" d=\"5_\"/></g>";
    assert_eq!(canonical(a), canonical(b));
    assert_eq!(
        String::from_utf8(canonical(a)).unwrap(),
        "<g><pattern id=\"0_line\" x=\"1\"><rect fill=\"url(#0_line)\" /></pattern><!-- 5_x --><pattern id=\"1_ANSI31\"/><path fill=\"url(#1_ANSI31)\" d=\"5_\"/></g>"
    );
    // Text and the other attributes are compared as they are.
    assert_ne!(canonical(b"<a b=\"1\"/>"), canonical(b"<a b=\"2\"/>"));
}

/// The fixture holds every output of ACadSharp's `write-svg --config test` for export_sample.dwg (41 files and
/// `index.tsv`); the fork's tool must write the same bytes after the hatch id canonicalisation.
#[test]
fn export_sample_matches_the_csharp_outputs_byte_for_byte() {
    let Some(dir) = fixtures() else { return };
    let scratch = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("svg-export-sample");
    let _ = std::fs::remove_dir_all(&scratch);
    let status = Command::new(env!("CARGO_BIN_EXE_acad-dump"))
        .arg("write-svg")
        .arg(dir.join("samples/svg/export_sample.dwg"))
        .arg(&scratch)
        .args(["--config", "test"])
        .status()
        .expect("run acad-dump");
    assert!(status.success());
    let fixture = dir.join("svg/export_sample");
    assert_eq!(
        std::fs::read(scratch.join("index.tsv")).unwrap(),
        std::fs::read(fixture.join("index.tsv")).unwrap(),
        "index.tsv"
    );
    let mut compared = 0;
    for entry in std::fs::read_dir(&fixture).unwrap() {
        let path = entry.unwrap().path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".zlib")) else {
            continue;
        };
        let expected = inflate(&path);
        let actual = std::fs::read(scratch.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(canonical(&expected) == canonical(&actual), "{name} differs from the C# output");
        compared += 1;
    }
    assert_eq!(compared, 41, "41 SVG outputs");
}
