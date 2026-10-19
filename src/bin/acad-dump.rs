use acadrust::compat::acadsharp::{dump_document, dump_file, new_document};
use std::path::Path;

fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 4 || args[1] != "dump" {
        return Err("usage: acad-dump dump <file.dwg|file.dxf|--new-document> <out.json>".into());
    }
    let input = Path::new(&args[2]);
    let value = if args[2] == "--new-document" {
        dump_document(&new_document(), "", "new")
    } else {
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
        dump_file(input)
    };
    let data = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    std::fs::write(&args[3], &data).map_err(|e| e.to_string())?;
    let name = if args[2] == "--new-document" {
        String::new()
    } else {
        input
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    println!("{name}\t{} bytes", data.len());
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(2);
    }
}
