# C# SVG reference fixture

Generated from the repository root with the pinned upstream ACadSharp writer:

```sh
DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1 Tools/AcadDump/build.sh --no-restore -nodeReuse:false -p:UseSharedCompilation=false
DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_NOLOGO=1 Tools/AcadDump/run.sh write-svg Tests/CadCoreTests/Fixtures/samples/svg/export_sample.dwg .build.noindex/p10-0/fixture --config test
```

Each generated SVG is stored as `<filename>.zlib`: raw DEFLATE, without a zlib header or trailer, using Python
`zlib.compressobj(level=9, wbits=-15)` followed by `compress(data) + flush()`. Decompression was checked byte for byte.
`index.tsv` is copied unchanged and uncompressed; it records the model, all 8 paper-space layouts and all 32 block
records, in upstream enumeration order. All 41 writes succeeded. The test configuration sets `DefaultLineWeight`
to `0.15` and `LineWeightRatio` to `50`.

Determinism caveat: two gate runs matched, but this fixture's separate generation differed in `model.svg`,
`layout-02.svg`, `block-0008.svg` and `block-0030.svg`. Only hatch IDs and their references differed: upstream
`SvgXmlWriter.cs` lines 499 and 541 use object `GetHashCode()` values. The fixture preserves the original bytes;
no IDs were normalized. `index.tsv` was identical across all three runs.
