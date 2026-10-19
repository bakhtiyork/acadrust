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
| DXF class retention | Preserve source CLASS records separately from the native writer registry, including source order and metadata. | Reader data-loss fix: merging with writer defaults previously discarded source metadata. The native registry remains unchanged; source retention is an upstream-PR candidate. ACadSharp-specific numbering and collection projection stay in compat. |

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

R12 DXF polyline projection intentionally follows ACadSharp's legacy reader: native
flag-based types remain intact, while the view exposes Polyline2D/Vertex2D, including
polyface face records. Child appearance follows ACadSharp's MatchProperties callback;
identity, reactors, XData and extension dictionaries remain the child's. DWG/modern DXF
retain subtype-specific views. Seqends are emitted only from retained records, never
invented from guessed handles. These oracle quirks are not native reader fixes.

Remaining oracle registration differences: modern DXF can register the default
Seqend when the first child is added, before the parsed terminator replaces the
collection reference (`SeqendCollection.Add`/`Seqend`, `CadPolyLineTemplate.build`,
`CadDocument.RegisterCollection`). The registry then exposes a constructor-default
terminator allocated from HANDSEED, rather than the source handle/common data.
The current view keeps the source record; reproducing that registry requires the
document-wide registration order, not a per-file handle substitution. Legacy DXF
INSERT terminators without an owner are registered as block entities by
`DxfEntitiesSectionReader`, another difference from native compound ownership.
