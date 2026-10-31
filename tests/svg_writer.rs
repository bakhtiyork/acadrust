//! The SVG writer against ACadSharp's: the port of `SvgWriterTests` and a byte-for-byte replay of the
//! outputs of ACadSharp's `SvgWriter` for svg/export_sample.dwg, through `acad-dump write-svg`.
//!
//! The sample (ACadSharp's, MIT) and the C# outputs are in tests/fixtures/svg (see its README.md).
#![cfg(feature = "acadsharp-compat")]

use acadrust::compat::acadsharp::{read_document_file, view_document};
use acadrust::svg::{SvgConfiguration, SvgWriter};
use flate2::read::DeflateDecoder;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/svg")
}

fn sample() -> PathBuf {
    fixtures().join("export_sample.dwg")
}

fn test_configuration() -> SvgConfiguration {
    let mut configuration = SvgConfiguration::test();
    configuration.acadsharp_view = true;
    configuration
}

/// `SvgWriterTests.WriteLayouts`: every paper-space layout of export_sample.dwg is written.
#[test]
fn write_layouts() {
    let native = read_document_file(&sample()).expect("read export_sample.dwg");
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
    let native = read_document_file(&sample()).expect("read export_sample.dwg");
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

fn inflate(path: &Path) -> Vec<u8> {
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
    let scratch = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("svg-export-sample");
    let _ = std::fs::remove_dir_all(&scratch);
    let status = Command::new(env!("CARGO_BIN_EXE_acad-dump"))
        .arg("write-svg")
        .arg(sample())
        .arg(&scratch)
        .args(["--config", "test"])
        .status()
        .expect("run acad-dump");
    assert!(status.success());
    let fixture = fixtures().join("export_sample");
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

/// The status and the number of entity comments of every output of `document`, in the order of `write-svg`.
fn outputs(document: &acadrust::CadDocument, configuration: SvgConfiguration) -> Vec<Result<usize, &'static str>> {
    enum Target<'a> {
        Model,
        Layout(&'a acadrust::objects::Layout),
        Block(&'a acadrust::tables::BlockRecord),
    }
    let probe = SvgWriter::new(std::io::sink(), document).with_configuration(configuration.clone());
    let targets = std::iter::once(Target::Model)
        .chain(probe.paper_space_layouts().into_iter().map(Target::Layout))
        .chain(acadrust::compat::acadsharp::block_records(document).into_iter().map(Target::Block));
    targets
        .map(|target| {
            let mut out = Vec::new();
            let mut writer = SvgWriter::new(&mut out, document).with_configuration(configuration.clone());
            let result = match target {
                Target::Model => writer.write(),
                Target::Layout(layout) => writer.write_layout(layout),
                Target::Block(record) => writer.write_block_record(record),
            };
            result
                .map(|_| out.windows(4).filter(|w| w == b"<!--").count())
                .map_err(|e| e.csharp_exception())
        })
        .collect()
}

/// The default configuration reads the native model: every output of export_sample.dwg that the ACadSharp view writes
/// is written natively too, with the same entities (one comment per entity).
#[test]
fn the_native_default_writes_every_output_with_the_same_entities() {
    let native = read_document_file(&sample()).expect("read export_sample.dwg");
    let viewed = view_document(&native);
    let view = outputs(viewed.as_ref(), test_configuration());
    assert_eq!(view.len(), 41);
    assert!(view.iter().all(Result::is_ok));
    assert_eq!(outputs(&native, SvgConfiguration::test()), view);
}
