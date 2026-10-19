# CadKit fork notes

This is the CadKit fork of [acadrust](https://github.com/hakanaktt/acadrust) (MPL-2.0), branch `cadkit`, started from
upstream tag `v0.6.5` (289a616, identical to the crates.io 0.6.5 sources apart from line endings). Its goal is "same
data" as [ACadSharp](https://github.com/DomCR/ACadSharp): a read yields the same document as ACadSharp, checked by
canonical dumps against the C# library, and written files read back the same in ACadSharp. The upstream Rust API stays.

Every behaviour change to upstream code is recorded here with its reason. ACadSharp-specific representation lives in
`src/compat/`. Upstream is merged in by tag, never rebased.

## Build setup

- `Cargo.lock` is committed on this branch (upstream ignores it) so tests and the CadKit bridge build the same
  dependency set, offline from `cargo vendor`.

## Behaviour changes

| Area | Change | Reason |
|---|---|---|
| Hatch DXF provenance | Retain elevation XY, omitted-field presence, the pattern trailer code and original gradient ACI values alongside native RGB colors. Exclude provenance from native Hatch equality. | Additive source retention permits source-dependent defaults and reader quirks without changing native seed points, styles, pixel sizes, scales or colors. No whole-document scans or payload clones. |
| Hatch gradient recovery provenance | Move the original gradient into optional provenance when legacy round-trip metadata restores a gradient; exclude it from native equality. | Preserve native recovery while allowing compat to expose the original values, since ACadSharp does not perform this recovery. No additional document scan or gradient-payload clone. |
| ACadSharp hatch view | Map boundary edges and ordered unique entity references, pattern lines and gradient stops; project DXF elevation seeds, ignored styles/RGB, consumed pattern trailers, omitted scales and names, and absent DWG non-rational weights. | Reproduce the reference's nested schema, constructor defaults and reader quirks only in compat; preserve native data and DWG version semantics. |
| IMAGEDEF DXF units | Read group 281 into the existing resolution-unit enum. | Genuine discarded-data bug; candidate for an upstream PR. Native defaults and explicit values remain unchanged. |
| BookColor DXF provenance | Retain whether a DXF color code was present; exclude provenance from native equality. | ACadSharp's absent-color default is ByBlock while native Color defaults to ByLayer. Project the difference only in compat and preserve explicit ByLayer. |
| ACadSharp media identities | Resolve empty group/underlay/book-color names from owner dictionary entries, split color-book components, expose the base underlay markers and little-endian TrueColor packing. | Match the reference's name getters and inherited markers without changing native names, page identity, RGB values or common entity appearance. |
| DXF string provenance | Retain the original caret-escaped value only when text decoding changes it; the decoded native value is unchanged. GeoData keeps source definition text and scalar-code presence. | ACadSharp exposes raw DXF definition text and distinct omitted-field defaults. Additive source retention; not a decoder-semantic change. |
| R2009 GeoData DXF | Read version-1 reference/obsolete points, scale, north angle and legacy mesh coordinate codes; write the matching version-1 layout. Mesh counts do not drive allocation or unbounded reads. | Genuine version-layout/data-loss bugs, candidates for an upstream PR. Keep both native points and matching native horizontal/vertical units and scales. |
| ACadSharp VisualStyle view | For DWG, expose the read name/type and C# constructor defaults; correct modern/legacy property-slot mappings for other sources. | C# leaves the DWG reader incomplete. Rich native visual-style values remain intact; this projection is not an upstream decoder fix. |
| ACadSharp GeoData/SpatialFilter view | Project DWG vertical-unit assignment, DXF scale slots/defaults/obsolete-point overwrites, unassigned DXF host block and raw definition text; DXF SpatialFilter exposes the last matrix as inverse and identity as insert. | Reproduce documented C# reader/attribute quirks only in compat, preserving native units, coordinate state and both transforms. |
| DWG DIMASSOC provenance | Retain a shared immutable main-stream buffer and bounded reference offsets alongside typed references. | The C# osnap reader stops early, so later osnap-type bytes cannot be recovered from decoded values alone. No native values change and no record buffer is cloned. |
| ACadSharp DimensionAssociation view | Map the typed DIMASSOC payload and four active osnap slots; reproduce incomplete DWG consumption and discarded DXF geometry templates in compat. | The native associative payload was retained but dispatched through the common-only adapter. Native continuation chains remain available. |
| Document construction | Extract an internal `new_without_defaults` constructor, starting allocation at the initial header seed; native `new()` still installs the same reserved handles and writer defaults. | Allows the compatibility constructor to create/register ACadSharp's distinct default graph in source-defined order, without remapping fixture handles or changing native initialization policy. |
| DWG/DXF header retention | Retain PELLIPSE/UNITMODE, UCS references and orthographic origins, dimension settings and reference names/handles, stacked text and shadow settings; write them in their version-specific slots. Keep obsolete DIMFIT separate from DIMATFIT (`dim_text_arrow_fit`). | Genuine discarded-data and aliased-variable bugs; upstream-PR candidates. Native defaults and DWG bit alignment are preserved; ACadSharp defaults remain in compat. |
| Header source metadata | Retain DXF variable presence, ACADMAINTVER independently of the DWG framing byte, original HANDSEED and raw DWG date/duration pairs. DXF writers preserve explicit maintenance values, including zero; DWG writers preserve unchanged timestamp pairs. | Distinguish omitted variables from explicit values and retain source precision without changing native date/duration units. ACADMAINTVER retention is an upstream-PR candidate. |
| DWG Template retention | Retain the optional Template bytes in shared immutable storage, and preserve/transcode its description when writing with the current native measurement. | Native description data loss is an upstream-PR candidate. Compat alone reproduces ACadSharp's absent-section metric default and byte-count interpretation of UTF-16 descriptions. |
| DWG SummaryInfo dates | Read and write creation/modification dates at millisecond precision; retain raw source pairs separately on the document, using edited native dates when changed. Keep zero-date source presence distinct from a missing section. | Genuine timestamp data loss, an upstream-PR candidate. Source provenance does not change SummaryInfo value equality. |
| ACadSharp header view | Resolve current DXF names and DWG handles; map retained fields, standalone UCS/override references as #0, source-aware defaults, nullable strings, string GUIDs, version maintenance values, packed colors, flag masks and date/duration conversion. The explicit compatibility constructor installs ACadSharp header defaults. | Reproduce C# getters and reader quirks without changing native defaults, units, flags, colors or object registration. Remaining R12 reference identities and HANDSEED depend on the registration work. |
| ACadSharp summary view | Map retained summary timestamps through the oracle date conversion and expose DXF LastSavedBy only in the header. | C# keeps header and summary values separate; native DXF summary behavior remains unchanged. |
| DXF class retention | Preserve source CLASS records separately from the native writer registry, including source order and metadata. | Reader data-loss fix: merging with writer defaults previously discarded source metadata. The native registry remains unchanged; source retention is an upstream-PR candidate. ACadSharp-specific numbering and collection projection stay in compat. |
| Dynamic-block DXF decoding | Read alignment-grip orientation from 140/141/142 (also accepting legacy 140/150/160), and accept the AutoCAD `AcDbBlockLookUpParameter` spelling alongside older acadrust output. | Genuine coordinate and lookup-label/action-ID data-loss bugs; candidates for an upstream PR. |
| ACadSharp dynamic-block view | Project stored graph indices/flags, grip IDs and versions, parameter/action connections, lookup columns and stretch bindings; emit absent evaluations as null and visibility states as ordered dictionary entries with resolved references. | Adapter field identities and nested structures differed from the C# public model; no dynamic-block evaluation is needed. |
| ACadSharp visibility DWG view | Project the visibility initialization bit as the final ChainActions value and leave Value281 false. | The C# DWG reader overwrites the inherited ChainActions bit and never assigns Value281; preserve both native bits. |
| ACadSharp visibility DXF view | Expose the default GripId of zero while retaining the native source grip ID. | The C# visibility callback tries its entity-count mapping for code 93 before the inherited one-point mapping, consuming GripId without assignment. |

| Ellipse thickness | Retain DXF group 39 in the ellipse model, write nonzero thickness, and preserve it when circles/arcs become ellipses. | Genuine discarded extrusion data; upstream-PR candidate. DWG ellipses have no thickness slot and retain zero. Coordinate decoding is unchanged. |
| DXF modeler data | Preserve the explicit modeler version and Region/Body identifiers; accept plain numeric-header SAT alongside encoded SAT. Retain chunk lengths, original line endings, sparse lexical differences and stripped suffixes. | Genuine version/identifier/plain-SAT data-loss bugs; upstream-PR candidates. Compact source metadata reconstructs the source without copying the normalized payload. Native SAT remains normalized. |
| DWG modeler provenance | Retain a shared immutable record buffer and bounded modeler offsets for Region/Solid3D/Body. | ACadSharp returns early for inline bodies and interprets raw wire types, scalar scales and silhouette gates differently. Source replay stays in compat and does not clone SAT/SAB buffers or scan the document. |
| ACadSharp primitive getters | Expose signed ellipse rotation and negative arc sweep. | Match the reference getters without changing native coordinates or positive native arc sweep. |
| ACadSharp spline view | Map retained tolerances, raw/derived flags, version-dependent knot parameterization and computed knot validity/closure/periodicity. | Reference fit-point Flags1 precedence and pre-R2013 scenario defaults belong only in the view; the native spline model is unchanged. |
| ACadSharp mesh view | Map missing crease values to the reference reader's zero and retrieve UVW texture coordinates from the retained mesh XRecord. | Native optional crease and texture storage stay unchanged. |
| ACadSharp modeler view | Project source SAT bytes/chunks, modeler version, base subclass marker, source point/wires/silhouettes, and the ACIS-only binary-signature getter. | Preserve C# inline-body early returns, raw wire bytes, scalar-scale parsing and DXF display-cache defaults only in compat. Native ASM/SAB recognition and richer cache data remain intact. |
| MText lexical source retention | Retain original MText text only when DWG/ASCII or binary DXF escape decoding changes it; keep source DXF alignment assignment order, field presence and embedded-column presence. | Additive data retention, an upstream-PR candidate for lossless formatted text. Native Unicode/caret decoding, rotation, geometry and column layout are unchanged; edited native text supersedes its original spelling. |
| MText background provenance | Mark background fills recovered from legacy ACAD round-trip XData. | Preserve the useful native recovery while allowing the ACadSharp view to expose the reader's unrecovered defaults; no additional document scan. |
| ACadSharp MText view | Map retained text, PlainText, columns, rectangle height, alignment/spacing defaults, background byte packing/transparency, ignored extents and unassigned DWG column counts. | Reproduce C# reader/getter quirks only in compat; native extents, native formatting parser and recovered fills remain available. PlainText uses deterministic LF, with a terminating fallback for malformed controls on which C# can loop forever; no font-dependent layout. |
| Tolerance DXF provenance | Retain direction-code presence and the original caret-escaped text while preserving native UNIT_X defaults and decoded text. | The C# DXF view defaults omitted direction to zero and exposes literal caret escapes. Additive retention; projections alone reproduce these differences, and edited text supersedes retained spelling. |
| MText/Tolerance equality | Keep equality defined by the pre-existing native fields; new source spelling/presence and recovery markers do not affect it. | Source provenance must not change native drawing-state equality. All existing round-trip comparisons remain intact; focused tests separately verify provenance and projection. |
| ACadSharp dimension subtype view | Map radius point names, linear/angular offsets, Angular2Line center, Diameter/Ordinate/Arc angular flags and Arc endpoint measurement. | Preserve C# bit-test, exact-intersection, ratio-parallel and throwing-getter quirks only in compat. Native geometry, dimension styles and common fields are unchanged. |

## ACadSharp differential dump

The optional `acadsharp-compat` feature exposes `compat::acadsharp` and the `acad-dump` binary:

```sh
cargo build --features acadsharp-compat --bin acad-dump
acad-dump dump drawing.dwg out.json
acad-dump dump --new-document out.json
```

The tool reads DXF/DWG once in failsafe mode and catches reader panics. A written reader-error dump still exits 0;
usage and filesystem errors exit 2. It uses the canonical CadKit dump envelope and embedded ACadSharp schema
(`src/compat/acadsharp/schema.tsv`, regenerated by `Tools/AcadDump/run.sh gen-rust-checklist`). Every emitted type has
its exact schema member order; unsupported properties are `"!unmapped"`. Only retained data and simple computed
properties are mapped. Dumping an existing document does not repair it or assign new handles.
`compat::acadsharp::new_document()` explicitly constructs ACadSharp's default graph using native objects and the
normal allocator. The CLI's `--new-document` uses this constructor. Its order follows `CadDocument.CreateDefaults`,
dictionary insertion order, style reference registration, and layout/block/marker registration; its allocation starts
at the header seed rather than using a fixture handle map. `CadDocument::new()` retains the native defaults.
The native class registry remains populated for writers; DXF and new-document views use the separately retained
source class list (empty for a new document). DWG class projection keeps the first case-insensitive name, matching
ACadSharp's collection, while the native collection keeps every positional record and original class number.
The default-reader view projects `ProxyGeometries` as empty (ACadSharp ignores them by default), preserving the
native entity's graphic bytes.
Registered handles are gathered from tables, block records, entities and their children, and non-graphical objects,
then sorted numerically. Retained BLOCK/ENDBLK identities folded into block records are emitted with discarded
entity properties unmapped. Objects without registered handles are not assigned synthetic identities.
The normal build does not include this module or its optional JSON dependency.
