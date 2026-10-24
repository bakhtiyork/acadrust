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
| Canonical dump streaming | `acad-dump dump` writes through a buffered writer using a registration-metadata plan and borrowed per-record recipes. New `write_document`, `write_reader_document`, and `write_file` APIs emit the same ordered JSON; the existing Value-returning APIs remain available. | Avoid retaining the complete JSON property tree and serialized byte buffer on large drawings. Only graph/identity metadata and one full object record are needed in addition to the native document. Native readers/model and compatibility decisions are unchanged; source ordinals survive R12 prototype replay and modern SEQEND remapping. |
| AEC typed DWG access | Decode the small CleanupGroup, WallStyle, BinRecord and Wall headers and references on demand from retained opaque DWG records, with bounded bit slices and explicit source versions. Keep the existing Unknown records and their complete bytes for writing. | Additive native AEC inspection API, without a geometry engine, eager reader allocations, document scans or a second object lifecycle. Candidate for an upstream feature PR; unsupported proprietary data remains available. |
| ACadSharp AEC view | Project typed AEC versions/descriptions/references, class markers, the hidden BinRecord null name, and Wall's zero geometry/numeric-handle defaults. Non-entity raw tails round down to complete bytes; Wall's raw range includes consumed handles and rounds up, always within the retained record. | These are ACadSharp's partial reader behaviors, not native geometry. Reproduce its zero-base relative-handle lookup only in compat and resolve targets by type; native Wall handles use the owning object's base, and native opaque tails preserve partial bits. |
| ACadSharp dictionary names and legacy flags | Emit the inherited empty name for unnamed extension dictionaries, retaining dictionary-assigned names and entry order. For pre-R2000 DWG reads, project both dictionary flags as zero/false. | `NonGraphicalObject.Name` defaults empty; `DwgObjectReader.readCommonDictionary` consumes the R14 byte without assigning it and only assigns cloning/hard-owner flags in R2000+. Native dictionary flags and entries remain intact. |
| XRecord source text | Retain sparse original string spellings when DXF caret/legacy escapes or pre-R2007 DWG legacy escapes change the native decoded text. Expose them to compat only while the entry index, code and decoded value still match; exclude provenance from native equality and serialization. | Additive reader provenance preserves source lexical information otherwise lost (upstream-PR candidate). Native decoded entries and writer semantics remain unchanged; no document scans or unchanged-string copies are added. |
| ACadSharp XRecord strings | Project retained lexical strings, including literal caret and Unicode escape sequences, without changing typed handle resolution or coercing literal handle-looking strings. | The reference XRecord readers store raw reader values while native acadrust decodes text. Edited entries fall back to their current native values. |
| Table INSERT data | Retain DWG INSERT scale/rotation on Table; read DXF transforms from the block-reference subclass and retain overlapping outer DXF transform/count/spacing values separately. | Genuine discarded INSERT data, an upstream-PR candidate. Native transforms and table grid dimensions stay correctly scoped; only compat reproduces ACadSharp's inherited INSERT assignments from overlapping table codes. |
| Down-saved table colors | Read/write the complete CMC payload for legacy table cell, row and border overrides even in R14/R2000 files. | Genuine format bug, an upstream-PR candidate. Tables retain the R2004 color layout when down-saved; reading only an index shifted subsequent fields/cells, and the old writer lost RGB values. |
| Table content source presence | Retain no-data cell/row/column/base style type words, explicit content-format presence, and optional break-section presence/raw flow direction. Keep absent native styles as `None`. | Genuine discarded source state, upstream-PR candidates. Additive scalar retention avoids guesses from payload values and introduces no document scans or payload copies. |
| Table DWG break writing | Preserve explicitly present empty break sections and unchanged raw flow directions, using an edited native direction when it differs. | Makes retained break state round-trip without losing explicit zero/unknown source values or overriding native edits; upstream-PR candidate. |
| ACadSharp TableEntity/TableContent view | Map INSERT fields, rows/columns/cells, custom CadValues, geometry, merged ranges, styles and breaks through one nested projection. Reproduce constructor defaults, legacy block-content behavior, ignored cell-format/style references, distinct color/alignment slots, DXF overlapping codes, and DWG override booleans only in compat. | Closes slices 38–39 without a cell-style subsystem rewrite. Native effective styles, cached merge geometry, colors, fills, block contents and references remain available; reference dumps are unchanged. |
| DXF structural block state | Retain BLOCK/ENDBLK common metadata on BlockRecord, including appearance, scoped references and XData; retain all BLOCK flag bits and the unloaded flag. Use the existing common parser without adding marker entities to drawable block contents. | Genuine discarded-data fixes; upstream-PR candidates. Optional boxed common state avoids enlarging every entity or introducing document scans. |
| DXF existing space records | Copy parsed explodability, scaling and preview data when a source BLOCK_RECORD updates an initialized model/paper-space record. | Genuine data-loss bug: the merge previously retained only units, flags and references. Upstream-PR candidate; ordinary named block behavior is unchanged. |
| Block source provenance | Retain DXF field presence and last-preview-chunk bounds, the original DWG anonymous bit, and marker names replaced by native hydration. Native preview bytes, block naming, flags, scaling and unit recovery remain unchanged. | Additive source retention permits the reference's nullable defaults, final-chunk preview setter, raw BLOCK names and non-recovered flags without degrading native data. |
| ACadSharp block view | Project retained structural common state, flags, source-block XData references, version-dependent units/explodability, scaling-byte booleans and nullable comments/path/preview. Set the compatibility constructor's block explodability to false. | Reproduce reference getters, constructor defaults and reader semantics only in compat; actual entities take precedence over folded marker state. |
| ACadSharp Insert view | Resolve SpatialFilter through the typed ACAD_FILTER/SPATIAL dictionary path; exclude legacy ownerless attributes from the Insert collection while retaining their standalone records. | Match the reference getter and R12 builder attachment rules without changing native attributes, dictionary data or child identities. |
| Dimension DXF provenance | Retain raw flags and fixed-size presence/value metadata for attachment, spacing and text; exclude it from native DimensionBase equality. | Distinguishes omitted fields from explicit values without replacing native defaults or cloning text. Raw flags remain separate from the native dimension discriminator; discarded raw flag bits are an upstream-PR candidate. |
| ACadSharp dimension view | Project full flags, ordinate/text bits, first-record ACAD/DSTYLE override detection, nullable DXF text, and omitted DXF/pre-R2000 DWG defaults. | Reproduce the oracle's enum-as-flags and constructor/getter behavior only in compat, including DWG XData NUL trimming. The existing Arc flag assertion is corrected from 5 to 37 because ACadSharp's Dimension constructor adds BlockReference (32); its geometry assertions remain intact. |
| Text/attribute lexical provenance | Retain original text when decoding or embedded multiline recovery changes it, undeclared-encoding invalid UTF-8 bytes for ASCII DXF text, and tags changed by ACAD_MLATT recovery. Exclude source metadata from native equality; plain unchanged TEXT adds no source allocation. | Lossless source retention, an upstream-PR candidate. Native Unicode/legacy decoding and multiline/tag recovery remain intact; edits supersede retained source values. No vertex storage or reader document scans are added. |
| Text/attribute retained fields | Preserve stored DWG alignment points even for left/baseline justification; retain attribute thickness, version and DXF normals. Copy thickness/version when constructing an attribute from its definition and write them in the existing DXF/DWG slots. | Genuine discarded-data bugs, upstream-PR candidates. Absent DWG alignment remains absent; version stays distinct from the DXF lock flag and ATTDEF's repeated DWG version byte is retained. |
| ACadSharp text/attribute view | Resolve styles; project zero alignment, single-line type, height/mirror defaults, embedded MText handles, outer values/tags and DXF version/lock assignments. | Reference constructor/reader behavior belongs only in compat. Explicit encoding decision: reproduce .NET UTF-8 replacement only from retained invalid bytes in a DXF without a declared encoding; preserve correct native legacy text, declared code pages and valid UTF-8. |
| ACadSharp polyline/MLine view | Map LwPolyline vertex tangent/flag defaults and ordered MLine segments with both parameter arrays; project the DWG MLine open/closed flags. | Nested schema and the reference DWG flag quirk belong only in compat. Native widths, flags, vertices, identities and order remain intact; no per-vertex reader allocations are introduced. |
| DWG omitted lineweights | R13/R14 entity preambles use ByLayer and layer records use Default when the format has no lineweight slot. | Genuine decoder defaults bug; upstream-PR candidate. Modern explicit weights are unchanged. |
| DWG entity color provenance | Retain the R2004+ ENC word and packed transparency inline with copy-on-write entity properties; exclude them from native equality but include them in the child-property cache comparison. | Preserve distinctions lost by native color normalization without extra per-entity heap allocations or changing decoded appearance. The ACadSharp zero-color opacity and complex-color interpretation belong only in compat. |
| EED layer references | Share a generic EED decoder that can also return original numeric layer handles, with one output allocation on the native path. | Native DXF-friendly layer names and raw DWG payloads remain intact; numeric references can be projected even when the layer is unresolved. |
| DXF common object attachments | Capture scoped reactors, extension dictionaries and the first owner outside defined groups once; retain attachments in existing document side maps and restore typed owners. | Genuine discarded relationships and reactor-as-owner bugs; upstream-PR candidates. Absent groups allocate no collections, and explicit reactor order/duplicates remain native data. |
| DXF common entity material | Read group 347 in the shared entity parser. | Genuine material-reference loss across entity types; upstream-PR candidate. |
| DXF table-control metadata | Retain TABLE-header XData/reactors/extension dictionaries under the control handle and write retained header XData/reactors back. | Genuine table-header data loss (including VPORT DbSaveVer); upstream-PR candidate. Symbol table entry semantics are unchanged. |
| DXF structural common data | Retain BLOCK/ENDBLK common properties on BlockRecord and synchronize their assigned handles. | Genuine discarded appearance, XData and attachment data; upstream-PR candidate. Structural markers remain outside the drawing-entity collection. |
| DXF defined-group boundaries | Track nested groups and stop common group readers at a code-0 record boundary, preserving the next record for its reader. | Malformed or nested groups must not consume later entities or substitute nested handles; upstream-PR candidate. |
| Underlay owner lookup | Expose the retained UnderlayDefinition owner through the shared document lookup and object owner setter. | Genuine omission in common owner dispatch; upstream-PR candidate. |
| ACadSharp common view | Project absent common data as empty/null, retain undecoded EED as unavailable, encode XData layer references numerically and STYLE font data as ACAD records, trim DWG EED string terminators in the view, and remap R12 application dictionary keys. | Distinguish absence from decoding loss and match stored reference/value semantics without discarding native data. R12 numeric layer references retain the source handle at parse time, including zero. |
| ACadSharp entity appearance and attachments | Use little-endian RGB integers, source ENC transparency/book-color behavior, resolved typed references, source-reactor deduplication followed by group attachments, and document table defaults. Layer material remains null as in the unfinished C# layer builder. | Oracle reader/getter behavior only; native colors, transparency, materials, references and reactor duplicates remain available. |
| ACadSharp R12 sequence references | Leave deferred references, XData and attachments at defaults for legacy polyline Seqends, while retaining directly parsed scalar appearance. | C# assigns this Seqend without building its template. Native source metadata is preserved. |
| PlotSettings DXF provenance | Retain bounded group-code presence for standalone PlotSettings; exclude it from native equality and preserve/clear it with the existing storage-data helpers. | Distinguishes omitted plot fields from explicit empty/zero values without changing native defaults, flags, or the correctly scoped embedded Layout plot data. |
| ACadSharp Layout/PlotSettings view | Resolve block/UCS/viewport references by type; derive viewport lists from the associated block; project name-based paper state, defaults, independent paper-origin properties, plot-view/version behavior and inherited plot fields. | C# DXF Layout gives its own members precedence over PlotSettings codes 1/70 and leaves its last-active reference unassigned; DWG assigns the paper-origin vector while DXF assigns only scalar X/Y. These quirks and ignored shade-plot handles stay in compat. |
| Viewport source metadata | Retain bounded DXF code presence and the original DXF/DWG status word in an optional viewport source record; exclude provenance from native equality. | The native visibility normalization and typed flag subsets cannot reproduce raw source status or distinguish omitted fields from explicit defaults. Additive retention adds no document scans and leaves native geometry/flags unchanged. |
| Viewport DXF fields and coordinates | Read plot style, UCS icon/orientation, ambient ACI/RGB, UCS/style/background/shade/sun references, and independent point components instead of discarding them. | Genuine reader data loss; upstream-PR candidates. Preserve native defaults, prioritize ambient true color over ACI, and keep supplied coordinates when other axes are omitted. |
| ACadSharp Viewport view | Map raw status, group-74 icon, block-order ID, paper-layout state, typed boundary/frozen-layer/annotation-scale references, lighting, shading, brightness and IEEE scale/width getters. Project omitted/legacy fields, ignored active/visual-style references and constructor defaults only in compat. | Matches C# Viewport getters and reader templates without replacing native MVIEW geometry, lighting, visibility, source IDs or valid visual-style references. Compatibility new-document initialization sets only its paper viewport defaults. |
| MultiLeader DXF provenance | Retain bounded top-level code presence separately from embedded context codes, excluded from native equality. | Distinguish omitted fields from explicit native defaults without changing constructor values, geometry, content or reader normalization. The reader adds one fixed-size record per MultiLeader and no document scans. |
| ACadSharp MultiLeader view | Map overrides, typed references, text settings and ordered block attributes; expose embedded context as the unattached `#0` handle and preserve standalone context/root/line/break ordering and RGB packing. Project ignored DXF colors, block links/attributes, degree conversion and omitted/version-dependent defaults only in compat. | The reference dump serializes CadObject references as handles. Its DXF mapper ignores several fields that Rust correctly retains, and pre-R2010 DWG leaves vertical text attachments at zero; native data remains available and unchanged. |
| MultiLeaderStyle DXF fields | Read missing draw orders, constraints, attachments, block scales/settings and flags; decode packed colors; read code 3 description and code 300 default text, retaining legacy code 304 fallback. | Genuine discarded/misinterpreted data, upstream-PR candidates. Keep the existing code-3 native name fallback while retaining description independently; the compatibility name comes from the owner dictionary. |
| MultiLeaderStyle DXF writer | Write the description in code 3 and retain vertical attachments/direction and flag 298. | Genuine description/attachment round-trip data loss, upstream-PR candidates. Dictionary entries continue to carry style names. |
| MultiLeaderStyle provenance and view | Retain fixed-size DXF presence, excluded from native equality. Project omitted defaults, dictionary names, ignored DXF packed colors/block links, rotation conversion, and the reference's reversed DWG top/bottom assignment. Initialize style enum defaults only in the explicit compatibility new-document constructor. | Source defaults and reference reader quirks belong in compat; native constructors, colors, rotations, attachments and editable Standard styles retain their values. |
| Leader DXF provenance | Retain source/arrow/text-box/creation-type presence in one byte, excluded from native equality. | Distinguish absent DXF fields from explicit values while preserving native arrow, annotation and text-box defaults. No document scans or per-vertex metadata. |
| ACadSharp Leader view | Resolve dimension styles; derive HasHookline from the reversed final segment with the reference's zero-vector exception and 1e-12 tolerance; project omitted DXF and modern DWG text-box defaults. | Mirror the pinned reference's computed getter and reader defaults only in compat. Native hookline flags, geometry, creation type and annotation dimensions remain unchanged. |
| MLineStyle DXF retention | Read group-70 style flags and retain whether the description was present; exclude description provenance from equality. | Discarded native flags are a genuine reader bug and upstream-PR candidate. Presence allows compat to distinguish null from an explicitly empty description without changing native strings. |
| TableStyle source retention | Retain the DXF/DWG description in its native field while preserving the existing name fallback; retain legacy DWG flags and suppression bits before native normalization, excluded from equality. | Discarded descriptions are an upstream-PR candidate. Source flags let compat preserve the reference's independent values without changing native suppression behavior. |
| ACadSharp style view | Map legacy rows and modern named cell styles, separate color fields, raw border identities and presence, version-gated formats, source defaults, dictionary-assigned style names and empty source CellStyles. | Reproduce the reference builder's omitted custom/unnamed templates, immutable top-edge flag and modern-reader defaults only in compat. Preserve native rows, flags, border visibility and custom styles. |
| Material DXF provenance | Retain bounded scalar-code presence, advanced-tail code presence and the last group-90 value; exclude source metadata from native material equality. | Distinguish omitted fields from explicit zero/default values and empty strings, and preserve reused-code semantics without cloning maps or textures. Native defaults, colors and procedural textures remain unchanged. |
| ACadSharp material view | Project absent colors as ByBlock, little-endian TrueColor packing, nullable filenames/descriptions, DXF omitted-code defaults and advanced-code aliases, and pre-R2007 channel defaults. Install material defaults in the explicit compatibility new-document constructor. | Match the reference's color mapper, constructor and version-specific reader behavior only in compat. Native material construction, RGB values, map fields and XRecord-backed textures remain intact. |
| Entity common storage (R3-P1) | Keep handle, owner and raw DWG record inline; store appearance/references/XData in shared `Arc<EntityProperties>` with copy-on-write field access. Default common data shares one allocation; serde remains flat. Appearance fields in `EntityCommon` struct literals/destructuring and owned field moves must use `properties` (or constructors and field assignments/clones); borrowed field reads and field edits remain available through `Deref`/`DerefMut`. | R3-A2 added a 400-byte inline common record to every child. Compact common storage preserves every native value, independent edits and child identities without per-child empty collections or strings. Native layout/API adjustment; upstream performance candidate, not an ACadSharp quirk. |
| DWG child appearance reuse (R3-P1) | Reuse equal source properties in a bounded, chunk-local previous-child cache in the shared serial/parallel decoding path; compare every mapped appearance field before resolving names or allocating collections. Keep raw bytes and handle/owner per child. | Avoid allocating identical vertex/face appearance data and copying it through pending/final child vectors. No document scans, cross-worker locks or discarded provenance; upstream performance candidate. |
| R12 DXF template provenance | Retain source table and block order, explicit marker handles, pre-normalization child/viewport owners, and unnamed shape-style records otherwise overwritten by the native name index; observe the explicit handle maximum while decoding. | Additive provenance supports compatibility allocation without changing native defaults or deleting valid entities. Duplicate unnamed style loss is a candidate for an upstream table-model improvement. |
| ACadSharp R12 identities | Replay source-template allocation, then table/block registration and handle collisions; construct the reader's dictionary graph and rewrite typed references. | Allocation follows source order, including empty-table behavior, dynamic layer/line-type insertion, and ownerless ENTITIES registration, without fixture names or seed cutoffs. The native document remains unchanged. |
| ACadSharp sequence registration | Project the default Seqend registered when a modern DXF collection receives its first child, missing UpdateCollections dictionaries, and the resulting HandleSeed. | Constructor defaults and document allocation belong only to the compatibility view; native parsed terminators are preserved. |
| Source APPID provenance | Retain the names decoded from DXF/DWG APPID tables independently of native writer defaults. | Additive reader provenance lets the ACadSharp registration view omit synthetic defaults while preserving explicit source applications, including identical names/handles. No native records are removed; not an upstream decoding bug. |
| Symbol-table flags and font retention | Retain full APPID/LAYER/LTYPE/STYLE source flags, direct STYLE font flags independently of ACAD font XData, LTYPE alignment, and DXF layer material/plot-style handles. | Previously discarded source fields are genuine reader bugs and upstream-PR candidates. Existing native font metadata, visibility flags and references remain available; the compat view does not conflate font XData with the STYLE property. |
| Symbol-table nullable/default provenance | Retain LTYPE description presence and STYLE filename/last-height/big-font presence; exclude this provenance from native equality. | Distinguishes omitted fields from explicit empty strings or explicit format-default numbers without replacing native constructor defaults. Fixed-size per-entry metadata; no reader document scans. |
| Legacy DWG layer lineweight | Default omitted pre-R2000 layer lineweight to Default (-3), instead of explicit zero millimeters. | Genuine missing-field default bug; candidate for an upstream PR. Modern stored weights are unchanged. |
| ACadSharp basic table view | Map symbol flags, complete simple/complex segments, computed pattern length, nullable strings, case-aware unnamed STYLE ordering, font flags and Defpoints plotting. Apply reference text/linetype constructor defaults only in the explicit compatibility new-document constructor. | Preserves native table names/order, font metadata, editable values and writer defaults. Source DXF segment style references reproduce the oracle's ignored code 340; source DWG flags omit the oracle's unused resolved bit; reference layer builders leave material unassigned and DWG plot-style names at zero, so those quirks stay in compat. |
| DIMSTYLE DXF provenance | Retain a fixed-size group-code presence mask, excluded from native equality. | ACadSharp constructor defaults differ from the native DXF defaults. This metadata preserves explicit source values while allowing omitted-value projection in compat, with bounded storage and constant-time updates. |
| ACadSharp dimension-style view | Map stored scalars, colors and resolved table references; derive prefix/suffix; project source defaults, setters, legacy DWG bit order and modern DWG fit-slot naming. Set empty postfix and separate-arrow defaults only in the compatibility new-document constructor. | ACadSharp's reader quirks belong in compat. Native DIMFIT/DIMATFIT values, DXF defaults, explicit fields and source colors remain intact. |
| VIEW partial DXF UCS axes | Retain individually supplied axis coordinates; initialize omitted components of a partially supplied axis to zero instead of discarding the entire axis. | Genuine reader data loss when a record omits X or Y; upstream-PR candidate. Fully omitted axes retain native defaults. |
| ACadSharp View/VPort view | Map mode/icon/axis fields and source grid defaults; expose only resolved in-memory visual styles and reproduce unassigned source visual-style references. Initialize VPort constructor defaults only in the compatibility new-document constructor. | The reference VIEW/VPORT builders never assign their parsed style handles. Native references and native constructor defaults remain available; the compatibility projection alone reproduces this quirk. |
| VIEW/VPORT source provenance | Retain bounded presence bits for packed view mode, UCS axes/icon, grid flags, circle zoom and grid display fields; keep native constructor values and equality unchanged. | The ACadSharp reader applies defaults when these DXF groups are omitted, while explicit zero values must remain distinguishable. The metadata is fixed-size and per-entry, with no document scan; the compatibility view applies the reference defaults and packed fields. |
| ACadSharp registration view | Index table/block collections, dictionary entries, extension attachments and evaluation expressions once; apply default unknown-object policy and DXF VisualStyle callback behavior. Resolve dictionary-assigned names/owners and collection counts from the reachable set; XRecord handle values resolve against accepted templates, including known unregistered records, while literals stay unchanged. | Mirrors ACadSharp registration without deleting richer native data. Extension-dictionary ownership uses the same fallback as the native lookup, indexed once instead of scanning the document per object. `dump_file` and `dump_reader_document` apply read registration; `dump_document` retains its explicit inventory and edited allocator semantics. Constructed documents retain explicit registration semantics. |
| Hatch DXF provenance | Retain elevation XY, omitted-field presence, the pattern trailer code and original gradient ACI values alongside native RGB colors. Exclude provenance from native Hatch equality. | Additive source retention permits source-dependent defaults and reader quirks without changing native seed points, styles, pixel sizes, scales or colors. No whole-document scans or payload clones. |
| Hatch gradient recovery provenance | Move the original gradient into optional provenance when legacy round-trip metadata restores a gradient; exclude it from native equality. | Preserve native recovery while allowing compat to expose the original values, since ACadSharp does not perform this recovery. No additional document scan or gradient-payload clone. |
| ACadSharp hatch view | Map boundary edges and ordered unique entity references, pattern lines and gradient stops; project DXF elevation seeds, ignored styles/RGB, consumed pattern trailers, omitted scales and names, and absent DWG non-rational weights. | Reproduce the reference's nested schema, constructor defaults and reader quirks only in compat; preserve native data and DWG version semantics. |
| IMAGEDEF DXF units | Read group 281 into the existing resolution-unit enum. | Genuine discarded-data bug; candidate for an upstream PR. Native defaults and explicit values remain unchanged. |
| BookColor DXF provenance | Retain whether a DXF color code was present; exclude provenance from native equality. | ACadSharp's absent-color default is ByBlock while native Color defaults to ByLayer. Project the difference only in compat and preserve explicit ByLayer. |
| ACadSharp media identities | Resolve empty group/underlay/book-color names from owner dictionary entries, split color-book components, expose the base underlay markers and little-endian TrueColor packing. | Match the reference's name getters and inherited markers without changing native names, page identity, RGB values or common entity appearance. |
| Compound entity identity | Retain inline common state on 2D/3D vertices and full Seqend records on INSERTs and polyline/mesh parents; DXF captures child handles, owners, appearance, XData and scoped references. DWG serial and rayon assembly move the retained state into children, including polygon-mesh flags and the implicit DWG face-record flag (also initialized by face constructors); the shared postpass decodes child/Seqend XData after APPID resolution. | Genuine reader data-loss fixes, candidates for an upstream PR. Seqends are boxed once per compound parent to keep the entity enum narrow; no separate allocation per vertex or per-object document scan; 3D vertex `handle`/`layer` access delegates to common state rather than duplicating strings (struct literals now use `common`). |
| DXF polyline source values | Retain raw parent/2D vertex widths alongside native effective widths; retain 3D thickness and polyface parent geometry fields previously discarded. | Native width inheritance remains unchanged; raw widths permit the ACadSharp view to reproduce its non-inheriting reader. The discarded geometry fields are upstream-PR candidates. |
| Child handle allocation | Reserve inline child handles in document add/load, allocator synchronization and collision repair. | Retained vertex/Seqend identities must not collide with later entity/table allocation; allocation-free local traversal, an upstream-PR candidate. |
| Compound entity writers | Preserve non-null child/Seqend identities and common metadata, reserve allocator space above retained child handles, and keep heavy 2D polylines when converting to LWPOLYLINE would lose child identity. | Genuine round-trip data-loss/collision fixes, candidates for an upstream PR. Newly constructed plain 2D polylines without child identities still down-save as before. |
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
| ACadSharp tolerance style reference | Resolve the retained style handle first, then the case-insensitive style name, falling back to Standard. | CadToleranceTemplate resolves DWG handles and DXF names; unresolved references keep the default attached by Tolerance.AssignDocument. The native reader already retains both forms; only the compatibility projection changes. |
| ACadSharp dimension style colors | Use the existing appearance-color projection for DIMCLRD, DIMCLRE, DIMCLRT and DIMTFILLCLR, packing TrueColor as R \| G<<8 \| B<<16. | ACadSharp Color stores RGB bytes in little-endian order and removes its internal true-color flag in the getter. Native channels and RRGGBB packing are correct and unchanged; indexed/ByLayer/ByBlock colors retain their semantics. |
| MText/Tolerance equality | Keep equality defined by the pre-existing native fields; new source spelling/presence and recovery markers do not affect it. | Source provenance must not change native drawing-state equality. All existing round-trip comparisons remain intact; focused tests separately verify provenance and projection. |
| ACadSharp dimension subtype view | Map radius point names, linear/angular offsets, Angular2Line center, Diameter/Ordinate/Arc angular flags and Arc endpoint measurement. | Preserve C# bit-test, exact-intersection, ratio-parallel and throwing-getter quirks only in compat. Native geometry, dimension styles and common fields are unchanged. |
| R4-C3 DWG linetype segment scale | `LineTypeElement` keeps the scale stored for plain dash/space segments (`source_scale`, excluded from equality); the DWG builder fills it. | ACadSharp exposes the stored scale (often 0) for every segment. Native constructors leave it `None` and keep scale 1 in the view. Upstream-PR candidate (lossless metadata). |
| R4-C3 source table metadata | Mark decoded DIMSTYLE records (`DimStyle::source_present`), keep every CLASS record including the empty-name placeholder in the source list, keep the source spelling of `*MODEL_SPACE`/`*PAPER_SPACE` block records read from DXF, remember a DXF dictionary without group 281 (`Dictionary::cloning_absent`), and the position of the STYLE `ACAD` typeface block among its DWG extended-data blocks. | The view must tell constructor defaults from source data and keep reference ordering/spelling. Fields are `#[doc(hidden)]`, serde-skipped and excluded from equality; native validation is unchanged. Empty-name CLASS and block-name spelling are upstream-PR candidates. |
| R4-C3 DWG block marker collision | A decoded BLOCK/ENDBLK marker keeps its identity when it collides with a constructor table default; the table repair relocates the default. | Both the marker and the default were relocated before, leaving a second marker and a wrong BlockRecord reference (seen with a Standard DIMSTYLE at the ENDBLK handle). Genuine native identity bug; upstream-PR candidate. |
| R4-C3 OLE2FRAME payload | The DWG reader keeps the stored payload bytes when `encoded_payload()` would lay the compound file out differently (`Ole2Frame::source_payload`, excluded from equality). | The structured storage re-encodes CFB sectors differently, so the dumped `BinaryData` differed. Retained only when the re-encode differs (no copy in the common case). |
| R4-C3 3DSOLID/surface wireframe layout | The silhouette count is read after the isoline block also when `isoline_present` is clear, unless the next bit pair is `1,1` (an invalid bit-long, i.e. the older layout without a count); a clear ACIS-empty flag after the wireframe in R2013+ records means the body follows inline, even when the AcDs flag is set. The DWG writer writes the count in the same place. | Real R2013 drawings (et2 kt.dwg, owerko hatches_part2.dwg) store a zero count there, and some set the AcDs flag with an inline body; the old reader desynchronised and dropped 26 modeler entities (Solid3D, Extruded/Swept/Nurb surfaces). ACadSharp reads the count unconditionally. The pre-existing "absent isoline cache" test keeps passing through the bit-pair rule. |
| R4-C3 AcDs section reading | Accept sparse AC18 sections (an unwritten zero page makes the declared size exceed the written pages) by bounding the size with the last page offset; pick the current listing of a handle by the segment sequence number (offset 8) instead of file order; a stale listing without a blob does not block pairing of external blobs. | AcDs of incrementally saved files was rejected or paired with older body revisions (owerko 3d_solids.dwg, markov task-043, et2 kt.dwg). Matches the revision ACadSharp attaches in every corpus case. |
| R4-C3 modeler view | Surface entities are dumped as their subclass (Plane/Extruded/Lofted/Revolved/Swept/Nurb) with the Surface properties; DWG surfaces report isoline counts 0 as the reference never decodes them, and carry the captured DWG source for the modeler replay. | ACadSharp reads each subtype as its own class. Compat only. |
| R4-C3 collection dictionaries | The view creates every missing UpdateCollections dictionary (layout, group, scalelist, mlinestyle, mleaderstyle, tablestyle, image, pdf, color, variables, material) in reference order after the source records, adopts the first header dictionary with owner zero (group, mlinestyle, named objects, ...) as root as ACadSharp does for a damaged file, and omits R13/R14 placeholders. | Missing dictionaries shifted every later handle and left default entries unregistered. In a damaged file the reference root is the group dictionary and the real root stays unregistered. R14 placeholders never register in the reference (all three R14 drawings). Native documents are untouched. |
| R4-C3 entity-attached defaults | A dimension, leader or tolerance in a block makes the view contain a "Standard" dimension style with constructor values and the next free identity; a MultiLeader makes it contain a "1:1" scale. Without such an entity the native default dimension style is omitted. | `Entity.AssignDocument` adds the default style/scale to the document tables before the entity's own reference is resolved. |
| R4-C3 table entries | Empty table-entry names become `unnamed<N>` (not for VPORT), an empty-name VPORT is dropped, ByBlock/ByLayer DWG layer colours read as index 7, BLOCK_RECORD.Layout is null unless a decoded layout exists (registered for DXF), Group.IsUnnamed follows the dictionary-assigned name, graph expressions the reader never produced are null, MTEXTATTRIBUTE context data is not read from DWG. | Table.Add naming, the DWG layer reader, VPortsTable.Add, CadLayoutTemplate, the Group getter, the EvaluationGraph template and the TODO in `readMTextAttributeObjectContextData`. Compat only. |
| R4-C3 record projections | DWG ucs icon bit 1 maps to OnOrigin (3), DWG R2000+ headers expose DIMSD1/DIMSD2 as the extension-line suppression flags, OLE frames report no source application or paper-space flag for DWG, MLineStyle colours pack RGB like the other appearance colours, a DWG DIMENSION association resolves polyline vertices as targets, BlockReference context data keeps the annotation-scale identity, an unresolved MLeader linetype is ByLayer. | Reference reader quirks and getters. Compat only. |
| R4-C1 DWG invisibility | The common entity data reads the invisibility word as a flag: only bit 0 hides the entity (was: any non-zero value). | DXF group 60 is 0/1 and ACadSharp tests `(value & 1) == 1`. Real files carry 2 (206,650 SEQENDs of bridge-slab/UIT BHUWANA PART1.dwg, 77 of cadviewer/ma1000_133.dwg), which hid entities AutoCAD shows. Upstream-PR candidate. |
| R4-C1 DWG dimension elevation | The record's single elevation is also the Z of the dimension's insertion point (point 12), not 0. | The DWG stores one elevation for points 11, 12 and 16; owerko/dimensions.dwg has dimensions at -208. The writer still writes only X and Y. Upstream-PR candidate. |
| R4-C1 DWG owned-vertex lists | When a POLYLINE/mesh record lists its owned vertices (R2004+), vertices that merely name it as owner but are absent from the list are dropped. An empty list keeps the owner-derived members. | Stale vertices are not part of the polyline in AutoCAD; bridge-slab/PIER REINFORCEMENT 02122007.dwg has 5 polylines with 2 unlisted vertices each (18 `Vertex2D`). |
| R4-C1 DXF hatch ellipse edges | The DXF reader converts HATCH elliptic-arc edge angles (groups 50/51) from degrees to radians, as it already did for circular arcs; the DXF writer converts back. | The model holds radians (the DWG reader and the doc comment agree) and DXF stores degrees; the reader kept degrees, so DXF and DWG ellipse edges differed (ezdxf/hatches_2.dxf). Upstream-PR candidate. |
| R4-C1 table cell group 91 | `CellValue::dxf_long` (additive) keeps DXF group 91 as read, even when the 93 flags suppress the value body. | Needed to tell "91 present" from "absent" in the ACadSharp view (below). |
| R4-C1 LWPOLYLINE flags | The view reports Flags as Closed (1) plus Plinegen (128). | `LwPolylineFlags` has both bits; the view only reported Closed (6,048 polylines in 29 files). |
| R4-C1 arc dimension name | `DimensionArc.ObjectName` is `ARC_DIMENSION`. | ACadSharp overrides it; every other dimension reports `DIMENSION`. |
| R4-C1 two-line angular measurement | The view reproduces `DimensionAngular2Line.Measurement`: the angle between the two direction vectors, replaced by its supplement when the one-sided cross-product signs against the normal differ, with the exact-equality `Center`. | The native `measurement_radians` picks the sector geometrically. ACadSharp's rule disagrees on markov-ai/task-020_gold.dwg #1191 (3.0595 vs 0.0821). |
| R4-C1 legacy code-page replacement | In the view, a U+FFFD in a pre-R2007 DWG TEXT value becomes `?`, and an ASCII character right after it is dropped. | .NET's DBCS decoders replace an invalid lead byte and the following ASCII byte with one `?`; encoding_rs emits U+FFFD and keeps the ASCII byte (scau-gis/别墅建筑结构全套图纸.dwg, 462 texts). Applied to TEXT only, the only mismatch seen. |
| R4-C1 block membership view | For DWG sources the view lists as a block's entities only the handles the block record itself lists (`block_entity_handles`: R2004+ owned list, R13-R2000 chain); the native record keeps owner-derived members too. A BlockRecord's owner pointer registers only its BLOCK/ENDBLK markers and non-graphical objects. | ACadSharp adds an entity to a block only through that list, so an entity that names the block as owner but is unlisted is never registered (et2_blocks/py.dwg `py_resist90`, 12 stale entities; most owerko/*.dwg files, in anonymous blocks). DXF sources and documents without a source list are unchanged. |
| R4-C1 DXF unknown-typed table cell values | In a DXF TABLE, a cell value of unknown type reports group 91 as its Value (also when the 93 flags say empty); with no group 91 it stays null. | `readCadValue` calls `SetValue(int)` for group 91 whatever the flags (ezdxf/acad_table_with_blk_ref.dxf). |
| R4-C2 proxy view | `ProxyEntity` / `ProxyObject` resolve `DxfClass` by class number in the name-deduplicated class list (all DXF classes are 500) and `ClassId` reads its number, throwing `NullReferenceException` without a class. `Version` is the stored low word and `DrawingFormat` the combined word (the native record keeps them the other way round); `Data` is an empty-object stream only for DWG R2010+ (the reference learns the handle-section offset there only) and `BinaryData` is never assigned by the DWG reader. Before R2000 the version fields stay zero. | View only; the native proxy records keep the stored class id, both words and the bit-exact payload. Dump quirks of `readCommonProxyData`. |
| R4-C2 field values | `Field.Value` / `Values` are `CadValue` objects (flags, type, units, format strings from R2007, value). The reference does not wire the value templates, so handle values stay unset; before R2007 it decodes strings with their stored NUL. `FormatString` is the value string (the DWG reader overwrites the older format string with it, DXF group 301), `CadObjects` holds only the 331 references that resolve, `Children` and `FieldList.Fields` only fields that resolve (the list once each), and a field outside a dictionary has an empty `Name`. | View only. Native `Field` keeps format, children and references separate. |
| R4-C2 field registration | A registered field registers the child fields it lists (`Field.Children`) and owns them. | `CadFieldTemplate.build` adds each child to `Children`, which registers it; the children are not in any dictionary (259 fields in 15 files were missing). |
| R4-C2 DXF field value string source | `Field::value_string_source` (additive, excluded from equality, not serialized) keeps group 301 as written when caret-escape decoding changed it. | ACadSharp does not decode caret escapes in this group (`^I` stays `^I`); the native value stays decoded. |
| R5a `acad-dump write-dxf` | New command: failsafe read as `dump` does, then `DxfWriter` in the document's own version (`--binary` for binary DXF). Exit 0, 1 (`read failed:` / `write failed:` on stderr), 2 (usage). | The fork side of gate G4 (`Tools/roundtrip-report.sh --impl rust`). `dump` is unchanged. |
| R5a STYLE font XDATA | The STYLE writer merges its `ACAD` font record into the style's retained XDATA instead of writing a second `1001 ACAD` block; `ExtendedData::merge_modeled_record` matches application names ignoring case and replaces the record in place. | A style read from a file kept its `ACAD` record, so the output carried the application twice and ACadSharp threw on the duplicate key. R14 DWG files store names in upper case (`ACCMTRANSPARENCY`), so the layer writer added a second record, and moving a merged record to the end swapped the layer's records. Upstream-PR candidate. |
| R5a object attachments | `Scale`, `MLineStyle`, `Group`, `ImageDefinition` (and its reactor), `MultiLeaderStyle`, `TableStyle`, `SortEntitiesTable`, `RasterVariables`, `BookColor`, `DictionaryWithDefault`, `WipeoutVariables`, `Field`, `FieldList` write `{ACAD_REACTORS` / `{ACAD_XDICTIONARY` from the document side maps; `SpatialFilter` and DIMSTYLE records write their reactors. | These writers dropped the reactors and extension dictionary the reader had kept. Upstream-PR candidate. |
| R5a symbol-table flags | LAYER, LTYPE, STYLE, APPID and BLOCK writers add the source bits without a typed field (`source_flags`: 64 referenced, 32 resolved) to group 70; a DWG block keeps its anonymous bit. | The writers rebuilt group 70 from the typed flags only and lost the 64 bit. |
| R5a layer plot style | A layer without a plot style gets the Normal one only in a document built in memory. | A layer read from a file that had none was given 390 = Normal on write. |
| R5a layer transparency | A retained `AcCmTransparency` record that still decodes to the layer's value is written unchanged. | A packed zero (opaque default) came back as 33554687. |
| R5a BLOCK / ENDBLK | The markers write their retained common data (layer, color, linetype, lineweight, attachments, XDATA) from `block_entity_common` / `block_end_common` (the marker entity for a DWG read); group 4 is written when the text is not empty or the source had it (every R2000+ DWG record has one). BLOCK_RECORD groups 70/280/281 follow the source presence of a DXF record. | The markers were written bare, so the retained state never reached the file. |
| R5a source-absent groups | A DXF record writes only the groups its source had, unless the value differs from the reader default: DIMSTYLE (via `dxf_present_codes`), LEADER (71/40/41/73), MULTILEADER (272/273), MLEADERSTYLE (173/174/178/46/272/273/300), VIEWPORT (61), TOLERANCE (11), MTEXT (441), ATTDEF/ATTRIB group 71 when zero, DIMENSION (71/72, and 41 when zero), DBCOLOR (62/420). Header variables absent from a DXF source whose value is still the default are left out (`$ACADVER`, `$ACADMAINTVER`, `$HANDSEED`, `$DWGCODEPAGE` always written). | ACadSharp applies its own defaults to an absent group; writing the native default asserted a value the source never had. Edits still reach the file. |
| R5a version gates | DIMENSION 71/72/41 and MTEXT 73/44 are R2000 groups and are not written below AC1015; DIMFXL/DIMJOGANG are not written for a record read from a DWG older than R2007; LAYOUT group 78 is skipped when 0; `$DIMSTYLE` takes the table entry named by the header handle when the name is still the default. | A DWG read leaves zero placeholders for fields its version lacks; writing them changed the value ACadSharp reads. |
| R5a header | `$CMLJUST` and `$CMLSCALE` are written. | The reader read them, the writer never wrote them (ACadSharp then read 20 for the multiline scale). |
| R5a line breaks and spelling | MTEXT writes the source spelling (`\U+00B0`, `^J`) while it still decodes to the value (`MText::source_value` now also kept for caret escapes and binary line breaks); binary DXF writes it raw; a line break in TOLERANCE text, GEODATA definitions and retained spellings is a caret escape. XRECORD strings, TOLERANCE text and ATTRIB/ATTDEF tags keep their source spelling while unchanged. Text without a source spelling: see R5a-3 (a line feed is `^J`, `\P` stays a paragraph break). | The writer turned every line break into `\P` and every decoded escape into the character. New stream-writer methods `writes_source_spelling` / `write_source_spelling`. |
| R5a pre-R2004 restored data | A gradient or MTEXT background fill the reader restored from the round-trip XDATA of a pre-R2004 save is not also written as native groups below AC1018. | The native groups made ACadSharp read a fill the original file never stated. |
| R5a alignment grip | BLOCKALIGNMENTGRIP orientation uses groups 140/141/142. | Written as 140/150/160, which ACadSharp rejects ("Invalid dxf code"). |
| R5a LTYPE reader | An empty group 9 after a shape element no longer turns it into text. | ACadSharp writes `9` with an empty string after every shape element; the shape number was lost. |
| R5a layout provenance | `Layout::dxf_layout_flags_present` (additive, not serialized) records whether the `AcDbLayout` subclass had its own group 70. | The ACadSharp view needs it (below). |
| R5a ACadSharp view | `LayoutFlags` of a DXF without the `AcDbLayout` group 70 is the PlotSettings word; DXF `ShadePlotMode` and DIMSTYLE `Flags` are the reference defaults (it never assigns them); `ImageDefinition.DefaultSize` is (1, 1) when a DXF omits groups 11/21; a `BookColor` name with several `$` keeps the first and last segment. | ACadSharp's own writer omits the layout group 70 and the image default size; its readers route codes 76 and the first 70 elsewhere. |
| R5a-2 CLASSES | A document read from DXF writes its CLASSES section as read (flags, application, instance count); the default class registry it also carries is not written. A class a written entity/object needs and the section lacks is added only for records created after the read (no handle yet, or at or above `$HANDSEED`). Other documents (DWG source, built in memory) write their registry as before. | The fixed 260-entry list replaced the source's flags and invented instance counts; a built-in DXF type (MLINE) has no class in AutoCAD files. |
| R5a-2 MATERIAL groups | A MATERIAL read from DXF writes the groups its record had plus those whose value differs from the native default; ambient RGB (90) and the self-illumination integer (90) are told apart by group 149 and the ambient method. A built-in (ByLayer/ByBlock/Global) material without a DXF source keeps the minimal form plus any changed value. | Native defaults were written for absent groups (projection 2, source 0, channel flags 127, matrices, colours), which ACadSharp reads differently from the absent group, and a trailing `90 0` replaced the ambient colour. |
| R5a-2 header variables | About 85 reader-known system variables the writer never wrote (`$PLIMMIN/MAX`, `$PEXTMIN/MAX`, `$PINSBASE`, `$USRTIMER`, `$TDUSRTIMER`, `$XEDIT`, `$HIDETEXT`, `$SORTENTS`, `$PROJECTNAME`, `$STYLESHEET`, `$DIMBLK/1/2`, `$DIMLDRBLK`, `$LENSLENGTH`, `$LOFT*`, `$TIMEZONE`, `$LATITUDE/$LONGITUDE`, `$USERI/R1-5`, ...) are written when the DXF source had them, when a DWG source of a version that stores them was read (even at the native default), or when the value differs from the default. Arrow block names of a DWG header come from the block record handle. `$HYPERLINKBASE` is written empty when the source stated it. `$SKTOLERANCE`, `$CANNOSCALE`, `$CTABLESTYLE/$CMLEADERSTYLE` policies are unchanged. | The reader read them, the writer dropped them; a source-absent or default value in a built document is still not written. |
| R5a-2 purge preventer | `ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION` is written (class version in group 70, no block pointer). Unknown or associative records of that name without decoded data are still skipped. | AutoCAD's own DXF has the record; dropping it removed a dictionary entry (`AcDbDynamicBlockRoundTripPurgePreventer`) from the file. |
| R5a-2 default application IDs | The native default APPIDs `AcadAnnotative`, `AcCmTransparency`, `AcAecLayerStandard` are not written for a document read from a file whose APPID table lacked them, unless XDATA, a layer transparency or a layer description needs them. | The writer invented applications the source never registered. |
| R5a-2 handle seed and sort handles | `$HANDSEED` of a document read from a file is the source's seed (or one above the highest handle written) when no handle is allocated while writing; a SORTENTSTABLE entry keeps its sort handle (only a missing or repeated key is allocated); `count_extra_handles` no longer counts the entries of a table written verbatim. | The seed grew by the defaults the reader creates on every round trip, and every sort entry got a new handle (so SEQEND identities shifted). |
| R5a-2 MLEADER line type | MLEADER group 341 keeps the ByBlock linetype handle. | It was rewritten to ByLayer. |
| R5a-2 entity material | Group 347 (material handle) is written from R2007 for an entity whose `material_flags` say a handle follows. | The reader read it, the writer dropped it. |
| R5a-2 VIEWPORT groups | Groups 1 (sheet style), 74, 79, 345, 346, 332, 333, 348, 361, 141, 142, 63/421, 292, 282 are written when the DXF source had them, a DWG record of the version stores them (sheet style R2000+, lighting R2007+), or the value differs from a new viewport's. | Only the geometry groups were written. |
| R5a-2 dynamic block connections | An action's connections are written as code, name, code, name (`92 301 93 302`), not all codes first. | The reference reader names the connection it has just read: the second name was lost. |
| R5a-2 dimension record | Bit 32 of group 70 is set for a dimension without DXF source word that has a geometry block; group 41 is written at 1.0 for a record read from a DWG of R2000 or later (and for a DXF source that had it). | DWG dimensions lost the bit and the spacing factor (ACadSharp then reads 0). |
| R5a-2 hatch gradient | A pattern hatch of an R2004+ drawing keeps its disabled gradient definition (450 = 0, colours, name). | Written only when enabled; the DWG record holds the definition either way. |
| R5a-2 polyline widths | POLYLINE groups 40/41 are written from the source's default widths once the widths live on the vertices. | A second write dropped them (not idempotent). |
| R5a-2 DWG header omissions | For a document read from DWG the writer leaves out the variables the source version has no slot for: `$DIMFIT` (R2000+ reads DIMATFIT), `$OSMODE` above R14, `$PSTYLEMODE` below R2000, `$MEASUREMENT` without a template or below R2004. Text style, linetype and dimension text style names of the header come from the table record handle, `$MAINTENANCE` falls back to the document's maintenance version, `$DIMMZS/$DIMALTMZS` only when set. | The names were stale or invented for a DWG source; a DXF source and a built document are unchanged. |
| R5a-2 layer description | The `AcAecLayerStandard` XDATA record of a layer is regenerated only when the description differs from the one the source record carries; the record the source had is written as it was. | A one-string record was rewritten as two strings, the layer then differed from its source. |
| R5a-2 layout plot view name | LAYOUT group 6 is written (empty or not) for a document read from a DWG of R13..R2000, which stores the plot view name. | The group was left out when empty. |
| R5a-2 leader box | LEADER groups 40/41 are not written for a record read from a DWG of R2010+ (it stores no box size). | The native 2.5 default was written as if stated. |
| R5a-2 spline | A control-vertex spline carries a parametrisation bit in group 70 only when it states chord or uniform, the DXF source had one, or it is a fit-method spline. (The 1e-7 tolerances this row once wrote for the DWG scenario 2 spline are gone, see R5a-3.) | The DWG custom-parametrisation default (15) became bit 256 for every spline. |
| R5a-2 XDATA strings | Superseded by R5a-3: the DWG reader removes the UTF-16 terminator, the DXF writer no longer trims. | DSTYLE override strings read from DWG ended in a NUL and the overrides were lost on reading. |
| R5a-2 viewport override layers | Layer names of a VIEWPORT's per-viewport layer overrides ("0 @ 1" form) are resolved from the layer table when the entity carries only handles. | The groups held a handle number. |
| R5a-2 underlay reactors | PDF/DWF/DGN underlay definitions take their reactor list from `reactors_by_handle` when the object has none (DWG). | The owner reactor was lost. |
| R5a-2 R13/R14 dimension style | The DWG reader gives `dimlwd`/`dimlwe` -2 (ByBlock) for an R13/R14 DIMSTYLE record, which has neither field. | It gave 0, a 0.00 mm line weight, which the DXF then stated. |
| R5a-2 table cell style | A cell style read from a binary R2010+ file is written as only the properties its property flags override (alignment, background fill/colour, content colour, text style/height, borders), not as the full style. A cell that overrides nothing writes no style groups. | The full style gave every cell an explicit background colour (0 or 257 in ACadSharp's reading) instead of none. Per-row styles (`Rows[n].CellStyle`) have no DXF group and stay lost. |
| R5a-3 `acad-dump native-dump` | New command `native-dump <in.dwg\|in.dxf> <out.jsonl>`: failsafe read, then `acadrust::native_dump::write_native_dump` (module `src/document/native_dump.rs`, feature `acadsharp-compat`). JSON lines, one record per line, no ACadSharp view. Built from the `Debug` text of every record (so no serde attribute hides a field, and a new field appears by itself); the fields it leaves out are the named lists `EXCLUDED_FIELDS`, `EXCLUDED_FIELDS_WHEN` and `EXCLUDED_DOCUMENT_FIELDS` (a reason each; a document field in neither list does not compile; a test checks that every name is a real field). Three splits of one datum between the two readers are folded: BLOCK/ENDBLK markers, object reactors/extension dictionaries, object XDATA still held as raw DWG EED (decoded). | The compat view copies ACadSharp's DXF-reader losses, so RR measured against it could not see a writer that drops those fields (review of R5a-2). RR and idem now compare this dump. |
| R5a-3 MATERIAL R2007 fields | Translucence, self-illumination, reflectivity, illumination model, channel flags (94) and mode of a material read from a DWG older than R2007 are written only when changed; the minimal built-in form keeps group 94 where the source version stores it (R2007+ DWG, built in memory). | The DWG record has none of these fields; the native defaults (94 = 127) were written as if stated. |
| R5a-3 VIEWPORT group 61 | Grid major is not written for a DWG older than R2007 (the record has no such field) unless its value was changed. | Default 5 was written as if stated. |
| R5a-3 spline tolerances | Groups 42/43 are left out for a DWG scenario 2 (fit points) spline whose knot and control tolerances are both zero; stored and changed values are written as they are. | The writer wrote 1e-7 to imitate ACadSharp's initialisers; the native model has 0 and ACadSharp applies its own default to the absent groups. |
| R5a-3 XDATA terminator | `eed_codec` and `typeface_eed` remove one UTF-16 NUL terminator from a counted wide EED string and write it back; `write_xdata` no longer trims NULs. The ACadSharp view is unchanged (it trims). | The DWG reader kept the terminator in the native string (root cause) and the writer trimmed it. Upstream-PR candidate. |
| R5a-3 ATTRIB / ATTDEF group 1 | Group 1 is the record's own value; the text of a multiline attribute stays in its embedded MTEXT only. | The embedded text was copied into group 1; AutoCAD's own file has group 1 empty there. |
| R5a-3 MTEXT line feed | A line feed in MTEXT is written `^J` (carriage return `^M`) in ASCII DXF and as is in binary DXF; `\P` stays a paragraph break. The tests that pinned the old `\P` (`test_binary_dxf_roundtrip_mtext_newlines`, `mtext_without_source_spelling...`) now pin the line feed. | `\P` is a paragraph break, not a line feed; the writer turned every line feed into one (a loss). |
| R5a-3 layer material | LAYER group 347 is written from R2007. | The reader read it, the writer dropped it. |
| R5a-3 MLEADER word break | Group 295 of the context is written when false in any version (R2013+: always). | A file older than R2013 that stated 0 came back as true. |
| R5a-3 table cell values | A point value carries a size group (92) only when the source had one; the DXF reader ends a table value at an end marker (309), the next data item (301, or 300 before a unit group 94) as well as at 304. | AutoCAD's R2004 files have no size and no ACVALUE_END; the reader ran on through the following groups and read garbage into the custom data of a cell. The writer still writes 304 (ACadSharp's reader needs it, else it loses later objects). |
| R5a-3 modeler uid | 3DSOLID/REGION/BODY write their `uid` (group 2, R2013+), the zero GUID when empty. | Always the zero GUID. (A SURFACE has no `uid` field in the model.) |
| R5a-3 angles | Angle groups are written as the degree value that `to_radians` reads back as exactly the stored radians when one exists (`ToDxfDegrees`). | A degree value read from DXF came back a unit in the last place away after a write (not idempotent). |
| R5a-3 DIMPOST | An empty DIMPOST is written (group 3 empty); `<>` is what an absent group reads as. | An empty text came back as `<>`. |
| R5a-3 VISUALSTYLE layout | A style that carries more than the core properties is written in the R2013 layout whatever the file version. | The older layouts dropped the extended properties (58 became 28). |
| R5a-3 `$CMATERIAL` | The DXF reader reads and the writer writes `$CMATERIAL` (347, R2007+) as `current_material_handle`. | AutoCAD's header has it; the fork dropped it (found by comparing the groups of the AutoCAD samples with the written file, not by RR). |
| R5a-4 dictionary hard owners | The DWG reader lists a dictionary's entries as hard-owned (`hard_owner_entries`) when the DWG states the dictionary hard-owning (the flag) or the key is canonical (ACAD_FIELD), as the DXF reader does for groups 360/280. | One drawing had two models: the DWG states ownership per dictionary only, the DXF reader recorded every 360 entry. |
| R5a-4 header names from handles | After a DWG read, the current layer/text style/linetype/dimension style, `$DIMTXSTY`, `$DIMLTYPE`, `$DIMLTEX1/2`, `$DIMBLK/1/2`, `$DIMLDRBLK` and `$CMLSTYLE` name fields are filled from the records the header handles point at; `header.last_saved_by` is the summary's. | The DWG header gives handles and the name fields kept their defaults; the writer wrote ByBlock for the dimension linetypes and "Standard" for the current dimension style. |
| R5a-4 legacy header state | The DXF reader fills `HeaderVariables::legacy` (the R12 view/drafting state) only for R12 files; a later file keeps `$UNITMODE` in `unit_mode` only. | A modern DXF got a default `LegacyHeaderState` from `$UNITMODE`, a DWG of the same drawing none. |
| R5a-4 GROUP names | `CadDocument::resolve_dictionary_names` (both readers) names a GROUP by its dictionary key. | A DXF GROUP has no name group; the DWG reader already did this. |
| R5a-4 MLEADERSTYLE | The DWG reader names the style by its description ("Standard" without one), as the DXF reader does; a DWG before R2010 keeps the DXF defaults for the text attachment fields; the DXF writer leaves groups 272/273 out of such a style unless changed. | The record has no such fields before R2010; the reader took 0 for them. |
| R5a-4 THUMBNAILIMAGE | DXF reader and writer implement the section (R2000 and later): group 90 size, 310 chunks of the preview image (a DIB without file header as AutoCAD writes it; PNG and metafile are told by their signature). | The preview was dropped (`THUMBNAILIMAGE section skipped`). |
| R5a-4 entity graphics | The DXF section reader takes group 92/160 and the following 310 groups of the AcDbEntity subclass out of every ENTITIES and BLOCKS record into `common.graphic_data`, before the entity reader runs. | Only proxy, MLINE, LIGHT, surfaces and TABLE read them; MESH, WIPEOUT, IMAGE and UNDERLAY lost them, and a MESH's group 92 is its vertex count. |
| R5a-4 STYLE typeface | After reading, the DXF reader removes the `ACAD` font record from a style's retained XDATA when its fields were decoded from it (a string and a 32-bit flags word) and keeps its position, as the DWG reader does. | One datum lived in two places in a DXF read (`true_type_font` and the XDATA record). |
| R5a-4 TABLESTYLE | (1) The DXF reader resolves a row's text style name (group 7) to the style's handle. (2) The `CELLSTYLEMAP` object of a style's extension dictionary is restored into `modern_overrides`, the border type and double-line spacing of the rows (which the legacy groups cannot state) are taken from the cell styles, and the object is taken out of the document; its handle is kept in the new `TableStyle::cell_style_map_handle` (not part of equality). (3) The DXF writer writes the object again for every style with modern cell styles, of every version (`store_table_style_carriers`, on a copy of the document), under that handle; a style from an R2010+ DWG has none (NULL), and the writer gives the object the lowest free handle above the style's, below the allocator (`allocate_handle_in_gap`; "free" means no table entry, block marker, entity, child, object or dictionary entry uses it), so `$HANDSEED` stays as the drawing states it. (The R5a-4 agent reserved that handle in the DWG reader; the review moved it to the writer: the DWG-read model invented a handle the file does not have, and every R2010+ DWG read paid a whole-document handle scan.) | An R2010+ DWG keeps the cell styles in the record; every DXF keeps them in the object. Without this the DXF lost `modern_overrides` and the writer created a new handle that moved `$HANDSEED` and every default object. The `ACAD_XREC_ROUNDTRIP` record stays as the file has it. |
| R5a-4 viewport override layers | The DXF reader names an entity on a `<layer> @ <n>` override record by its base layer and keeps the record's handle (`layer_handle`); the DXF writer writes the record's name again. | The DWG reader names it so; the DXF read kept the record's own name. |
| R5a-4 RADIUS | The DXF reader sets `base.definition_point` to the chord point, as `Dimension::set_definition_point` and the DWG reader do. | It stayed zero. |
| R5a-4 SUN colour | SUN writes the true colour in group 421 beside 63 and reads it. | AutoCAD's SUN has `63 7` and `421 16777215`; the true colour was lost. |
| R5a-4 3DSOLID/REGION/BODY | R2013+: group 290 is `acis_data.acis_empty_bit` (it was always 0), group 2 the revision GUID as `{xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx}` (`AcisRevision::guid_string`/`from_guid_string`; the DWG reader sets `uid` the same way, the zero GUID reads as none); the ACDSDATA attach sets `has_ds_data`; a SAT body sets `point_of_reference` from its geometry as the SAB path does. | AutoCAD's DXF of the sample drawing has `290 1` and the GUID of the DWG's revision block; each was dropped. |
| R5a-4 dynamic-block class | A dynamic-block object takes the C++ class name of the file's CLASSES record (`source_classes`). | The DXF subclass marker (`AcDbBlockRotationAction`) differs from the class (`AcDbBlockRotateAction`) the DWG reader keeps. |
| R5a-4 VX table | A DXF of R2004 or later has no VX_CONTROL: `vx_table` gets the null handle. | The document default reserved a handle for a table the file does not have; a DWG of that version has none. |
| R5a-4 anonymous blocks | The DXF reader sets `flags.anonymous` for a block whose BLOCK word has bit 1 and whose name starts with `*`; the writer keeps bit 1 of `source_flags` for any block. The ACadSharp view still takes the bit as it is. | A down-saved dynamic definition read from a DWG is named by its public name and not anonymous, but its marker says anonymous. |
| R5a-4 layout viewports | The DWG reader lists the viewport entities of the layout's block in `Layout::viewports` when the record has no list (before R2004), as the DXF reader does; shade-plot DPI keeps the default 300 before R2004. | DXF has no list and no DPI before R2004 either. |
| R5a-4 ATTRIB/ATTDEF embedded text | The default direction (1,0,0) of an unrotated embedded MTEXT read from a DWG is no stated direction (`dwg_x_direction` None). | The DXF object states group 11 only for a rotated text. |
| R5a-4 TABLE | The DWG reader names the block of an ACAD_TABLE (`block_name`) from its block record; a cell style without a background colour (`None`) has no fill. | The DXF names the block (group 2); `Color::None` was read as a fill. |
| R5a-4 compat view | `CadHeader.MultiLeaderStyle.TextTop/BottomAttachment` of a DWG before R2010 are ACadSharp's defaults; a block's DXF word keeps bit 1 whatever its name. | Keeps the ACadSharp view (G1 45/49) while the native model changed. |
| R5a-5 carriers without a copy | `plan_table_style_carriers` computes the CELLSTYLEMAP objects, the changed extension dictionaries and round-trip records of the table styles with modern cell styles and the handles they take, without changing the document; the DXF writer states them beside the document's objects (`SectionWriter::carriers`: objects, extension dictionary lookup, valid handles, CLASSES, handle seed). `store_table_style_carriers` (the DWG writer's copy) is the plan applied. | The DXF writer copied the whole document (`to_mut`) for any style with modern cell styles. |
| R5a-5 DXF TABLE completion | After a DXF of R2010 or later is read, a table whose `ACAD_XREC_ROUNDTRIP` record (first entry the string `ACAD_ROUNDTRIP_2008_TABLE_ENTITY`) names a TABLECONTENT object (360) of the same cell grid takes from it the name, description, base style, column and row styles and custom data, and per cell the tooltip, custom data, links, style, style id and, for a cell with one item, that item; from the TABLEGEOMETRY object (361) the cell geometry; from the record's groups 90/10/90/90 the break ranges. A cell with no or several items keeps its legacy item, whose format is the cell's own content format. The objects stay, as in a DWG. The legacy rows, columns and base style are kept in `Table::dxf_legacy` (compat provenance, excluded from the native dump, see EXCLUDED_FIELDS; it takes part in `PartialEq`, since the writer states legacy cells from it; the R5a-5 review removed an always-equal `PartialEq`) so that the ACadSharp view of the table is still the oracle DXF reader's, and the DXF writer states the legacy cell styles and items from it while the cell still shows what the legacy entity showed. | An R2010+ DWG holds the table content in the entity record; the DXF read had the legacy cells only (791 rows per cell in each R2013+ DWG). AutoCAD's own DXF carries the rest in TABLECONTENT and TABLEGEOMETRY. |
| R5a-5 legacy cell word | A legacy cell's override word is group 91 (R2007+) or 177 (before): the writer states the style's `override_flags` when the style has legacy bits, else `cell.flag`; the reader reads 177 as unsigned 16 bits and puts the bits no cell group states (above bit 13) on the cell's style instead of keeping the word apart. | A DWG cell states the word on its style; AutoCAD's DXF has `91 = 262176`, the reader dropped bit 18 and the writer wrote `cell.flag` (0). |
| R5a-5 legacy block cell | Pre-R2010 DXF: the CELL_VALUE after a block pointer (340) is an item of its own (as in a DWG record), the writer states it from the second item, and the block pointer is also the cell's `value_handle` (set by a pass after the read, as the DWG record has it). A DWG reader derives `TableCell::cell_type` Block from a block item for the R2010+ record too (the TABLECONTENT reader of the DXF does), and a pre-R2007 DWG text cell has no formatted value. A typed cell value keeps group 91 as its number only (`dxf_long` is for a value of unknown type). | The two readers disagreed on the cell type, the item list, the pointer copy, the formatted value and the second number of the same cell. |
| R5a-5 material ByBlock | An entity whose material flag is ByBlock writes group 347 naming the built-in `ByBlock` material (R2007+); the DXF reader gives an entity whose 347 names that material the flag and no handle. The ACadSharp view reports the handle for a DXF source (the oracle's DXF reader keeps the reference). | A DWG states "ByBlock" in two bits and no handle; AutoCAD's DXF has `347 = 97`. The writer dropped the flag. |
| R5a-5 `$ACADMAINTVER` | A DWG of R2004 to R2013 states the maintenance byte of its file header as `header.maintenance_version`, which is what the DXF variable is (AutoCAD's DXF of the samples: 104, 50, 226, 125, equal to the bytes). Not R2000: sample_AC1015.dwg has byte 15 where AutoCAD's DXF of it states 20 (R5a-5 review); an R2000 DWG has no `$ACADMAINTVER` in the model, and the DXF writer no longer writes the file-header byte as the variable (it writes the header's value). The ACadSharp view for a DWG source is unchanged (its constants). | The DWG header section has no such variable; the byte lived on the document only, so the DXF read had a number the DWG read lacked. |
| R5a-5 DATATABLE | A column of hard-owned handles (cell type 6, DXF group 360, DWG hard-ownership handle) has a codec in the DWG reader, the DXF reader and both writers; types 7 to 9 stay unsupported (the record is kept raw). The two tests that used type 6 as the unsupported example now use 7. | AutoCAD's R2004 files hold two such tables; the DWG kept them as `Unknown` and the DXF writer dropped them. |
| R5a-5 application names | `ExtendedData::get_record` and the DXF reader's `AcadAnnotative` test compare application names without regard to case (a DWG before R2000 stores them in upper case). Upstream PR candidate. | The layer description, layer transparency and the annotative flag of an R14 drawing were lost on a DXF round trip. |
| R5a-5 layout dictionary | The DWG writer's repair of the layout dictionary (rename sync) treats a key that differs from the layout name in case only as in sync. | An R14 DWG keys its layouts in upper case; the repair rewrote and reordered them. |
| R5a-5 R13/R14 leader colour | (R5a-5 review) The by-block colour of an R13/R14 LEADER record is DXF group 77: the DWG reader also states it as `override_color`, the DXF reader of an R13/R14 file also as `byblock_color`. | RR-042 accepted the colour as lost; the group carries it. |
| R5a-5 R13/R14 records | A DWG before R2007 leaves DIMFXL and DIMJOGANG at the model defaults (1.0, 45 degrees) like a DXF without the groups; an R13/R14 viewport has the world UCS axes; the DXF reader restores the view parameters of an R13/R14 viewport from its MVIEW extended data (as the DWG reader). | The DWG reader's zero placeholders and the missing restore made the same drawing differ after DXF. |
| R5a-5 annotation scale | The DXF reader reflects the `CANNOSCALE` entry of the variable dictionary into the header (`current_annotation_scale`, `annotation_scale_value`), as the DWG reader does. | AutoCAD's DXF has no header variable for it; the header said 1:1 for a drawing in 1:100. |
| R5a-5 `acad-dump acis-same` | New command `acis-same <sab.hex> <sat.txt>` (`acis::sab_matches_sat`): exit 0 when the binary ACIS data and the SAT text parse and print the same SAT text. | Lets the gate verify a SAB written as SAT (RR-035) without a second converter. |
| R5a-6 binary DXF version | `DxfWriter` writes binary DXF at the version of the document (or the requested one), as it writes the text form: R13, R14, R2000 and R2004 files are binary R13, R14, R2000 and R2004, strings in the declared code page. It promoted every binary file before R2007 to R2010 (`output_version`, "BricsCAD rejects the older binary headers"). | Binary DXF exists for every version from R10 (R13 and later with two-byte group codes); the promotion was a policy, not a format constraint, and lost the version, the R2007+ defaults it implied and the code page of the file. UNVERIFIED: BricsCAD's acceptance of binary R2000 and R2004 (the reason the promotion was added; no BricsCAD here). |
| R5a-6 R12 DXF writer | An `AC1009` document is written as an R12 file, text or binary (`DxfWriter::write_r12`): the section writer in its R12 mode (`SectionWriter::set_r12`) behind `R12Stream`, which drops groups 100, 102, 105, 330 and 360. Sections HEADER, TABLES, BLOCKS, ENTITIES (no CLASSES, OBJECTS, ACDSDATA, thumbnail, BLOCK_RECORD table). Binary R12 has one-byte group codes (`DxfBinaryWriter::new_r12`; 255 escapes a longer code). An entity R12 lacks is written by the converter of `writer/r12.rs` in its R12 form, one entity at a time (`SectionWriter::write_owned_entity`, `r12::write_r12_entity`; R5a-6 review), so the rest of the document keeps its records, handles and extended data; `with_version(AC1009)` of a document of another version goes through that converter as a whole, now text or binary (it refused binary). The AC1009 DWG writer builds its records from the R13-structured text of the document and keeps asking for that (`DxfWriter::with_r13_structure`). | Upstream issue #68: the writer declared R12 documents R13. An R12 file now reads back to the same native model (G4-RR and idem exact for AC1009 ASCII and binary, and for the six BricsCAD `acR12_dxf` fixtures). Upstream PR candidate. |
| R5a-6 R12 records | R12 form of the records: header variables only from the R12 list (`R12_HEADER_VARIABLES`: AutoCAD's R12 DXF plus `$DWGCODEPAGE`, which is written only when the source stated it, and the pre-R13 view state of `HeaderVariables::legacy`, written per variable when it differs from the default state), `$HANDLING` (0 when the source stated no handle); TABLE headers `name, count` only and table records without handles; tables in the order of the source (`legacy_dxf_tables`), missing ones only when they hold records; LTYPE without the BYLAYER and BYBLOCK defaults, DIMSTYLE without the Standard default the source lacked; APPID in source order; DIMSTYLE groups 4 to 7 always, no 270; LAYER without 290, 370, 420; LTYPE without the zero flag word 74; no entity groups 48 and 370, no DIMENSION 42, no VERTEX 91; BLOCK has no handle and no description, ENDBLK states the handle the source stated (`legacy_dxf_blocks`; the document keeps the one it assigned), the space blocks are `$MODEL_SPACE` and `$PAPER_SPACE`; VIEWPORT is centre, size, status and number with the view in a regenerated `ACAD`/`MVIEW` record (`legacy_viewport::record_r12`, as the R12 DWG writer does). Handles (R5a-6 review): with `$HANDLING` 1 every entity, VERTEX and SEQEND states the handle the document holds, an entity added after the read included; with 0 (the source stated none) no record states one. A handle the writer allocates (a VERTEX or SEQEND without one) and the BLOCK end marker of a block the source did not have are stated only for a document without an R12 DXF source (`source_explicit_handle_max`): otherwise the reader allocated them and allocates them again, so the re-read allocates the same numbers. | Structure and values of the R12 files in the fixtures (AutoCAD-style sample and BricsCAD exports): the groups above do not exist in R12, and writing a handle the reader allocated would shift every allocated handle of the re-read. |
| R5a-6 polyline vertex widths | A VERTEX of a 2D polyline states group 40/41 when its width differs from the default width of the POLYLINE record, and also a zero width read from a file that stated it (`source_widths` set). It stated them when non-zero only. | A vertex without the groups inherits the record's default width: an explicit zero next to a non-zero default came back as the default (AutoCAD-style R12 sample: polyline 0.2/0.1, three vertices with 0/0). A vertex built in memory with width 0 still means the default. Upstream PR candidate. |
| R5a-6 binary DXF strings | The binary DXF reader decodes a string that is not UTF-8, in a file with no declared code page, byte per byte as Latin-1 (what the text reader does); it replaced the bytes with U+FFFD. The bytes of such a TEXT/ATTRIB/ATTDEF string stay with the pair (`undecoded_text_bytes`), as the text reader keeps them, so the ACadSharp view still shows the U+FFFD that ACadSharp decodes (G1 unchanged). | An R12 binary file without `$DWGCODEPAGE` lost its non-ASCII text on the read (sample_AC1009_binary.dxf: one TEXT, `0xD8`), and the text reader kept it. Upstream PR candidate. |
| R5a-7 R13/R14 dynamic block name | The DWG reader finds the `AcDbDynamicBlockTrueName` application case-insensitively, so a dynamic block down-saved to R13/R14 (applications are upper case there: `ACDBDYNAMICBLOCKTRUENAME`) gets its public name back for its anonymous `*U` record, as in R2000+ files. | The lookup compared the exact spelling: in sample_AC1014.dwg the record stayed `*U17` where the record's own extended data and ACadSharp name it `my-dynamic-block`. Genuine reader bug (the spelling of application names in R13/R14); upstream-PR candidate. |
| R5b DWG extended data order | The DWG writer keeps the place of an application's EED block when it merges a modelled value into it (layer description, annotative flag; the writer encodes the merged record into the retained raw block in its position) instead of dropping the raw block and writing the application after all the others. | A DIMSTYLE with `AcadAnnotative` between two other applications came back with the annotative block last (800 DimensionStyle and 638 Layer ExtendedData rows over the 288 cells). Upstream-PR candidate. |
| R5b layer transparency block | A retained `AcCmTransparency` value that still decodes to the layer's transparency is written as it was (the DXF writer does so since R5a); the DWG writer wrote the explicit opaque value 33554687 over a packed zero. | Every layer of a read file changed its extended data; same rule as the DXF writer. Upstream-PR candidate. |
| R5b header extents | The DWG writer writes the header's `EXTMIN`/`EXTMAX` as the document holds them and computes them from the model space entities only when the header has none (the 1e20 / -1e20 defaults). It recomputed them always, and an entity with an unbounded box (a ray) wrote +-1e10. | AutoCAD keeps the extents until the next zoom; 326 rows (89 / 95 cells). |
| R5b TABLE style types | A row, column, cell or table style that the source stated without data (`data flags` 0) is written with the style type it had (`source_style_type`, `base_style_type`: column 3, row 2, cell 1, table 4); the DWG writer wrote 0. The DXF writer already used them. | ACadSharp's `CellStyle.Type` came back 0 for every column and row override of a modern TABLE. |
| R5b underlay definition reactors | `prepare_database_references` adds the underlay entity to the reactors of its definition on top of the reactors the DWG reader keeps in the document map (a definition read from DWG has an empty own list), instead of replacing them with the underlay alone. | The reactor of the owning dictionary (`ACAD_PDFDEFINITIONS`) was lost on every write of a drawing with a PDF underlay (68 rows). Upstream-PR candidate. |
| R5b referenced bit | The DWG writer writes the "referenced" bit (64 of the DXF group 70) of LAYER, STYLE, LTYPE and APPID records from the source flags; the pre-R2007 records state it, the writer wrote 0. | AutoCAD's R2000/R2004 files carry it (about 300 rows of Layer, TextStyle, LineType and AppId Flags in the baseline: 214 AppId, 95 Layer). Upstream-PR candidate. |
| R5b style dictionaries of a same-version source | `prepare_legacy_document` leaves `ACAD_TABLESTYLE` and `ACAD_MLEADERSTYLE` in the root dictionary when the document was read from a DWG of the very version it is written as. | The profile drops them before R2004 / R2007, but a file of that version that has them is valid for it: sample_AC1015.dwg lost the dictionaries, the style objects and the references of the MULTILEADERs to the style. The drop is upstream's (commit 66af2cc used the same same-version rule for MLEADERSTYLE); it is not what AutoCAD does: AutoCAD's own R14/R2000/R2004 saves of the sample drawing (sample_AC1014/15/18.dwg) keep both dictionaries and their objects, so a down-save from another version still loses them (open, owner decision). |
| R5b viewport grid bits | The VIEWPORT status word carries the grid behaviour bits (18..21) before R2007 too when the document was read from a DWG of the same version (the writer cleared them for every version before R2007). | The R2000 sample states them (status 819232); 98 rows. AutoCAD states them in its R2000 and R2004 down-saves too (sample_AC1015/18.dwg, from the R2018 drawing), so clearing them for any other source is not AutoCAD's behaviour either; the R2007+-only rule is pinned by the R3-VP test `acadsharp_viewports::dwg_version_controls_lighting_grid_and_full_status_projection` (open, owner decision). |
| R5b unused default application ids | The DWG writer leaves out of the APPID table the default applications (`AcadAnnotative`, `AcCmTransparency`, `AcAecLayerStandard`) of a document whose source did not register them and nothing uses (`unused_default_app_ids`, shared with the DXF writer; annotative styles and raw EED blocks count as use). | Every DWG read added up to three application records the file never had; they took handles (a default dictionary of ACadSharp's read moved) and grew the table. |
| R5b handle seed | The DWG writer writes `HANDSEED` as `max(seed of the source file, highest written handle + 1)` for a document read from a file (`source_handle_seed`), as the DXF writer does. | The native seed advances when the reader moves a default application id off the handle of a record of the file; writing it back grew the seed by one with every round trip (a test builds the case). |
| R5b anonymous block markers | The BLOCK marker of an anonymous block is written with the number the file stated (`Block::source_name`, "*U7") while the native name is AutoCAD's numbering on reopening; the table record keeps the bare "*U". A down-saved dynamic block definition the file stated anonymous (`source_flags & 1`, public name in `AcDbDynamicBlockTrueName`) is written as stated: bare "*U" and anonymous bit, its extended data untouched. | ACadSharp names a record after its BLOCK marker, so the markers' numbering decided its names (400 BlockRecord and Block Name rows), and the anonymous flag of the dynamic definition was lost. |
| R5b class instance counts | The DWG writer counts the objects of the fixed classes (LAYOUT, ACDBPLACEHOLDER, DICTIONARYVAR, ACDBDICTIONARYWDFLT, PLOTSETTINGS, SCALE: written under fixed type codes, so the census by class number never saw them) and states that number; a file of the version being written that declares a class live without instances keeps it live. | The R2004+ class records stated 0 instances for LAYOUT and PLACEHOLDER (AutoCAD states the object counts: LAYOUT 4, SCALE 33 in sample_AC1018/32.dwg) and zombie for classes the source declared live. AutoCAD's down-saves declare such classes live too (EXACXREFPANELOBJECT, 0 instances, in sample_AC1018.dwg), so the same-version condition is narrower than AutoCAD; upstream marks them zombie for an AUDIT reason that could not be checked here. |
| R5b legacy TABLESTYLE alignment word | `RowCellStyle` keeps an alignment word of a legacy DWG record outside 1..9 in `source_alignment` (provenance, not equality; the typed `alignment` stays the default MiddleCenter) and the ACadSharp view shows the word. | ACadSharp writes 0 for a style it read from an R2010+ file and reads 0 back; the view showed 5. Same pattern, for the view only: `MText::source_line_spacing_style` (a style word neither 1 nor 2) and `DimensionBase::source_attachment_point` (a word outside 1..9). |
| R5b GEODATA civil trailer | The R2009 GEODATA record (version 1) reads its civil trailer only when the object's main data has a byte or more left (`DwgMergedReader::main_remaining_bits`: up to the string stream of an R2007+ record, up to the handles before); a record that ends after the mesh (ACadSharp writes none) was dropped whole with a truncation error. The ACadSharp view shows `ScaleEstimationMethod` 1 for a DWG record of that layout (ACadSharp's R2009 branch never reads it and keeps the class default `None` = 1). | Genuine reader bug (an optional trailer treated as required); upstream-PR candidate. |
| R5b acad-dump named colors | `acad-dump write-dwg` drops the name of a named (color book) color before R2004 and writes the color index, as ACadSharp does; the library keeps refusing such a write (issue #132, `Named color on entity ... requires DWG R2004 or later`). | 32 round trips (16 samples x R14 / R2000) could not write at all, where ACadSharp writes the index. The refusal is upstream's decision (tests `issue132*`); the tool makes the downgrade explicit instead of changing it. |
| R5b synthesized BLOCK markers | A BLOCK marker the document does not hold (every block of a document read from DXF, or built in memory) is written with the record's full name ("*Paper_Space0", "*D3"); it was written with the bare table-record name ("*Paper_Space", "*D"). | AutoCAD's markers state the full name (ACadSharp's dumps of sample_AC1014..AC1032.dwg: Block "*Paper_Space0", "*D3"). ACadSharp names a record after its marker, so it threw `An item with the same key has already been added. Key: *Paper_Space` on every fork DWG written from a DXF with two or more layouts: 144 of the 288 G5 cells (24 DXF samples x 6 versions; 132 counted as mismatches without rows, 12 R14 cells as `same-failure` passes), whose fork -> C# rows were never compared. Upstream-PR candidate. |
| R5b-2 DXF source version | `CadDocument::dxf_source_version` (set by the DXF reader from `$ACADVER`) and `is_source_version(v)` (DWG or DXF source of version `v`); the DWG writer's same-version rules (VisualStyle and Material objects, R13/14 schema dictionaries, viewport grid bits, style dictionaries, class validity) test `is_source_version` instead of `dwg_source_version` alone. Provenance only (excluded from the native dump and the semantic inventory). | A DXF of version V is a file of V as much as a DWG of V: AutoCAD's DXF and DWG of one drawing state the same objects (21 twins in the corpus). The open owner decision on the same-version rules themselves is untouched. |
| R5b-2 class table of a DXF source | A document read from a DXF that states a CLASSES section writes that list in its order with its own flags and names, inserts LAYOUT and ACDBPLACEHOLDER after ACDBDICTIONARYWDFLT (as AutoCAD's DWG does), appends the classes the written objects need and are missing, and skips MLINE and OLE2FRAME (fixed type codes). For a valid same-version source (DWG or DXF) the stated instance counts and live flags are kept; otherwise the census stays. | The DWG of a DXF source stated a census of its own (3,549 class rows in the baseline of the 21 DXF same-version cells). Upstream-PR candidate for the order and flags. |
| R5b-2 BLOCK / ENDBLK of a DXF source | The BLOCK and ENDBLK entities of a document read from DXF are written from the record's `block_entity_common` / `block_end_common` (colour, layer, linetype, ...), without an entity mode (derived from the owner). | The DWG of a DXF source wrote default marker properties (BLOCK/ENDBLK colour rows). |
| R5b-2 DXF-source current styles and names | For a DXF source `prepare_header` resolves the header's current layer / text style / dimstyle / linetype, `$DIMBLK*` arrow blocks (also `_name` blocks) and the dimension linetypes by name; a constructed document keeps handle precedence. | The DXF states names and the DWG header states handles; the handles of a DXF read were not those of the written tables. |
| R5b-2 material map source | (Superseded in R5b-3 by "R5b-3 DXF material map source": the DXF reader now states the source.) The DWG writer stated map source 1 for the diffuse, specular, reflection, opacity and bump maps when the DXF omitted groups 72/77/171/175/179, and kept the native default (0). | AutoCAD's DWG of the same drawing states 1 (twin dumps). |
| R5b-2 DIMSTYLE DIMPOST | (Superseded in R5b-3 by "R5b-3 DIMSTYLE DIMPOST"; `DimStyle::dwg_dimpost()` is gone.) The DXF record that omits group 3 was written with an empty `DIMPOST` while the model kept the native default "<>". | AutoCAD omits group 3 for an unset DIMPOST and its DWG states an empty string. |
| R5b-2 hatch gradient name | A hatch read from DXF without a gradient is written with the gradient name LINEAR. | AutoCAD's DWG states LINEAR for non-gradient hatches (twin dumps). UNVERIFIED in AutoCAD itself. |
| R5b-2 TABLE covered cells | R2010+ content items that are empty and covered by a one-row merge (`Table::is_merged_away`, `CellContent::is_empty_item`) are not written. | AutoCAD states no content for cells a merge covers; the writer wrote an item per cell. |
| R5b-2 template section and maintenance versions | AC1018 files write the all-zero single-page Template section when the source had one (none for a same-version DWG without it); the maintenance version of the AUXHEADER is 104 (AC1018), 226 (AC1024), 125 (AC1027). | AutoCAD's samples state these values (AC1021 25 and AC1032 4 unchanged). UNVERIFIED in AutoCAD itself. |
| R5b-2 ACIS reference point | Solid3D / Region / Body backed by an AcDs record write the retained reference point instead of the origin. | AutoCAD's DWG states the point of the entity. |
| R5b-2 VisualStyle and TABLESTYLE round trip | The retained R2010+ VisualStyle stores 24- or 28-property styles (PropertyOp group added), `upgrade_r2010_visual_styles` runs for >= AC1027, and the PRE2007 TABLESTYLE carrier is dropped for every modern write; the R2010+ TABLESTYLE keeps bit 8 of its first long: the reader keeps it out of `flags` (it stays in `modern_unknown_long1`) and the writer states it again. | A re-read style differed from the first read (VisualStyle properties, TableStyle flags). AutoCAD states 8 in the first long in every R2010 to R2018 sample. |
| R5b-2 MTEXT source spelling in DWG | The DWG writer writes an MTEXT's text as the source spelled it (`MText::unchanged_source_value`: `\U+00B0` escapes) while the value is still the decoded form of that spelling, as the DXF writer does since R5a; an edited value is written as it is. | AutoCAD keeps the escape in the MTEXT of its own DWG of every version (C# dumps of sample_AC1014..AC1032.dwg: `94\U+00B0`); the writer wrote the decoded character, which the R5b-2 rules G5-RC-014/015 accepted (deleted in the review). Upstream-PR candidate together with the R5a DXF row. |
| R5b-2 viewport-override layer and multi-line attributes | An entity on a layer with a viewport override writes the record handle of the override (`pending_layer_handle`); the DWG writer writes a multi-line attribute's source tag (`source_tag`) and own value like the DXF writer. | The dangling layer handle and the attribute tag/value rows of the same-version DWG cells. |
| R5b-3 DXF material map source | The DXF reader sets map source 1 (image file) on the diffuse, specular, reflection, opacity and bump maps whose groups 72 / 77 / 171 / 175 / 179 are omitted (`read_material`); the refraction and normal maps keep the native 0. The DWG writer's special case for this (R5b-2) is gone. Test `acadsharp_materials::omitted_dxf_material_fields_use_view_defaults_and_omitted_map_sources_are_the_image_file` (renamed in the review from `..._without_changing_native_values`, which no longer described it) pins 1 for the five maps and 0 for the refraction map (it pinned the old default 0 for the diffuse map). | A DXF omits the group for the default, the image file; AutoCAD's DWG of the same drawing states 1 (twin dumps), and the ACadSharp view already reports 1 for an omitted group, so the native model of a DXF read and of its DWG are now the same. Owner confirmation wanted: R5b-2 kept the writer rule because a reader default conflicted with a pinned test; the pinned assertion is that one (the full suite passes with it updated). |
| R5b-3 DIMSTYLE DIMPOST | The DXF reader gives a DIMSTYLE record without group 3 an empty `dimpost` (it was the native default `<>`), and the DXF writer always states group 3 (it omitted `<>`). `DimStyle::dwg_dimpost()` is removed; the DWG writers state `dimpost`. | AutoCAD omits group 3 for an unset suffix (empty), so `<>` is a stated value and must be written to be read back as `<>`; a DWG states the text. The native model of a DXF read and of its DWG are the same. |
| R5b-3 MTEXT default direction in a DWG read | `mtext_from_data` keeps `dwg_x_direction` unless it is exactly (1, 0, 0) on the normal (0, 0, 1) (it was always `Some`; the agent's version dropped (1, 0, 0) on any plane, narrowed in the review). The DXF reader keeps a stated group 11 as it was. | A DXF MTEXT states group 11 for a turned text only; a DWG always states a direction. The writers derive the same direction from `rotation`; the ACadSharp view falls back to cos/sin of the rotation. On another plane the default axis of a DXF without group 11 is the plane's own X axis, so a stated (1, 0, 0) is kept there and the DXF writer states it (test `an_mtext_on_another_plane_keeps_its_stated_x_axis`). |
| R5b-3 $CMLSTYLE name from a DWG | The header name of the current multiline style is the key of the style in its owner dictionary (`ACAD_MLINESTYLE`); the style's own name is the fallback (it was always the style's name). | `$CMLSTYLE` names the style by its dictionary key; AutoCAD's DXF keys it `Standard` and names the style `STANDARD`. The ACadSharp view is unchanged (it states `Standard` for a DWG). The R14 sample keys it `STANDARD`, which G4-494 still maps. |
| R5b-3 R14 VX table of a DXF source | `prepare_legacy_document` gives the VX table a handle (the header's `vpent_hdr_control_handle`, or a new one) when it makes records for a document that has none, and makes one record per viewport for a document without VX records; a drawing with its own VX table (an R14 DWG) keeps its records and gains one only for a viewport of the active paper space that lacks one (review; the agent's version never added one, so a viewport added to an R14 drawing's paper space had none). | A DXF has no VX table: the records were written without the control object (the control is written for a table with a handle), so the next read had no `vx_control_entries`, and a second write numbered the viewports differently (idem failed at AC1014, 10 cells). AutoCAD's R14 file holds records for the viewports of the active paper space only (sample_AC1014.dwg: records for viewports 615 and 619 of `*PAPER_SPACE` 259, none for the four of the other layouts' blocks 58 and 5D), so the writer's record per viewport changed sample_AC1014.dwg (4 records, handle seed +4, viewport ids). Test `a_partial_vx_table_of_an_r14_drawing_is_filled_for_the_active_paper_space_only`. |
| R5b-3 EED block order | `write_extended_data_with` writes the blocks in the order of the modeled records and takes the verbatim block of an application where it has one; a verbatim block no record restates comes first. | The reader turns the ACAD `DesignCenter Data` block of a block record into `units` and drops its verbatim copy, so the block, written again from the field, went after the others (R14 `BlockRecord.ExtendedData` order differed from the source). |
| R5b-3 R14 dictionary byte | The R14 DICTIONARY states `hard_owner` in the byte ACadSharp calls unknown and writes as 0 (`write_dictionary`). | AutoCAD's own R14 file states 1 in 179 dictionaries (the hard-owner flag of R2000+, which the reader reads from that byte); sample_AC1014.dwg now round-trips natively exact. |
| R5b-3 block scalability of an unstated record | **Reverted in the review.** The agent's writer stated byte 1 (uniform scaling only) for a block record read from a DWG before R2007 or from a DXF without group 281. The writer again states `scale_uniformly` (0 for such a record); test `a_block_that_never_stated_its_scaling_keeps_any_scaling_in_a_later_version` (fails with the agent's version). | The claimed evidence was wrong: AutoCAD states 0 for 23 of the 27 block records of its own sample_AC1024.dwg and sample_AC1032.dwg (1 for 4; DXF group 281 likewise), so the change made the sample_V comparison worse (22 CanScale rows per AC1018 -> AC1024+ cell instead of 4), and the generated no-loss rule G5-RC-S085 (ACadSharp's default CanScale true for an R2004 source) accepted the new rows. |
| R5b-4 down-saves keep the style, material and visual-style objects | `is_writable_object` writes `Material` and `VisualStyle` at every version (the class decides), `prepare_legacy_document` drops ACAD_TABLESTYLE / ACAD_MLEADERSTYLE only when their class is missing (it dropped them before R2004 / R2007 for a source of another version), and `write_dictionary` keeps ACAD_LAYOUT, ACAD_PLOTSETTINGS, ACAD_PLOTSTYLENAME, ACAD_MATERIAL, ACAD_COLOR and ACAD_VISUALSTYLE entries in an R13/R14 file whatever the source. | Owner decision of 2026-10-23: AutoCAD's own R14, R2000 and R2004 files hold them (sample_AC1014.dwg, sample_AC1015.dwg, sample_AC1018.dwg: 11 materials, 24 visual styles, 4 layouts, the two style dictionaries). The "dropped" sample_V objects went from 1,905 to 0. |
| R5b-4 class table of a DWG source | A DWG source written at another version below R2013 keeps its class table whole, in its order and numbering (`prepare_dwg_source_classes`; it was shrunk to the legacy list and renumbered); only the version profile changes it: LWPOLYLINE, HATCH, XRECORD (R14 only) and DATATABLE (up to R2004) are added when missing and dropped when the version does not declare them (a later target as well: `has_stale_profile_classes`). A class of such a source is no zombie unless its source said so. | AutoCAD's six files of one drawing hold the same 50 classes 500..549 in every version, plus 550..553 at R14 and 550 at R2000 / R2004 (the C# dumps). 33 class-number rows per sample_V cell are gone. |
| R5b-4 viewport grid bits from R2000 | The status word of a VIEWPORT states the grid behaviour bits (18..21) from R2000 on (R2007+ only before). | AutoCAD's R2000 and R2004 files state them (sample_AC1015.dwg, sample_AC1018.dwg: 0xC8020 as sample_AC1032.dwg). Test `acadsharp_viewports::dwg_version_controls_lighting_grid_and_full_status_projection` follows (the lighting and the major grid frequency stay R2007+). |
| R5b-4 VPORT grid flags of an older source | The writer states 3 (not the 2 the reader leaves) in the R2007+ VPORT grid flags of a record read from a DWG before R2007. | AutoCAD's R2010 file of a drawing converted from R2004 states 3 (sample_AC1018.dwg to sample_AC1024.dwg). |
| R5b-4 material channel flags | The DWG writer derives the R2007+ channel flags of a material read from a DWG before R2007 whose flags are the unstated default (127) from its maps (one bit per diffuse / specular / reflection / opacity / bump map that is the image file, 63 when all five are). | AutoCAD's R2010 file states 63, 17, 1, 0 for its 11 materials (sample_AC1024.dwg); its R2004 file states none. |
| R5b-4 visual style round-trip record | `store_visual_style_roundtrip` runs for every version before R2010 (R2004..R2009 before), and `prepare_roundtrip_records` keeps a style's record at R2010 (it dropped it at R2010 and later). | AutoCAD's R14 to R2010 files hold an ACAD_XREC_ROUNDTRIP record for each of the 24 styles (252 entries before R2010, 126 in R2010); R2013 has none. An R2010 record could also carry the properties after the 28th: not done (a re-read R2010 style stays at 28 properties). |
| R5b-4 TABLE cells between the layouts | A cell of the R2010 layout written before R2010 (`TableCell::legacy_layout`) states its content format overrides (text height, content colour, text style, alignment), background colour and border colour / line weight overrides in the legacy override word and edge flags. A legacy cell written as R2010 (`binary_layout_with`) keeps the style's text height where the legacy text height equals a row height of the table (the legacy style's `row_heights`), states a text item's text as its formatted value, and drops the empty text of a cell a one-row merge covers (`CellContent::is_empty_text`). | AutoCAD's own sample_AC1018.dwg / sample_AC1024.dwg of the two tables of the drawing. TABLE rows in the sample_V cells: 2,316 to 975. |
| R5b-4 SPLINE knot parameterization | The R2013+ spline record of a source read from a DWG before R2013 states 15 (custom) for a spline without fit points whose parameterization is the 0 the older reader leaves. | AutoCAD's R2013 file states 15 for the control-point spline of the drawing (#433 in sample_AC1027.dwg and sample_AC1032.dwg; corrected in the review: #434 is a fit-point spline there, 2, and 15 with control points in sample_AC1024.dwg), and ACadSharp reads every pre-R2013 control-point record as 15. |
| R5b-4 R14 header, dimension style and viewport data of a newer drawing | The R14 header states OSMODE 4133 (AutoCAD's default) when the source is an R2000+ DWG (the reader leaves 0); the R14 DIMSTYLE states DIMFIT from DIMATFIT for such a source; the MVIEW xdata of an R14 viewport states fast zoom 1 and the UCS icon from the status word. | sample_AC1014.dwg from sample_AC1015.dwg / sample_AC1032.dwg: DimensionFit 3, fast zoom 1 and icon words 1 / 3. OSMODE (corrected in the review): AutoCAD's own R14 file states 39, its session value; no R2000+ file (DWG or DXF) states OSMODE, so 4133 is AutoCAD's initial value and ACadSharp's default (the 4133 it reads from the R2000+ files), not a value of the drawing. DIMFIT ignores DIMTMOVE (the samples state DIMTMOVE 0; R14 has no DIMTMOVE). UNVERIFIED in AutoCAD itself. |
| R5b-4 review: deterministic class table | `prepare_legacy_classes`, `required_class_names` and `add_missing_object_classes` take the objects in handle order (`objects_in_handle_order`). | They took them in the object map's iteration order, which changes with every map and every process: the classes appended for a DXF source written at another version below R2013 came in a different order on each run (88 of the 288 G5 cells wrote different bytes on repeated runs, e.g. dynamic-blocks/BLOCKFLIPPARAMETER.dxf at AC1018: 32,844 or 32,876 bytes). Test `r5b4_write_determinism`. Upstream candidate (HashMap order in the writer). |
| R5b-5 AutoCAD's round-trip records (`RTTcAl`, `RTLwPs`, `RTMaterial`) | `io/dwg/autocad_roundtrip.rs`: an R14, R2000 or R2004 DWG (and a DXF of R2000 or R14) read by the fork has the true colour (before R2004), line weight (R14) and material (before R2007) of an entity restored from the leading records of its `ACAD` extended data, which are then removed (`Restorer`, DWG builder and `restore_document` of the DXF reader); the writers state them again for the versions that cannot store them (`DwgObjectWriter::round_trip_xdata`, `prepare_for_dxf`). A named colour is stated in the `RTTcAl` identity and no longer refused before R2004 when the entity has a colour value; the pre-R2004 `BookColor` refusal stays for a name without a colour. | AutoCAD's own down-saves keep exactly these values this way (sample_AC1014.dwg: 75 `RTLwPs`, 36 `RTTcAl`; sample_AC1015.dwg, sample_AC1018.dwg: `RTMaterial`); before, the fork wrote the ACI index and lost the value. Entities of a class (type code 500 and up: the PDF underlay) get none, as in AutoCAD's files. Compat: the ACadSharp view states the nearest index, no line weight (R14) or material and the records (`EntityCommon` view in `compat/acadsharp/entities.rs`). Native fields `dwg_transparency_dropped` (the transparency flag of the colour word, whose value AutoCAD does not keep) are provenance. DXF (corrected in the review): AutoCAD's own R2000 and R2004 DXF of the drawing hold no `RT*` record (sample_AC1015_ascii.dxf: the point #29A states colour 230 and nothing else; sample_AC1018_ascii.dxf has no `RTMaterial`), so `prepare_for_dxf` keeps in the DXF what AutoCAD's DXF drops (the fork's DXF of a DWG source of these versions carried the records as plain extended data before this job too); UNVERIFIED that AutoCAD restores the records from a DXF. **Superseded in R5b-7:** the DXF writer states none of them (owner decision), see "R5b-7 DXF down-saves state no `RT*` record". R13 (AC1012) gets the records and blocks as R14 does: no AutoCAD file of R13 is in the samples. A colour-book record (word 0x4000) restores the RGB value, the name and the colour-book object of that name (review: before, the object was never found, see the next row). Upstream-PR candidate (data loss). |
| R5b-5 review: colour-book objects of a DWG before R2004 | `CadDocument::resolve_book_colors` skips a colour-book object without names (the DBCOLOR record before R2004 holds an index only; its names are its dictionary key); `autocad_roundtrip::Restorer` matches an `RTTcAl` identity against that key. | The resolution gave such an entity the object's index and dropped its name, so the restore could not name the object: an R14 / R2000 read had the colour and the name but no colour-book handle where the R2004 read of the drawing has it (circle #99F and DBCOLOR #996 "RAL_CLASSIC$RAL_1006" in sample_AC1014.dwg, "RAL CLASSIC$RAL 1006" in sample_AC1015.dwg). The compat view states no colour-book handle for such an entity (ACadSharp reads none from an R14 / R2000 record). |
| R5b-5 MTEXT blocks before R2004 | `MTEXTBEGIN 73 <style> 44 <factor> 74 0 MTEXTEND` for every MTEXT and `ACAD_MTEXT_BBRT 91 <flags> 46 <scale> 64 <colour> 65 "" 66 "" 442 <transparency> ACAD_MTEXT_BERT` for one with a background fill are stated by the DWG and DXF writers below R2004 and taken out of the extended data on read (`recover_mtext_bg_roundtrip(.., strip)`); R14 takes the line spacing from them. `MText::dwg_without_spacing_block` (provenance) keeps that a source stated no `MTEXTBEGIN`. | AutoCAD's R14 and R2000 files (sample_AC1014.dwg: 33 of 36 MTEXT; sample_AC1015.dwg: 36 of 36; corrected in the review: the three without the block, #1229, #1233 and #123E, are the texts of the dimension blocks *D4, *D5 and *D8 AutoCAD regenerated for R14, handles no other file of the drawing has; every MTEXT AutoCAD carries over gets the block, as the fork's writer states it); a fill was lost before in a down-save. Reader and DXF writer used to leave the blocks in the data. |
| R5b-5 TABLE of a legacy DWG written as R2010+ | `Table::with_roundtrip_content`: a table read from a DWG before R2010 whose extension dictionary links the TABLECONTENT / TABLEGEOMETRY AutoCAD keeps (`ACAD_ROUNDTRIP_2008_TABLE_ENTITY`) takes the typed cells, cell styles, row styles and geometry from them when writing R2010+, with the legacy text as display, the value format and unit of the content, AutoCAD's value flags (6 / 4 / 5 / 7) and a date as the SYSTEMTIME of its day; a plain text or number cell edited since the save makes the table keep its converted legacy cells (review: a typed or formatted cell, date, angle, currency, percentage, point, is not checked, so an edit of such a cell in memory is replaced by the round-trip value: open; the date is the day nearest to its UTC seconds, which shifts a date stored with a time of day after noon to the next day). `table_roundtrip` stops at the first pre-R2007 marker (group 102), which took its handles for the content before. | sample_AC1018.dwg to sample_AC1024.dwg: date, angle, currency, block cells and border override flags (63 / 37) come from the round-trip object; AutoCAD's up-conversion does not derive them from the legacy cells. The native model of the legacy table is unchanged (the compat view keeps the legacy cells). |
| R5b-5 TABLE written before R2010 | `TableCell::legacy_layout_in`: a cell without a text height of its own takes that of its row style (title 0.1425, other rows 0.1026 in the sample) and a last-column cell takes its right edge (flag 2, line weight, visibility) from the inside horizontal border; a block cell states the content format scale as its block scale. | sample_AC1015.dwg (edge flags 11 against 9, text heights, BlockScale 0.00596) from sample_AC1032.dwg. Review: fitted to one table (#528: 11 cells for the text heights, the 2 last-column cells of rows 4 and 5 for the edge); the other table (#A35) exercises neither; UNVERIFIED as AutoCAD's rule. |
| R5b-5 LEADER extents before R2010 | The R14 to R2007 record states the extents of the MTEXT annotation as its box height and width when the leader (read from R2010+) holds the defaults. | sample_AC1015.dwg: 1.8448 and 16.374 are the extents of MTEXT #513, which sample_AC1032.dwg states in the MTEXT itself. |
| R5b-5 MLEADERSTYLE attachments | A style read from a DWG before R2010 whose attachments are the defaults (top of top line, bottom of bottom line) states 9 and 9 (center of text) in the R2010+ record. | AutoCAD's R2010 and later files of the drawing (sample_AC1024.dwg ...) state 9 / 9, as 69 of the 70 R2010+ MLEADERSTYLEs of the sample dumps do (AutoCAD's default); the samples hold no up-save of a pre-R2010 file by AutoCAD itself (corrected in the review: UNVERIFIED as AutoCAD's conversion). |
| R5b-5 DXF writer before R2015 | Group 370 (line weight) is not written below AC1015 (R14 DXF has none; `RTLwPs` carries it). `named colours` without a colour value are still refused below R2004. | R14 DXF; see the round-trip records. |
| R5b-5 acad-dump `write-dwg` | Below R2004 the tool drops the colour-book name and handle only of an entity without a colour value (no RGB colour, no colour-book object with a colour), which the library refuses; a named colour with a value goes into `RTTcAl` (review: the agent had removed the drop, so such a document could no longer be written below R2004). | The library no longer refuses a named colour that has a colour value. |
| R5b-6 version-gated records of AutoCAD's other conversions | `io/dwg/autocad_conversions.rs`: a DWG or DXF read below the version that holds a value natively has it restored from the record AutoCAD keeps it in, and the record taken out of the model (`CadDocument::consumed_xdata` keeps it with its position): the attribute lock (`AcDbAttr 0 1`, before R2007: `lock_position`), DIMTFILL / DIMTFILLCLR (`ACAD_DSTYLE_DIMTEXT_FILL`) and the line types (`ACAD_DSTYLE_DIM_LINETYPE`, `..._EXT1_...`, `..._EXT2_...`) of a dimension style (before R2007), the revision GUID of 3DSOLID / REGION / BODY (`ACAD_OBJECT_CHANGE_GUID`, before R2013), the multileader class version (`ACAD_MLEADERVER`, before R2010); a visual style whose record is the stamp `AcDbSavedByObjectVersion 0` and a colour-book object whose record is the `DBColXD` block alone lose that record the same way (before R2018). The DWG and DXF writers (`prepare_for_write`) state the record from the native value for a version that lacks the field, at the position the file had it; a version that holds the value states none of its own, and a record the model still holds there stays (review: it was not taken in by the reader, because a file of that version or a document made in memory states it; the corpus has `ACAD_DSTYLE_DIMTEXT_FILL` in an R2007 file, `DBColXD` in an R2018 one; the agent's writer dropped it); a record of another shape than AutoCAD's is neither rewritten nor dropped (review); a text fill that is off states the record when its colour is not the default or the file had it (review: autodesk architectural_-_annotation_scaling_and_multileaders.dwg states fill 0 with ByLayer; the agent's writer lost the colour); the stamps (multileader version, visual style stamp, colour block) are stated for a conversion or where the file had them, never for a drawing made in memory, and R2018 drops the visual style stamp of a drawing of an older version only; a DXF states no visual style stamp (**superseded in R5b-7**: AutoCAD's DXF files of R2000 to R2013 hold it) and, at R2000, no colour block of its own accord. `DesignCenter Data` (block units) is no longer written into R2007+ for a drawing read before R2007, whose reader took the units in (review: the record of an R2007+ file stays, cadviewer City_base_map.dwg (R2013) has two). A 3DSOLID / REGION / BODY whose revision is restored takes its GUID as `uid`, as the reader of an R2013+ DWG states it (review: the agent left `uid` empty, 162 RR rows). | The records of sample_AC1014.dwg to sample_AC1032.dwg: the same drawing saved at six versions states exactly these records up to the version that holds the value and AutoCAD's later files do not (AcDbAttr: 5 ATTDEF and 4 ATTRIB up to R2004, native `IsLocked` from R2007; DIMTEXT_FILL / DIM_LINETYPE: styles #994 and #C88 up to R2004; OBJECT_CHANGE_GUID: #D65, #D69, #D6A up to R2010, none in R2013 and R2018 which hold the GUID in the modeler data; MLEADERVER: 15 multileaders and 2 styles up to R2007; the stamp: 24 visual styles up to R2013; DBColXD: up to R2013); their DXF twins hold the same records from R2000 (not the visual style stamp, not DBColXD at R2000). Before, an up-save wrote the stale records into the new file (and the native value stayed at its default: a locked attribute was unlocked, the text fill of a style lost) and a down-save lost the values. Compat: the ACadSharp view puts the taken-out records back where the file had them (`consumed_xdata`), shows `IsLocked` false, no `TextBackgroundFillMode` and no dimension line types for a source before R2007 (ACadSharp reads none of them). UNVERIFIED: AutoCAD reading the fork's down-saves; the record shapes other than the sample's (a record of another shape stays as it was). |
| R5b-6 binary modeler data of a drawing written to an older release | `entities/acis/reversion.rs`, `AcisData::sab_for_version`: AutoCAD's binary ACIS / ASM data (save versions 20800, 21200, 21500, 21800, 22300 = R2004, R2007, R2010, R2013, R2018) written for an older drawing version takes that release's save version: the magic (`ASM BinaryFile4` only at 22300), the version and fourth header word (0, 26, 24, 12, 4), one integer less per `vertex` record (from R2007) and per `coedge` record (from R2013), `End of ACIS data` for `ASM`. Only a body whose every record has a kind and shape of the sample's three bodies is converted (`KNOWN`: asmheader, body, lump, shell, face, loop, coedge, edge, vertex, point, transform, plane-surface, straight-curve, persubent-acadSolidHistory-attrib, the end record; review: the agent's converter copied every other record verbatim, so a body with a cone, spline or torus surface, an elliptic or interpolated curve or a tolerant vertex would have been stated as an older save version with a newer layout); any other body, other save versions, text SAT data and every conversion to a newer release stay as they are. | The three bodies of the sample (3DSOLID #D65, #D6A, REGION #D69) in sample_AC1018.dwg to sample_AC1032.dwg are byte for byte the same apart from these items and the date of the `asmheader`; the fork's down-saves of sample_AC1032.dwg / sample_AC1027.dwg / sample_AC1024.dwg to R2004, R2010 and R2013 (R2007 is no target of the gate) now equal AutoCAD's own data of those versions in all three bodies (the date aside). The fork wrote ASM data into R2004 to R2013 files before, which those releases cannot read. Open: SAT 400 (R2000) and SAT 106 (R14) differ in every record layout (body, shell, face: fewer fields, other enum spellings), a modeler-level conversion that is not mechanical. |
| R5b-6 hatch gradient of an R2000 drawing | `recover_roundtrip_gradients` takes the two colours of a recovered gradient from the `ACAD_XREC_ROUNDTRIP` record when it states them (24-byte blocks, true colours) instead of the nearest indices of the `GradientColor*ACI` EED; the DWG writer states the default name `LINEAR` for a gradient that is off when the source is a DXF or a DWG before R2004. | sample_AC1015.dwg: the record states 0x0000FF and 0xFFFF00 for the hatches #371 and #376, AutoCAD's R2004 file states these true colours and `LINEAR` for all 8 hatches, also those whose gradient is off (its R2000 file has the EED colours only). |
| R5b-6 colour-book object before R2004 | The model keeps the index the DBCOLOR record states (as ACadSharp reads it); a writer of R2004 or later states the colour and the names of the `DBColXD` block in the object (`restore_book_colors`, before `prepare_book_colors`, which no longer adds a second object for it), while the block still names the object's index (review: an index changed after the read neither takes the old colour back nor writes the stale block again). | sample_AC1015.dwg DBCOLOR #996 holds index 42, AutoCAD's R2004 file the true colour (226, 144, 0) and the names of `DBColXD`; the DXF of a DWG source keeps writing the index (the restore at read would have changed the DXF: G4-RC). |
| R5b-6 TABLE typed cell edited after the save | `Table::with_roundtrip_content`: the value of a cell of the content object is trusted only while the checksum AutoCAD stores with the cell (the custom data item `ACAD_ROUNDTRIP_2008_CELL_CHECKSUM`, a double: the sum of every UTF-16 unit of the displayed text times its 1-based position) still equals that of the legacy cell's text, for every value cell that has the item (review: the agent took the first double item of any name and checked typed values only, so a plain number edited into a text that does not parse, "60" -> "n/a", lost the edit); a cell without the item keeps the older checks; else the table keeps its converted legacy cells (the edit survives). | sample_AC1018.dwg: "Table sample" 7783, "Text" 1110, "0.500000" 1739, "Hello this is a title" 21385; the review checked all 19 cells with text of the two tables (#528 / #747, #A35 / #F7A: texts, the upper-case display "CELL FORMAT 1" of "cell format 1", numbers, a point, a date, a percentage, a currency): 19 of 19 exact. Non-ASCII text: UTF-16 units assumed, UNVERIFIED (no sample). Before, such a cell was never checked and an edit was replaced by the saved value. |
| R5b-6 DXF writer R12 | The native R12 path states the records of the version-gated families (`AcDbAttr` of a locked attribute) as the other paths do. | sample_AC1009_ascii.dxf holds `AcDbAttr 0 1` on all 5 ATTDEF and 4 ATTRIB; without it the record taken out at read was lost in an R12 round trip (G4). |
| R5b-6 review: repair of an object's handle | `CadDocument::repair` moves the records a reader took out of an object (`consumed_xdata`) to the object's new handle, with its other side data. | Else they stayed under the old handle: the object lost them (a same-version write no longer stated them) and an object given that handle later took them. |
| R5b-7 DXF down-saves state no `RT*` record | `autocad_roundtrip::prepare_for_dxf` no longer adds `RTTcAl`, `RTLwPs` and `RTMaterial` to the entities (nor to the BLOCK and ENDBLK markers) of a DXF: it states the MTEXT blocks only. A DXF of R14 or R2000 carries the nearest index for a true colour, a DXF of R14 no line weight, a DXF before R2007 no material and no colour-book name; the DXF reader still restores the values from a file that has the records (`restore_document`). Supersedes the DXF paragraph of "R5b-5 AutoCAD's round-trip records". | Owner decision of 2026-10-24: AutoCAD's own R2000 and R2004 DXF files hold none (sample_AC1015_ascii.dxf and sample_AC1018_ascii.dxf against sample_AC1015.dwg and sample_AC1018.dwg: 41 and 5 objects with a record in the DWG, none in the DXF twin, and 37 of 39 and 5 of 5 of those objects that the twin has under the same handle hold no extended data at all, the other two are MTEXT with their `MTEXTBEGIN`; the point #29A states colour 230 and nothing else). R14: no AutoCAD DXF of R14 in the samples, extended like R2000 (UNVERIFIED). A DWG down-save keeps the records. G4: the RC and RR rows this causes for the DWG sources of R14, R2000 and R2004 are accepted by G4-522 / G4-523 and RR-049..054 (CadKit allow-lists). |
| R5b-7 visual style stamp in a DXF | `autocad_conversions::prepare_for_write` states `ACAD {AcDbSavedByObjectVersion 0}` in the visual styles of a DXF of R2000 to R2013 written for a drawing of another version, as a DWG does (a DXF had none for a conversion). | The DXF twins of the samples state it in each of the 24 visual styles: sample_AC1015_ascii.dxf, sample_AC1018_ascii.dxf, sample_AC1021_ascii.dxf, sample_AC1024_ascii.dxf, sample_AC1027_ascii.dxf (24 of 24), sample_AC1032_ascii.dxf none. The R5b-6 comment "AutoCAD's DXF files hold no stamp at all" was wrong; its test assertion changed (see the changed tests). |
| R5b-7 dimension style records in an R12 DXF | `prepare_for_write` states neither `ACAD_DSTYLE_DIMTEXT_FILL` nor the `ACAD_DSTYLE_DIM_*LINETYPE` records for a DXF before R13. | AutoCAD's R12 DXF (sample_AC1009_ascii.dxf) holds extended data on entities only: 87 records, none on a table record. The other families the fork writes at R12 (`AcDbAttr`, 9 of 9) are the ones that file holds. |
| R5b-7 MTEXT columns before R2018 | `autocad_roundtrip::mtext_blocks` / `column_block`: an MTEXT with one dynamic column of a height of its own (`column_type` 2, not automatic height, exactly one height) states `ACAD_MTEXT_COLUMN_INFO_BEGIN 75 <type> 79 0 76 <n> 78 <flow reversed> 48 <width> 49 <gutter> 50 <n> <heights> ACAD_MTEXT_COLUMN_INFO_END` in the `ACAD` data of every DWG and DXF written before R2018 (R2018 keeps the columns in the MTEXT record), before the `MTEXTBEGIN` block of R14 and R2000; a height that is not positive is stated as 0. `with_mtext_blocks` replaces a column block only when the MTEXT states columns of its own (a record a source stated stays). `prepare_for_dxf` runs below R2018 for it. Two or more columns, static columns and automatic heights state no record (review of R5b-7). | sample_AC1015.dwg to sample_AC1027.dwg (and the DXF twins: 4 records each) state the 4 MTEXT that sample_AC1032.dwg holds as column type 2 with the record `75 2, 79 0, 76 1, 78 0, 48 15.74.., 49 110.22.., 50 1, 0` where R2018 states the heights -2.94, -23.73, -3, -204.35 (the height of the text AutoCAD laid out: not derivable, so the up direction stays open); corpus records (aspose ZD, et2, images, gdal, america map, cadviewer PWR07_2 and ma1000_133: 91) are all `75 2 79 0 76 1 50 1`; ezdxf columns_R2007.dxf (an R2007 file saved by "manfred") also has 4 static records (`75 1 76 3`), 2 with automatic height (`79 1`) and 5 more of two to four columns, and each of those 11 is followed by `ACAD_MTEXT_COLUMNS_BEGIN 47 <n> <handles> ACAD_MTEXT_COLUMNS_END` naming the MTEXT entities of the other columns (and `ACAD_MTEXT_DEFINED_HEIGHT` where the MTEXT has a defined height), which a drawing of R2018 does not have: the writer states only the one-column form; a positive height is stated as it is (columns_R2007.dxf: 23.85, 52.56). Cell sample_AC1032.dwg -> AC1027 of the sample_V gate passes with it. |
| R5b-7 block description and preview before R2000 | `autocad_conversions::restore_document` (DWG and DXF readers): a drawing of R13 or R14 has the description (`CONTENTBLOCKDESCRIPTION 1071 0 1000 <text>`) and the preview image (`CONTENTBLOCKICON 1071 0` and the bytes as `1004` chunks of 127) of a block record taken from the records of its extended data into `BlockRecord::description` and `preview_data`; the records are taken out (`consumed_xdata`). `prepare_for_write` states them again, at the position the file had them, for a block record whose records were taken and whose value is not empty (DWG and DXF of R13 and R14). The ACadSharp view hides the values of such a document (`Comments`, `Preview` null: ACadSharp reads the records as extended data). Only AutoCAD's form is taken in (a non-empty text; the image in chunks of exactly 127 bytes, so it is written back byte for byte), and only into an empty value (a DXF of another producer that also has group 4 or 310 keeps that value and the record as data); the view hides a value exactly when it came from such a record (DWG and DXF of R14; review of R5b-7). | sample_AC1014.dwg (blocks #A7E and #77F): `CONTENTBLOCKICON` is byte for byte the `Preview` of sample_AC1015.dwg and sample_AC1018.dwg (4,136 bytes = 32 chunks of 127 and one of 72, both blocks) and `CONTENTBLOCKDESCRIPTION` the `Comments` of the block #A8E ("My dynamic block description."); AutoCAD's R14 file states them after the records the block had. The 7 `block` rows of the AC1014 -> newer cells are gone. Open: a block of a newer drawing written as R14 gets no record: AutoCAD's R14 file states them after `DesignCenter Data`, which the writer adds last, under application handles of its own numbering (the fork's new applications take handles AutoCAD's file gives to a layer: unacceptable `<type>` rows). |
| R5b-7 acad-dump `write-dxf --version` | `acad-dump write-dxf <in> <out> [--binary] [--version ACxxxx]` writes the document as a DXF of that version (as `write-dwg --version`). | Tooling for the per-record DXF table of R5b-7; G4 does not pass it. |
|  R5b-8 classes stated by a DXF stay live | `dwg_writer::reconciled_classes` (`declared_live_unused`): a class a DXF source states is kept live at every version whatever its instance count (it was dropped as unused when the count was 0); `DwgWriter` (class preparation below R2018): a DXF source with stated classes at another version uses `prepare_dxf_source_classes` then `prepare_dwg_source_classes` (the list whole + the version profile) instead of `prepare_legacy_classes`. | AutoCAD's DXF of a drawing lists the classes of the drawing; the DWG AutoCAD saves from it lists the same ones (sample_AC1032 DXF/DWG twins). The legacy profile dropped about 20 classes that AutoCAD keeps. Effect: The class table of a DXF down-save is longer.  |
|  R5b-8 visual style record at R2010 | `stub_objects::store_visual_style_roundtrip` also stores at R2010 (a 28-property style of an older source, no `RTVSPropertyOp0..62` group, an existing consistent record kept, no empty xrecord); `restore_post_2010_properties(entries, require_values)`: the reader extends a 28-property style to 62 only when the record states `RTVSPost2010Prop` values. | AutoCAD's own R2010 file (sample_AC1024.dwg): 126 entries, `RTVSPost2010Prop28..`, no `RTVSPropertyOp` group. Effect: A visual style read from an R2010 DWG written by the fork keeps its post-28 properties.  |
|  R5b-8 block description and preview at R14 for newer drawings | `autocad_conversions::prepare_for_write`: a block of a drawing with a description or preview gets the R14 records `CONTENTBLOCKDESCRIPTION` / `CONTENTBLOCKICON` also when the source did not state them (the `at[i].is_some()` requirement is gone). | AutoCAD's R14 DWG of a newer drawing (sample_AC1014.dwg). Effect: An R14 down-save carries the two records; R2000 and later none.  |
|  R5b-8 MTEXT columns before R2018 are native | `dwg_reader::recover_mtext_columns` (DWG read paths and `dxf/reader.rs`, before `autocad_roundtrip::restore_document`): the exact one-column `ACAD_MTEXT_COLUMN_INFO` block of an MTEXT of a file below R2018 becomes `column_data` (height 0 stays 0) and leaves the extended data; `parse_column_block`, `take_column_block`, `drop_stated_column_block`, `states_columns_in_block`; DXF writer (`section_writer.rs`): the standard group-75 column layout is not written when the block states the columns (as AutoCAD's DXF). | The block is AutoCAD's way of stating the columns below R2018 (sample_AC1015.dwg, sample_AC1024.dwg and their DXFs); the native model held them as extended data before and lost them in a cross-version read. Effect: The `ACAD` record of an MTEXT read below R2018 no longer holds the column block; the writers state it again. Views: `compat/acadsharp/mtext.rs` and `entities.rs` (`mtext_common`) project the columns and the block as ACadSharp reads them.  |
|  R5b-8 TABLE from TABLECONTENT / TABLEGEOMETRY below R2010 | `entities/table.rs` `restore_dwg_table_roundtrip` (both DWG paths and the DXF reader; `legacy_source`, `as_r2010_record`): a table of a DWG / DXF of R2007 or older is completed from its `TABLECONTENT` / `TABLEGEOMETRY` objects in the record form AutoCAD reads at R2010 (value flags 0, override false, cell edge flags 0, block scale 1.0, covered cells empty, unset values flags 7); `unrestored_legacy` and `write_table` (`object_writer/entities.rs`) write the original legacy cells while the table is unchanged; `write_acad_table` (DXF writer) patches the table words and the cell scalars from the legacy view; `TableLegacyView` gained `value_flags`, `override_flag`, `legacy_style_override` (compat `table.rs` reads it without the `source.dxf` filter). | Down-save then up-read must give the table AutoCAD gives: sample_AC1024.dwg against sample_AC1015.dwg (R2000 flat model) and the R2010 content model. Effect: A table read from an old file has the content model; writing it below R2010 re-states the old cells.  |

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
Direct document inventories gather explicit handles from tables, block records, entities and their children, and
non-graphical objects, then sort numerically. Read dumps separately apply source registration and allocate the
reader's collection defaults. DXF BLOCK/ENDBLK markers folded into block records retain their common state and
are projected through the same entity mapper as explicit DWG markers.
The normal build does not include this module or its optional JSON dependency.

R12 DXF polyline projection intentionally follows ACadSharp's legacy reader: native
flag-based types remain intact, while the view exposes Polyline2D/Vertex2D, including
polyface face records. Child appearance follows ACadSharp's MatchProperties callback;
identity, reactors, XData and extension dictionaries remain the child's. DWG/modern DXF
retain subtype-specific views. Seqends are emitted only from retained records, never
invented from guessed handles. These oracle quirks are not native reader fixes.

The registration view reproduces modern DXF registering the default
Seqend when the first child is added, before the parsed terminator replaces the
collection reference (`SeqendCollection.Add`/`Seqend`, `CadPolyLineTemplate.build`,
`CadDocument.RegisterCollection`). The registry then exposes a constructor-default
terminator allocated from HANDSEED, rather than the source handle/common data.
The projection uses document-wide registration order and preserves the native
source record. Legacy DXF
INSERT terminators without an owner are registered as block entities by
`DxfEntitiesSectionReader`, another difference from native compound ownership.

## R3-DX remaining reference differences

The 42ffa8f baseline has 73 extra objects and 17 dictionaries with different
entry lists. Investigation against the committed dumps and the local ACadSharp
reader diagnostics establishes the following; no fixture-specific filter is used.

* `sample_AC1015_ascii.dxf`, `sample_AC1015_binary.dxf`,
  `sample_AC1018_ascii.dxf`, and `sample_AC1018_binary.dxf` each retain 18 extra
  objects: eight dictionaries, eight XRecords and two BlockRepresentationData.
  Their source owner/key paths are identical:
  * `#758 / AcDbContextDataManager -> #759 / ACDB_ANNOTATIONSCALES -> #75A`;
    `#75E / AcDbContextDataManager -> #75F / ACDB_ANNOTATIONSCALES -> #760`.
  * `#ABB / AcDbBlockRepresentation -> #ABC` contains `AcDbRepData -> #ABD`
    and `AppDataCache -> #ABE`. `#ABE` contains `ACAD_ENHANCEDBLOCKDATA -> #ABF`
    (keys `1`, `10`, `23` point to XRecords `#AC0`, `#AC1`, `#AC2`),
    `ACAD_ENHANCEDBLOCKHDATA -> #AC3`, and `ACAD_ENHANCEDBLOCKHISTORY -> #AC4`.
  * `#AC6 / AcDbBlockRepresentation -> #AC7` contains `AcDbRepData -> #AC8`
    and `AppDataCache -> #AC9`. `#AC9` contains `ACAD_ENHANCEDBLOCKDATA -> #ACA`
    (keys `1`, `10`, `23` point to XRecords `#ACB`, `#ACC`, `#ACD`),
    `ACAD_ENHANCEDBLOCKHDATA -> #ACE`, and `ACAD_ENHANCEDBLOCKHISTORY -> #ACF`.
  * The reference registers `#759`, `#75F`, `#ABC`, `#AC7` with empty entry lists
    in each file (16 dictionary instances). Its TABLECONTENT reader consumes
    subsequent object records as table data: diagnostics report group-5 handles
    `#75A` and `#ABD` in `readCell`, `#760` and `#ABE` in `readLinkedTableRow`,
    `#AC8` in `readTableRow`, and `#AC9` in `readLinkedTableDataSubclass`.
    The relevant routines in `IO/DXF/DxfStreamReader/DxfObjectsSectionReader.cs`
    advance into nested readers and continue row/cell loops across record
    boundaries. The outer `Read` never receives templates for those records;
    `CadDictionaryTemplate.build` then reports `Entry not found`.
  * The same cursor loss consumes TABLECONTENT targets `#149C`, `#24AC`,
    `#13EF`, `#23FF`, respectively, in `readTableRow`. XRecord `#149F`, `#24AF`,
    `#13F2`, `#2402` code 360 remains numeric in C#, while the native reader
    retains each known orphan template and the view resolves it. These are the
    four remaining `XRecord.Entries` mismatches. Registered-object absence alone
    is not a resolution rule: `CadXRecordTemplate.build` uses the builder's
    broader template map (`CadDocumentBuilder.TryGetCadObject`).
  * This is source-stream parser desynchronization, not an ownership/key policy.
    The decoded graph does not retain the reference's cursor and nested table
    parse state. Reproducing that state belongs to TABLECONTENT compatibility;
    filtering these valid keys, versions, or orphan types would discard valid
    records in other documents. For example, the reference retains these same
    `#759` and `#ABC` entries in `sample_AC1015.dwg` and
    `sample_AC1021_ascii.dxf`. Native readers and the registration graph are
    therefore preserved.
* `sample_AC1014.dwg`: resolved in R4-C3. The reference registers no
  ACDBPLACEHOLDER from R13/R14 drawings (observed on this sample and on both
  corpus R14 files), so the plot-style dictionary-with-default has no entries
  and `#F` is absent; the view now omits R14 placeholders. The earlier remark
  that a blanket policy is incorrect rested on this one sample.

## R4-C1 remaining reference differences (decision D-b)

Corpus entity rows left after R4-C1, each a case where ACadSharp provably loses data that the fork reads
correctly. Nothing is emulated. The reference side was traced with the CadCore port (a 1:1 port of the C#
reader) printing its notifications.

* **ACDBVIEWREPBLOCKREFERENCE inserts (35 `Insert`).** The reference reads this class as an `UnknownEntity`
  ("Unlisted object with DXF name ACDBVIEWREPBLOCKREFERENCE") and `CadBlockRecordTemplate.addEntity` drops
  unknown entities (`KeepUnknownEntities` is false), so they are never registered. The fork decodes them as
  `Insert` (the class derives from AcDbBlockReference). Handles: cwoffenden/radiator-bracket.dwg `#5371 #544C
  #59F2`; markov-ai task-030 `#3A6 #3AA #4C1 #4C5 #512 #516 #573 #577 #C0A #C0E`; task-040 `#548 #54C #5B9 #5BD
  #603 #607`; task-043 `#4D3C #4D40 #6BE3 #6BE7 #846B #846F #897D #8981`; task-050 `#374 #378 #722 #726 #EE5
  #EE9 #1031 #1035`. Rows: `Insert` extra-object, `BlockRecord.Entities` for their blocks.
* **LARGE_RADIAL_DIMENSION (3 `DimensionRadius`).** Read as `UnknownEntity` ("Unlisted object with DXF name
  LARGE_RADIAL_DIMENSION" in DWG, "Entity not supported" in DXF) and dropped. The fork reads the jogged radial
  dimension. Handles: owerko/dimensions.dwg `#73FA`; ezdxf/uncommon.dxf `#37C #414` (the reference reports a third one). Follow-on rows: the entities' extension dictionaries and their DimensionAssociation
  (`CadDictionary` extra-object 3, `DimensionAssociation` 3), `DimensionStyle.Reactors` of `#27` (2 files) and
  `BlockRecord.Entities` of model space.
* **MINSERT with a zero Z scale (scau-gis/10号别墅建筑施工图.dwg, AC1015).** The reference's `Insert.ZScale`
  setter throws "ZScale value must be none zero", the MINSERT reads fail (`#11E8`, `#11EB`, `#2442`: three
  Error notifications) and, because AC1015 walks the block's first/last entity chain until a template is
  missing, every later model-space entity is lost: 242 `Insert`, 184 `AttributeEntity`, 182 `Seqend`, 3,526
  `Line`, 1,067 `TextEntity`, 317 `DimensionLinear`, 217 `LwPolyline`, 115 `Hatch`, 87 `Arc`, 63
  `DimensionAligned`, 60 `Circle`, 7 `Spline` and 2 `AttributeDefinition` (the latter two plus
  `BlockRecord.AttributeDefinitions/HasAttributes` of `#2`). The fork keeps the stored zero scale and the
  whole chain.
* **XDATA strings with a legacy code page (scau-gis/10号别墅建筑施工图.dwg `Line #446F`, 1 row).** The
  reference decodes each 1000 string through `ReadTextUnicode`, with the code-page key stored in the record,
  and gets U+FFFD for every byte of "西文字型", "中文字型", ... (6 strings); the fork decodes them with the
  drawing's GB2312/GBK page and reads the Chinese text.
* **Rows owned by the next job:** `ProxyEntity`, `ProxyObject`, `Field` and `FieldList` (closed in R4-C2, below).

## R4-C2 remaining reference differences (decision D-b)

* **DXF FIELD code written as 3-chunks before group 2 (ezdxf/mtext_complex_inline_codes.dxf, 8 `Field.FieldCode`).**
  ezdxf writes the long field code as groups 3 (255-character chunks) followed by the closing group 2. The
  reference appends group 3 to `FieldCode` and then assigns group 2 through the generic property mapping, which
  replaces everything read before it, so it ends with only the last chunk (", sed diam nonumy ..."). The fork
  concatenates 2 and 3 in file order and keeps the complete code (`{\A1;Complex MTEXT content:}...`). Handles:
  #A9 #DD #E4 #EB #F7 #100 #107 #10E. The closing `Value` string of these fields (groups 2 chunks then group 1)
  agrees in both readers, since the fork also takes the code-1 group; no other row of the file differs.

## R5a DXF round trips (gate G4, decision D-b)

`Tools/roundtrip-report.sh --impl rust` writes each of the 48 gate samples as ASCII and binary DXF with C# and with
`acad-dump write-dxf`, and compares four dumps with the C# reference R (C# reads its own file):
C#→Rust (the fork reads the C# file), Rust→C# (C# reads the fork's file), Rust→Rust. Results at R5a:

* C#→Rust matches in 88 of 96 cells; Rust→C# and Rust→Rust match in none, bytes are identical in none.
  Of the 8 other C#→Rust cells, 4 differ only by `$HANDSEED`/SEQEND handles (AC1014) and one ATTDEF
  `HorizontalAlignment` that ACadSharp takes from the last group 72 (AecObjects); 4 are the R12 cells (class 7).
* Comparing with R is the wrong reference under D-b: R already carries ACadSharp's own round-trip losses, and losses
  both implementations share are invisible. A triage script that sorted the remaining rows into the classes below
  reported 92 of 96 cells explained; an adversarial review found it hides real writer losses (it matched whole
  properties regardless of value, counted every extra object as explained), so it is not a gate and is not kept.
* The fork's own round trip (dump of the input vs dump of the fork's re-read DXF output) matched in 0 of 96 cells at R5a.

### R5a-2 / R5a-3: gate G4 as redefined

`Tools/roundtrip-report.sh --impl rust` (driver `Tools/g4-roundtrip.py`) measures per sample and mode (ASCII, binary):

* **RR**: the native dump (`acad-dump native-dump`, see R5a-3 above) of the fork's re-read output equals the native
  dump of the input. Comparer `Tools/g4-native-compare.py` (numbers exact, signed zero one value; `--self-test`),
  closed list `Tools/g4-rr-allowlist.json` (11 rules, below).
* **idem**: the native dump of a second write's re-read equals the first re-read's. No rules.
* **RC**: ACadSharp reading the fork's output equals the committed C# dump D of the original (canonical dump,
  `Tools/g4-compare.py`, `Tools/g4-allowlist.json`, 478 rules, `python3 Tools/g4-compare.py --self-test`). Every rule
  is RC only: its cause is ACadSharp's reader or a D-b row, which RR and idem do not see.

RR allow-list (format-inherent differences, each rule names record scope, struct type, property, relation, exact
values and a reason that holds without the run):

| id | property | relation | reason in one line |
| --- | --- | --- | --- |
| RR-001 | TableStyle `raw_dxf_codes` values | same text after trimming blanks, or the same number | DXF numbers are free-format text; binary DXF has no spelling |
| RR-002 | Layout `raw_plot_settings_codes` values | same | same |
| RR-003/004 | SummaryInfo created/modified date (DWG source) | number to none | DXF has no SUMMARYINFO; the instants are `$TDCREATE/$TDUPDATE` (header, compared) |
| RR-005 | Hatch `pattern.lines[].angle` (DWG source) | within 2 ulp | DXF group 53 is degrees, the model radians |
| RR-006..008 | SortEntitiesTable / TableStyle / Layout raw DXF group pairs (DWG source) | none to any | a DWG has no DXF group pairs; the typed fields are compared |
| RR-009 | Shape `shape_number` (DWG source) | number to 0 | a DXF SHAPE names the shape (group 2), no group for its .shx number |
| RR-010 | Layer `source_flags` (DWG source) | equal outside bits 1, 2, 4, 16 | those bits are typed fields; the DXF reader keeps the whole word, the DWG reader the rest |
| RR-011 | TextStyle `source_flags` (DWG source) | equal outside bits 1, 4, 16 | same |

Counts, cells of 96 passing (ASCII + binary, 48 samples each):

| | RR | RC | idem |
| --- | --- | --- | --- |
| before R5a-3 (native dump and the rules above, writer of 78947fc) | 0 (0 + 0) | 60 (30 + 30) | 74 (37 + 37) |
| after R5a-3 (agent) | 44 (24 + 20; 4 exact, 40 allowed) | 64 (32 + 32; 24 exact, 40 allowed) | 92 (46 + 46; all exact) |
| after the R5a-3 review | 44 (24 + 20) | 68 (34 + 34) | 92 (46 + 46; all exact) |
| after R5a-4 | 70 (37 + 33; 4 exact, 66 allowed) | 68 (34 + 34; the same cells, no cell with more unexplained rows) | 92 (46 + 46; all exact) |
| after the R5a-4 review (RR-033 narrowed, RR-034, carrier handle chosen by the writer) | 70 (37 + 33; 4 exact, 66 allowed) | 68 (34 + 34) | 92 (46 + 46; all exact) |
| after R5a-5 | 83 (45 + 38; 4 exact, 79 allowed) | 68 (34 + 34; the same cells, no cell with more unexplained rows) | 92 (46 + 46; all exact) |

(The numbers recorded before this job, RR 44 / RC 60 / idem 92, compared the compat view for RR and idem and are not
comparable.) No cell passes less than before in any check, and no cell has more unexplained RC rows.


The review of R5a-3 removed three native-dump exclusions that could hide losses (`EntityProperties.graphic_data`, which
DXF carries in groups 92/160/310; `MText.dwg_x_direction`, group 11; `Polyline2D.source_widths`, groups 40/41), added
`doc/header_references` (current records by name, so excluded header handles hide nothing) and `entity_xdata_raw`
(EED the DWG reader could not structure), and fixed MTEXT group 11 (written only when the text has a direction or a
rotation) and ATTRIB/ATTDEF group 1 (the record's own empty group 1 written back for an unedited multi-line value).
Losses this exposed, closed in R5a-4 (see the end of this section): DWG→DXF loses entity graphic data for Mesh, Wipeout, RasterImage and Underlay (the DXF
reader keeps common graphics only for proxy, MLine, Light, Surface and Table); `$CMLSTYLE`, `$DIMLTYPE`, `$DIMLTEX1`,
`$DIMLTEX2` are not resolved from their DWG handles (sample_AC1021 writes BATTING/TRACKS/ZIGZAG as ByBlock).
UNVERIFIED: a VISUALSTYLE with extended properties is written in the R2013 layout into R2010-and-older files; whether
older AutoCAD accepts it needs a manual check in AutoCAD.

Open, listed by cause (RR unless noted). Nothing here is allow-listed.

* **DXF-sourced, 8 cells (all closed in R5a-6: binary DXF is written at the version of the document and an AC1009 document as R12; see the end).** Binary DXF of R2000/R2004 sources (4 cells: AC1015/AC1018 ascii and binary inputs, binary
  output) is written as R2010 binary DXF (`DxfWriter::output_version`, BricsCAD rejects the older binary headers); the
  document version is the only difference. This is writer policy, not a format fact, so it is not an RR rule (the RC
  rules G4-009..011 keep it). The four R12 cells (AC1009, ASCII and binary output; idem fails there too): the writer
  declares an R12 document as R13 (`writable_version`, issue #68), so owners (group 330) the R12 source lacks appear,
  the second unnamed shape-file STYLE comes back with another handle and `$HANDSEED` grows on every pass (the idem failure; cause not traced), polyline widths move
  from the polyline to its vertices, and viewports/layouts/MLEADERSTYLE/TABLESTYLE the R13 structure adds differ.
* **DWG-sourced, 18 of 44 cells (9 samples; R5a-4 closed the other 26, see below; R5a-5 closed 13 of the 18; the causes (1) to (5) below are those before R5a-5, see its section at the end for what is left).** AecObjects (2 cells, 3,527
  rows each), sample_AC1014 (116), sample_AC1015 (72), sample_AC1018 (76), sample_AC1021 (49), sample_AC1024 (806),
  sample_AC1027, sample_AC1032 and sample_base/sample_base (792 each). The rows left, by cause:
  (1) modern TABLE (entity and object, every cell of the nine except AecObjects: 791 rows in each R2013+ cell, 36-50 in
  the older ones): cell geometry, custom data, cell style overrides and the `legacy_override_bits`, contents with
  block/value, formatted values. AutoCAD's own DXF (R2018 sample) writes the entity in the legacy cell layout and keeps
  the modern data in TABLECONTENT and TABLEGEOMETRY objects; the fork's DWG reader keeps the R2010+ entity record
  inline and the DXF writer writes only the legacy layout for it. Not analysed further: a carrier scheme like the one
  for CELLSTYLEMAP (restore into the entity, take the objects out, write them again) is the likely shape of the fix.
  The R2013+ cells (AC1027, AC1032, sample_base) have nothing else but this and one POINT row;
  (2) ACIS written as SAT: a DWG of R2004-R2010 stores SAB, a DXF of that version only SAT (`is_binary`, `sab_data`,
  `sat_data`, `version` of 3DSOLID and REGION in AC1018, AC1021 and AC1024); the conversion is the fork's
  (`SabReader::to_sat_string`) and nothing in RR can tell it from a loss;
  (3) AEC proprietary objects (AecObjects): `Unknown` DWG objects of AEC classes have no DXF form and are not written,
  with their dictionary entries, reactors and extension dictionaries (2,101 + 1,412 rows per cell);
  (4) R14/R2000 leftovers: AC1014 viewport UCS, LEADER and TOLERANCE dimension variables, layer description and
  transparency, annotative flags, layout dictionary order, no THUMBNAILIMAGE (written from R2000; UNVERIFIED that
  R14 has no such section), VX_TABLE records and `vx_control_entries` (not read or written in DXF), `DATATABLE`
  objects (AC1015, AC1018: `Unknown`, no model), DIMSTYLE `dimfxl`/`dimjogang`/`dimfit`/`dimblk_name`/`annotative`
  of DWGs before R2007 (the DXF reader's defaults 1.0 and 45 degrees, the record has none);
  (5) POINT `material_flags` 1 (ByBlock: AutoCAD's DXF has `347` with the handle of the ByBlock material; ACadSharp's
  DWG reader ignores the flag, so mapping it needs a compat change) in AC1021-AC1032.
* **Invisible to RR.** Where both the DXF reader and the model drop a group, RR cannot see it. A model-independent
  audit (group counts per record type in the AutoCAD ASCII samples against the written file, 19 samples) lists group
  kinds with fewer occurrences in the output; most are defaults left out (normals 210/220/230, viewport UCS 110..146)
  or a changed layout, but these look like real losses and are not fixed: header `$INTERFEREOBJVS`/`$INTERFEREVPVS`
  (345/346), `$UCSORTHOREF` text, `$HANDLING` (R12), LAYER 348 (visual style), TABLE 340 (default dimension style
  handle), MLEADERSTYLE 179, LWPOLYLINE 43 (constant width), MTEXT 46/63, DIMENSION 73/74/75/280, the ACDSDATA
  schema and records (dynamic blocks, `ACDSRECORD` 8 to 6), GEODATA 303 and SUN 421. (`$CMATERIAL` was found this way
  and fixed.) Each is UNVERIFIED as a loss until its group is looked up.

Classes, each with the reason (classes 1-3 are decided by experiment, the rest by inspection):

1. **C# loses on its own round trip (D-b).** The fork's value equals the C# dump of the original: ACadSharp's writer
   or reader changes it. About 10,000 property rows: header dimension variables (`DIMGAP`, `DIMEXO`, `DIMDLI`, ...
   written as constructor defaults), `$EXTMIN/$EXTMAX/$LIMMAX`, `$TRACEWID`, `$SKETCHINC`, DIMSTYLE `Flags` (its
   reader ignores group 70), LTYPE/LAYER/APPID/STYLE flags, plot style names, `Layer` XDATA, dictionary entries of
   dynamic blocks, 62/420 colors, and so on.
2. **Objects the fork keeps** (about 14,700 rows: XRECORD, dictionaries, MATERIAL, dynamic-block objects, TABLE,
   SOLID3D, ...): ACadSharp drops or never loads them; reading the fork's file it sometimes loads more than it
   did from the original. No data is lost on the fork side.
3. **Objects ACadSharp invents** (about 350 rows): an empty `ACAD_MATERIAL` dictionary and similar defaults written
   by its writer.
4. **Counters and defaults.** `$HANDSEED`; SEQEND handles (the reader gives them handles above the file's highest);
   the fork writes its complete class list (260 entries; ACadSharp only the used classes); a read document keeps the
   default AppIds `AcCmTransparency` / `AcAecLayerStandard` and default dictionaries the compat view hides.
5. **Fork writer policy kept.** AcDbDynamicBlockRoundTripPurgePreventer and other association objects other
   applications cannot restore are not written (`is_unrestorable_assoc_object`); binary DXF of an R2000/R2004
   drawing is written as R2010 (BricsCAD rejects the older binary headers), which adds the R2007+ groups; R12
   sources are written as R13 structure (upstream issue #68). (Both closed in R5a-6.)
6. **DWG originals.** D holds ACadSharp's DWG-reader values, which the compat view reproduces and a DXF cannot carry:
   `CreateDateTime`/`UpdateDateTime` are half a day later (DWG date conversion), `DIMFIT` is read from the DIMATFIT
   slot, entity transparency of an ENC word 0 is "opaque", the DWG has-attributes bit and attachment/spacing
   defaults of old records, `BlockRecord.Preview` (the DXF reader keeps only the last group 310 chunk), anonymous
   block numbering, `HorizontalWidth`/`VerticalHeight` constants. About 4,100 rows in 44 cells, classified by
   inspection; some rows of this class are real losses listed above.
7. **R12 originals (4 samples × 2 modes, open).** ACadSharp reads and writes an AC1009 drawing with R13 handles in
   its tables, the dumps compare by handle, and the fork's R12 reader numbers the tables itself, so every owner
   differs. Not closed.

### R5a-4: DWG-sourced round trips

RR rows over the 44 DWG-sourced cells (22 samples x ASCII/binary; the comparer's rows before the rules / the rows no
rule accepts): **22,821 / 22,283 before, 15,170 / 13,991 after**; 26 of the 44 cells pass (the ten dynamic-block
samples, geoloc, sample_base/empty and svg/export_sample, ASCII and binary). Gate: RR 44 -> 70 of 96 (ASCII 24 -> 37,
binary 20 -> 33), RC 68 -> 68, idem 92 -> 92; no cell passes less in any check and no cell has more unexplained RC rows
(40,447 before and after). G1 45/49, the same rows as before.

| Family (rows no rule accepts, before -> after) | |
| --- | --- |
| Dictionary (hard owners, entries, owner) | 7,674 -> 2,838 (the rest: entries of AEC objects that are not written) |
| TableStyle | 972 -> 0 |
| ACIS 3DSOLID/REGION | 500 -> 72 |
| Header / doc | 328 -> 26 |
| DimStyle | 341 -> 83 |
| Layout | 92 -> 0 |
| XDATA records (STYLE typeface) | 164 -> 6 |
| MultiLeaderStyle | 96 -> 0 |
| Modern TABLE | 7,511 -> 6,647 |
| AEC / DATATABLE unknown objects | 4,214 -> 4,214 |
| Other entities and tables | 391 -> 105 |

Closed by changing the reader or the writer (root cause): dictionary hard owners (DWG reader); header names and
`last_saved_by` (DWG reader kept defaults); `legacy` header state (DXF reader built it for modern files); GROUP names;
MLEADERSTYLE name and R2010 fields (DWG reader); THUMBNAILIMAGE (both); entity graphic data of MESH/WIPEOUT/IMAGE/UNDERLAY
(DXF reader); STYLE typeface XDATA (DXF reader kept a second copy); TABLESTYLE text style handles, modern cell styles
and the border groups (DXF reader and writer, DWG reader); override layers (DXF reader); RADIUS definition point (DXF
reader); SUN colour (both); ACIS empty bit, revision GUID, `uid`, `has_ds_data`, SAT `point_of_reference` (both);
dynamic-block class names (DXF reader); VX handle (DXF reader); anonymous bit (DXF reader and writer); layout
viewport list and shade-plot DPI (DWG reader); embedded ATTRIB text direction (DWG reader); TABLE block name and cell
fill (DWG reader). The matching rows of "Behaviour changes" name each one.

New RR rules (Tools/g4-rr-allowlist.json; each has a cited DXF reason, an example the self-test runs, and `source: dwg`):
RR-012/013 `dimmzf`/`dimaltmzf` (no DXF group) 100 -> 0; RR-014 TableStyle base `Table` cell style -> none (AutoCAD's own
CELLSTYLEMAP has three styles, the DWG four); RR-015 the block pointer of the dynamic-block purge preventer (AutoCAD's
DXF has the version only); RR-016/017 ARC angles within 2 ulp; RR-018 `Spline::dxf_flags` (the DXF word, restated by
typed fields); RR-019 custom knot parameterisation 15 -> 0 for a control-vertex spline (AutoCAD writes no bit);
RR-020/021 wireframe isolines 4 -> 0 (3DSOLID, REGION); RR-022/023 ACIS empty bit true -> false before R2013;
RR-024..027 DIMSTYLE true colours (ACI only in DXF); RR-028 `modern_unknown_long1` 0 -> 8 (undocumented DWG long);
RR-029 `$CECOLOR` true colour -> exactly its nearest ACI index (`rgb_nearest_aci`, Tools/aci-palette.json); RR-030..032
`Table::dxf_insert_*` (DXF restatements of typed fields); RR-033 `maintenance_version` 0 -> 377, R2018 inputs only (an
R2018 DWG has no `$ACADMAINTVER`; the review narrowed it from "0 -> any": for R2000-R2013 inputs the writer states the
file-header byte, which the native dump does not hold, so those rows stay open); RR-034 (review)
`TableStyle::cell_style_map_handle` 0 -> a handle for R2010+ inputs (the DXF carrier object an R2010+ DWG does not
have). New comparer features: `when` (a condition on the expected record), `unit` (a rule judging a whole
subtree), relations `input_object_members_output_null` and `rgb_nearest_aci`. New RC rules (RC only, Tools/g4-allowlist.json):
G4-491..493 `$DIMLTYPE`/`$DIMLTEX1`/`$DIMLTEX2` (ACadSharp reads the handles and keeps ByBlock), G4-494 `$CMLSTYLE`
(Standard -> STANDARD): D-b rows the fork now creates by resolving what ACadSharp loses.

Open: see the DWG-sourced bullet above. UNVERIFIED: AutoCAD's acceptance of a THUMBNAILIMAGE with a PNG; a CELLSTYLEMAP
object in an R2000 DXF; `$ACADMAINTVER` of a DWG R2018 file is written as 377 (RR-033 accepts it).

### R5a-5: the remaining DWG-sourced losses

Gate G4 after the job: **RR 83 / 96** (ASCII 45, binary 38; 70 before), **RC 68 / 96** (the same cells; none has more
unexplained rows, the R2010+ DWG cells have fewer: 48 -> 46, the R2007 ones 6 -> 4, AC1014 68 -> 62), **idem 92 / 96**.
G1 45 / 49 with an identical `rs-gap.tsv`. Of the 18 DWG-sourced cells that failed RR, 13 pass: AC1014, AC1015 and
AC1018 ASCII, AC1021, AC1024, AC1027, AC1032 and sample_base ASCII and binary. Still failing, all DWG-sourced: AecObjects
(2 cells, 3,525 rows) and AC1014, AC1015, AC1018 binary (3 cells, 13, 2 and 1 rows).

RR rows over the nine samples' ASCII cells before the job (comparer rows no rule accepts; the binary cells repeat
them), per family: modern TABLE 3,382 (791 in each of AC1027, AC1032 and sample_base, 793 AC1024, 50 each AC1018,
AC1015 and AC1014, 36 AC1021); AEC unknown objects and what hangs on them 3,527 (AecObjects); ACIS written as SAT 36
(12 each in AC1018, AC1021, AC1024); DIMSTYLE fields of a pre-R2007 DWG 38; the rest of AC1014 52 (viewports 18,
dictionary order 7, VX table and entries 7, TOLERANCE and LEADER 11, preview, universal dates, layer description and
transparency, annotative flags, DATATABLE 2) and of AC1015 11 (VX table, DATATABLE, dates); `$ACADMAINTVER` 5 and POINT
`material_flags` 5 (one row per cell of AC1015 to AC1032). After: the three R2013+ cells and AC1024 pass through RR-035
and RR-048 (one cell with several items), the older ones through the rules below, AecObjects and the three binary cells
keep the rows listed under "Open".

Fixes (R = reader, W = writer): see the R5a-5 rows of "Behaviour changes" above. Reader: DXF TABLE completion, legacy
cell word, legacy block cell, material ByBlock, annotation scale, R13/R14 viewport restore, application names; DWG
reader: `$ACADMAINTVER`, cell type, formatted value, DIMSTYLE and viewport defaults, DATATABLE type 6. Writer: carriers
without a copy, legacy cell word and items, material 347, DATATABLE type 6, layout dictionary case.

New RR rules (Tools/g4-rr-allowlist.json; each has a reason that cites the DXF group or version and a self-test example):
RR-035 ACIS: binary on the input, SAT on the output, verified equal by `acad-dump acis-same` (Solid3D, Region, Body;
AC1018, AC1021, AC1024; AutoCAD's own DXF of those versions holds SAT, from R2013 the ACDSDATA section holds SAB);
RR-036 R14 `$TDUCREATE/$TDUUPDATE`; RR-037 R14 preview (no THUMBNAILIMAGE before R2000); RR-038..041 the VX table of R13 to
R2000 (no DXF table); RR-042/043/047 the R13/R14 LEADER and TOLERANCE values with no group; RR-044/045 the 16-bit
cell word of R2000 to R2004 (`177`); RR-046 the R2010 TABLE bit; RR-048 the cached geometry and resolved text style of the
effective item of a cell whose TABLECONTENT object lists several items (found through the entity's round-trip record by
the new relation `table_cell_cache_of_multi_item_cell`; single-item cells are compared in full). New relations:
`acis_sab_as_sat`, `input_type_output_absent`, `input_set_output_clear`, `table_cell_cache_of_multi_item_cell`;
`type` and `property` of a rule may be lists, and a rule may name the record `key`. New RC rules (RC only, D-b rows):
G4-495 `Point.Material` null -> the ByBlock material (new relation `null_to_named_object`), G4-496 / G4-497 a block
cell's `Type` and empty `Contents` in a DXF read. The comparer's `--self-test` of both lists passes (48 and 485 rules).
R5a-5 review: G4-495 limited to R2007+ sources; G4-496 narrowed from `output_const` (any number -> 2) to `const_pair`
0 -> 2 for R2010+ sources; G4-497 narrowed from any list -> [] to a list of block items only (new relation
`items_with_member_to_empty`), R2010+ sources; RR-042 no longer accepts `byblock_color` (group 77 carries it, now
fixed); G4-052 pairs (20, 15) and (0, 10) replaced by (20, 0) (the R2000 and R14 file-header bytes are no longer
written as `$ACADMAINTVER`).

Measured (item 0): peak memory footprint of `acad-dump write-dxf` (`/usr/bin/time -l`, debug build), before -> after:
sample_AC1014.dwg (the largest gate DWG, 1.3 MB) 23.43 -> 23.17 MB, sample_AC1024.dwg 22.51 -> 19.84 MB,
sample_AC1032.dwg 18.53 -> 17.43 MB, `PIER CAP REINFORCEMENT.dwg` (26.5 MB) 1,406,388,000 -> 1,409,255,000 bytes (the
file has no table style with modern cell styles, so neither build copies the document; the difference is noise).
`CadDocument::clone` shares the entities (Arc), so the copy was cheap in memory for entity-heavy drawings; the objects,
tables and side maps were copied, and the document is no longer copied at all.

Open (DWG-sourced):

* **AecObjects, 2 cells, 3,525 rows each** (892 `Unknown` objects and what hangs on them: dictionary entries, reactors,
  owners, extension dictionaries). They have no DXF form. Carrying the verbatim DWG record in a proxy object (the way the
  fork carries registered-class objects: `ACAD_PROXY_OBJECT`, class 499, a private envelope) closes the RR rows but two
  (a BLOCK marker and a dictionary owner that point at no record: 3,527 -> 2), and was tried and **not adopted**: the
  oracle's DXF reader then reads 883 proxy objects, 321 dictionaries and 160 dictionary entries ACadSharp's DWG reader
  never saw, and four AEC objects (Wall, AecWallStyle, AecCleanupGroup, AecBinRecord) change their type, which no RC rule
  may allow; RC unexplained rows would go from 21 to about 1,370. AutoCAD's acceptance of such proxies is UNVERIFIED too.
  The `CANNOSCALE` row of this drawing is fixed (DXF reader).
* **AC1014, AC1015, AC1018 binary (3 cells; closed in R5a-6)**: the binary DXF of an R2000/R2004 drawing is written as R2010 (writer
  policy, see "Open" above), so `doc/version` differs, plus the knock-ons of the R2010 form (VX control handle,
  `value_handle` of a block cell, AC1014: dimension style `dimfit`/`dimblk_name`, viewport scale, and the R14 leader's
  `byblock_color`, which an R2010 DXF carries in group 77 as `override_color` only). The ASCII cells of the same samples pass.
* **Tables**: a cell with several items keeps only the effective item's cache out of DXF (RR-048); a DWG table whose
  TABLECONTENT / TABLEGEOMETRY objects are missing has no carrier (the writer does not create them: the modern data of
  the entity record is then lost in DXF). Cell styles are written to the legacy groups from the legacy snapshot
  of a completed table only; a table built in memory derives its legacy groups from the modern style.

Review doubts: the DWG writer states its own maintenance byte (0, 25, 30, 29 for R2004 to R2013), not the header's, so a
DWG to DWG round trip (G5) now differs in `maintenance_version`; a table completed from TABLECONTENT keeps its legacy
snapshot, so an edit of a cell style after a DXF read does not reach the legacy groups (and the TABLECONTENT object is
written as read, so an edit of the entity does not reach it either); the ACadSharp view looks up the ByBlock material
per entity (a scan of the objects for each ByBlock entity of a DXF source).
UNVERIFIED: the VX table, the preview and `$TDUCREATE` rules for AC1014 rest on AutoCAD's R2000 DXF, the DXF reference's
TABLES list and the oracle (there is no R14 DXF in the fixtures); AutoCAD's acceptance of `91` = the style word in R2007+
legacy cells, of the second CELL_VALUE of a block cell and of the carriers is not checked; the DATATABLE group of types
7 to 9. Upstream PR candidates: case-insensitive application names; layout dictionary repair; legacy cell word.

### R5a-6: DXF written at the source version (binary R13 to R2004, and R12)

Gate G4 after the job (clean build of the final tree, 48 samples): **RR 94 / 96** (ASCII 47, binary 47; 83 before),
**RC 72 / 96** (36 + 36; 68), **idem 96 / 96** (92). G1 45 / 49 with an `rs-gap.tsv` identical to R5a-5's. Fork suite
`cargo test --features acadsharp-compat --no-fail-fast`: 2,491 passed, 0 failed, 60 ignored after the review (2,482 before; the nine
new tests are `tests/r5a6_dxf_versions.rs`), 0 warnings in `cargo build --lib` and the feature build. No cell regresses
in RR, RC or idem; the RC unexplained rows of the cells that still fail are equal to before.

Closed: the 7 binary RR cells (AC1014, AC1015, AC1018 DWG; AC1015 and AC1018 ASCII and binary DXF, binary output): the
file is written at the version of the document, so the R2007+ defaults of the R2010 form no longer appear; and the 4
R12 cells (AC1009 ASCII and binary DXF, ASCII and binary output): RR, RC and idem are exact (RC went from 9,895
unexplained rows to 0). The six R12 DXF files a real CAD wrote (`tests/fixtures/r12/cad-created`, BricsCAD `acR12_dxf`)
also read back to the same native model in both modes (RR and idem, checked outside the gate).

Removed from `Tools/g4-allowlist.json` (CadKit): the 38 RC rules whose reason is "Binary DXF of a pre-R2007 drawing is
written as R2010" (G4-009..014, 016..020, 053..058, 157..173, 482..484, 486). None is used in the final run (before, the used ones
covered 1 to 546 rows each), and their reason is no longer true, so they would only widen the list. The two comparers'
`--self-test` pass (447 and 48 rules). No rule was added; no native-dump exclusion or fold was added. G4-413, G4-414 and
G4-423 are unused as before.

Binary version: the only reason the promotion gave was BricsCAD's refusal of older binary headers (UNVERIFIED here: no
BricsCAD). A binary DXF of R13 to R2004 differs from R2010's in the two-byte codes it shares and in its strings, which
are in the declared code page.

R12 writer (rows "R5a-6 R12 ..." above): an AC1009 document made of R12 entities (POINT, LINE, CIRCLE, ARC, SOLID/TRACE,
3DFACE, TEXT, SHAPE, INSERT with ATTRIB, ATTDEF, POLYLINE/VERTEX/SEQEND in its 2D, 3D, polyface and polygon-mesh forms,
VIEWPORT, the seven R12 DIMENSION kinds) is written as it is. A newer entity of an AC1009 document is converted on its
own by `writer/r12.rs` (its R12 parts carry no handle and no extended data; a hatch becomes its parts in place, no `*X`
block), and the document around it keeps its R12 records, handles and extended data (the first version of R5a-6 sent the
whole document through the converter when it held one newer entity, which dropped every handle, all extended data and
the MVIEW records; fixed in the review). `with_version(AC1009)` of another version is converted as a whole, as before
except that it now writes binary too; the module doc lists the rules: LWPOLYLINE becomes a 2D POLYLINE with its bulges and widths, RAY and
XLINE long LINEs, everything else is decomposed with `EntityType::explode` until only R12 entities remain (MTEXT into TEXT,
ELLIPSE and SPLINE into polylines, HATCH into lines or an INSERT of an anonymous block of SOLIDs, ...) and what cannot be
decomposed (3D solids, images, OLE, ...) is left out; names are mapped to the R12 alphabet there (the native path keeps
the names of the document). ARC_DIMENSION and LARGE_RADIAL_DIMENSION are not R12 kinds and take that path.

Header and tables of the native path: only the variables of an R12 file are written (`R12_HEADER_VARIABLES`), so what an
AC1009 document holds beyond them (R13+ variables a DWG R12 or an edit set, `HeaderVariables::legacy.user_unit_*`) has no
place in the file. The pre-R13 view state is written as `$VIEWCTR`, `$SNAPMODE`, ... when it differs from the default
state (those are R11 variables; R12 keeps them in the VPORT; AutoCAD's acceptance of them in an R12 header is UNVERIFIED).

Open:

* **AecObjects, 2 cells** (RR and RC): as in R5a-5.
* **RC, 24 cells**: unchanged (the R2000+ DWG- and DXF-sourced cells of R5a-5, all with explained causes there).
* **R12 file written from a document built in memory or read from an R12 DWG** states all handles of its entities and
  none of its table records. The table records of an R12 file have no handles, so the reader allocates them above the
  highest handle the file states: when an entity added to a document read from an R12 DXF states its handle (above the
  source's highest), the table records of the re-read get other numbers than in the document that was written.
* **Names on the native path**: the section writer keeps the names of the document; a document built in memory at
  AC1009 with names outside the R12 alphabet (spaces, more than 31 characters) is written with them, while the
  converter (`with_version(AC1009)` of another version, and the parts of a converted entity) maps them.
* **R12 VIEWPORT**: the view is written only through the regenerated MVIEW record (what the record has: target,
  direction, twist, height, centre, lens, clips, status, circle sides, fast zoom, UCS icon, snap and grid state, snap
  angle, base and spacing, hide-plot, frozen layers); the fork's other viewport fields (render mode, UCS references,
  shade plot, ...) are lost in an R12 file, as in an R12 DWG.
* UNVERIFIED (no AutoCAD or BricsCAD here): that AutoCAD accepts the R12 files (the groups written beyond the fixtures'
  alphabet are R12 groups at their defaults: TEXT 73, ATTDEF/ATTRIB 7, 41, 50, 51, 72, 73, 74, POLYLINE 73 to 75, VERTEX
  50, DIMENSION 3 and 53), and BricsCAD's acceptance of binary R2000 and R2004 DXF. Upstream PR candidates: the R12 DXF
  writer (issue #68), the polyline vertex width rule, the binary Latin-1 fallback.

### R5a-7: the remaining RC failures (ACadSharp reading the fork's DXF against D)

Gate G4 after the job (clean build of the final tree, 48 samples, four chunks of 12): **RR 94 / 96** (unchanged),
**RC 94 / 96** (72 before), **idem 96 / 96**. No cell regresses in RR, RC or idem (baseline and final `cells.tsv` compared
cell by cell). G1 45 / 49 with an `rs-gap.tsv` identical to R5a-6's. Fork suite `cargo test --features acadsharp-compat
--no-fail-fast`: 2,492 passed, 0 failed, 60 ignored (2,491 before; one new test, `tests/block_record_xdata.rs`), 0 warnings in the
test build, `cargo build --lib` and the feature build. The two comparers' `--self-test` pass (471 and 48 rules).
Review (clean `cargo clean -p acadrust` build, full gate re-run with G4-499 and G4-508 narrowed): RR 94 (ASCII 47, binary
47), RC 94 (47, 47), idem 96 (48, 48); against the R5a-6 cells no cell regresses; the 4 failing cells are AecObjects RR and
RC. G1 45 / 49 (`rs-gap.tsv` identical to R5a-6's), suite 2,492 / 0 / 60.

One fork change (row "R5a-7 R13/R14 dynamic block name"), twenty-four RC rules (G4-498..521, CadKit `Tools/g4-allowlist.json`).
Each family was compared with AutoCAD's own DXF of the drawing where one exists (ACadSharp run on it is the committed
dump of the `_ascii.dxf` twin) and with ACadSharp's reader source.

1. **TableEntity.Rows (46 rows per R2010+ DWG cell, 1 in AC1021): an ACadSharp DXF-reader limitation.** ACadSharp's
   DXF reader (`readTableEntity`) maps the legacy TABLE groups only and builds no `Rows[].CellStyle`, no cell geometry, and
   reads no date value (groups 92/310); its DWG reader fills them from the modern record. For every one of the 46 rows of
   AC1027, AC1032 and sample_base (and AC1024, AC1021) the value ACadSharp gives for the fork's output equals the value it
   gives for AutoCAD's own DXF of the same drawing (`sample_AC1027_ascii.dxf` etc.). Rules G4-498 (row CellStyle object to
   null), G4-499 (cell background index -1 to 0), G4-500 (relation `cad_value_to_null`: a number to null on a ValueType 8
   value). Not fixed: for a DWG-sourced table the fork writes the legacy groups at their defaults (group 90 = 0, 93 = 0, no
   40/41 margins, no 140 text heights, 91 = 0 per cell) where AutoCAD's DXF has the values (`sample_AC1027_ascii.dxf`, TABLE
   528: `90 22, 93 7340056, 40/41 0.0342, 140 x3, 91 262176 ...`); the modern TABLECONTENT carries the same data, and none of
   the RC rows depends on them, but a reader that takes the legacy groups gets defaults (open item).
   Review: all 370 rows these three rules accept in the 10 cells equal ACadSharp's value on AutoCAD's DXF twin. G4-499 is
   narrowed (relation `const_pair_expected_sibling`) to a D background whose TrueColor is 0 (the DWG reader's unset colour),
   so a real true-colour background the output dropped keeps failing. The legacy-group gap is a loss for legacy-only
   readers, and no rule masks it: ACadSharp on AutoCAD's DXF and on the fork's output differ for TABLE 528 in `ValueFlag`
   (22 / 0), `OverrideFlag` (true / false), `EdgeFlags` of rows 4 to 6 (9, 11, 1 / 0), the content colour of row 4 cell 0
   (7 / ByBlock), the right border of row 4 and 5 cell 2 (63, 40 / ByBlock, -2) and the empty value records of the merged
   title cells (one content / none); the gate cannot see it because ACadSharp's DWG reader leaves the same members at those
   defaults (D of the DWG equals the fork's output there). Not derivable from the native model without AutoCAD's style
   resolution: those legacy values are effective values (the right border 63/40 of row 4 cell 2 is the cell's
   inside-horizontal border in the model, its right border is not overridden), group 91 carries bit 0x40000 and the
   visibility bits without the matching groups, and value flag 22 has no counterpart in the model; fitting them to one
   drawing was not done.
2. **CadDictionary entries / XRECORD / BlockRepresentationData in the AC1015 and AC1018 DXF cells (35 rows per cell): ACadSharp
   loses a run of objects in the original.** The original holds TABLECONTENT objects (`sample_AC1015_ascii.dxf` 747, 149C, ...);
   `readCell`, `readLinkedTableRow` and `readTableRow` end their loop with a `ReadNext` after the sub-reader stopped on the
   next group 0, so the object that follows is read as table content (the notifications read `Unhandled dxf code 5 value
   1882 at readCell method`) and is not registered; the run reaches #75A, #760, #ABD..#AC4 and the like, and the dictionaries
   that own them show no entries in D. Evidence: with TABLECONTENT 747 cut out of the original ACadSharp registers #75A
   (checked with a small program on the built ACadSharp.dll). The fork's output holds the same objects in another order
   and ACadSharp reads them (RR shows the model equal). Rules G4-501..503 (`object_lost_below_dictionary`: an object only in
   the output whose owner chain ends at a dictionary of both that lists it in the output but not in D), G4-504..507
   (`entries_of_lost_objects`: the output's entries, less those naming such objects, are D's), G4-508 (the XRECORD
   handle that D keeps as an integer, as G4-181 for DWG sources). Sources dxf, versions AC1015 and AC1018.
   Review: with both TABLECONTENT objects (747, 149C) cut out of `sample_AC1015_ascii.dxf` and `sample_AC1018_ascii.dxf`,
   ACadSharp (`Tools/AcadDump/run.sh dump`) registers all 18 objects G4-501..503 accept in the gate (#75A, #760,
   #ABD..#AC4, #AC8..#ACF); in file order #75A/#760 follow 747 (after its TABLEGEOMETRY and ACDBDATATABLE) and
   #ABD..#AC9 follow 149C, the others are owned by them. The relations accept only objects present in the output (never one
   it lacks), and RR proves the fork read them from the original. G4-508 is narrowed (relation `xrecord_marked_handle_int`)
   from any handle/integer pair to an integer of D turning into the reference of the same number, on a group-360 entry
   right after the marker `ACAD_ROUNDTRIP_2008_TABLE_ENTITY` (the reference to the TABLECONTENT).
3. **Solid3D / Region AcisData in AC1018, AC1021, AC1024 DWG: SAB written as SAT.** The DXF of these versions has SAT text
   only; AutoCAD's own DXF agrees (D of `sample_AC1021_ascii.dxf` holds SAT). Rules G4-509/510 (`acis_sab_as_sat_text`)
   accept the pair only when `acad-dump acis-same` (the tool of RR-035, reached through RS_DUMP) finds the SAB and the SAT the
   same model; verified on the three real pairs of AC1018 outside the gate. One difference to AutoCAD's text remains: the SAT
   header's fourth number is 26 (R2007) or 24 (R2010) there, the value of the SAB header, and 0 in the fork's (its SAT header
   keeps a boolean `has_history`); the rule does not look at it (open item).
4. **Viewport rows and two block names in AC1014: one reader fix and a rule.** (a) The DWG reader looked for the
   application `AcDbDynamicBlockTrueName` by exact spelling; R13/R14 files state `ACDBDYNAMICBLOCKTRUENAME`, so the
   down-saved dynamic block kept its anonymous name `*U17` where ACadSharp names it `my-dynamic-block` (BlockRecord and
   Block name rows): case-insensitive now (test `r14_dynamic_block_recovers_its_true_name_from_an_upper_case_application`,
   fails without the fix). (b) The view parameters of an R13/R14 viewport live in its ACAD/MVIEW extended data only;
   ACadSharp's DWG reader reads none (`DwgObjectReader.cs:6447-6476` is R2000+), D holds zeros while D's own MVIEW record states
   the values the fork wrote (R5a-5 restores them, as it does on a DXF read). Rules G4-511..518 (`viewport_mview_real`, one per
   field: the zero turns into the n-th real of the expected viewport's own MVIEW record), G4-519/520 (`viewport_derived_from_mview`:
   ScaleFactor and ViewWidth are ACadSharp's getters of the view height). No R14 DXF of AutoCAD is in the fixtures, so there is no
   external check of the values (only that they are D's own MVIEW numbers).
5. **AecObjects (RR and RC, 2 cells): not a writer fix.** The 21 RC rows are one header value and the AEC objects
   (`Wall`, `AecWallStyle`, `AecCleanupGroup`, `AecBinRecord`) with the dictionary entries and the block record entity list that
   name them: the fork keeps them as opaque `Unknown` records with no DXF form, so they are not in the output (ACadSharp's
   DWG reader types them). A proxy carrier is not a form (R5a-5: it passed RR and broke RC on ~1,350 objects). The header row
   (`MeasurementUnits` 256 against 1) is ACadSharp's `readTemplate` skipping a UTF-16 description by its character count
   as bytes (a one-character description gives the bytes 00 01 = 256; the fork reads two bytes a character and accepts only 0 or 1):
   rule G4-521 (D-b). The cells keep failing on the 20 object rows and RR.

Rule use in the final run: G4-498 88 rows, 499 272, 500 10, 501 64, 502 64, 503 16, 504..507 32 each, 508 8, 509 12, 510 6,
511..520 12 each (AC1014, both modes), 521 2 (AecObjects).

Open:

* **AecObjects, 2 cells** (RR and RC): as above.
* **DWG-sourced TABLE legacy groups** at defaults (family 1): the fork's DXF is shorter than AutoCAD's for the same table;
  a legacy-only reader (ACadSharp's DXF reader, pre-2008 AutoCAD) loses the table and cell override flags, edge flags and
  the effective colours/borders AutoCAD resolves into them (see family 1, review). The gate cannot detect it.
* **SAT header flags**: the fork writes `0` as the fourth number of the SAT header, AutoCAD writes the SAB header's value (26 for
  R2007, 24 for R2010); content is the same model (`acis-same`).
* UNVERIFIED: the R14 viewport values against an AutoCAD-written R14 DXF; that AutoCAD opens the fork's DXF with the TABLE legacy groups
  at defaults.


## R5b DWG round trips (gate G5)

Job R5b-1 (2026-10-22): tooling, baseline and the first writer/reader fixes. Fork tree at 740e8d2 plus this job's
working-tree changes; CadKit `Tools/roundtrip-report.sh --impl rust --dwg`, `Tools/g5-roundtrip.py`.

### What G5 measures, and what it cannot

For each of the 48 gate samples and each version V (AC1014, AC1015, AC1018, AC1024, AC1027, AC1032; 288 cells) both
implementations write the sample (`write-dwg --version V --pin`) and read the files. A cell is judged in four directions:
C# reference (R = C# reads C#'s own DWG), C# -> fork (the fork reads C#'s DWG; compat dump = R), fork -> C# (C# reads the
fork's DWG = R) and fork -> fork (the fork reads its own DWG; compat dump = R). The comparator is `cadcore-dump diff-json`
(exact under DUMP-FORMAT's rules; no allow-list). `same-failure` (both fail at the same step: write, or read) passes;
`better` (C# fails, the fork does it; decision D-a) is reported and not counted as a pass. Diagnostic columns, not part
of G5: the native round trip (the fork's native dump of the source against that of its re-read DWG, no rules, only the values
`--pin` overwrites left out) and the bytes.

**R carries ACadSharp's own losses.** The reference dump R is what ACadSharp reads back from its own file. Where its writer or
reader loses data the fork keeps, a faithful fork can never equal R. The driver therefore attributes every differing row of a
reader direction with D, the committed C# dump of the source: `fork-differs` (R equals D, the fork's side does not: a fork
defect), `csharp-lost` (the fork's side equals D and R does not: ACadSharp's round trip lost it) and `both-differ`.
The attribution is exact only in the 42 cells whose source version is V: elsewhere a value ACadSharp converts for V and the
fork keeps counts as `csharp-lost`. In those 42 cells, after the review fix of the synthesized BLOCK markers (before it C# could
not read any fork DWG written from DXF), fork -> C# has 27,944 differing rows: 6,401 `csharp-lost`, 20,646 `fork-differs`, 897
both. The csharp-lost rows of the DWG sources are ACadSharp losses (checked by hand, sample_AC1032.dwg and
BLOCKFLIPPARAMETER.dwg at AC1032: VISUALSTYLE and DIMASSOC objects are skipped by its DWG writer, `DwgObjectWriter.Objects.cs`
`skipEntry`, and the reactors to them go with them; LAYOUT writes `MinLimits` twice, `writeLayout`; TABLE and TABLESTYLE top
borders carry the edge flag of the right border, `TableStyle.CellStyle.cs` `TopBorder = new(CellEdgeFlags.Right)`; SHAPE is
written only with `WriteShapes` and dynamic block data only with `WriteDynamicBlockData`, both false by default; 3DSOLID,
REGION, BODY and surfaces are not written; `$PSTYLEMODE` is written only when the read flag equals 1 while the reader stores
0x2000). Every reader direction cell of a DWG source has such rows, so fork -> C# and fork -> fork cannot pass as specified
(equality with R) and G5 needs the redefinition G4 got in R5a-2/3. But the rows of the DXF sources are mostly `fork-differs`
(all 20,646): fork DWG writer differences from DXF input (TABLE rows 8,208, class table 3,549, cell styles, BLOCK/ENDBLK and
MULTILEADER colours, hatch seed points, materials) mixed with ACadSharp DXF-reader losses where the fork's file has more than D
(VISUALSTYLE objects that D lacks). Not worked yet. The counts below are the gate as specified.

### Counts

| | baseline | final |
|---|---|---|
| C# reference (ACadSharp reads its own DWG) | 256 match, 12 better, 20 error (AC1014: ACadSharp throws reading its own R14 file) | the same (C# is untouched) |
| C# -> fork | 135 / 288 (AC1014 0, AC1015 0, AC1018 3, AC1024 44, AC1027 44, AC1032 44), 32 better | **204 / 288** (0, 25, 44, 45, 45, 45), 32 better |
| fork -> C# | 2 / 288 (both fail like C#, AC1014), 14 better | 0 / 288, 32 better (review: the 12 AC1014 `same-failure` cells were C# failing on the fork's bare paper-space markers; it now reads them: `better`) |
| fork -> fork | 0 / 288, 16 better | 0 / 288, 32 better |
| native round trip (diagnostic) | 0 exact | 2 exact (sample_AC1015.dwg and sample_AC1032.dwg at their own version) |
| bytes | 0 identical | 0 identical |

`better` cells: the 12 R12 sources (`sample_AC1009_ascii.dxf`, `_binary.dxf` x 6 versions: ACadSharp's DWG writer throws
`KeyNotFoundException: 'Standard'`, the fork writes), AC1014 cells where ACadSharp cannot read its own file (20), and the R14
fork files C# reads while it fails on its own (12, DXF sources).

Differing rows by attribution (fork -> C#, all cells): baseline 34,663 = 19,989 fork-differs + 14,498 csharp-lost + 176 both;
agent's final 33,645 = 17,419 + 15,085 + 1,141; after the review fix 153,410 = 113,547 + 32,330 + 7,533 (the 144 DXF-source
cells now reach the comparison). In the 21 same-version cells of DWG sources fork-differs went **995 -> 12 rows** (left: class
instance counts, the multi-line attribute tag/value of sample_AC1015/sample_AC1032 (see open), a dangling layer handle, MTEXT
`\U+00B0` that the fork decodes); the 21 same-version cells of DXF sources had no rows before the review fix and have 20,634
fork-differs rows now. In the other 246 cells D is another version: there `fork-differs` and `csharp-lost` both include
version-driven values; read them as indications, not as lists of defects. For C# -> fork: 876 rows (786 fork-differs) -> 269
(179).

Largest families before -> after (fork-differs rows, fork -> C#): DimensionStyle ExtendedData 800 -> 0, Layer ExtendedData
638 -> 0, BlockRecord/Block Name 400 -> 0, AppIdsTable/AppId 73 -> 0, header ModelSpaceExtMin/Max 326 -> 0, PdfUnderlayDefinition
Reactors 68 -> 0, TableEntity Columns 96 -> 0, Layer/AppId Flags ~300 -> 0, HandleSeed 49 -> 13; C# -> fork: TableStyle cell
style 330 -> 0 (110 cells), MText LineSpacingStyle 180 -> 0, dimension AttachmentPoint 55 -> 0, GeoData 6 -> 0.

### Fixes (R = reader, W = writer, V = ACadSharp view; rows in "Behaviour changes" above)

W extended data order, layer transparency packed zero, header extents, TABLE style types, underlay definition reactors,
referenced bit, style dictionaries of a same-version source, viewport grid bits, unused default application ids, handle seed,
anonymous block markers and stated-anonymous dynamic definitions, class instance counts; R+V legacy TABLESTYLE alignment word,
MTEXT line spacing style word, dimension attachment point word; R GEODATA civil trailer, V its scale estimation; tool: named
colours before R2004; review: W synthesized BLOCK markers (`synthesized_block_markers_state_the_full_name`). Tests (25 new): `tests/r5b_dwg_roundtrip.rs` (15 synthetic round trips at every affected version),
`tests/r5b_dwg_read.rs` (5: the ACadSharp view and the application table), `tests/acad_dump_write_dwg.rs` (3: the
`write-dwg` contract), and 2 unit tests in `io/dwg/dwg_stream_readers/object_reader/objects.rs` (the alignment word, the
GEODATA trailer). Each test of a fix fails with that fix reverted (checked one by one); two tests guard behaviour kept
(layer transparency change, unset header extents).

### Open families (none has an allow-list)

* **csharp-lost rows** (above): not fixable in the writer. Evidence that the fork is right: a fork file of the source version
  read by ACadSharp equals D (e.g. dynamic-blocks/BLOCKFLIPPARAMETER.dwg at AC1032: all 66 remaining differences from R are
  csharp-lost; ACadSharp's own file reads without VisualStyle, dictionaries, TABLE rows, block parameters).
* **Cross-version conversion** (246 cells; the largest rows): TABLE rows between the legacy and the R2010+ layout
  (`TableEntity.Rows` 6,741, 12 cells: R14/R2000/R2004 -> R2010+), class table (the fork writes the pre-R2013 profile with its
  own numbering and drops unused classes; `ClassNumber`, `DxfClass`, `classes.<order>`), DICTIONARY hard-owner flag (R14 -> R2000,
  715 rows), CanScale/IsExplodable (R14..R2004 -> R2007+ defaults, 243 rows), MULTILEADER attachments, materials, XRecord and
  dictionary objects that exist in one profile only. The AutoCAD-saved samples are the evidence for these: the six
  `sample_ACxxxx.dwg` are one drawing saved at six versions, so the right result of `sample_AC1032.dwg` at V is the dump of
  `sample_V.dwg` (not R). A first look (AC1032 -> AC1018, 807 rows): 181 TABLE row rows (empty merged cells have no content in
  the fork's legacy layout), 148 XRecord objects and 56 dictionaries that exist only in AutoCAD's R2004 file, 34 AppId and 19
  Layer referenced-bit rows (R2007+ does not state the bit, so there is nothing to write), classes. Not worked in this job.
* **R14 (AC1014) files written by ACadSharp**: the fork's R14 reader drops `ACAD_PLOTSTYLENAME` (ACDBDICTIONARYWDFLT /
  ACDBPLACEHOLDER written under class codes its class table does not state): 26 cells.
* **AecObjects** (5 cells): `Cannot encode custom entity AEC_WALL ... compatible raw DWG data is required` at every version but
  the source's; also G4's two failing cells.
* **Multi-line attributes**: R2000/R2010 files keep the attribute as `TAG_001...`; the fork merges and writes one attribute
  (Tag `MULTI_LINE_ATT_001` -> `MULTI_LINE_ATT`, Value moved). Design question, not changed.
* **Named colours before R2004**: the library refuses the write (upstream, issue #132); only the tool downgrades.
* **ACadSharp view differences left**: MTEXT / dimension words are shown as stated only for files read from DWG
  (`source_*` words); the DXF reader folds the same words (not needed by any round trip).
* UNVERIFIED: that AutoCAD opens the fork's pre-R2007 files with the referenced bit and the viewport grid bits, the R2000 file
  with `ACAD_TABLESTYLE` (a file that already had it); G8 (speed/memory) was not re-run: the reader changes are two 4-byte
  provenance words (MText, DimensionBase), one 4-byte word per legacy row style and one comparison in the GEODATA reader;
  the writer gained one pass over the entities to find unused default application ids (shared with the DXF writer).

## R5b-2: G5 under the G4-style definition (2026-10-23)

Fork tree: r5a at 740e8d2 + R5b-1 + this job's working-tree changes; CadKit `Tools/g5-roundtrip.py`, `Tools/g5-twin-rules.py`,
`Tools/g4-compare.py`, `Tools/g4-native-compare.py`, `Tools/g4_relations.py`. The definition below is a **proposed working definition**.

### Definition (per cell: sample x version V; 288 cells)

* **G5-RR**: the fork's native dump of the source against the native dump of the fork's re-read of its own pinned DWG at V,
  exact except the closed RR allow-list (values `--pin` overwrites in header and summary are excluded):
  `g5-rr-allowlist.json` (hand rules only: ACIS SAT as SAB, TABLE `data_size` 0 -> 24, class members matched by name, classes the
  writer adds: LAYOUT, ACDBPLACEHOLDER). The generated `g5-rr-twin-allowlist.json` was deleted in the review (below).
* **G5-RC**: C# reading the fork's DWG against D (the committed C# dump of the source) for the 42 same-version cells (V = source
  version; a DXF source counts at its own version) and against the C# dump of `sample_V.dwg` for the 36 cells of the six
  `sample_AC10xx.dwg`. Files: `g5-rc-allowlist.json` (hand rules G5-RC-001..013, 016..024), generated `g5-rc-twin-allowlist.json` (251 rules) and
  `g5-rc-source-allowlist.json` (308 no-loss rules, sample_V cells).
* **G5-idem**: a second pinned write of the re-read document re-reads natively identical (no rules).
* C# -> fork and the C# reference/bytes stay informational columns.
* Separate allow-list files (not `g4-allowlist.json`): RC uses `g4-compare` rules and RR `g4-native-compare` rules, which have
  different schemas; G4 rules never apply (their modes are `dwg`-less, G5 rules carry `target_versions`/`cell_kinds`).
* **Twin rules** (`Tools/g5-twin-rules.py`, independent of the fork's output where RC): the corpus holds 21 DXF samples that
  AutoCAD saved with a same-version DWG twin. Where ACadSharp reads the DXF and the DWG of one drawing differently at a
  property, the fork's DWG is accepted for a DXF source at that property if it equals what ACadSharp reads from AutoCAD's DWG
  (`equals_twin`: the twin's value at the same handle; the same expected -> actual pair at another object of the type only for
  an object the twin does not have under its handle; never a missing object). No-loss rules (`equals_source`) accept a sample_V
  cell's value where ACadSharp reads the fork's file as it reads the source at the same object (never a whole object). Both lists
  are generated from the committed C# dumps only (`g5-twin-rules.py --check` regenerates and compares).

### Counts (baseline = clean build of R5b-1 with the old tool; final = this tree; `build.noindex/g5-baseline-cells.tsv`, `g5-final2-cells.tsv`)

| | baseline | final |
|---|---|---|
| G5-RR (288) | 2 exact (5 errors) | **29** (19 exact + 10 allowed), 254 mismatch, 5 error |
| G5-RR same-version (42) | 2 | **29** |
| G5-RC same-version (42) | 0 (raw; review figure 11) | **35** (all through rules), 7 mismatch |
| G5-RC sample_V (36) | 0 | **5** (the diagonal cells) |
| G5-idem (288) | 261 | **273** (10 mismatch, 5 error); same-version 42 / 42 |
| C# -> fork | 204 | 204 |

Per version (final; RR / RC same / RC sample_V / idem): AC1014 0/48, 0/1, 0/6, 37/48; AC1015 1/48, 1/3, 1/6, 47/48; AC1018 1/48, 3/3, 1/6, 47/48;
AC1024 1/48, 3/3, 1/6, 47/48; AC1027 1/48, 1/3, 1/6, 47/48; AC1032 25/48, 27/29, 1/6, 48/48. Baseline RR: AC1015 1, AC1032 1.
G4 (final tree): RR 94, RC 94, idem 96 / 96 (only AecObjects fails); G1 45 / 49.

### Allow-list rules used (rows / cells / cells rescued by rules alone) — as the agent reported them; see the review below

RR: G5-RR-001 ACIS SAT as SAB 144 / 12 / 0 (the fork writes SAB from R2013 on; `sab_matches_sat_with`, header date ignored);
G5-RR-002 TABLE `value.data_size` 96 / 60 / 0 (AutoCAD states 24 bytes for the content value, the DXF stores 0); G5-RR-003 class
members by name 9,892 / 114 / 10; G5-RR-004 LAYOUT and ACDBPLACEHOLDER added by the writer 900 / 120 / 10; twin rules: T002 header+classes 43 / 21 / 10,
T052 MLineStyle linetype 42 / 21 / 10, T059 TableStyle 55 / 21 / 10, T065 BlockRecord entity handles 98 / 15 / 5, T066 DimStyle 82 / 21 / 10, T015 Leader 22 / 10 / 0,
T030/T033 ACIS 18 / 12 / 0, T031 Shape 10, T056 SortEntitiesTable 30, T067/T069 layer / text style source_flags 30 / 12 / 0, T003.. others as in the report.
RC: 001..010 pinned constants (dates, editing times, GUIDs) rescue 40 / 23 cells; 011/012 VisualStyle-owned extras 144 rows / 6 cells / 4; 013 XRecord
handles 4; 014/015 MText CIF decoding 6; 016 HandleSeed (Seqend objects) 4; 017/018 hex LF -> CRLF; 019/020 SAT as SAB 4 + 4; 021 GeoData 1;
023 DxfClass ClassNumber 15; twin T001 DxfClass 15, T005/T008/T012/T013 header words 15 each, T010/T014 13, T011 4. Every rule has the evidence in its `reason`
(an AutoCAD file of that version, or an ACadSharp source line) and a self-test example (`g4-compare.py`, `g4-native-compare.py --self-test`: 471 / 74 rules).
The full per-rule table is the "Allow-list rules" block of `build.noindex/roundtrip-rs-dwg-report.md`.

### Fixes (W writer, R reader; line numbers of this tree)

W DXF source version `src/document.rs:1600,1957`; W DXF class table `io/dwg/dwg_writer.rs:828-880` and `classes/mod.rs:260`; W class counts `dwg_writer.rs:1018`;
W BLOCK/ENDBLK common data `object_writer/mod.rs:2389,2457`; W same-version gating (VisualStyle, Material, schema dictionaries, grid bits, style dictionaries)
`object_writer/objects.rs`, `entities.rs`, `dwg_writer.rs`; W material map source `objects.rs:1024,1127`; W DIMPOST `tables/dimstyle.rs:526`, `object_writer/mod.rs:1697,1719`;
W hatch gradient name `entities.rs:4792`; W TABLE covered cells `entities/table.rs:659,2444`; W Template section `file_headers/file_header_ac18.rs`;
W maintenance versions `aux_header_writer.rs:35`; W ACIS reference point `entities.rs:5357`; W VisualStyle round trip `objects/stub_objects.rs:433`, `dwg_writer.rs:694`;
W header names and arrow blocks `dwg_writer.rs:1134`; W viewport-override layer `object_writer/mod.rs:116`; W multi-line attribute tag `entities.rs:1366,4769`;
W+R TableStyle bit 8 (`objects.rs` writer and reader `object_reader/objects.rs:473`). Tests: `tests/r5b2_dwg_roundtrip.rs` (22 synthetic tests; the 11+ that exercise a changed behaviour
fail on the baseline tree, checked by stashing; the guards that pass on both are the constructed-document style handle, the no-template case and the MTEXT case);
`cargo test --features acadsharp-compat --no-fail-fast`: 2,539 passed (the 22 new included), 0 failed, 60 ignored.
Reverted after test conflicts (kept in the writers instead): DXF reader defaults for DIMPOST and material map source.

### Open families

* Cross-version RR (246 cells, ~336K rows): version conversion (VisualStyle 58 -> 24 properties, class tables, dictionaries, tables). sample_V RC: 31 cells fail;
  AC1024 -> AC1027/AC1032 and AC1027 <-> AC1032 are within a handful of rows (class EXACXREFPANELOBJECT WasZombie - the open "live classes" decision; TABLECONTENT / TABLEGEOMETRY
  InstanceCount 2 vs 5-7; Spline KnotParametrization; ClassNumber; TableEntity rows; XRecord objects).
* Same-version RR failures (13 of 42): sample_AC1014.dwg (825 rows: R14 dictionary hard owner, xdata order), AecObjects (BLOCK markers), DXF sources: layer / text style
  referenced bit (64) at AC1015 / AC1018, MText `dwg_x_direction`, anonymous `*U` renumbering, Solid3D `acis_empty_bit`, Spline `dwg_flags1` (fork 1, AutoCAD 9), VPort `aspect_ratio` (1 ulp).
* Same-version RC failures (7): sample_AC1014.dwg (BlockRecord xdata order, HandleSeed), BookColor Color in the AC1015 DXFs, Spline Flags1 in the AC1027/AC1032 DXFs.
* idem failures (10 + 5): DXF sources written at AC1014 (viewport ids / VX table); AecObjects (custom AEC entities cannot be re-encoded across versions, no proxy carrier).
* Not changed (design / owner): multi-line attribute merge, named colours before R2004, BookColor.
* UNVERIFIED (no AutoCAD here): that AutoCAD opens the fork's pre-R2007 files, the maintenance bytes 104 / 226 / 125, the TableStyle bit 8, ACIS point retention, LINEAR gradient names,
  the Template section for DXF sources. G8 not re-run (reader changes: one mask in the TABLESTYLE reader).

### Review of R5b-2 (2026-10-23)

The generated rules were the main risk. Findings and changes (CadKit `Tools/`):

* **RR twin rules deleted** (`g5-rr-twin-allowlist.json`, 74 rules; the RR part of `g5-twin-rules.py`, `rel_equals_twin` and the twin
  plumbing of `g4-native-compare.py` / `g5-roundtrip.py`). Their evidence was the fork's own native reader of AutoCAD's DWG: a reader
  that misreads the DWG excuses the same misreading of the fork's DWG (circular), they accepted whole `<record>` rows missing where
  the twin lacked the key, and the twins' native dumps were cached across fork builds. They rescued the 10 RR cells of the DXF
  sources at AC1032 (10 dynamic-block drawings and geoloc.dxf); those cells now fail. Families they hid there: Layout
  `raw_plot_settings_codes`, Material `*_map.source` (writer states 1 for an omitted DXF group), DimStyle `dimpost` (`<>` -> ""),
  TableStyle `modern_style` / `cell_style_map_handle` / `raw_dxf_codes`, MLineStyle element linetype case, header `dim_linetype*`,
  `dim_*mzs`, `maintenance_version`, `multiline_style`, anonymous `*U` block records and Insert `block_name`. Several are AutoCAD's
  format (candidates for hand RR rules with sample evidence); Material source, DIMPOST and the gradient name are native-model
  defaults of the DXF reader that the writer overrides, so the native round trip differs by design until the reader states them.
* **RC `equals_twin` narrowed**: a missing object (`<object>` row with the actual side absent) is never accepted, even where the
  twin lacks the handle (AutoCAD renumbers objects between its DXF and its DWG, and the fork keeps the DXF handles, so a missing
  object there was a fork loss passing); the expected -> actual pair at another object counts only for an object the twin does not
  have under its handle and never for `<object>`/`<owner>`/`<type>` rows. The SEQEND rows it had accepted go to the new hand rule
  G5-RC-024 (`seqend_renumbered`, as G4-237). No RC cell was lost. Every twin-rule application of the agent's run (about 19,500 rows) was re-checked
  mechanically against the twin dumps (every key match exact within the dump tolerance; the pair fallback was used only for Block.Comments, BlockRecord.Preview,
  Layer.PlotStyleName of renumbered objects) and 32 sampled applications by hand: all equal to AutoCAD's own file at the same object.
* **RC `equals_source` narrowed**: never a whole object (16 generated rules removed, 324 -> 308); they rescued no cell.
* **G5-RC-014/015 (MText `\U+XXXX` decoded) deleted**: not an ACadSharp limitation but a fork DWG writer defect (it wrote the
  decoded character where AutoCAD and the fork's DXF writer keep the escape); fixed in the writer (row "R5b-2 MTEXT source spelling",
  test `an_mtext_keeps_the_escape_its_source_spelled`, fails with the fix reverted). The 6 cells they rescued now pass with the pinned
  rules alone.
* **G5-RR-003/004 (class list) hardened**: both now require every class of the input to be in the output; before, a class dropped
  with LAYOUT written in its place passed (self-test cases added).
* G4 behaviour unchanged (G4 rules name no `target_versions`/`cell_kinds`, G5 rules carry `modes: ["dwg"]`); measured below.
* The "0 raw" RC baseline of the agent counts the ten pinned header/summary fields (every pinned write differs there); with them
  left out, as the native comparison does, the baseline is the earlier review's 11 / 42 (DWG sources 11 / 21 with 74 rows, DXF
  sources 0 / 21 with 25,536 rows), recomputed from `g5-baseline-cells.tsv`.
* Test file: the `#[allow(dead_code)]` stubs that kept unused imports alive were removed (the imports went instead).

Measured on a clean build (`cargo clean -p acadrust`; CadKit `build.noindex/roundtrip-rs-dwg`, 288 cells):

| G5 check | no rules | pinned-field rules only (G5-RC-001..010) | all hand rules | all rules |
|---|---|---|---|---|
| RR (288) | 19 | 19 | 19 | 19 (DWG sources 19 / 132, DXF 0 / 156) |
| RC same-version (42) | 0 | 20 | 20 | 35 (DWG 20 / 21, DXF 15 / 21 through the twin rules) |
| RC sample_V (36) | 0 | 5 | 5 | 5 (the diagonal) |
| idem (288, no rules) | 273 | | | 273 |

Per version (all rules; RR / RC same / RC sample_V / idem): AC1014 0/48, 0/1, 0/6, 37/48; AC1015 1/48, 1/3, 1/6, 47/48; AC1018 1/48, 3/3, 1/6, 47/48;
AC1024 1/48, 3/3, 1/6, 47/48; AC1027 1/48, 1/3, 1/6, 47/48; AC1032 15/48, 27/29, 1/6, 48/48. C# -> fork 204 / 288. Against the agent's
cells only the 10 RR cells above changed. G4: RR 94, RC 94, idem 96 / 96, no cell changed. G1 45 / 49.
`cargo test --features acadsharp-compat --no-fail-fast`: 2,540 passed, 0 failed, 60 ignored.

Open doubts (owner): the same-version rules (`is_source_version`) now also apply to DXF sources of that version, and a valid
same-version source (DWG or DXF) now keeps its stated class instance counts instead of the census (stale after an edit; the
counts are advisory); TABLE covered-cell items, the gradient name LINEAR and the material map source are writer outputs keyed to
DXF provenance that make G5-RR differ from the native model.

## R5b-3: same-version G5 for DXF sources, idem and the sample_V cells (2026-10-23)

Fork tree: r5a at ab82a76 + this job's working-tree changes; CadKit `Tools/g4-native-compare.py`, `Tools/g5-roundtrip.py`,
`Tools/g5-rr-allowlist.json`, `Tools/g5-rc-allowlist.json`, `Tools/g4-allowlist.json` (G4-494 reason). Definitions of R5b-2 unchanged
(G5-RR native, G5-RC vs D / sample_V, G5-idem; the same-version rules still apply to DXF sources: open owner decision, not touched).

### Counts (clean build of the final tree, 288 cells; before = R5b-2 review)

| | before | after |
|---|---|---|
| G5-RR (288) | 19 (DXF sources 0 / 21) | **41** (20 exact + 21 through hand rules); same-version 41 / 42, the cross-version cells 0 / 246 |
| G5-RC same-version (42) | 35 (DWG 20 / 21, DXF 15 / 21) | **42** (DWG 21 / 21, DXF 21 / 21) |
| G5-RC sample_V (36) | 5 (diagonal) | **6** (the diagonal, AC1014 now included); the 30 off-diagonal cells cannot pass (below) |
| G5-idem (288) | 273 (10 mismatch + 5 error) | **283** (0 mismatch, 5 error = AecObjects) |
| C# -> fork | 204 | 204 |
| G4 | RR 94, RC 94, idem 96 / 96 | unchanged (only AecObjects fails; per-cell statuses identical) |
| G1 | 45 / 49 | 45 / 49 |

Per version (RR / RC same / RC sample_V / idem): AC1014 1/48, 1/1, 1/6, 47/48; AC1015 3/48, 3/3, 1/6, 47/48; AC1018 3/48, 3/3, 1/6, 47/48; AC1024 3/48, 3/3, 1/6, 47/48;
AC1027 3/48, 3/3, 1/6, 47/48; AC1032 28/48, 29/29, 1/6, 48/48. Before: RR 0, 1, 1, 1, 1, 15; RC same 0/1, 1/3, 3/3, 3/3, 1/3, 27/29; idem 37, 47, 47, 47, 47, 48.
Rule subsets (rows files of the final run): RR no rules 20, all rules 41 (the RR list is hand rules only; the pinned header and summary values are left
out by the tool); RC same-version no rules 0, pinned-field rules only (G5-RC-001..010) 21, all hand rules 21, all rules 42 (DXF sources through the twin rules);
RC sample_V no rules 0, pinned-field rules 6, all hand rules 6, all rules 6. The only same-version RR cell that still fails is `aec_objects/AecObjects.dwg` at AC1032
(its two BLOCK / ENDBLK entities have no block record in the file, so no DWG can hold them; the other five AecObjects cells cannot be written, see R5b-2).

### Fixes (R = reader, W = writer; lines of this tree)

R+W DIMPOST, R material map source, R MText default direction, R $CMLSTYLE name, W VX control of a DXF source and no filling of an R14 VX table, W EED block order,
W R14 dictionary byte (W block scalability of an unstated record: reverted in the review): the behaviour rows above, "R5b-3 ...". Places: `io/dxf/reader/section_reader.rs:8637,11125`,
`io/dxf/writer/section_writer.rs:2634`, `io/dwg/dwg_document_builder.rs:476`, `io/dwg/dwg_reader.rs:893`, `io/dwg/dwg_writer.rs:933,944`,
`object_writer/common.rs:1074`, `object_writer/objects.rs:1576`, `object_writer/mod.rs:2272`. Tests (`tests/r5b3_dwg_roundtrip.rs`, 7; each fails with its fix reverted, checked one by one):
the default X axis of an MTEXT, the current multiline style named by its key, the VX records of a DXF source hang below their control, a partial VX table is not
filled up (review: filled for the active paper space only), the order of the xdata applications of a block record in R14, the R14 dictionary hard-owner flag, an unstated block scalability (review: replaced by the
test of the reverted behaviour). The assertions of
`r5b2_dwg_roundtrip::an_omitted_dimension_postfix_is_written_empty`, `::materials_of_a_dxf_source_keep_their_map_files`, `r5a3_tests::an_empty_dimension_postfix_and_the_default_are_both_stated`
(renamed) and `acadsharp_materials::omitted_dxf_material_fields_use_view_defaults_without_changing_native_values` (renamed in the review) follow the new reader defaults (they pinned the old ones).
`cargo test --features acadsharp-compat --no-fail-fast`: 2,547 passed, 0 failed, 60 ignored; 0 warnings in `cargo build --lib` and the feature build.

### Same-version G5-RR of the 21 DXF sources: every family, fix or rule

Fixed in the fork (reader or writer defect, or a model default the DWG states differently): Material `*_map.source` (R), DimStyle `dimpost` (R+W), MText `dwg_x_direction` (R),
header `multiline_style` (R), the VX table (W, idem at AC1014), R14 xdata order and dictionary byte (W, sample_AC1014.dwg); block scalability (W, sample_V) was reverted in the review.
Hand RR rules (`g5-rr-allowlist.json` G5-RR-005..050; each names type, property, relation, target versions, source `dxf`, a reason and a self-tested example;
evidence is a cited ACadSharp source line, the DWG format, or the C# dumps of AutoCAD's own DWG/DXF twins, never the fork's reader; see the review below for
010, 021, 030, 044 and 049):
provenance the DWG has no place for: Layer / TextStyle `source_flags` (005, 006, group 70 words), `raw_plot_settings_codes` / `raw_dxf_codes` of Layout, SortEntitiesTable, TableStyle (007..009),
Table `dxf_insert_*` (019..021), TableStyle `cell_style_map_handle` and `modern_style` (010, 011), Spline `dxf_flags`, `planar`, `dwg_flags1`, `knot_parameterization` (014..018), Leader `hookline_enabled`, box size
(031..033), Shape name (034); legacy TABLE fields an R2010+ record lacks (022..028; 029 the cell value size; 030 the empty item of a covered cell); case or spelling the format resolves
(012 line type case, 041 / 042 `_BoxBlank`, 039 / 040 `ByBlock`, 013 gradient name LINEAR, 038 unstated `$DIMMZS`), header words (043 `$ACADMAINTVER`); ACIS empty bit (035, 036);
VPORT aspect ratio ulp (037); the numbering of anonymous blocks (044, 049 dynamic definition named, both judged together with the Insert rows that name the blocks); the classes (050 DATATABLE);
MLEADER line properties and background colour before R2010 (045..048). No rule accepts a missing object other than the anonymous / covered-cell / class pairs above, whose relations check the counterpart.
RC rules (`g5-rc-allowlist.json`): 025 / 026 BookColor of a DXF that states no colour (index 0 in D, 256 in the fork's file; cited ACadSharp source), 027 Spline `Flags1` (ACadSharp's DXF reader never sets it).
G4-494 (RC, G4) stays for the R14 sample only (its dictionary key is `STANDARD`).

### sample_V cells: why the 30 off-diagonal cells cannot pass, and what was done

AutoCAD's sample_W and sample_V are one drawing saved at two versions but not the same object set: sample_AC1024.dwg holds 84 objects sample_AC1018.dwg lacks and sample_AC1018.dwg holds 133 that sample_AC1024.dwg lacks
(objects only in W / only in V by handle: AC1014 to AC1032 387 / 42, AC1024 to AC1018 84 / 133, AC1027 to AC1032 26 / 11, AC1032 to AC1027 11 / 26). Every one of the 30 cells has rows of whole objects that only one of the two files
holds (9,393 of 14,318 unexplained rows, 66 %): no conversion of sample_W can create or remove them, and the rules never accept a missing object. So G5-RC sample_V as defined is
unreachable off the diagonal whatever the writer does; a definition that compares the objects both files hold (`common` rows) would be reachable in principle (4,932 rows now, 3 to 5 in the four nearest
cells: classes, `TABLECONTENT` counts, `KnotParametrization`). **Corrected in the review** (below): of the 9,393 rows only 4,158 are objects that sample_W lacks; 1,905 are objects both AutoCAD files
hold that the fork's file lacks (conversion losses), and a common-objects-only comparison would hide them. `Tools/g5-roundtrip.py` now records and reports that split (`sv_rows`, informational). Writer defects found and fixed: the R14
dictionary byte (the unstated block scalability was no defect: reverted in the review); found and **not** fixed: the pre-R2010 TABLE has table-level border colour / weight / visibility arrays that an R2010+ source never fills
(about 700 rows: `StyleOverride.*Border.Color / LineWeight`, AutoCAD's own down-save keeps the borders), the pre-R2010 -> R2010+ cell formats (FormattedValue, ValueDataType, TextHeight,
border override flags), entity transparency of pre-R2004 sources (AutoCAD states ByBlock for 37 lines the fork leaves ByLayer), material `ChannelFlags` (127 against AutoCAD's content-derived 63 / 17), class numbering and instance
counts (AutoCAD's order is its own), MLEADERSTYLE / TABLESTYLE dictionaries before R2007 (open owner decision), `KnotParametrization` of an R2010 spline converted to R2013+ (AutoCAD states 15 for a control-point spline; writing it
would need the inverse rule for DXF sources).

### Doubts and UNVERIFIED

* UNVERIFIED (no AutoCAD here): that AutoCAD opens the fork's R14 files with the hard-owner byte set (AutoCAD wrote it itself in its sample), the VX control object for a DXF source, the DIMSTYLE group 3 `<>` in DXF,
  `ACAD_TABLE` files, and the maintenance bytes (R5b-2).
* The Material reader default changes a pinned assertion (see the row); the DIMPOST writer change adds group 3 `<>` to every written DIMSTYLE whose suffix is `<>` (it was omitted).
* Several RR rules use broad relations (`input_type_output_const`: any number to a constant); each is limited by type, property, target version and source kind, and its reason names why the format has no place for the value
  (review: kept where the evidence is the format or a provenance carrier, so it covers every input value; 010 and 030 narrowed).
* G8 (speed / memory) was not re-run: the reader changes are five comparisons in the MATERIAL reader, one in the DIMSTYLE reader and one in the DWG MTEXT builder; the rest is writer-side.

### Review of R5b-3 (2026-10-23)

Clean build (`cargo clean -p acadrust`, feature build), full G5 (288 cells) and G4 (96 cells) on the reviewed tree; per cell the G5 and G4 statuses equal the agent's run
(`build.noindex/roundtrip-rs-dwg-agent3`, `roundtrip-rs-g4-agent3`): 0 cells differ.

* **Reverted: block scalability of an unstated record** (`object_writer/mod.rs` R2007+ BLOCK_HEADER byte). The claim "AutoCAD states scalable for an ordinary block"
  was false: AutoCAD states 0 (any scaling) for 23 of the 27 records of sample_AC1024.dwg / sample_AC1032.dwg (DXF group 281: 23 x 0, 4 x 1). The change gave every
  record of a pre-R2007 source byte 1 (uniform scaling only), which made the sample_V cells AC1014/AC1015/AC1018 -> AC1024+ worse (22 CanScale rows per cell instead of 4);
  the generated no-loss rule G5-RC-S085 (ACadSharp's default CanScale true for the R2004 source) accepted them. The agent's
  test is replaced by `a_block_that_never_stated_its_scaling_keeps_any_scaling_in_a_later_version` (fails with the agent's writer).
* **Narrowed: MTEXT default direction** (`dwg_document_builder.rs` `mtext_from_data`): the direction is dropped only for (1, 0, 0) on the normal (0, 0, 1). On another
  plane the default X axis of a DXF without group 11 is the plane's own, so dropping (1, 0, 0) lost the direction in a DWG -> DXF conversion. Test
  `an_mtext_on_another_plane_keeps_its_stated_x_axis` (fails with the agent's version).
* **Narrowed: R14 VX records** (`dwg_writer.rs` `prepare_legacy_document`): a drawing with its own VX table still gains a record for a viewport of the active paper space
  that lacks one (AutoCAD's sample_AC1014.dwg holds records exactly for the viewports of `*PAPER_SPACE`); the agent's version never added one, so a viewport added to an R14
  drawing got none. The agent's test now drops one record of each kind (`a_partial_vx_table_of_an_r14_drawing_is_filled_for_the_active_paper_space_only`, fails with the
  agent's version).
* Kept after checking against AutoCAD's files (C# dumps, no fork reader): DIMPOST (AutoCAD's R2000+ DXFs omit group 3 for the empty suffix, its R12 DXF states it empty;
  ACadSharp's default PostFix is empty and its writer always states group 3; no AutoCAD sample states `<>`), material map source (AutoCAD's DXFs omit 72 for the image file
  and state 77 / 171 / 175 = 0 for the current scene; its DWGs state 1 for the five maps, 0 for the refraction map), `$CMLSTYLE` (AutoCAD's DXFs: `$CMLSTYLE Standard`,
  dictionary key `Standard`, style name `STANDARD`), R14 dictionary byte and EED order (write back what was read; no data dropped: the same set of blocks is written).
  The four changed older assertions state AutoCAD's behaviour and are as strict as before; the materials test was renamed (its old name, "without changing native
  values", no longer held for the map sources) and now pins all five map sources and the refraction map.
* **RR rules** (CadKit `g5-rr-allowlist.json`): G5-RR-010 narrowed from "input null, output any" to the new relation `table_style_default_base_style` (exactly the base
  style the writer states, built from the input style's margins; self-tested); G5-RR-030 now requires the cell to be covered by a merged range of one row (evidence
  corrected: AutoCAD's sample_AC1032.dwg has no content in such cells, the two-row merge keeps its items; self-tested); reasons corrected for 021 (`dxf_insert_scale` is the
  carrier of ACadSharp's misreading of the AcDbTable group 41, the cell margin), 044 (the DWG record states the bare `*U`, ACadSharp DwgObjectReader.cs:686-687; the
  number lives in the BLOCK entity, and the fork's reader numbers by position by upstream design) and 049 (the stated anonymous bit is compared through `source_flags`).
  The other rules were checked against the format or the twins and kept; the "any value" relations that remain (raw DXF code lists, `dxf_insert_*`, legacy TABLE fields,
  Leader box size, MLEADER line properties before R2010, Shape name, `$ACADMAINTVER`) rest on the format or on a provenance carrier, so their evidence covers every input
  value. No cell was lost.
* **RC rules** G5-RC-025..027 kept: sample_AC1015_ascii.dxf DBCOLOR 996 states no colour (sample_AC1015.dwg: 42; BookColor.cs:70-71), ACadSharp's DXF reader never
  sets Flags1 (Spline.cs "only valid for dwg"; no Flags1 in IO/DXF); AutoCAD's own DWG states 9 where the DXF has fit-point bit 1024 but no fit points.
* **sample_V split corrected** (`g5-roundtrip.py` `sv_rows_split`): the agent counted every `<...>` row, class rows included, as "objects only one of AutoCAD's files
  holds". Split against the objects of sample_W (C# dump of the source), the 14,318 unexplained rows of the 30 cells are: unreachable 4,158 (objects sample_W lacks),
  **dropped 1,905** (objects both AutoCAD files hold that the fork's file lacks: in the 15 cells whose target is AC1014, AC1015 or AC1018,
  119 to 137 objects each: Materials, VisualStyles, their dictionaries and XRecords, MLeaderStyles; the writer writes VisualStyle and Material before R2007
  for same-version sources only, R5b-2), kept 3,105 (objects of sample_W that AutoCAD's conversion dropped), added 41, common 5,109 (class rows included). Every one of
  the 30 cells has unreachable rows, so the conclusion "cannot pass as defined" stands; but a common-objects-only gate would hide the dropped objects and is not sound
  without the `dropped` column.

## R5b-4: down-saves, the sample_V gate and conversion defects (2026-10-23)

Fork tree: r5a at 947c8f5 + this job's working-tree changes; CadKit `Tools/g5-roundtrip.py` (sample_V gate, `--rc-only`), `Tools/g4-compare.py` (`from_versions`),
`Tools/g4_relations.py` (four relations), `Tools/g5-rc-allowlist.json` (G5-RC-028..042). Definitions of R5b-2 / R5b-3 unchanged; the sample_V cell now follows the owner decision of
2026-10-23 (the rows of the objects both AutoCAD files hold are exact after the RC rules, none of those objects is dropped, none that neither holds is added; objects only one file holds are reported).

### Counts (clean build of the final tree, all 288 cells with the C# columns; before = R5b-3 review)

| | before | after |
|---|---|---|
| G5-RR (288) | 41 (same-version 41 / 42, cross-version 0 / 246) | 41 (41 / 42, 0 / 246): no cross-version cell is close, see the families |
| G5-RC same-version (42) | 42 | **42** |
| G5-RC sample_V (36) | 6 (diagonal) | **15** (6 diagonal + 9 `reported`); per version AC1014 3, AC1015 2, AC1018 1, AC1024 3, AC1027 3, AC1032 3 of 6 (1 each before) |
| sample_V rows of the 30 cross cells | unreachable 4,158, dropped 1,905, kept 3,105, added 41, common 5,109 | unreachable 4,165, **dropped 0**, kept 3,628, **added 0**, **common 1,365** (21 cells still have common rows) |
| G5-idem (288) | 283 (5 AecObjects refusals) | 283 |
| C# -> fork | 204 | 204 (reader unchanged) |
| G4 | RR 94, RC 94, idem 96 / 96 | unchanged (only AecObjects fails) |
| G1 | 45 / 49 | 45 / 49 |

Rule subsets of the final run (rows files): RR no rules 20, hand rules (the whole RR list) 41; RC same-version no rules 0, pinned-field rules 21, all hand rules 21, all rules 42; RC sample_V no rules 0, pinned-field rules 6,
all hand rules 6, all rules (hand + generated no-loss / twin rules) 15. The nine cross cells pass only with the generated `equals_source` rules next to the new hand rules.
The kept / unreachable growth (3,105 to 3,628 kept) is the objects AutoCAD's own conversion dropped that the fork now keeps (the style dictionaries and their objects among them): reported, not gated.

### The 21 sample_V cells that still fail (common rows; no cell has dropped or added objects)

18 cells have TABLE rows (sample_AC1014 / 1015 / 1018 to AC1024+ and AC1024+ to AC1014 / 1015 / 1018): 975 rows left of 2,316. Legacy to modern: `#A35` date / block cell formats (ValueDataType 2, `%tc1`,
BlockScale), border override flags (AutoCAD 63 / 37 for applied edges, the fork 26), row cell styles of an R14 source (margins, double line spacing). Modern to legacy: the text height AutoCAD states for a cell without
override is the row height of its legacy style (0.1425 title, 0.1026 data = 0.57 x the style text height) which a modern table does not state, the right edge of a last-column cell comes from the inside-horizontal
border (edge flags 11 against 9). Three non-table cells (sample_AC1014 to AC1015 / AC1018, sample_AC1015 to AC1018) fail on information the older file cannot hold: Hatch gradient (R2000 file: ACI approximation only), BookColor RGB,
viewport status words of R14, BlockRecord preview / block description / CanScale (R2007+ fields), Solid3D ACIS revision, header MeasurementUnits. Other leftovers: class InstanceCount of CELLSTYLEMAP / DICTIONARYVAR / DATATABLE
(AutoCAD counts what the dump does not enumerate; census counts written), MultiLeaderStyle TextTopAttachment (R2010 field AutoCAD states 9), Leader TextHeight.

### Rules (CadKit g5-rc-allowlist.json; `from_versions` = version of sample_W, evidence the format or AutoCAD's own files, never the fork's reader)

G5-RC-028..033 entity Transparency (Line, Point, MText, Solid, Insert, Arc) exp 100 act 0, source R14 / R2000, target R2004+: 820 rows in 8 cells (a `reported` cell needs no rescue); R2000 entity records hold no transparency, ACadSharp reads the fork's ByBlock colour word without transparency as opaque (DwgStreamReaderAC18.cs:111-115).
G5-RC-034 TABLECONTENT / TABLEGEOMETRY InstanceCount (the two classes only; the id 035 is unused) equals the number of TableEntity objects the fork holds (AutoCAD counts 9, 7, 6, 5 in its four files of the two tables).
G5-RC-036 / 037 added CadDictionary / XRecord of a visual or table style whose counterpart in the expected file has an extension dictionary (new handles), 038 VisualStyle.XDictionary both set, 042 entry handles of such a dictionary (relations `added_visual_style_record`, `both_set`, `entry_renumbered_in_style_dictionary`): 297 added rows, source R2010+, target R14 to R2004.
G5-RC-039 / 040 Spline Flags1 0 to 1 and KnotParametrization 15 to 0: a fit-point spline of an R2013+ source is held as control points by AutoCAD's R14 to R2010 files (4 control points, 8 knots), the fork writes the fit points (DwgObjectReader.cs:5663-5670).
G5-RC-041 HandleSeed exceeds every handle the file holds (relation `seed_exceeds_objects`; AutoCAD's seeds 4807, 4343, 4127, 3828, 3828, 3684 of one drawing).
Rescued cells: the nine `reported` cross cells need these plus the generated rules. No rule accepts a missing object. `Tools/g4-compare.py` `reject_ctx` examples now inherit the rule's target version, cell kind and source version (a rejection test was vacuous for rules that name them).

### Changed older test expectations (each as strict as before)

* `acadsharp_viewports::dwg_version_controls_lighting_grid_and_full_status_projection`: the grid bits are stored at R2000 / R2004 too (13), as AutoCAD's sample_AC1015.dwg / sample_AC1018.dwg state them (viewport #240 status 0xC8020 in both, as in sample_AC1032.dwg).
* `r5b_dwg_roundtrip::viewport_grid_bits_of_a_same_version_source_are_kept` (a document of no file states 0b0011 before R2007 as well), `r5b2_dwg_roundtrip::a_dxf_source_keeps_the_grid_bits_of_its_viewport_before_r2007` (a constructed document keeps its bits): same evidence.
* `r5b_dwg_roundtrip::style_dictionaries_of_a_same_version_source_are_kept`: the down-save keeps the table style (sample_AC1015.dwg holds ACAD_TABLESTYLE).
* `dwg_writer::tests::test_pre_r2013_write_uses_legacy_document_profile` and `roundtrip::dwg_roundtrip_deep_r2000`: 17 objects at R2000 instead of 13 (the table style and multileader style dictionaries and their two styles; same evidence).

### Tests

`tests/r5b4_downsaves.rs` (3) and `tests/r5b4_conversions.rs` (10), each checked to fail with its fix neutralised one by one (16 neutralisations: materials, spline, dimfit, osmode, viewport xdata, zombie, legacy layout, row height, merge covered, visual style at every version, at R2010, grid bits, VPORT grid, class table, material objects, table style dictionary).
`cargo test --features acadsharp-compat --no-fail-fast`: 2,561 passed, 0 failed, 60 ignored; 0 warnings in `cargo build --lib` and the feature build.

### Cross-version G5-RR families (210,893 unexplained rows in the final run, by record type VisualStyle 53,628, Table 48,318, XRecord 33,695, doc 21,804 (classes, header), Dictionary 6,452, Line 5,897; not closed)

Largest, by rows (cells): VisualStyle `properties[]` 41,040 (26; the R2010 layout holds 28 of the 58 properties: AutoCAD's own R2010 record keeps none of the later ones either), XRecord `entries[]` 27,216 (3; the regenerated visual-style
record has more entries than the R2010 source's), class `instance_count` 4,440 (105; counts of AutoCAD's own classes follow the objects it holds), XRecord `cloning_flags` 3,224 (21) and Dictionary `duplicate_cloning` 3,008 (22) (R14 records
have neither: ODA specification), R12 sources (AC1009) linetype / style / block spelling 3,140 + 2,928 + 1,140, Line / Point / MText transparency 3,300 and Line line_weight 1,593 (R2000 and R14 entities hold none), Table cell fields (legacy to modern and back:
geometry, custom data, formats, borders: 48,318 rows), Material `channel_flags` 99 per cell pair (new: the derived flags against the native default 127). The families whose cause is the target format (cloning flags, transparency, line weight, geometry, custom data of a legacy table) are
candidates for hand rules citing the format; none was written (no cell would pass: every cross cell has thousands of rows from the table and visual style families).

### Doubts and UNVERIFIED

* UNVERIFIED (no AutoCAD here): AutoCAD opening the fork's R14 / R2000 / R2004 files with the kept dictionaries, the derived OSMODE / DIMFIT / MVIEW words, the legacy table cell words (0x40000 and the visibility bits 0x4000.. are not written), or the added round-trip records.
* The legacy table conversion is derived from the two tables of one sample: the row-height rule and the border-edge mapping fit 21 + 12 cells; the edge flags of a last-column cell (11) do not follow it.
* G5 `reported` is a gate decision of the owner: a cell passes while objects only one AutoCAD file holds remain; the 4,165 unreachable and 3,628 kept rows are listed per cell in the report.
* G8 (speed / memory) was not re-run: reader hot paths are untouched; the writer additions run once per document or per cell of a down-saved table.

### Review of R5b-4 (2026-10-24)

* **Write determinism (found by the gate speed-up job).** The classes the writer appends for a DXF source written at another version below
  R2013 followed the object map's iteration order, which changes with every map and process: 88 of the 288 G5 cells (the cross-version cells
  of the DXF sources) wrote different bytes on repeated `write-dwg --pin` runs. Fixed (`objects_in_handle_order`, behaviour row above); all 288
  cells now write identical bytes on 3 runs each; test `r5b4_write_determinism` (fails without the fix). No gate verdict was flaky: those 88
  cells are G5-RR mismatches and G5-idem matches in every run (only their class rows varied), and no RC check covers them.
* **The no-loss rules hid conversion losses.** `equals_source` accepted the ExtendedData rows where AutoCAD's own down-save keeps, per entity, what
  the version's records cannot hold: RTTcAl (the R2004 true colour: 36 entities in sample_AC1014.dwg / sample_AC1015.dwg), RTLwPs (the line
  weight at R14: 75 entities in sample_AC1014.dwg) and RTMaterial (the material before R2007). The fork writes the ACI colour and drops the rest,
  so the true colours, R14 line weights and pre-R2007 material assignments are lost where AutoCAD keeps them. The relation no longer accepts an
  ExtendedData row where AutoCAD's file holds such a record that the fork's file lacks (CadKit `Tools/g4_relations.py`, g5 self-test). Cells
  lost: sample_AC1015 -> AC1014, sample_AC1018 -> AC1014, sample_AC1018 -> AC1015 (`reported` -> mismatch). Open: write the round-trip records.
* **sample_V gate.** A fork object under a handle of sample_W was `kept` whatever its type; it is `kept` only under sample_W's type, otherwise
  `added` (gated; g5 self-test case).
* **Corrected evidence.** OSMODE: AutoCAD's own R14 file states 39 (its session value); 4133 is AutoCAD's initial value and ACadSharp's default.
  SPLINE: sample_AC1027.dwg states 15 for the control-point spline #433 only (#434 is a fit spline there). The R2000 / R2004 grid bits test note
  named R2015 / R2018.
* **Kept as the agent wrote them (checked):** the down-saves (the fork's R14 / R2000 / R2004 files of sample_AC1032.dwg hold every object type
  AutoCAD's files hold, the same class table in number and name, no unknown objects; only AutoCAD's own round-trip dictionaries and records
  are missing); the class table of a DWG source and its profile classes (the six C# dumps); no zombie classes for a DWG source; viewport grid
  bits from R2000 (status 0xC8020 / 0xC8260 in sample_AC1015.dwg and sample_AC1018.dwg); VPORT grid flags 3; material channel flags (a stated
  R2007+ value is never replaced: the writer only replaces the 127 default of a record read from a pre-R2007 DWG, which states none; test added);
  the visual-style round-trip records (252 entries per style in the R14 to R2004 files, 126 in R2010); the MVIEW words. A reader-side variant
  (the reader stating the derived channel flags, 15 and 3 for pre-R2007 / pre-R2013 records) was tried and dropped: it changes the native round
  trip of in-memory documents through the old versions (`roundtrip::dwg_roundtrip_deep_r2000`: a Chord control-point spline came back as 15).
  The writer conversions leave cross-version G5-RR rows (Material channel_flags, Spline knot_parameterization, VPort grid_flags,
  object_snap_mode, dimfit) where the source format holds no such field: candidates for hand RR rules citing the format, none written.
* Counts (clean build, `--jobs 3`, 288 cells): G5-RR 41 (same-version 41 / 42, cross 0 / 246; no rules 20); RC same-version 42 / 42 (no rules 0,
  pin 21, hand 21); RC sample_V 12 / 36 (6 diagonal + 6 `reported`; no rules 0, pin 6, hand 6, all 12); idem 283; C# -> fork 204. G4 RR 94,
  RC 94, idem 96 / 96 (no per-cell change). G1 45 / 49. Tests 2,562 passed, 0 failed, 60 ignored.


## R5b-5: AutoCAD's round-trip records, TABLE conversion and cross-version rules (2026-10-24)

Fork tree: r5a at e91457e + this job's working-tree changes; CadKit `Tools/g4_relations.py` (`both_integers`, `extra_class_named`), `Tools/g4-native-compare.py` (`input_any_output_const`,
`input_const_output_int`, `input_const_output_positive`, `integer_to_integer`), `Tools/g5-rc-allowlist.json` (G5-RC-043..046), `Tools/g5-rr-allowlist.json` (G5-RR-051..).

### Counts (final run: all 288 cells with the C# columns; clean rebuild; before = R5b-4 review)

| | before | after |
|---|---|---|
| G5-RR (288) | 41 (same-version 41 / 42, cross-version 0 / 246) | 41 (41 / 42, 0 / 246): see the families |
| G5-RC same-version (42) | 42 | **42** (no rules 0, pin rules 21, hand rules 21, all 42) |
| G5-RC sample_V (36) | 12 (6 diagonal + 6 reported) | **23** (6 diagonal + 17 reported; per version AC1014 3, AC1015 4, AC1018 4, AC1024 4, AC1027 4, AC1032 4 of 6); no rules 0, pin 6, hand 6, all 23 |
| G5-idem (288) | 283 (5 AecObjects refusals) | 283 |
| C# -> fork | 204 | 204 |
| G4 (96 cells) | RR 94, RC 94, idem 96 / 96 | unchanged (only AecObjects fails) |
| G1 | 45 / 49 | 45 / 49 (rs-gap.tsv identical) |

The 17 reported cells pass with the generated `equals_source` rules next to the hand rules, as before; the hand rules alone rescue none of the cross cells. Rescued by this job (11 cells): AC1015 -> AC1014, AC1018 -> AC1014 and AC1018 -> AC1015 (round-trip records and MTEXT blocks), AC1024 / AC1027 -> AC1015 (TABLE down-save, leader extents),
AC1018 -> AC1024 / AC1027 / AC1032 (TABLE up-conversion from the round-trip content, MLEADERSTYLE attachments, G5-RC-043 / 044 / 046) and AC1024 / AC1027 / AC1032 -> AC1018 (G5-RC-044 / 045). AC1032 -> AC1015 / AC1014 / AC1018
need the MTEXT column records; AC1024 / AC1027 -> AC1014 the R14 BlockRecord data; the 13 cells with an R14 or R2000 source other than those above fail on Hatch gradient, BookColor, viewport, ACIS and header data the older file cannot hold.

### Rules

* G5-RC-043 BlockRecord.CanScale true -> false (source R14 to R2004, target R2010+): the scaling byte exists from R2007 only; AutoCAD's later files state 1 for 4 of 27 blocks, no older file states why.
* G5-RC-044 DxfClass.InstanceCount, two integers: the count follows the objects, which the cell reports as `kept` / `unreachable`.
* G5-RC-045 DxfClass.DwgVersion 23 -> 0 (DATATABLE written as R2004): a version stamp, no data.
* G5-RC-046 an extra class DATATABLE in the fork's file (R2004 source, R2010+ target): its DataTable objects are `kept`.
* G5-RR-051..: format-inherent differences of cross-version cells, each citing AutoCAD's own files: the referenced bit (64) of symbol table records and `xref_reference` (7 rules), class instance counts, material channel flags (both
  directions), spline knot parameterization 0 -> 15, MLEADERSTYLE attachments (both directions), leader annotation box (4 rules), TOLERANCE derived gap and height, VPORT grid bits, maintenance version, DIMALTMZS / DIMMZS, the
  document version of a cell between two versions (6 rules), entity transparency below R2004 (6 entity types), layer material and plot style, DIMMZF / DIMALTMZF, DIMFIT (both directions), TEXT alignment point.
  Rows left after them: the cross-version RR rows are 214,245 in 242 cells (R5b-4 review: 210,893 in 246 cells; the TABLE family grew from 48,318 to 62,082 rows because the cells of a legacy table are compared with the restored
  R2010 cells AutoCAD's up-conversion produces; VisualStyle 53,628, XRecord 33,695, doc 18,584 (R5b-4: 21,804), Dictionary 6,452). No cross-version RR cell passes.

### Doubts and UNVERIFIED

* UNVERIFIED (no AutoCAD here): AutoCAD opening the fork's R14 / R2000 / R2004 files with the `RT*` records, the MTEXT blocks and the named-colour identity; the order `RTLwPs`, `RTTcAl`, `RTMaterial` when an entity holds more than one kind (the samples
  show `RTLwPs` first; no entity holds `RTTcAl` and `RTMaterial` together); an `RTLwPs` record for the default line weight (the samples have none); `RTLwPs` bits 5-6 (all plot style flags are 0 in the samples).
* The date of a round-trip cell is the day nearest to its UTC seconds (local midnight within twelve hours of UTC; one cell).
* Entities of a class (type code 500 and up) get no `RT*` records because the one such entity of the samples (PDF underlay) has none: the generalisation to other class entities is unverified.
* The round-trip content of a table is used when the plain numbers and strings of the legacy cells still match it; AutoCAD checks a checksum of its own. A typed cell with a format is accepted without a check.
* Not done: MTEXT columns (R2010 xdata <-> R2013 native; 4 MTEXT of the samples), the 29th+ visual-style properties of R2010, DIMFIT against DIMTMOVE 1 / 2 (no sample has a TMOVE other than 0), R14 BlockRecord `CONTENTBLOCKDESCRIPTION` / `CONTENTBLOCKICON` data
  (the apps carry handles AutoCAD created), the Hatch gradient records for R14 / R2000 sources, the TABLE round-trip objects in the fork's own down-saves (the native model of a legacy table stays the legacy layout: cross-version RR
  of tables is open).
* G8 was not re-run: the reader additions run only for files below R2007 (`Restorer::new` is `None` otherwise) and the writer additions only below R2007 / R2010.

### Review of R5b-5 (2026-10-24)

* **The no-loss rules hid conversions again; sample_V is 6 / 36, not 23.** `equals_source` accepted, in all 17 `reported` cells (and in the six
  AC1024 / AC1027 / AC1032 cells the R5b-4 review counted), rows where AutoCAD converts and the fork does not: the extended data AutoCAD's
  down-saves add and its up-saves consume (sample_AC1032.dwg -> AC1018: ACAD_MTEXT_COLUMN_INFO of the MTEXT columns, the attribute lock record,
  ACAD_MLEADERVER, ACAD_DSTYLE_DIMTEXT_FILL; -> AC1014: the R14 DSTYLE overrides of dimensions and leaders, block records' data; sample_AC1018.dwg
  -> AC1024+: the fork keeps those records and leaves DIMTFILLCLR, the dimension line types and the attribute lock at their defaults where AutoCAD
  restores them; sample_AC1027.dwg -> AC1024: the spline and solid records of R2010), and the modeler data of every down-save (the fork's R14
  file holds the SAT 400 of an R2000 source or the binary ACIS of an R2004 one where AutoCAD writes SAT 106; its R2000 / R2004 files of an R2018
  source hold ASM data). CadKit `Tools/g4_relations.py` `source_row_excluded`: `equals_source` no longer accepts the contents of ExtendedData
  (only the renumbered application handle `ExtendedData[i][0]`) nor AcisData / IsBinaryAcisData in a down-save; `Tools/g5-twin-rules.py` skips
  the same rows (`g5-rc-source-allowlist.json` regenerated: 288 rules, the 20 ExtendedData-only families gone); g5 self-test cases. Each of the
  17 cells now fails on such rows only (no other column changed). Open: the round-trip records of MTEXT columns, attributes, multileaders,
  dimension styles, splines and solids both ways, and the ACIS / ASM version of a down-save.
* **Rules.** G5-RC-044 (any two integers for any class count) split: DICTIONARYVAR by `equals_object_count` (the count equals the fork's
  DictionaryVariable objects), and G5-RC-047 `both_integers` for ACDB_TEXTOBJECTCONTEXTDATA_CLASS, CELLSTYLEMAP and DATATABLE only (objects the
  dumps do not enumerate; `both_integers` now requires the class list). G5-RR-058 (any two integers for every class count of every cross cell)
  deleted, its relation `integer_to_integer` removed: an arbitrary count (1,740 rows, no cell). G5-RR-091 / 094 (TEXT alignment point: any value
  from none, at every version) narrowed to exactly none <-> (0, 0, 0) between an R14 file and a later source. Other new rules checked against
  the samples and kept: the referenced bit and xref flags (051-057), channel flags (059 / 060), knot parameterization 0 -> 15 (061), leader box
  (066-069), TOLERANCE gap and height (070 / 071), grid bits (072), maintenance version (073), DIMALTMZS (074), the document version (075-080),
  transparency below R2004 (081-086; AutoCAD keeps only the 0x2000 flag), layer material (087) and plot style (088), DIMMZF / DIMALTMZF
  (089 / 090) and DIMFIT (092 / 093): AutoCAD's older files keep none of these values in extended data (their layers carry AcAecLayerStandard
  and AcCmTransparency only; their dimension styles ACAD_DSTYLE_DIMJAG / DIMTALN / DIMBREAK / DIMTEXT_FILL / DIM linetypes, no DIMMZF).
  MLEADERSTYLE 062-065: evidence corrected (behaviour row).
* **Verified.** The `RT*` records the fork writes from sample_AC1018.dwg and sample_AC1032.dwg at R14 / R2000 / R2004 equal AutoCAD's at every
  handle both files hold (C# values of the word, the packed colour, the identity, the line-weight word and the material handle; the only
  differing MTEXT records are those with columns); "72 / 72 ACI choices equal `nearest_aci`" is true (sample_AC1014.dwg and sample_AC1015.dwg,
  and every entity record states the same index); the MTEXT blocks equal AutoCAD's; the leader box is the MTEXT extents; the date conversion is
  free of the machine's time zone (pure arithmetic; the unit test fixes UTC seconds).
* **The TABLE RR family** grew by 13.8k rows because the cross cells now compare the source's legacy model with the modern cells the up-conversion
  takes from AutoCAD's round-trip objects (+1,091 rows in each of the nine legacy -> R2010+ cells of the six samples) and the down-conversion
  states row text heights, edges and block scales (+163 in each of the nine R2010+ -> legacy cells): a different representation of one table,
  not a loss of the fork's own round trip (same-version cells unchanged).
* **Changed older tests** follow AutoCAD's files and are as strict: R14 line weights restored (`acadsharp_common_appearance`), R13 / R14 MTEXT
  spacing restored (`entity_audit_regressions`; R13 has no sample), the R2000 MTEXT fill in ACAD_MTEXT_BBRT (`r5a_tests`, sample_AC1015.dwg).
  `acad_dump_write_dwg` had lost its named colour without a value: restored (with the valued one), and the tool drops only such a name.
* **New tests fail without their fix** (checked by reverting the 102 marker stop and the last-column edge: two tests of `r5b5_conversions`
  fail); `a_document_without_such_values_gets_no_record` and `an_mtext_without_the_block_in_its_source_is_written_without_it` are guards that
  passed before the job too.
* Counts (clean build, `--jobs 3`, 288 cells): G5-RR 41 / 288 (same-version 41 / 42, cross 0 / 246; no rules 20; unexplained cross rows
  215,949); RC same-version 42 / 42 (no rules 0, pin 21, hand 21, all 42); RC sample_V 6 / 36 (diagonal; up-saves 0 / 15, down-saves 0 / 15; no
  rules 0, pin 6, hand 6, all 6); idem 283 / 288; C# -> fork 204. G4 RR 94, RC 94, idem 96 / 96 (no per-cell change from the R5b-4 review).
  G1 45 / 49 (rs-gap.tsv identical). Tests 2,583 passed, 0 failed, 60 ignored. G8 not run: the review changes no reader hot path (one dictionary
  lookup per colour-book object in `Restorer::new`, a filter in `resolve_book_colors`); the job's own reader passes (one pass over the objects
  and entities of an R14 to R2004 file) are UNVERIFIED under G8.


## R5b-6: AutoCAD's other round-trip records, modeler data in down-saves and TABLE generality (2026-10-24)

Fork tree: r5a at 62c986d + this job's working-tree changes; CadKit `Tools/g4_relations.py` (`application_renumbered`, `acis_equal_outside_date`, `application_registered_elsewhere`), `Tools/g5-rc-allowlist.json` (G5-RC-048..055).
Baseline = the R5b-5 review tree (62c986d; `build.noindex/rv5-g5` for the families, `target.noindex/g5-base` for the 36 AutoCAD-sample cells), final = this tree (clean rebuild, `--jobs 3`, all 288 cells, binary d60ec3ada951; a last reordering of one object scan in `restore_document`, no behaviour change, came after the G5 run).

### Counts (288 cells)

| | before (R5b-5 review) | after |
|---|---|---|
| G5-RR | 41 (same-version 41 / 42, up 0 / 59, down 0 / 187 incl. 5 AecObjects refusals); no rules 20; 215,953 unexplained cross rows (down 163,735, up 52,214, same 4) | **41** (same 41 / 42, up 0 / 59, down 0 / 187); no rules 20; **213,202** unexplained rows (down 160,439, up 52,759, same 4) |
| G5-RC same-version (42) | 42 (no rules 0, pin 21, hand 21, all 42) | **42** (no rules 0, pin 21, hand 21, all 42) |
| G5-RC sample_V (36) | 6 (diagonal; no rules 0, pin 6, hand 6, all 6) | **6** (diagonal; no rules 0, pin 6, hand 6, all 6) |
| G5-idem | 283 (5 AecObjects refusals) | 283 |
| C# -> fork | 204 | 204 |
| G4 (96 cells) | RR 94, RC 94, idem 96 / 96 | RR 94, RC 94, idem 96 / 96 (only AecObjects fails) |
| G1 | 45 / 49 | 45 / 49 (rs-gap.tsv identical) |

Per version, G5-RR / RC same-version / RC sample_V / idem / C# -> fork are the same as before in every row: AC1014 1/48, 1/1, 1/6, 47/48, 0/48; AC1015 3/48, 3/3, 1/6, 47/48, 25/48; AC1018 3/48, 3/3, 1/6, 47/48, 44/48; AC1024 3/48, 3/3, 1/6, 47/48, 45/48; AC1027 3/48, 3/3, 1/6, 47/48, 45/48; AC1032 28/48, 29/29, 1/6, 48/48, 45/48.

**No off-diagonal sample_V cell passes** (the 30 cells fail on the open items below), but the rows on the objects both AutoCAD files hold (`common`) fell from 2,603 to **863** in the 30 cells; `dropped` and `added` are 0 in every cell (the AppId records of the conversions are `added` until G5-RC-055 names them); `unreachable` 4,165 and `kept` 3,622 (3,628 before) are reported only.
Common rows per cell, before -> after (source -> target): AC1014 -> 1015 37 -> 37, 1018 71 -> 43, 1024 254 -> 166, 1027 262 -> 168, 1032 323 -> 179; AC1015 -> 1014 15 -> 15, 1018 29 -> 6, 1024 94 -> 7, 1027 102 -> 9, 1032 163 -> 20; AC1018 -> 1014 18 -> 18, 1015 6 -> 6, 1024 66 -> 2, 1027 74 -> 4, 1032 132 -> 12; AC1024 -> 1014 76 -> 21, 1015 64 -> 9, 1018 61 -> 3, 1027 8 -> 2, 1032 66 -> 10; AC1027 -> 1014 84 -> 23, 1015 72 -> 11, 1018 69 -> 5, 1024 11 -> 3, 1032 58 -> 8; AC1032 -> 1014 91 -> 28, 1015 79 -> 16, 1018 82 -> 13, 1024 72 -> 11, 1027 64 -> 8.

### What still fails in the off-diagonal sample_V cells (every row is on an open item; no cell is left with a row the job could close)

* Annotative text (`TextEntity`, 2 rows in 18 cells; the only row left in AC1018 -> AC1024 and AC1024 -> AC1018): sample_AC1018.dwg states the annotative texts #3BA and #757 on the layers "0 @ 1" (`AcadAnnotativeDecomposition`) with `AnnotativeData {1, 0}`, AutoCAD's R2010 file on layer 0 with `{1, 1}` and the scale contexts: AutoCAD decomposes annotative objects per scale for R2007 and merges them again. Not mechanical (layer naming, scale contexts); open.
* Spline (`ACAD_SPLINE_DATA {100 0, 101 0, 102 2}`, 2 rows in 16 cells; AC1024 -> AC1027 and AC1027 -> AC1024 fail on it alone, with the application count): R2013 stores the spline #434 as fit points (2 points, tangents = 3 x the end differences of the control polygon, knot parameterization 2, flags 9), R2010 as the 4 control points of the NURBS; the record says "fit spline, parameterization 2". The conversion is a spline fit / evaluation, not a record: open.
* MText columns (`ACAD_MTEXT_COLUMN_INFO`, 8 rows in 14 cells; AC1027 -> AC1032 fails on it alone): the R2018 record states the column heights AutoCAD computes from the text layout (-2.94, -23.73, -3, -204.35 for the four MTEXT whose R2010 record states height 0); not derivable, so the record stays in an R2018 file (stale, not converted).
* Modeler data to R2000 / R14 (`AcisData`, 1-2 rows per body): SAT 400 (sample_AC1015.dwg) and SAT 106 (sample_AC1014.dwg) have other record layouts than the binary save versions (`body $-1 $1 $-1 $2 #` against six fields, `shell`, `face`, `transform ... no_rotate no_reflect no_shear` against `1 0 0 0`, `forward_v` against `0`): a modeler-level conversion, not a header change. The fork writes binary data into those files as before. R2000 -> R2004 (text SAT 400 to the 20800 layout) likewise.
* R14 (5 + 5 cells): the DSTYLE overrides of dimensions and leaders (R14 states `270 DIMUNIT`, block names `6 / 7 ARCHTICK`, `271 0` on angular dimensions, `5 ""` on a leader; R2000 states handles `343 / 344` and none of the others), the block record's `CONTENTBLOCKDESCRIPTION` / `CONTENTBLOCKICON` (description and preview of R2000 blocks), the `MVIEW` extended data and the status / UCS icon of viewports, `DIMBLK` of the header, the key spelling of colour-book entries (`RAL_CLASSIC$RAL_1006`), TABLE rows 121 in the three R14 -> R2010+ cells, MEASUREMENT.
* DXF class counts (`DICTIONARYVAR`, `CELLSTYLEMAP`, the extra `DATATABLE`): unchanged R5b-5 open items.

### Full list of the round-trip records AutoCAD's conversions add or consume (the six AutoCAD DWGs and their DXF twins; status of this job)

| record | holder | present | what AutoCAD does | status |
|---|---|---|---|---|
| `RTTcAl`, `RTLwPs`, `RTMaterial` | entities | before R2004 / R14 / R2007 | native values from there | R5b-5 |
| `MTEXTBEGIN`, `ACAD_MTEXT_BBRT` | MTEXT | before R2004 | native from R2004 | R5b-5 |
| `AcDbAttr {0, 1}` | ATTDEF, ATTRIB | before R2007 (DXF R12 to R2004 too) | lock native from R2007 | **done**, both ways |
| `ACAD_DSTYLE_DIMTEXT_FILL`, `..._DIM_LINETYPE`, `..._EXT1_LINETYPE`, `..._EXT2_LINETYPE` | DIMSTYLE | before R2007 | DIMTFILL / DIMTFILLCLR / line types native from R2007 | **done**, both ways |
| `ACAD_OBJECT_CHANGE_GUID` | 3DSOLID, REGION (BODY by the same shape) | before R2013 | GUID in the modeler data (`AcisRevision`) from R2013 | **done**, both ways |
| `ACAD_MLEADERVER {2}` | MULTILEADER, MLEADERSTYLE | before R2010 | class version native from R2010 | **done**, both ways |
| `ACAD {AcDbSavedByObjectVersion 0}` | VISUALSTYLE | before R2018 (DWG only) | none | **done** (stamp) |
| `ACAD {DBColXD index colour book$name}` | DBCOLOR | before R2018 (DWG; DXF from R2004) | colour and names in the object from R2004 | **done** |
| `ACAD {DesignCenter Data}` | BLOCK_RECORD | before R2007 | units native from R2007 | stale record no longer written into R2007+ (read side existed; review: only for a drawing read before R2007) |
| `GradientColor1ACI`, `GradientColor2ACI`, `ACAD_XREC_ROUNDTRIP` | HATCH | R14, R2000 | gradient native from R2004: true colours, default name `LINEAR` | **done** (colours from the record, name) |
| `ACAD_MTEXT_COLUMN_INFO` | MTEXT | R2000 to R2013 | native columns from R2018 | open (layout-computed heights) |
| `ACAD_SPLINE_DATA` | SPLINE | up to R2010 | fit data native from R2013 | open (geometry) |
| `AcadAnnotative`, `AcadAnnotativeDecomposition` and the layers "L @ scale" | TEXT, layers | up to R2007 | merged for R2010 | open |
| DSTYLE of dimensions / leaders, `CONTENTBLOCKDESCRIPTION`, `CONTENTBLOCKICON`, `MVIEW` | R14 | R14 | other forms from R2000 | open |
| modeler data | 3DSOLID, REGION, BODY | every version | save version of the release | R2004 to R2018 re-versioned; SAT 400 / 106 open |

### Fixes (R = reader, W = writer, V = ACadSharp view)

* `src/io/dwg/autocad_conversions.rs` (new): R `restore_document` (called by `dwg_reader.rs` and `dxf/reader.rs` after the block units): native values from the records, records taken out and kept with their positions in `CadDocument::consumed_xdata` (`document.rs`; not serialized, not in the native dump or the semantic inventory); W `prepare_for_write` (called by `dwg_writer.rs` and `dxf/writer/mod.rs`, also for the native R12 path) and `restore_book_colors`; `dwg_writer.rs` calls them after `prepare_roundtrip_records` / before `prepare_book_colors`. V `compat/acadsharp/xdata.rs` puts the records back (`consumed`), `entities.rs` (`legacy_lock`) and `table_dimstyle.rs` hide the restored lock, fill and line types of a source before R2007.
* W `entities/acis/reversion.rs` + `AcisData::sab_for_version` (`solid3d.rs`), used by `object_writer/entities.rs` (inline data, history, extra data, AcDs entries) and the DXF `queue_sab_data`.
* R `dwg_reader.rs` `gradient_record_colors`; W `object_writer/entities.rs` `gradient_name`.
* `entities/table.rs` `cell_text_checksum`, `with_roundtrip_content`.

### Rules (CadKit Tools/g5-rc-allowlist.json; evidence AutoCAD's own files, never the fork's reader)

* G5-RC-048..052 `application_renumbered` for AttributeDefinition, AttributeEntity, DimensionStyle, Solid3D, Region (`ExtendedData[i][0]`): both handles are AppId records of the same name in their own dump; the application handle of a record the conversion writes is numbered by the file (AcDbAttr #10BD in sample_AC1015.dwg, #FEB in sample_AC1018.dwg; DIMTEXT_FILL #10C0 / #FEC; OBJECT_CHANGE_GUID #10E7 / #1011). Not data: the contents of the record are not accepted by it. Rescues no cell by itself (the cells fail on the open items); rows accepted in the final run: 45 (AttributeDefinition, 9 cells), 36 (AttributeEntity, 9), 36 (DimensionStyle, 9), 16 (Solid3D, 8), 8 (Region, 8).
* G5-RC-053 / 054 `acis_equal_outside_date` for Solid3D / Region `AcisData`: the same bytes except the date in the header (sample_AC1018.dwg and sample_AC1021.dwg `Tue Dec  2 10:27:20 2025`, sample_AC1024.dwg and sample_AC1027.dwg `...:19`, sample_AC1032.dwg `Wed Sep 24 12:29:03 2025`): the moment of the save. Targets R2004, R2010, R2013 from newer sources only; accepted in the final run: 10 rows (Solid3D, 5 cells), 5 (Region, 5).
* G5-RC-055 `application_registered_elsewhere` for AppId objects (`<object>`): an AppId the fork's file registers under its own handle where the expected file has one of that name (these were `added`, a gated category; 59 rows in 11 cells in the final run).
* No RR rule was added. The cross-version RR rows these conversions change are not data the target cannot hold: AutoCAD's own conversion adds / consumes them, so the rows that remain are the native model's representation (see the families); they stay unexplained.

### RR families, cross-version cells (unexplained rows, before -> after)

down: ExtendedData 2,376 -> 360 (the stamps, locks, fills, GUIDs are no longer records of the model), Solid3D 878 -> 246, Region 400 -> 83, DimStyle 328 -> 118, AttributeDefinition 255 -> 60, Insert 419 -> 263 (attribute locks), AppId 144 -> 367 (the 223 new records are the applications the down-saves register); up: ExtendedData 0 -> 387, Solid3D 276 -> 328, Region 138 -> 164 (the revision GUID is stated by the older file, the newer lacks it), Hatch 0 -> 48 (default name `LINEAR` against the R2000 source's none), BookColor 8 -> 56 (colour and names restored on write); unchanged: Table 46,218 down / 19,764 up (the legacy layout of the source against the restored R2010 cells), VisualStyle 40,980 / 12,648, XRecord 33,002, doc 17,490 / 4,205, Dictionary 5,676, Text 3,440, Solid 3,140. Total 215,953 -> 213,202.

### TABLE generality (item 3)

* Tables of the corpus (build.noindex/rs-corpus C# dumps and the fixtures): owerko tables.dwg (6: #7218 and #7C37 (style #87), #76FE and #7B63 (style #76E2), #72C0 and #7C38 (style #8B6); AC1032), autodesk blocks_and_tables imperial / metric (2 + 2; AC1027), ezdxf acad_table_with_blk_ref.dxf (1; AC1032), the two tables of sample_AC1014.dwg to sample_AC1032.dwg (#528, #A35) and their DXF twins. All are R2010+ tables or legacy tables of the samples; AutoCAD's legacy form of a modern table exists only for #528 (and #A35, which the rules do not touch), so the down-conversion rules can be checked for firing, not for correctness, on the rest.
* Row text height rule (a row style with override bit 0x80 and a text height gives it to the cells that override none): the only table whose rows state one is owerko #7B63 (rows 1.5, 0.6, 0.375, 0.375, 0.375 with cell override words 408576 / 405504 / 409088 / 408592: the cells there state their own heights, so the rule does not fire for the cells that have one). Kept; no evidence to widen it.
* Last-column right edge (the inside-horizontal border of a last-column cell with no explicit right border becomes its right edge): it fires on no table of the corpus except #528 (owerko #72C0 / #7C38 have an explicit right border in their last column, the other tables none). Kept as narrow as it was (last column only, right border not explicit); UNVERIFIED as AutoCAD's rule. A note on the evidence: the border order of the R2010 cell style ("top, horizontal inside, bottom, left, vertical inside, right", DwgObjectReader.cs:5452) puts a border into the slot the rule reads for a cell AutoCAD flags as its right edge (11 = top | right | left); if ODA's order is top, right, bottom, left, inside, the rule is the general "explicit right border" and would hold for every column; the samples do not tell (the non-last columns of #528 and of the corpus never state it).
* Block scale (`TableCell::legacy_layout_in`, cell type 2 states the scale of the content format): fires for #A35's block cells only; no other table has a block cell.
* Edited typed cells (item "an edited date / angle / currency / point cell must keep the edit"): AutoCAD's own check is a checksum stored with every cell; it is the sum of the UTF-16 units of the displayed text times their 1-based positions (5 of 5 cells of the sample, including the multi-digit number display "0.500000" = 1739; review: 19 of 19). The fork now trusts the typed value of the content object only while the checksum of the legacy cell's text equals it (`cell_text_checksum`), for typed values with a checksum item; a cell without a checksum item keeps the old plain-number and string checks. An edit anywhere makes the table keep all its converted legacy cells (the typed values of its other cells are then lost: per-cell restore is open). Non-ASCII text: UTF-16 units assumed, UNVERIFIED.

### Changed older tests

None. The four tests that failed during the job (`unused_default_application_ids_stay_out_of_the_appid_table`, `handle_seed_does_not_grow_with_round_trips`, `dwg_roundtrip_deep_r2000`, the table-style carrier plan) failed because the first version stamped documents made in memory; the stamps are now added only for a conversion of a drawing read from a file or where the file had them, and the tests are unchanged.

### Tests

`cargo test --features acadsharp-compat --no-fail-fast`: **2,599 passed, 0 failed, 60 ignored** (R5b-5 review 2,583; +16: `autocad_conversions` 7, `acis::reversion` 2, `gradient_record_tests` 1, `entities::table` checksum 1, `tests/r5b6_conversions.rs` 5), 0 warnings in `cargo build --lib`, the feature build and the test build. Each fix has a test that fails without it (checked by disabling: `prepare_for_write` -> 8 tests of the module and the integration file fail; `restore_document` -> 5 and 2; the checksum branch -> the edited-cell test; the gradient name -> the hatch test; `sab_for_version` -> the modeler test). `tests/r5b4_write_determinism.rs` passes. Python: `g4-compare.py --self-test` on the four allow-lists (471, 52, 288, 251 rules, 0 failures), `g4-native-compare.py --self-test`, `g5-roundtrip.py --self-test` (0 failures), `g5-twin-rules.py --check` (up to date).

### Doubts and UNVERIFIED

* No AutoCAD here: AutoCAD opening the fork's R14 / R2000 / R2004 / R2007 down-saves with these records, the SAB re-versioned for R2004 to R2013 (byte-equal to AutoCAD's own for the sample's three bodies, but other record shapes are not converted), `DBColXD` and the visual style stamp as AutoCAD's rule for other drawings (one drawing).
* The 24-byte colour block of `ACAD_DSTYLE_DIMTEXT_FILL` is read as size 24, 1, packed colour, 12 zero bytes (one sample, a true colour); an index or named colour is stated in the same shape and was never seen.
* The position a record is put back at is the position the file had it at (stash); for a drawing converted from another version it is appended, and AutoCAD's order of the applications is not known beyond the samples (DIMBREAK, DIMJAG, DIMTALN, DIMTEXT_FILL; DIM_LINETYPE before the EXT records).
* `restore_document` runs for every DWG / DXF read below R2013 (an object scan for the stamps below R2018, an entity scan below R2013); G8 was not run: AC1027 and AC1018 / AC1015 files pay one pass over the entity Arcs and one over the object map with a type test; UNVERIFIED under G8. G2 (corpus) was not re-run for the view's position fidelity.
* Up RR rows of Solid3D / Region rose by 78 although the revision rows fell by 256 (bytes, major, minor, has_guid): `uid` (+16) and `sab_data` (+12) now differ in the up cells; the cause was not investigated (no sample_V cell depends on it). Review: all 78 are `uid`, fixed.

### Review of R5b-6 (2026-10-24)

Fork tree: r5a at 220de72 + the review's working-tree changes; CadKit-wt-r5a at 11384ed + `Tools/g4_relations.py` (`application_registered_elsewhere` narrowed) and `Tools/g5-rc-allowlist.json` (G5-RC-055: one more reject example). Evidence for the records: the C# dumps of sample_AC1014.dwg to sample_AC1032.dwg and their DXF twins, plus a streamed scan of the 130 C# corpus dumps (`build.noindex/rs-corpus/cs`) for the same applications.

* Records per version: the six AutoCAD files confirm the agent's table (AcDbAttr {0, 1} on all 9 attributes up to R2004, `IsLocked` native from R2007; DIMTEXT_FILL / DIM_LINETYPE up to R2004; OBJECT_CHANGE_GUID up to R2010; MLEADERVER up to R2007; the visual style stamp and DBColXD up to R2013; DesignCenter Data up to R2004). The corpus adds: `ACAD_DSTYLE_DIMTEXT_FILL {376 0 377 <ByLayer>}` in an R2007 file (autodesk architectural_-_annotation_scaling_and_multileaders.dwg: AutoCAD states a fill that is off when its colour is set, and an R2007+ file can hold the record), `DBColXD` in an R2018 file (cadviewer SE-Barcelona.dwg) and a repeated `DBColXD` block in an R2013 one (gdal Building_A_Floor_0_Mapwize.dwg), `DesignCenter Data` in an R2013 file (cadviewer City_base_map.dwg), an all-zero `ACAD_OBJECT_CHANGE_GUID` on SPLINE and ELLIPSE (cadviewer 9_Border_2Rows_V2.dwg, R2010; not a modeler entity, kept as it is), no AcDbAttr outside the samples. The agent's writer dropped every such record a holding version's file states (none of the 48 gate samples has one, so G5 could not see it): fixed, see the behaviour rows.
* Edits: an attribute lock, a fill, a line type, a revision changed after the read are stated from the native value (unit tests); an edited index of a pre-R2004 colour-book object made the stale `DBColXD` block restore the old colour and be written again: fixed. A record of another shape than AutoCAD's (AcDbAttr {0, 0, 9}, MLEADERVER {2, 5}, a DIM_LINETYPE with a string) was replaced by the native value's record below the holding version: fixed. In-memory documents get only the records of native values they hold (lock, fill, line types, revision), no stamps (unchanged).
* `consumed_xdata`: filled once per read (`restore_document`), bounded by the records of the file, not serialized; a clone of an entity added to the document gets a new handle and no stash (its record is made from the native value). Found: `CadDocument::repair` gave an object a new handle without moving its stash: fixed (behaviour row). Left: the object-handle collision remap of `resolve_references` (an object whose handle an entity or table record also has, a malformed file) leaves the stash under the old handle, where entity and object stashes are one list (UNVERIFIED, no file).
* G1 / G2 fidelity of the view's put-back: G1 45 / 49 with rs-gap.tsv identical; on the 43 corpus files that hold any of these records (all but the 39 MB scau-gis file, which has none of the gated shapes), the fork's dump against the C# dump has no `ExtendedData` mismatch on any object type and no row of AttributeDefinition / AttributeEntity / DimensionStyle / MultiLeader / MultiLeaderStyle / VisualStyle / BookColor / Solid3D / Region / BlockRecord / Hatch that differs from the corpus baseline per-file TSVs.
* Modeler data: the fork's down-saves of the three bodies (sample_AC1032 / 1027 / 1024 / 1021 to every older binary release, 27 bodies) equal AutoCAD's own bytes outside the header date (6 of them byte-equal, the same save moment): verified. The converter copied every record except vertex and coedge verbatim; the bodies hold 16 record kinds only (no curved surface, no spline, no tolerant topology), so the vertex / coedge integer rule is fitted to them: the converter now refuses any body with another record kind or shape (`KNOWN`), keeping the data as it was (unit test with a cone surface and a face of another shape). SAT 400 / 106 for R2000 / R14 left open: right call (other record layouts, not a header change).
* The +78 up RR rows of Solid3D / Region were all `uid` (52 + 26; no `sab_data` row is new in the up cells, the 59 new down `sab_data` rows are the re-versioning itself): the restore set the revision but not the identifier the DWG builder derives from it; fixed, 162 RR rows fewer (up 78, down 84).
* TABLE: the checksum holds for 19 of 19 cells (above); it is now read by its name and governs every value cell that has it. Hatch gradient colours: the record's layout (count 2, then per colour a double and a 24-byte colour, then the name) matches the parse; LINEAR for R2000 sources matches AutoCAD's R2004 file.
* Rules G5-RC-048..055: narrow (the handle of an application record only, never its contents; bytes equal outside the 24-character date at the same offset; an AppId the expected file has by name). Narrowed: `application_registered_elsewhere` no longer accepts an AppId whose name the fork's file registers twice (a second registration is not a renumbering; reject example added, fails without the narrowing). `equals_source` untouched. Rows accepted in the final run as the agent reported (45, 36, 36, 16, 8, 10, 5, 59); they rescue no cell.

Counts (clean build `cargo clean -p acadrust`, `--jobs 3`, all 288 cells, CadKit `build.noindex/rv6-g5`): G5-RR 41 (20 exact + 21 allowed; same-version 41 / 42, up 0 / 59, down 0 / 187; no rules 20, hand = all 41), RC same-version 42 / 42 (no rules 0, pinned-field rules 21, hand 21, all 42), RC sample_V 6 / 36 (no rules 0, pin 6, hand 6, all 6), idem 283 / 288 (5 AecObjects), C# -> fork 204; per version identical to the agent's table. Against the agent's g5-final: no cell changed status in any check; sample_V off-diagonal rows identical (common 863, dropped 0, added 0, kept 3,622, unreachable 4,165). Unexplained cross-version RR rows 213,036 (down 160,355, up 52,681; agent 213,202; R5b-5 review 215,953). G4 (`build.noindex/rv6-g4`): RR 94, RC 94, idem 96 / 96, every cell's status and detail identical to the agent's run. G1 45 / 49 (rs-gap.tsv identical). Determinism: `tests/r5b4_write_determinism.rs` passes; two pinned writes of sample_AC1032.dwg as R2004, R2010 and R2018 and of sample_AC1018.dwg as R2004 are byte-identical.

Restore-pass cost (G8 not run, as instructed): the entity pass below R2013 is skipped when the drawing registers none of AcDbAttr / ACAD_MLEADERVER / ACAD_OBJECT_CHANGE_GUID, else it is one pass over the entity Arcs with a type match, no allocation and no copy unless an entity changes; the two object-map passes (multileader styles below R2010, stamps below R2018) are one pass over `object_xdata` only (the objects with extended data, not the object map), with no allocation per visual style (the agent built the stamp block once per visual style to compare it); the dimension-style pass below R2007 is over the style table. The writer's object pass looks up extended data only for the three object types it handles.

## R5b-7: DXF down-saves as AutoCAD's DXF, the single-cause sample_V cells (2026-10-24)

Fork tree: r5a at d14877b + this job's working-tree changes; CadKit `Tools/g4-compare.py` (rules name a list of types), `Tools/g4-native-compare.py` (`unit` rules list types and properties), `Tools/g4-roundtrip.py` (report line), `Tools/g4_relations.py` (`roundtrip_records_not_in_dxf`), `Tools/g4-allowlist.json` (G4-522, G4-523), `Tools/g4-rr-allowlist.json` (RR-049..054). Baseline = the R5b-6 review tree (`build.noindex/rv6-g4`, `rv6-g5`); final = this tree (`cargo clean -p acadrust`, clean build, `--jobs 3`, CadKit `build.noindex/r7i-g4`, `r7i-g5`, fork binary d18b564e51c5).

### Counts

| | before (R5b-6 review) | after |
|---|---|---|
| G4 (96 cells) | RR 94, RC 94, idem 96 / 96 (only AecObjects fails) | RR 94, RC 94, idem 96 / 96; 0 status changes in 96 cells; the 90 cells away from the DWG sources of R14, R2000 and R2004 have identical rows |
| G5-RR (288) | 41 (same 41 / 42, up 0 / 59, down 0 / 187); 213,036 unexplained cross rows | **41** (same 41 / 42, up 0 / 59, down 0 / 187); **212,985** (down 160,355 -> 160,565: the column record of the MTEXT is extended data in the re-read, where the native source holds the columns: 8 rows in each of the 20 down-save cells of sample_AC1032.dwg / _ascii / _binary and sample_base.dwg, 10 in each of the 5 of svg/export_sample.dwg (review of R5b-7; not 42 rows in five cells); up 52,681 -> 52,420: the block description and preview of the R14 source are values now, 261 rows) |
| G5-RC same-version (42) | 42 (no rules 0, pin 21, hand 21, all 42) | **42** (no rules 0, pin 21, hand 21, all 42) |
| G5-RC sample_V (36) | 6 (diagonal; pin 6, hand 6, all 6); common rows in the 30 off-diagonal cells 863, dropped 0, added 0 | **7** (diagonal 6 + sample_AC1032.dwg -> AC1027; pin 6, hand 6, all 7: that cell passes through the generated no-loss rules, none of the hand rules); common rows **788**, dropped 0, added 0 (MText 80 -> 40, the 35 block rows of the R14 source gone) |
| G5-idem | 283 (5 AecObjects refusals) | 283 |
| C# -> fork | 204 | 204 |
| G1 | 45 / 49 | 45 / 49 (rs-gap.tsv identical) |

No G5 cell changed any verdict except the one above (all 288 cells compared, nine checks each).

### G4 per cell (the six DWG sources whose records the DXF no longer carries; ASCII and binary alike)

| cell | RR before | RR without the new rules | RR after | RC before | RC without the new rules | RC after |
|---|---|---|---|---|---|---|
| sample_AC1014.dwg | allowed (81 rows) | 266 unexplained of 347 | allowed (347) | allowed (828 / 822 binary) | 214 unexplained | allowed (1,042 / 1,036) |
| sample_AC1015.dwg | allowed (68) | 191 of 259 | allowed (259) | allowed (646 / 640) | 82 | allowed (728 / 722) |
| sample_AC1018.dwg | allowed (82) | 9 of 91 | allowed (91) | allowed (803 / 801) | 10 | allowed (813 / 811) |

The unexplained RC rows without the rules are exactly the ExtendedData rows of the objects that carried a record (`Line` 148, `Block` / `BlockEnd` 14 each, `Circle` 10, ... at R14; `Line` 20, `Block` / `BlockEnd` 14, `Circle` 10, `Solid`, `Point`, `DimensionLinear`, `MText` ... at R2000; `Circle` 8 and `Point` 2 at R2004). The RR rows are the values the DXF cannot hold without the record: a true colour of R14 and R2000 (nearest index), the colour-book name of the circle #99F, the line weight of R14, the material of R14 to R2004.

### Per-record DXF table (the fork's DXF down-save of sample_AC1032.dwg against AutoCAD's own DXF of the drawing: count of the record)

AutoCAD / fork. No AutoCAD DXF of R14 or R13 is among the samples (the fork's count is stated alone; the R14 rows extend R2000 and are UNVERIFIED); AutoCAD's R12 DXF (sample_AC1009_ascii.dxf) holds `AcDbAttr` 9 and `ACAD_MLEADERVER` 15 (exploded multileaders; the fork writes none: not this job's test) and no record on a table record. The R2007 to R2013 `ACAD_MLEADERVER` counts are 17 / 0 / 0 against AutoCAD's 17 / 0 / 0.

| record | R14 | R2000 | R2004 | R2007 | R2010 | R2013 | R2018 | evidence / status |
|---|---|---|---|---|---|---|---|---|
| `RTTcAl`, `RTLwPs`, `RTMaterial` | - / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | before: the fork wrote them in R14 to R2004; owner decision, none |
| `MTEXTBEGIN` (36 MTEXT) | - / 36 | 36 / 36 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | the R2000 DXF holds it as its DWG does: kept |
| `ACAD_MTEXT_BBRT` | - / 2 | 2 / 2 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | kept |
| `ACAD_MTEXT_COLUMN_INFO` (4 MTEXT) | - / 4 | 4 / 4 | 4 / 4 | 4 / 4 | 4 / 4 | 4 / 4 | 0 / 0 | **new** (0 before): AutoCAD's DXF holds it from R2000 to R2013 |
| `AcDbAttr` (9) | - / 9 | 9 / 9 | 9 / 9 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | kept; R12 9 / 9 |
| `ACAD_DSTYLE_DIMTEXT_FILL`, `..._DIM_LINETYPE` | - / 1 | 1 / 1 | 1 / 1 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | kept; R12: AutoCAD 0, the fork wrote 1 each: **removed** |
| `ACAD_OBJECT_CHANGE_GUID` (3) | - / 3 | 3 / 3 | 3 / 3 | 3 / 3 | 3 / 3 | 0 / 0 | 0 / 0 | kept |
| `ACAD_MLEADERVER` (17) | - / 17 | 17 / 17 | 17 / 17 | 17 / 17 | 0 / 0 | 0 / 0 | 0 / 0 | kept |
| visual style stamp (24) | - / 24 | 24 / 24 | 24 / 24 | 24 / 24 | 24 / 24 | 24 / 24 | 0 / 0 | **new** (0 before in a DXF): AutoCAD's DXF holds it from R2000 to R2013 |
| `DBColXD` | - / 0 | 0 / 0 | 1 / 1 | 1 / 1 | 1 / 1 | 1 / 1 | 0 / 0 | kept (R2000 DXF has none, R2004 on has it) |
| `DesignCenter Data` (4) | - / 4 | 4 / 4 | 4 / 4 | 0 / 0 | 0 / 0 | 0 / 0 | 0 / 0 | kept |

The other differences of the extended-data census between AutoCAD's DXF and the fork's down-save are not records of the fork's conversions: `AcadAnnotativeDecomposition` of the two layers (open item below), the `DbSaveVer` of the TABLE, a `DICTIONARY` record, and the `ACAD_XREC_ROUNDTRIP` dictionaries of the visual styles (58 in AutoCAD's R2000 DXF, 3 in the fork's: the DXF writer states fewer than AutoCAD, the converse of this job's test; not done).

### Rules (CadKit allow-lists; evidence AutoCAD's own files)

* G4-522 (RC; DWG sources of R2000 and R2004) and G4-523 (RC; R14), relation `roundtrip_records_not_in_dxf`, for the types Block, BlockEnd, Circle, DimensionAligned, DimensionLinear, Insert, Line, MText, Point, Solid: the ExtendedData of an object where the actual list is the expected one without the leading `RTTcAl` / `RTLwPs` / `RTMaterial` blocks (exact shapes; per source version: R2000 `RTTcAl`, `RTMaterial`; R2004 `RTMaterial`; R14 all three) of its ACAD record, every other record, value and application equal. Evidence: sample_AC1015.dwg 41 objects with a record, sample_AC1018.dwg 5, their DXF twins none (37 of 39 and 5 of 5 such objects of the twin hold no extended data at all, the other two MTEXT keep `MTEXTBEGIN`); G4-523 extends it to R14 (sample_AC1014.dwg: 107 objects), UNVERIFIED for R14 itself. Accepted rows in the final run: 184 (4 cells) and 428 (2 cells). The self-tests carry 11 object-level rejections each (another application's data lost with them, the data behind them lost or changed, nothing removed, a record of another shape, not at the start, another application, a second record kept, an application that is no AppId of that name).
* RR-049 / RR-050 (`rgb_nearest_aci`, unit) for the colour of entities, dimension bases and BLOCK / ENDBLK markers of a DWG of R14 or R2000; RR-051 (colour name and colour-book object of a circle; none); RR-052 (line weight of R14: ByLayer); RR-053 / RR-054 (material flags 0 and handle none before R2007). Format-inherent: R14 and R2000 DXF have no true-colour (420 is R2004+), colour-name (430), line-weight (370 is R2000+, and AutoCAD's R14 file keeps it in `RTLwPs`) or material group (347 is R2007+). Accepted rows: 440 (4 cells), 280 (4), 8 (4), 150 (2), 30 (6), 24 (6).
* No G5 rule was added and `equals_source` is untouched.

### Fixes (R = reader, W = writer)

* W `autocad_roundtrip::prepare_for_dxf`: only the MTEXT blocks (no `groups`), below R2018. W `autocad_conversions::prepare_for_write`: the stamp for a conversion also in a DXF; no dimension style records in a DXF before R13. W `autocad_roundtrip::column_block`, `mtext_blocks`, `with_mtext_blocks` (DWG `write_mtext`, DXF `prepare_for_dxf`). Tooling: `acad-dump write-dxf --version`.
* R `autocad_conversions::restore_document` and W `prepare_for_write` for the block description and preview of R13 / R14 (`block_description`, `block_icon`, `description_record`, `icon_record`); W `dxf/writer/section_writer.rs` (`states_block_content_in_record`: the DXF before R2000 states the description (group 4) and the image (group 310) once, in the records); V `compat/acadsharp/tables.rs` (`block_preview`) and `entities/blocks.rs` (`Comments`) hide the restored values of a DWG before R2000.
* The restore of the round-trip records at DXF read is as before.

### Changed older tests

* `tests/r5b5_conversions.rs`: `a_dxf_of_r2000_keeps_true_colour_in_the_round_trip_record` is now `a_dxf_of_r2000_states_the_nearest_index_and_no_round_trip_record`: it asserted that the R2000 DXF holds `RTTcAl` (the owner decision and AutoCAD's DXF twins say it does not); it also asserts that a DXF with the record (text injected after the write) is restored on read, as the old test did.
* `autocad_conversions` test `the_visual_style_stamp_and_the_colour_book_block_exist_below_r2018`: the assertion that a DXF gets no stamp for a conversion now asserts that it gets the stamp up to R2013 and none at R2018 (sample_AC1015_ascii.dxf to sample_AC1027_ascii.dxf: 24 of 24; sample_AC1032_ascii.dxf: 0). The assertions about the colour-book block are as before.
* `autocad_conversions` test `the_text_fill_and_line_types_of_a_dimension_style_follow_the_version`: gained the R12 and R2000 DXF assertions.
* `tests/r5b3_dwg_roundtrip.rs` `a_block_record_keeps_the_order_of_its_xdata_applications_in_r14`: its `CONTENTBLOCKICON` record (`1071 0`, `1004` bytes) is the real shape of AutoCAD's R14 preview record (sample_AC1014.dwg), which the reader now takes in as the preview of the block record, so the assertion that the record stays in the model after the read could not hold. The test now has two cases: the real shape (the record is the block record's preview after each read, `ACAD` stays first, the bytes survive the second write) and a record of another shape (`1071 1`: another producer's data, which stays in the model in its place: the old assertions, unchanged).

### The single-cause sample_V cells (item 2)

What AutoCAD's files hold, from the committed dumps (sample_AC1018.dwg, sample_AC1024.dwg, sample_AC1027.dwg, sample_AC1032.dwg) and the corpus dumps:

* **Annotative text** (`TextEntity` #3BA and #757; the only rows in AC1018 <-> AC1024). R2004 and R2007: the texts are on the layers `0 @ 1` (#F65) and `Layer_color_80 @ 1` (#F6B), flag 64, with `AcadAnnotativeDecomposition {AnnotativeData 1 1}` on the layer, and `AcadAnnotative {AnnotativeData 1 0}` on the text; R2010 and later: layers `0` and `Layer_color_80`, no decomposition layers (21 layers against 19), `{AnnotativeData 1 1}`. The context data dictionaries (`ACDB_ANNOTATIONSCALES`) are empty in all versions, so the scale the suffix `@ 1` names is not stored in the sample; the corpus shows the suffix is the scale factor (`Dim @ 48`, `Mleader @ 24`, `0 @ 50`, `Выноска @ 2`), and that AutoCAD files of **R2013** hold decomposed entities (autodesk architectural_-_annotation_scaling_and_multileaders.dwg: 28 DimensionLinear and 5 MText with `{1, 0}` on `Dim @ 48`; et2_blocks kt.dwg, zzi.dwg), i.e. the decomposed state is legitimate in R2010+ files and AutoCAD does not merge it when it saves one. The sample pair is two saves of one original, not a conversion of the R2004 file. Down: the layer name needs the scale of each object (one copy per scale for an object with several), which the sample does not state; up: recomposition is AutoCAD's reading of the scale contexts. Not mechanical, not done; the layer, flag and application rows (`AppIdsTable Count` 42 / 41) stay.
* **Spline fit data** (`ACAD_SPLINE_DATA {100 0, 101 0, 102 2}` on #434; the only rows in AC1024 <-> AC1027). R2004 to R2010: 4 control points (Bezier knots 0 0 0 0 1 1 1 1), knot parameterization 15, flags 0, no fit points, the record; R2013 and R2018: 2 fit points (the first and last control point), start / end tangent 3 x (P1 - P0) = (12.7649, 32.4869) and 3 x (P3 - P2) = (-0.4902, 29.3531), knot parameterization 2, flags 9, no control points, no record. The other spline (#433, control points) has no record in any version. 102 states the knot parameterization (the corpus R2010 file cadviewer 9_Border_2Rows_V2.dwg has the record with 102 = 15 on 147 splines). Down: control points from fit points is an interpolation AutoCAD evaluates (the fork writes the fit points, which R2010 can hold; the geometry rows are accepted by the generated no-loss rules); up: fit points from a control polygon is the inverse evaluation, closed form only for this single Bezier. Writing the record next to fit points would be a marker AutoCAD could apply to data that already is fit data. Not mechanical, not done.
* **MText column heights** (`ACAD_MTEXT_COLUMN_INFO`, 8 rows: AC1027 -> AC1032 and the other cells to AC1032). R2000 to R2013: the record above with the height 0; R2018: column type 2, width and gutter as in the record, heights -2.94, -23.73, -3, -204.35, which AutoCAD computes from the text layout. Down (R2018 to R2013 and older): the record is derivable from the native fields (done, see the row; AC1032 -> AC1027 passes, and the 40 rows of the other five down-save cells are gone). Up: the heights are layout results: not derivable, the record stays in an R2018 file, the cells AC1018 / AC1024 / AC1027 -> AC1032 keep these 8 rows.

### What still fails in the off-diagonal sample_V cells (common rows per cell; 788 in total)

| from \ to | AC1014 | AC1015 | AC1018 | AC1024 | AC1027 | AC1032 |
|---|---|---|---|---|---|---|
| AC1014 | | dstyle 8, header 1, viewport 21 | classes 2, dstyle 8, header 2, modeler 3, viewport 21 | annot 2, classes 3, dstyle 8, header 1, modeler 3, table 121, viewport 21 | annot 2, classes 3, dstyle 8, header 1, modeler 3, spline 2, table 121, viewport 21 | annot 2, classes 3, dstyle 8, header 1, modeler 6, mtcol 8, spline 2, table 121, viewport 21 |
| AC1015 | block 4, dstyle 8, modeler 3 | | classes 2, header 1, modeler 3 | annot 2, classes 2, modeler 3 | annot 2, classes 2, modeler 3, spline 2 | annot 2, classes 2, modeler 6, mtcol 8, spline 2 |
| AC1018 | block 4, dstyle 8, modeler 6 | modeler 6 | | annot 2 | annot 2, spline 2 | annot 2, mtcol 8, spline 2 |
| AC1024 | annot 2, appcount 1, block 4, dstyle 8, modeler 6 | annot 2, appcount 1, modeler 6 | annot 2, appcount 1 | | spline 2 | mtcol 8, spline 2 |
| AC1027 | annot 2, appcount 1, block 4, dstyle 8, modeler 6, spline 2 | annot 2, appcount 1, modeler 6, spline 2 | annot 2, appcount 1, spline 2 | appcount 1, spline 2 | | mtcol 8 |
| AC1032 | annot 2, appcount 1, block 4, dstyle 8, modeler 3, spline 2 | annot 2, appcount 1, modeler 3, spline 2 | annot 2, appcount 1, spline 2 | appcount 1, spline 2 | **passes** | |

annot = annotative text, spline = ACAD_SPLINE_DATA, mtcol = column heights (up), appcount = the AppId `AcadAnnotativeDecomposition` or `ACAD_SPLINE_DATA` the older file registers (11 rows), modeler = SAT 400 / SAT 106 against binary modeler data, block = description and preview of a block written as R14 (`CONTENTBLOCKDESCRIPTION` / `CONTENTBLOCKICON`: open for a source that is not R14), dstyle = the R14 DSTYLE overrides, viewport = MVIEW, status word and UCS icon, table = the legacy table of R14 up-converted (121 rows), classes = DICTIONARYVAR / CELLSTYLEMAP counts and the DATATABLE class, header = DIMBLK and MEASUREMENT.

### Item 3: the fixed-count items and the R14 cluster

* Block description and preview (R14 source): done (behaviour row "R5b-7 block description and preview before R2000"): 35 rows gone, AC1014 same-version G5 and G4 cells unchanged. The conversion to R14 from a newer drawing is not (see the row). R14 -> R2000 (the one R14 cell without modeler, table or class rows) still fails on dstyle 8, header 1, viewport 21: the DSTYLE overrides (R14 `270 DIMUNIT` and `271` on angular dimensions are dropped in R2000, block names `6 / 7 ARCHTICK` become handles `343 / 344`, an empty `5` is dropped: derived from four entities, the name `ARCHTICK` against the block `_ArchTick` needs AutoCAD's table of arrowhead names), the viewport status word (R14 0x8080 / 0x8280, R2000 0xC8020 / 0xC8260: fast zoom out, icon bit, grid bits in; `DisplayUcsIcon` false where the fork states true for three viewports, which the status word does not explain) and the MVIEW records, the header name `BOXBLANK` against `_BoxBlank`. Not derivable with the evidence of one drawing; not done. Every other cell with R14 as target or source holds the modeler rows (SAT 106 against 400 or binary) or, to R2010 and later, the 121 legacy table rows; every cell with R2000 as target or source holds SAT 400: row reductions there have no verdict value.
* `DxfClass InstanceCount` (DICTIONARYVAR, CELLSTYLEMAP) and `DxfClass DATATABLE`: AutoCAD writes a different set of DICTIONARYVAR objects with new handles in every version (11 to 16, e.g. `MSLTSCALE`, `LIGHTINGUNITS`, `SORTENTS`, `INDEXCTL`, `XCLIPFRAME` in R2000 / R2004 only) and drops the two DATATABLE objects and their `ACAD_ROUNDTRIP_PRE2007_TABLE` records when it saves R2007 or later; the counts follow the objects the fork keeps (`kept` rows). Not derivable from the samples, not done. `AppIdsTable Count` follows the records of the annotative and spline items.
* `CadHeader DimensionBlockName` (R14 `BOXBLANK` against `_BoxBlank`) and `MeasurementUnits` (1 in the DWG of R14 to R2004, 0 from R2007): the R14 file holds the upper-cased name only, and the template value is a reader default of ACadSharp; no row is a writer defect.

### Tests

`cargo test --features acadsharp-compat --no-fail-fast` (clean tree, `target.noindex/test.log`): **2,616 passed, 0 failed, 60 ignored** (R5b-6 review 2,604; +12: `tests/r5b7_dxf_down_saves.rs` 3, `tests/r5b7_mtext_columns.rs` 3, `tests/r5b7_block_content.rs` 2, `acad_dump_write_dxf` 1, `autocad_roundtrip` unit tests 2, `autocad_conversions` unit test 1), 0 warnings in `cargo build --lib`, the feature build and the test build. Every new test fails without its fix (source files stashed: all eight new integration tests, the changed r5b5 test and the changed r5b3 test fail). `tests/r5b4_write_determinism.rs` passes. Python: `g4-compare.py --self-test` on the four allow-lists (473, 52, 288, 251 rules, 0 failures), `g4-native-compare.py --self-test` on g4-rr (54 rules) and g5-rr (93), `g5-roundtrip.py --self-test` (0 failures), `g5-twin-rules.py --check` (up to date).

### Doubts and UNVERIFIED

* No AutoCAD here: AutoCAD opening a DXF without the records, or a DWG / DXF with the column record the fork now states (R2010 to R2013 are the versions that read it).
* R14: no AutoCAD DXF; the DXF of R14 follows R2000 (MTEXT blocks and the other records kept, no `RT*`, no colour block).
* The column record for static columns, automatic heights and two or more columns is not stated (corrected in the review: ezdxf columns_R2007.dxf shows those records, each with the MTEXT entities of the other columns, which the writer does not make); a positive height passes through as the corpus records show (UNVERIFIED for AutoCAD's own conversion of a positive R2018 height).
* R2000 and later -> R14: the description and preview of a block are not stated (open, see the row). The 25 down-save cells of the R2018 sources with such MTEXT gained 8 or 10 cross-version RR rows each (210; the record as extended data; the reader does not take the record into `column_data`): the native model of the re-read holds the record where the source holds the columns.
* G8 not run: no reader change (the DXF writer's `prepare_for_dxf` walks the entities for versions below R2018 instead of R2021: writer only).

### Review of R5b-7 (2026-10-24)

Fork tree: r5a at 60d287c + the review's working-tree changes; CadKit-wt-r5a at 14c3fc5 (no tool change). Gate runs on a clean build (`cargo clean -p acadrust`, `--jobs 3`): CadKit `build.noindex/rv7-g4`, `rv7-g5`.

* DXF records per version (recounted: the fork's `write-dxf --version` of sample_AC1032.dwg against sample_AC1015_ascii.dxf to sample_AC1032_ascii.dxf, per handle and value): the table above holds for R2000 to R2018 (values equal up to the 16-digit printing of reals; the other census differences are AcadAnnotativeDecomposition, the TABLE `DbSaveVer`, one `ACAD` dictionary record, as stated). DWG down-saves keep `RTTcAl` / `RTMaterial` (R2000 36 / 5, R2004 0 / 5, as AutoCAD's). The R14 DXF holds what the R2000 DXF holds plus nothing of RT*: the owner decision names `RTLwPs`, so dropping it at R14 follows it; the other records mirror AutoCAD's R14 DWG and R2000 DXF (no AutoCAD R14 DXF: UNVERIFIED). R12: the dimension style records are gone as AutoCAD's R12 file shows; that file also has no extended data on any table record, while the fork's R12 DXF passes through the source's (17 layers `AcAecLayerStandard` / `AcCmTransparency`, `ACAD_DSTYLE_DIMBREAK` / `DIMJAG` / `DIMTALN` and `AcadAnnotative` on dimension styles, `ACAD` on text styles, `ACAD_NAV_VCDISPLAY` on the VPORT): not a conversion of this job, open.
* MTEXT columns: corrected. columns_R2007.dxf (corpus) states records of two to four columns, static columns and automatic heights, each with `ACAD_MTEXT_COLUMNS` naming the MTEXT entities of the other columns; the agent's writer stated a two-column record without them. `column_block` now states one column only (`heights.len() == 1`, the only form the 4 sample MTEXT and the 91 other corpus records have); the unit test asserts it.
* Block description / preview: the reader took any `1071 0 1000 <text>` (an empty text was then never written again) and any chunking of the image (written back in chunks of 127: not byte for byte), and overwrote a value the file also stated in group 4 / 310. Now only AutoCAD's form (non-empty text; the record `icon_record` states again) and only into an empty value; otherwise the record stays data of the file. The ACadSharp view hid the values only for DWG sources: an R14 DXF (the fork's own, which states the records and no group 4 / 310) showed `Comments` and `Preview` where ACadSharp reads extended data. The view now hides a value exactly when it came from a consumed record (`tables::block_value_from_record`), for DWG and DXF. Tests: `autocad_conversions` unit test (four kept cases), `tests/r5b7_block_content.rs` (the view of the DWG and the DXF of R14), `tests/r5b3_dwg_roundtrip.rs` (the file's record order through the view, which the R5b-7 change had stopped asserting). "G8 not run: no reader change" above is wrong: the restore is a reader change, one pass over the block records of a drawing before R2000 that registers either application.
* Annotative text and spline (item 2): the annotation-scale dictionaries of both texts are empty in every version, so `0 @ 1` names a scale the drawing does not hold: agreed, not mechanical. Spline #434: the R2010 control points are exactly the cubic Bezier of the R2013 fit data (P1 = F0 + T0 / 3, P2 = F1 - T1 / 3: bit-equal in x and y), so the down-conversion of this one two-point spline is mechanical; the record's values (100 0, 101 0, 102 2) and the general case (more fit points: AutoCAD's interpolation) are not derivable from one spline: not implemented; the up direction (T1 = 3 (P3 - P2)) misses AutoCAD's value by one unit in the last place.
* Rules: G4-522 / 523 reject other applications, partial removal, a changed value after the block, non-leading or malformed blocks, another kind of application object (self-test, 11 reject contexts each); RR-049 / 050 nearest ACI by the palette (ties only), RR-051..054 output constants for the R14 / R2000 / R2004 sources. No existing rule changed its accepted rows: before the change no RC rule and no unit rule named a list, and every pre-existing rule has the same row count in G4 (rv6 vs rv7) and the same per-cell usage in all 288 G5 cells. Without the new rules the three DWG sources leave exactly the ExtendedData rows (RC) and the colour / line weight / material rows (RR) of the objects with records.
* sample_AC1032.dwg -> AC1027: passes on 0 common rows after rules; the accepted common rows are the root dictionary entries, two XDictionary handles, the multi-line attribute #79D (`MULTI_LINE_ATT_001` / alignment against the source's), the version-info XRecord, three ACIS bodies (G5-RC-053 / 054) and the header dates: the same rules and rows as before R5b-7, the cell changed only by the 8 `MText ExtendedData` rows now equal.

Counts (clean build): G5-RR 41 / 288 (same 41 / 42, up 0 / 59, down 0 / 187; not-allowed rows down 160,565, up 52,420, same 4; no rules 20, hand = all 41); RC same-version 42 / 42 (no rules 0, pinned-field rules 21, hand 21, all 42); RC sample_V 7 / 36 (no 0, pin 6, hand 6, all 7); idem 283 / 288; C# -> fork 204; all 288 cells identical to the agent's r7i-g5. G4: RR 94, RC 94, idem 96 / 96, all 96 cells identical to r7i-g4; against rv6-g4 only the six cells of sample_AC1014 / 1015 / 1018.dwg changed (rows, not statuses). G1 45 / 49 (rs-gap.tsv identical). Write determinism: `tests/r5b4_write_determinism.rs` passes; `write-dxf --version AC1015 / AC1018` (ASCII and binary) and pinned `write-dwg` of sample_AC1032.dwg give the same bytes twice. Tests: 2,617 passed, 0 failed, 60 ignored, 0 warnings.


## R5b-8: cross-version G5-RR, the large families (VisualStyle, Table, XRecord, document) (2026-10-24)

Fork tree: r5a at aec54c1 (R5b-7) + this job's working-tree changes (nothing committed); CadKit `Tools/g4-native-compare.py` (new relations and `cross_version` rules), `Tools/g5-rr-allowlist.json` (G5-RR-095..192, changes to 062/064/074..080/126/155/175/176/185/186). Baseline = a fresh full G5 run of aec54c1.

### Counts (clean rebuild of acadrust, fresh full G5 / G4 / G1)

| | before | after |
|---|---|---|
| G5-RR (288) | 41 (same 41 / 42, up 0 / 59, down 0 / 187); 212,989 unexplained cross rows | **147** (same 41 / 42, up **14** / 59, down **92** / 187; exact 20, allowed 127); **29,954** unexplained rows (96,112 rows are accepted by rules) |
| G5-RR per DWG version | AC1014 1, AC1015 3, AC1018 3, AC1024 3, AC1027 3, AC1032 28 | AC1014 **13**, AC1015 **15**, AC1018 **16**, AC1024 **33**, AC1027 **35**, AC1032 **35** (of 48 each) |
| G5-RR by pair | down per target AC1014 0, 1015 0, 1018 0, 1024 0, 1027 0 | down AC1014 12, 1015 12, 1018 13, 1024 27, 1027 28; up AC1024 3 / 13, AC1027 4 / 16, AC1032 7 / 19 |
| G5-RC same-version (42) | 42 (no rules 0, pin 21, hand 21, all 42) | 42 (no rules 0, pin 21, hand 21, all 42); no hand rule of the RC list was changed |
| G5-RC sample_V (36) | 7 | 7 (pin 6, hand 6, all 7) |
| G5-RR with no / pin / hand / all rules | no rules 20 (pin = the RC rules G5-RC-001..010 and the RR list has no pin rules) | no rules **20**, hand (g5-rr-allowlist.json is all hand) **147**, all 147 |
| G5-idem | 283 (5 AecObjects write refusals) | 283 |
| C# -> fork | 204 | 204 (AC1014 0, 1015 25, 1018 44, 1024 45, 1027 45, 1032 45) |
| G4 (96) | C# 96, RR 94, RC 94, idem 96 | C# 96, RR 94, RC 94, idem 96 (only AecObjects fails) |
| G1 | 45 / 49 | 45 / 49 (rs-gap.tsv identical) |

### Family table (unexplained rows, cross-version cells; (a) fork defect fixed, (b) format limit, rule)

| family | before (up / down) | after | class |
|---|---|---|---|
| entity Table | 62,082 (19,764 / 42,318) | 498 (0 / 498) | (a) legacy flat model not rebuilt from TABLECONTENT / TABLEGEOMETRY; fixed (R + W); the rest: cell (1,4) text style / geometry of sample A35 not derivable |
| object VisualStyle | 53,628 (12,648 / 40,980) | 1,080 | (a) R2010 record dropped the post-28 properties (AutoCAD keeps them: sample_AC1024.dwg); the record chain objects and forms are rules (b) (R14..R2004 252 entries, R2010 126) |
| object XRecord | 33,695 | below the top 24 | entries[] of the VS record forms (rule, b), TableStyle record |
| doc (classes / header) | 20,324 | 5,027 | (a) classes of a DXF source (live, whole list); (b) instance_count (the class table states its own output: census rule), maintenance version, dim_mzf / dim_mzs, current_material, geo header, handle seed |
| object Dictionary | 6,452 | below the top 24 | (b) entries added / dropped with the VS record chain and the DesignCenter record |
| object Table | 3,900 | below the top 24 | (a) TABLECONTENT objects in the re-read |
| entity MText | 1,522 | 695 | (a) columns: record vs native; (b) multi-line attribute and height rules |
| entity Text / Solid / Line / Polyline2D / Point (R12 sources) | 3,788 / 3,448 / 1,976 / 1,380 / 770 | unchanged | open: linetype / style names of R12 sources |
| entity Viewport, Hatch, MultiLeader, Material | 1,570, 816, 1,308, 717 | 1,042, 768, 1,308, 405 | open (Hatch gradient at R2000 / R14; MLeader context lines) |

### Closest cells and what blocks them

Passing now: AC1032 -> AC1027 (all dwg dynamic / geoloc / empty / sample_AC1032 family and the DXF twins), AC1027 samples -> AC1032, AC1032 dwg family -> AC1024 / 1018 / 1015 / 1014 (12 or 13 of 28 per target), sample_AC1018.dwg -> AC1024 / AC1027 and sample_AC1021.dwg -> AC1024 / AC1027 / AC1032 (up), svg / export_sample -> AC1018, the dynamic DXFs -> AC1024 / AC1027. Blockers of the closest remaining: Table A35 cell (1,4) text style / geometry (not derivable); MultiLeader context lines (arrowhead_size 0.0 -> 0.18 / 4.0, line_type_handle None -> 20, text_attachment_direction); BookColor at R14 / R2000 (index only; names and RGB come from a consumed DBColXD the dump does not hold); ACIS SAT / SAB conversions of 3DSOLIDs with history attributes (acis-same differs; AC1024 DXF SAT -> AC1027); DXF sources at R2004 or older (visual style extended_lighting_model 3 -> 2, Material channel_flags 63 -> 127, MLeaderStyle attachments, loft_angle 1 ULP, layer material); sample_AC1027 / 32 DXF class tables at AC1024; R12 (AC1009) sources; AecObjects (write refused: custom entity); Hatch gradient at R2000 / R14.

### Fixes (R = reader, W = writer, V = ACadSharp view)

* W `io/dwg/dwg_writer.rs:1166` (`reconciled_classes`, `declared_live_unused`) and `:106..120` (class preparation of a DXF source below R2018).
* R/W `objects/stub_objects.rs:444` (`store_visual_style_roundtrip`), `:337` (`restore_post_2010_properties`), `:602` (`record_states_properties`), `:625` (`restore_visual_style_roundtrip`), `:631` (`upgrade_r2010_visual_styles`).
* W `io/dwg/autocad_conversions.rs:846..854` (R14 block description / preview records).
* R `io/dwg/autocad_roundtrip.rs:617..` (`parse_column_block`, `take_column_block`, `take_column_block_if`, `has_column_block`, `column_block_states_columns`, `drop_stated_column_block`, `states_columns_in_block`, `with_column_block`), `io/dwg/dwg_reader.rs:3810` (`recover_mtext_columns`, called at `:1698` and `dxf/reader.rs:677`); W `io/dxf/writer/section_writer.rs:5351`.
* R/W `entities/table.rs:2593` (`legacy_source`), `:2602` (`restore_dwg_table_roundtrip`, called at `dwg_reader.rs:1655`, `:1734`, `dxf/reader.rs:708`), `:2788` (`unrestored_legacy`), `:2812` (`as_r2010_record`); W `object_writer/entities.rs:3851` (`write_table`); W `section_writer.rs:12549` (`write_acad_table`).
* V `compat/acadsharp/mtext.rs`, `entities.rs` (`mtext_common`, `column_block_version`), `entities/table.rs` (`ValueFlag`, `OverrideFlag`, legacy view without the `source.dxf` filter). They keep G1 at 45 / 49.
* `cargo build --lib`: `with_column_block` is only used by the view, so it is `#[cfg(feature = "acadsharp-compat")]` (0 warnings in both builds).

### Rules (CadKit Tools/g5-rr-allowlist.json; exact replacement values; evidence AutoCAD's own files and DXF twins)

G5-RR-095..192: maintenance version per target (095..100); ClassObject display_name (101 / 102: group 300 only in R2018 DXFs); class census (103: the class table states its own output, AutoCAD counts more); VS record chain objects added / dropped (104..109) and form (110); handle seed (111); default AppId renumbered (112); header dim_mzf / dim_mzs / current_material (113..115); VPort ambient / grid / visual style (116..118); Layout shade_plot (119); TableStyle modern style, long1, cell style map (120..122, 175..180), record XRecord and Dictionary entries (123..125); DATATABLE class (126, AC1014 variant 142); DesignCenter record (127 / 128 / 169 / 170); geo header (129..131); VPort sun handle (132); instance_count 0 at R14 / R2000 (133); Dictionary duplicate_cloning and XRecord cloning_flags at R14 (134 / 135); last_saved_by (136 / 137); VS list colours -> nearest ACI (138); Sun colour (139); insertion units and OSMODE at R14 (140 / 141); AppId added by conversion (143..145); ACIS older save version (146: the independent Python port `acis_older_save` of the reversion); MText column height (147); multi-line attributes (148..153); has_ds_data (154 / 155); Table R2010 bit (156 / 157); Spline knot / flags / closed (158..160); TableContent value fields at R2004 (161..164, 192); scale_uniformly of Viewport / View / BlockRecord (165..168); Material illumination / reflectivity / translucence (171 / 172); dim linetype names (173 / 174); Hatch gradient name; Leader text height / width (187 / 188); SAT -> SAB for DWG R14 / R2000 sources (189); Solid history handle (190); View visual style (191); VX table rules (181..186). Each has accept and reject self-tests (`python3 Tools/g4-native-compare.py --self-test Tools/g5-rr-allowlist.json`: 191 rules ok); cross-version rules apply only when the input version differs from the target.

### Changed older tests

* `tests/r5b7_mtext_columns.rs` `a_dwg_before_r2018_states_the_columns_in_the_record_as_autocad_does`, `an_mtext_without_columns_of_its_own_keeps_the_record_its_source_stated`, `a_dxf_before_r2018_states_the_columns_in_the_record_as_autocad_does`: the reader now takes the block into the entity's columns (native `{type 2, count 1, heights [0.0]}`) and no `ACAD` record holds the block after a read; the rewritten file still states the block (asserted in the DXF text and by a second read). Evidence: R5b-7 open item; AutoCAD samples state the block only as the file's way of holding native columns. The unused helper `the_record` was removed with its last use.
* `src/io/dwg/autocad_conversions.rs` `the_description_and_preview_of_a_block_are_records_before_r2000`: a block of a newer drawing gets the 3 records at R14 (none at R2000); evidence sample_AC1014.dwg.

### Tests

`cargo test --features acadsharp-compat --no-fail-fast`: 2,627 passed, 0 failed, 60 ignored, 0 warnings (includes `tests/r5b8_cross_version.rs`, 10 tests, and `tests/r5b4_write_determinism.rs`). Each fix's test was checked to fail without it. Python: `g4-native-compare.py --self-test` for g4-rr and g5-rr lists, `g4-compare.py --self-test` (473 rules, 0 failures, g4 and g5-rc lists), `g5-roundtrip.py --self-test` 0 failures, `g5-twin-rules.py --check` up to date.

### Doubts and UNVERIFIED

* UNVERIFIED: AutoCAD opening the fork's output (R14 block records, R2010 visual style records, DXF class lists, table legacy re-write, the MText column block); the evidence is AutoCAD's own files read by this fork's reader.
* The ACIS rule (146) rests on a Python port of the reversion; G8 (reader performance) was not re-run: the restore passes run once per document and allocate nothing for absent data.
* Class `instance_count` mapping gaps remain for rarely used classes (rule 103 covers the 48 samples).
