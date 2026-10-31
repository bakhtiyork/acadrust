# SVG writer fixture

`export_sample.dwg` is ACadSharp's own SVG test sample (`samples/svg/export_sample.dwg` of
https://github.com/DomCR/ACadSharp, MIT, see `LICENSE`), the drawing its `SvgWriterTests` write.

`export_sample/` holds what ACadSharp's `SvgWriter` writes for it through CadKit's C# tool
(`AcadDump write-svg export_sample.dwg <dir> --config test`): every SVG as raw DEFLATE (`<file>.zlib`) and
`index.tsv` unchanged; `export_sample/README.md` describes the generation. The pattern ids of hatches are
`object.GetHashCode()` values in C# and differ from run to run, so `tests/svg_writer.rs` compares the files after
replacing those numbers by their first-use ordinals.
