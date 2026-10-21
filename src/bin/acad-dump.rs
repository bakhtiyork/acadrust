use acadrust::compat::acadsharp::{new_document, read_document_file, write_document, write_file};
use acadrust::io::dxf::DxfWriter;
use std::io::{BufWriter, Seek, Write};
use std::path::Path;

/// `write-dxf <in.dwg|in.dxf> <out.dxf> [--binary]`: failsafe read as `dump` does, then write the document back
/// in its own version. Exit 0 on success, 1 on a read/write failure (stderr `read failed: ...`/`write failed: ...`,
/// the lines `Tools/roundtrip-report.sh` classifies), 2 on a usage error.
fn write_dxf(args: &[std::ffi::OsString]) -> i32 {
    let mut binary = false;
    let mut paths = Vec::new();
    for arg in &args[2..] {
        if arg == "--binary" {
            binary = true;
        } else {
            paths.push(arg);
        }
    }
    if paths.len() != 2 {
        eprintln!("usage: acad-dump write-dxf <in.dwg|in.dxf> <out.dxf> [--binary]");
        return 2;
    }
    let document = match read_document_file(Path::new(paths[0])) {
        Ok(document) => document,
        Err(error) => {
            eprintln!("read failed: {error}");
            return 1;
        }
    };
    let written = std::panic::catch_unwind(|| {
        let writer = if binary {
            DxfWriter::new_binary(&document)
        } else {
            DxfWriter::new(&document)
        };
        writer.write_to_file(paths[1]).map_err(|e| e.to_string())
    });
    match written {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => {
            eprintln!("write failed: {error}");
            1
        }
        Err(_) => {
            eprintln!("write failed: Panic: writer panicked");
            1
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() > 1 && args[1] == "write-dxf" {
        std::process::exit(write_dxf(&args));
    }
    if args.len() != 4 || args[1] != "dump" {
        return Err("usage: acad-dump dump <file.dwg|file.dxf|--new-document> <out.json>".into());
    }
    let input = Path::new(&args[2]);
    if args[2] != "--new-document" {
        let extension = input
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "dwg" | "dxf") {
            return Err("expected .dwg or .dxf input".into());
        }
        // Input filesystem errors are command errors, not CAD reader failures.
        std::fs::File::open(input).map_err(|e| e.to_string())?;
    }
    // The old dumper finished reading before replacing its output. Defer
    // truncation so an output path aliasing the input still has that behavior.
    let output = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&args[3])
        .map_err(|e| e.to_string())?;
    let mut writer = BufWriter::with_capacity(256 * 1024, output);
    if args[2] == "--new-document" {
        write_document(&new_document(), "", "new", &mut writer)
    } else {
        write_file(input, &mut writer)
    }
    .map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;
    let length = writer.stream_position().map_err(|e| e.to_string())?;
    writer
        .get_ref()
        .set_len(length)
        .map_err(|e| e.to_string())?;
    let name = if args[2] == "--new-document" {
        String::new()
    } else {
        input
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    println!("{name}\t{length} bytes");
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
