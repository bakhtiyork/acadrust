//! Native dump of a [`CadDocument`] (`acad-dump native-dump`), used by gate G4 (round trips).
//!
//! The dump describes the native model, not the ACadSharp view: no compat rule is applied. It is built from the
//! `Debug` rendering of every record, so a field added to the model shows up without touching this file, and
//! nothing is hidden by a serde attribute. The only fields left out are the ones named in [`EXCLUDED_FIELDS`]
//! (how a record was read, raw DWG bytes kept for a DWG save, derived indexes), each with its reason; a document
//! field that is neither dumped nor named in [`EXCLUDED_DOCUMENT_FIELDS`] does not compile (exhaustive
//! destructuring in [`write_native_dump`]).
//!
//! Output: JSON lines, one record per line `{"k": key, "v": tree}`, grouped by kind and sorted by handle or name
//! inside a kind (bounded memory: one record at a time, plus the sorted handles). A tree is the parsed `Debug`
//! text: a struct is an object with a `"$"` member holding its type name, a tuple struct or variant an object with
//! `"$"` and `"0"`, `"1"`, ...; `Some(x)` is `x`, `None` is `null`; a unit variant is its name as a string; a map
//! or set is an array sorted by key (so the order of a hash map does not matter); a long byte vector is a
//! lowercase hex string; a handle is its number. `Debug` of an `f64` is the shortest text that reads back to the
//! same value, so equal values have equal text.
//!
//! Where the DWG reader and the DXF reader keep one datum in two places, the dump holds it in one:
//! * BLOCK / ENDBLK markers: entities (DWG) or `block_entity_common` / `block_end_common` of the block record
//!   (DXF): record `block_marker/<block record handle>`;
//! * the reactors and extension dictionary of an object: fields of the object (DXF) or the document maps (DWG):
//!   records `reactors/<handle>` and `xdictionary/<handle>`;
//! * the XDATA of an object or table record: structured records, plus the records the DWG reader still holds as
//!   raw EED (decoded here; bytes that cannot be decoded are kept as such): record `object_xdata/<handle>`;
//! * the EED blocks of an entity that the DWG reader kept only as raw bytes (no structured record of that
//!   application): record `entity_xdata_raw/<handle>`;
//! * the current records of the header (DWG: handles, DXF: names): record `doc/header_references`.
//!
//! The position of an entity in the entity vector is storage order and is not dumped; the order inside a block is
//! `BlockRecord::entity_handles`.

use super::CadDocument;
use crate::entities::EntityType;
use crate::tables::{Table, TableEntry};
use crate::types::Handle;
use serde_json::{Map, Number, Value};
use std::collections::HashMap;
use std::fmt::Debug;
use std::io::{self, Write};

/// `(struct name or "*", field, reason)`. The field is dropped from every record whose struct has that name
/// (`"*"`: whatever struct has the field). Only three kinds of field belong here, and each entry names its kind:
///
/// * `provenance`: the model itself says the field describes how the record was read and leaves it out of the
///   struct's native equality (a hand-written `PartialEq` that skips it, or "excluded from native equality",
///   "compatibility provenance", "source presence" in its documentation);
/// * `dwg-image`: the verbatim bytes (or bit positions) of the DWG record or section the value was decoded from,
///   kept so a DWG save can copy them; the decoded values are dumped and compared separately;
/// * `dwg-only`: a DWG encoding detail the model itself documents as having no DXF group (undocumented bits, DWG
///   type codes, storage scenarios).
///
/// Everything else in the model is CAD data and is compared, even when it looks like bookkeeping.
pub const EXCLUDED_FIELDS: &[(&str, &str, &str)] = &[
    // ---- provenance: struct-level sources (custom PartialEq skips them) --------------------------------------
    ("AttributeDefinition", "source", "provenance: TextSource, 'source spelling and field presence; excluded from native equality'"),
    ("AttributeEntity", "source", "provenance: TextSource, 'source spelling and field presence; excluded from native equality'"),
    ("Text", "source", "provenance: TextSource, 'source spelling and field presence; excluded from native equality'"),
    ("DimensionBase", "dxf_source", "provenance: 'DXF field presence used only by the ACadSharp compatibility view'; custom PartialEq skips it"),
    ("Hatch", "dxf_source", "provenance: 'source provenance; excluded from native drawing-state equality'"),
    ("Hatch", "gradient_recovery_source", "provenance: 'like other provenance, this does not affect native equality'"),
    ("Leader", "dxf_source", "provenance: 'DXF field presence used only by the ACadSharp compatibility view'"),
    ("MText", "source_value", "provenance: 'source lexical text'; the decoded `value` is compared"),
    ("MText", "dxf_source", "provenance: 'source DXF presence and ordered alignment assignments'; custom PartialEq skips it"),
    ("MText", "background_from_xdata", "provenance: records that the background fill was recovered from legacy XDATA; the fill itself is compared; custom PartialEq skips it"),
    ("MultiLeader", "dxf_source", "provenance: 'source presence does not affect native drawing-state equality'"),
    ("Tolerance", "dxf_source", "provenance: 'source-sensitive DXF fields retained for compatibility projections'; custom PartialEq skips it"),
    ("Viewport", "source", "provenance: 'source field presence and unnormalized flags; excluded from native equality'"),
    ("Material", "dxf_source", "provenance: 'DXF field presence; excluded from native material equality'"),
    ("MLineStyle", "dxf_description_present", "provenance: 'whether DXF explicitly stored the description; excluded from equality'"),
    ("MultiLeaderStyle", "dxf_present_codes", "provenance: 'group codes present in the source DXF record'; custom PartialEq skips it"),
    ("PlotSettings", "dxf_present_codes", "provenance: 'group codes present in the source DXF record'; custom PartialEq skips it"),
    ("BookColor", "dxf_color_present", "provenance: 'DXF color-code presence ... does not participate in native drawing-state equality'"),
    ("TableStyle", "legacy_source", "provenance: legacy DWG flags/title/header 'before native suppression normalization'; custom PartialEq skips it"),
    ("SortEntitiesTable", "raw_dxf_version", "provenance: 'DXF version that produced raw_dxf_codes'"),
    ("BlockRecord", "block_entity_common", "folded: the BLOCK marker's common data, dumped with the marker entity of a DWG read as `block_marker/<record>`"),
    ("BlockRecord", "block_end_common", "folded: the ENDBLK marker's common data, dumped with the marker entity of a DWG read as `block_marker/<record>`"),
    ("XRecord", "object_references", "dwg-only: 'object-id handle stream paired with 330-369 entries in DWG files; reference kind is encoded in the DWG handle header'; DXF carries the kind in the entry's group code, compared as `entries`"),
    ("XRecord", "preserve_object_reference_stream", "dwg-only: 'preserve the exact decoded DWG handle vector'"),
    ("XRecord", "source_values", "provenance: 'reader provenance only ... intentionally omitted from native equality'"),
    ("DimStyle", "source_present", "provenance: whether the record came from the source table; custom PartialEq skips it"),
    ("DimStyle", "dxf_present_codes", "provenance: 'group codes present in the source DXF record'; custom PartialEq skips it"),
    ("DimStyle", "dxf_last_linear_unit_code", "provenance: order of groups 270/277 in the source record; custom PartialEq skips it"),
    ("LineTypeElement", "source_scale", "provenance: 'compatibility metadata ... must not make a source-read element unequal'; custom PartialEq skips it"),
    ("LineType", "description_present", "provenance: 'source presence for nullable/omitted table values; excluded from native equality'"),
    ("TextStyle", "big_font_file_present", "provenance: 'source presence for nullable/omitted table values; excluded from native equality'"),
    ("TextStyle", "dxf_present", "provenance: 'source presence for nullable/omitted table values; excluded from native equality'"),
    ("TextStyle", "true_type_font_eed_position", "provenance: position of the font record inside the source XDATA; custom PartialEq skips it"),
    ("View", "source_field_mask", "provenance: 'DXF fields observed while reading this entry ... compatibility provenance only'"),
    ("VPort", "source_field_mask", "provenance: 'DXF fields observed while reading this entry ... compatibility provenance only'"),
    ("Layout", "dxf_layout_flags_present", "provenance: 'whether the DXF AcDbLayout subclass carried its own group 70 ... source provenance only'"),
    ("Dictionary", "cloning_absent", "provenance: SourceFlag, 'the DXF record carried no group 281'"),
    ("Field", "value_string_source", "provenance: SourceText, 'DXF group 301 before caret-escape decoding'; the decoded value is compared"),
    ("GeoData", "dxf_present_codes", "provenance: 'source DXF codes whose presence distinguishes omitted values from defaults'"),
    ("GeoData", "raw_dxf_definition", "provenance: 'source 301/303 text before caret-escape decoding'; the decoded definition is compared"),
    ("BlockRecord", "dxf_present", "provenance: 'DXF field presence: explodable, scaling, preview, comments, xref path, units'"),
    ("BlockRecord", "dxf_preview_last_chunk", "provenance: byte offset and size of the last DXF preview chunk; 'the native preview always retains all chunks'"),
    ("HeaderVariables", "dxf_source_variables", "provenance: 'variables present in a source DXF HEADER section'"),
    ("HeaderVariables", "source_handle_seed", "provenance: 'original HANDSEED before document registration advances the allocator'"),
    ("Vertex2D", "source_widths", "provenance: 'raw DXF widths before native inheritance/normalization, when read from DXF'"),
    ("Table", "dxf_legacy", "provenance: crate-private compatibility copy of the columns, rows and base style the legacy ACAD_TABLE entity of a DXF stated before the DXF reader completed the table from TABLECONTENT/TABLEGEOMETRY; superseded by the completed model, which is dumped; absent on a DWG read"),
    ("MText", "source_line_spacing_style", "provenance: 'the line spacing style word of a DWG record when it is neither 1 nor 2; the typed style holds the default for it'; custom PartialEq skips it"),
    ("DimensionBase", "source_attachment_point", "provenance: 'the attachment point word of a DWG record when it is none of 1 to 9; the typed attachment point holds the default for it'; custom PartialEq skips it"),
    ("RowCellStyle", "source_alignment", "provenance: 'the alignment word of a legacy DWG record when it is none of 1 to 9; the typed alignment holds the default for it'; custom PartialEq (SourceWord) skips it"),
    ("TableCell", "source_style_type", "provenance: 'source cell-style type, retained when the source had no style data'"),
    ("TableRow", "source_style_type", "provenance: 'source row-style type, retained when the source had no style data'"),
    ("TableColumn", "source_style_type", "provenance: 'source column-style type, retained when the source had no style data'"),
    ("AcisData", "wireframe_data_present", "provenance: 'whether the source record carried the optional wireframe cache'"),
    ("AcisData", "wireframe_point_present", "provenance: 'whether the wireframe cache carried its reference point'"),
    ("AcisData", "wireframe_isoline_present", "provenance: 'whether the wireframe cache carried the isoline/wire list'"),
    ("AcisData", "source", "provenance: 'shared DWG bits or compact DXF text layout for exact source projection ... transient provenance'"),
    ("DxfAcisChunk", "source_override", "provenance: 'sparse differences from native decoded text'"),
    // ---- dwg-image: verbatim DWG bytes or bit positions -------------------------------------------------------
    ("EntityCommon", "raw_record", "dwg-image: 'the exact merged-stream bytes this entity was decoded from (DWG only)'"),
    ("EntityProperties", "dwg_color_source", "dwg-image: 'original R2004+ DWG ENC word and packed transparency'; the decoded color and transparency are compared; custom PartialEq skips it"),
    ("EntityProperties", "layer_handle", "dwg-image: 'DWG layer handle as read from the file'; DXF names the layer (group 8) and `layer` is compared"),
    ("EntityProperties", "linetype_handle", "dwg-image: 'DWG linetype handle'; DXF names the linetype (group 6) and `linetype` is compared"),
    ("EntityProperties", "entity_mode", "dwg-image: 'entity mode (0=owned, 1=paper, 2=model), raw DWG value for round-trip'; the owner (`owner_handle`) is compared"),
    ("ExtendedData", "raw_dwg_eed", "dwg-image: 'raw DWG EED blobs ... used for verbatim round-trip ... ignored by the DXF reader/writer'; the structured records are compared"),
    ("*", "raw_dwg_data", "dwg-image: the original merged DWG object record, kept for exact-version DWG saves"),
    ("*", "raw_dwg_handle_bits", "dwg-image: handle-stream bit count stored alongside raw_dwg_data"),
    ("*", "raw_dwg_version", "dwg-image: DWG version that produced raw_dwg_data"),
    ("*", "eed_raw", "dwg-image: raw EED blobs of an AEC object, the DWG byte image of its extended data"),
    ("AecCleanupGroup", "raw_data_bits", "dwg-image: bit counts and positions of the opaque DWG payload"),
    ("AecCleanupGroup", "raw_data_start_bit", "dwg-image: bit counts and positions of the opaque DWG payload"),
    ("AecCleanupGroup", "raw_data_end_bit", "dwg-image: bit counts and positions of the opaque DWG payload"),
    ("AecWallStyle", "raw_data_bits", "dwg-image: bit counts and positions of the opaque DWG payload"),
    ("AecWallStyle", "raw_data_start_bit", "dwg-image: bit counts and positions of the opaque DWG payload"),
    ("AecWallStyle", "raw_data_end_bit", "dwg-image: bit counts and positions of the opaque DWG payload"),
    ("AecWall", "raw_data_bits", "dwg-image: bit counts and positions of the opaque DWG payload"),
    ("AecWall", "raw_data_start_bit", "dwg-image: bit counts and positions of the opaque DWG payload"),
    ("AecWall", "raw_data_end_bit", "dwg-image: bit counts and positions of the opaque DWG payload"),
    ("AssocDimensionAssociation", "dwg_reference_stream", "dwg-image: 'original main-stream bytes and bit positions at the first reference'"),
    ("UnknownEntity", "raw_dwg_data", "dwg-image: raw DWG merged-stream record bytes"),
    ("UnknownEntity", "dwg_handle_bits", "dwg-image: handle-stream bit count stored alongside raw_dwg_data"),
    ("UnknownEntity", "dwg_source_version", "dwg-image: DWG version raw_dwg_data was read from"),
    ("UnknownEntity", "dwg_type_code", "dwg-image: 'DWG object type code (from the binary record header); 0 if the entity did not come from a DWG file'"),
    ("AssociativeObject", "source_version", "dwg-image: version the object's DWG bytes were read from"),
    ("HeaderVariables", "dwg_template_data", "dwg-image: 'original optional DWG Template section'; `measurement` is the decoded value"),
    ("HeaderVariables", "dwg_timestamps", "dwg-image: 'raw DWG date/time pairs retained from the header stream'; the dates are typed header variables"),
    ("Ole2Frame", "source_payload", "dwg-image: 'the payload as stored in a DWG, kept only when encoded_payload would lay the compound file out differently'"),
    // ---- dwg-only: encoding details the model documents as having no DXF group -------------------------------
    ("DimensionBase", "dwg_unknown_bit", "dwg-only: 'undocumented R2007+ dimension state bit preserved for round-trip'"),
    ("MultiLeaderAnnotContext", "dwg_unknown_text_bit", "dwg-only: 'undocumented text-context bit following the word-break flag'"),
    ("Table", "dwg_unknown_byte", "dwg-only: 'undocumented header values retained by the native DWG codec'"),
    ("Table", "dwg_unknown_handle", "dwg-only: 'undocumented header values retained by the native DWG codec'"),
    ("Table", "dwg_unknown_short", "dwg-only: 'undocumented header values retained by the native DWG codec'"),
    ("Tolerance", "dwg_unknown_short", "dwg-only: 'undocumented R13/R14 value preserved for DWG round-trip'"),
    ("BlockBasePointAction", "dwg_tail_byte", "dwg-only: 'DWG-only byte after a scale action's connections (zero in every sample seen ... DXF has no group for it)'"),
    ("Spline", "dwg_scenario", "dwg-only: 'DWG storage scenario: 1 = control points, 2 = fit points; None for splines not read from DWG'"),
    ("DxfClass", "class_number", "dwg-only: 'assigned sequentially starting at 500'; DXF CLASSES has no such group (position in the section)"),
    ("DxfClass", "item_class_id", "dwg-only: '498 for entities, 499 for objects', derived from `is_an_entity` (group 281)"),
    ("DxfClass", "dwg_version", "dwg-only: 'DWG schema version recorded by the class entry (R2004+)'; DXF CLASSES has groups 1, 2, 3, 90, 91, 280, 281 only"),
    ("DxfClass", "maintenance_version", "dwg-only: 'DWG maintenance release recorded by the class entry (R2004+)'; DXF CLASSES has groups 1, 2, 3, 90, 91, 280, 281 only"),
    ("DxfClass", "unknown1", "dwg-only: 'first reserved class metadata value (normally zero)'"),
    ("DxfClass", "unknown2", "dwg-only: 'second reserved class metadata value (normally zero)'"),
    ("HeaderVariables", "model_space_ucs_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "model_space_ucs_base_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "paper_space_ucs_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "paper_space_ucs_base_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "current_layer_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "current_text_style_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "current_linetype_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "current_dimstyle_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "current_multiline_style_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "dim_text_style_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "dim_linetype_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "dim_linetype1_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "dim_linetype2_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "dim_arrow_block_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "dim_arrow_block1_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "dim_arrow_block2_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "dim_leader_arrow_block_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "block_control_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "layer_control_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "style_control_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "linetype_control_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "view_control_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "ucs_control_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "vport_control_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "appid_control_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "dimstyle_control_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "vpent_hdr_control_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "current_vx_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "named_objects_dict_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "acad_group_dict_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "acad_mlinestyle_dict_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "acad_layout_dict_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "acad_plotsettings_dict_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "acad_plotstylename_dict_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "acad_material_dict_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "acad_color_dict_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "acad_visualstyle_dict_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "model_space_block_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "paper_space_block_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "bylayer_linetype_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "byblock_linetype_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("HeaderVariables", "continuous_linetype_handle", "dwg-only: the DWG header stores this record handle (section 'Handles/References' of HeaderVariables); the DXF HEADER has no handle variable: it names the current records instead ($CLAYER, $TEXTSTYLE, $DIMSTYLE, $CELTYPE, $UCSNAME, ...), and the record each named one selects is compared in `doc/header_references` (table controls, dictionaries and the default records have no DXF variable)"),
    ("Tolerance", "dimension_style_handle", "dwg-only: 'dimension style handle (for DWG)'; DXF names the style (group 3) and `dimension_style_name` is compared"),
    ("Shape", "style_handle", "dwg-only: 'text style handle (for resolving the .shx file)'; a DXF SHAPE has no style group (AcDbShape: 39, 10, 40, 2, 50, 41, 51, 210)"),
    ("BlockRecord", "insert_count_bytes", "dwg-image: 'raw non-zero RL bytes before the zero terminator, R2000+' of the DWG block header"),
    ("BlockRecord", "insert_handles", "dwg-only: the INSERT handles recorded in the DWG block header; no DXF group carries them (the INSERT entities are compared)"),
    ("DimensionBase", "dwg_flags_byte", "dwg-only: the DWG dimension flag byte; its bit 0 is the typed `text_user_positioned` (both readers set it), the DWG writer passes the other bits through and no DXF group carries them"),
];

/// Conditional exclusions: `(struct, field, flag field, reason)`. The field is dropped only when the sibling field
/// `flag` is `true` in the same record, i.e. when the model states that the field adds nothing to the decoded
/// content.
pub const EXCLUDED_FIELDS_WHEN: &[(&str, &str, &str, &str)] = &[(
    "XRecord",
    "raw_data",
    "entries_complete",
    "dwg-image: 'whether every byte in raw_data was decoded into entries'; with a complete decode raw_data is only the DWG byte image of `entries`, which are compared (an incomplete payload is kept and compared)",
)];

/// Document fields that are not dumped, with the reason. Every other field of [`CadDocument`] is dumped (see the
/// destructuring in [`write_native_dump`], which lists each one).
pub const EXCLUDED_DOCUMENT_FIELDS: &[(&str, &str)] = &[
    ("maintenance_version", "dwg-image: byte 0x0B of the DWG file header (DXF carries it as $ACADMAINTVER, a header variable)"),
    ("source_app_id_order", "provenance: 'reader provenance for compatibility projections'"),
    ("legacy_dxf_tables", "provenance: 'source symbol tables in file order ... R12 compatibility'"),
    ("legacy_dxf_blocks", "provenance: 'source BLOCK/ENDBLK pairs in decode order'"),
    ("source_explicit_handle_max", "provenance: 'maximum explicit source identity seen by the DXF reader'"),
    ("source_entity_owners", "provenance: 'source owners ... captured before entity readers normalize child ownership'"),
    ("notifications", "diagnostics collected during the read, not drawing data"),
    ("entity_index", "derived index handle -> position of `entities`"),
    ("block_visibility_params", "derived side view of `objects` ('the objects themselves are still kept verbatim in objects')"),
    ("context_scales", "derived side view of `objects`"),
    ("block_representations", "derived side view of `objects`"),
    ("fields", "derived side view of the FIELD objects"),
    ("dwg_summary_timestamps", "dwg-image: raw (julian day, ms) pairs of the SummaryInfo dates; the dates are dumped in `summary_info`"),
    ("source_path", "provenance: 'filesystem path the document was read from'"),
    ("dgn_ls_definitions", "derived side view of `objects` ('read-side view; the objects stay verbatim')"),
    ("dgn_ls_components", "derived side view of `objects`"),
    ("block_entity_handles", "dwg-image: 'original BLOCK_HEADER entity handles from the DWG binary ... used by the writer to produce owned_object_count'"),
    ("dwg_source_version", "provenance: DWG version the document was read from"),
    ("acis_sab_handles", "dwg-image: transient 'DWG read artifact; empty for new/DXF documents'"),
    ("raw_acds_data", "dwg-image: original AcDsPrototype_1b section bytes"),
    ("r12_header_raw", "dwg-image: original AC1009 fixed-layout header bytes"),
    ("raw_records", "dwg-image: debug aid, every source DWG record verbatim"),
    ("original_objects", "dwg-image: copy of the objects as read, for the DWG writer"),
    ("raw_record_owners", "dwg-image: owners of the verbatim records"),
    ("raw_acds_fingerprint", "dwg-image: hashes of the AcDs section bodies"),
    ("dwg_data_store_handles", "dwg-image: objects whose record points into the AcDs data store"),
    ("section_view_style", "derived side view of `objects` (decoded for rendering)"),
    ("view_rep_refs", "derived side view of `objects`"),
    ("section_view_reps", "derived side view of `objects`"),
    ("next_handle", "allocator state; the document's $HANDSEED is a header variable and is compared"),
];

// ---------------------------------------------------------------------------------------------------------
// Debug text -> JSON tree
// ---------------------------------------------------------------------------------------------------------

struct Parser<'a> {
    s: &'a [u8],
    text: &'a str,
    i: usize,
}

type PResult<T> = Result<T, ()>;

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

impl<'a> Parser<'a> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn eat(&mut self, c: u8) -> bool {
        self.ws();
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn ident(&mut self) -> &'a str {
        let start = self.i;
        while self.i < self.s.len() {
            let c = self.s[self.i];
            if c == b':' && self.s.get(self.i + 1) == Some(&b':') && self.s.get(self.i + 2).is_some_and(|c| is_ident_start(*c)) {
                self.i += 2;
            } else if c.is_ascii_alphanumeric() || c == b'_' {
                self.i += 1;
            } else {
                break;
            }
        }
        &self.text[start..self.i]
    }

    /// Skip a bracketed region (strings respected) starting at the opening bracket; used when the content is
    /// not a plain value list (a bit-flag set prints `A | B`).
    fn skip_balanced(&mut self) -> PResult<&'a str> {
        let start = self.i;
        let mut depth = 0usize;
        while self.i < self.s.len() {
            match self.s[self.i] {
                b'"' => {
                    self.i += 1;
                    while self.i < self.s.len() && self.s[self.i] != b'"' {
                        if self.s[self.i] == b'\\' {
                            self.i += 1;
                        }
                        self.i += 1;
                    }
                }
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    depth = depth.checked_sub(1).ok_or(())?;
                    if depth == 0 {
                        self.i += 1;
                        return Ok(&self.text[start..self.i]);
                    }
                }
                _ => {}
            }
            self.i += 1;
        }
        Err(())
    }

    fn string(&mut self) -> PResult<Value> {
        // at the opening quote
        self.i += 1;
        let mut out = String::new();
        loop {
            let rest = &self.text[self.i..];
            let c = rest.chars().next().ok_or(())?;
            self.i += c.len_utf8();
            match c {
                '"' => return Ok(Value::String(out)),
                '\\' => {
                    let e = self.text[self.i..].chars().next().ok_or(())?;
                    self.i += e.len_utf8();
                    match e {
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        '0' => out.push('\0'),
                        '\\' | '"' | '\'' => out.push(e),
                        'u' => {
                            if self.peek() != Some(b'{') {
                                return Err(());
                            }
                            let end = self.text[self.i..].find('}').ok_or(())? + self.i;
                            let code = u32::from_str_radix(&self.text[self.i + 1..end], 16).map_err(|_| ())?;
                            out.push(char::from_u32(code).ok_or(())?);
                            self.i = end + 1;
                        }
                        _ => return Err(()),
                    }
                }
                _ => out.push(c),
            }
        }
    }

    fn atom_text(&mut self) -> &'a str {
        // a number or an unparsed word: up to the next delimiter at this level
        let start = self.i;
        while self.i < self.s.len() && !matches!(self.s[self.i], b',' | b')' | b']' | b'}') {
            self.i += 1;
        }
        self.text[start..self.i].trim_end()
    }

    fn atom(text: &str) -> Value {
        match text {
            "true" => return Value::Bool(true),
            "false" => return Value::Bool(false),
            "None" => return Value::Null,
            "NaN" | "inf" | "-inf" => return Value::String(text.to_string()),
            _ => {}
        }
        if let Ok(v) = text.parse::<i64>() {
            return Value::Number(v.into());
        }
        if let Ok(v) = text.parse::<u64>() {
            return Value::Number(v.into());
        }
        if text.bytes().next().is_some_and(|c| c.is_ascii_digit() || c == b'-') && !text.starts_with("0x") {
            if let Ok(v) = text.parse::<f64>() {
                if let Some(n) = Number::from_f64(v) {
                    // a float prints with a point or exponent; an integer text that overflowed stays a string
                    if text.contains(['.', 'e', 'E']) {
                        return Value::Number(n);
                    }
                }
            }
        }
        Value::String(text.to_string())
    }

    fn value(&mut self) -> PResult<Value> {
        self.ws();
        let c = self.peek().ok_or(())?;
        match c {
            b'"' => self.string(),
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                while !self.eat(b']') {
                    items.push(self.value()?);
                    self.eat(b',');
                }
                Ok(compact_bytes(items))
            }
            b'(' => {
                self.i += 1;
                let mut items = Vec::new();
                while !self.eat(b')') {
                    items.push(self.value()?);
                    self.eat(b',');
                }
                Ok(Value::Array(items))
            }
            b'{' => {
                self.i += 1;
                let mut entries: Vec<(String, Value)> = Vec::new();
                let mut set: Vec<Value> = Vec::new();
                while !self.eat(b'}') {
                    let k = self.value()?;
                    if self.eat(b':') {
                        let v = self.value()?;
                        entries.push((k.to_string(), Value::Array(vec![k, v])));
                    } else {
                        set.push(k);
                    }
                    self.eat(b',');
                }
                if !entries.is_empty() && set.is_empty() {
                    entries.sort_by(|a, b| a.0.cmp(&b.0));
                    Ok(Value::Array(entries.into_iter().map(|e| e.1).collect()))
                } else {
                    set.sort_by_key(|v| v.to_string());
                    Ok(Value::Array(set))
                }
            }
            b'\'' => {
                let start = self.i;
                self.i += 1;
                while self.i < self.s.len() && self.s[self.i] != b'\'' {
                    if self.s[self.i] == b'\\' {
                        self.i += 1;
                    }
                    self.i += 1;
                }
                self.i += 1;
                Ok(Value::String(self.text[start..self.i.min(self.s.len())].to_string()))
            }
            c if is_ident_start(c) => {
                let save = self.i;
                let name = self.ident();
                self.ws();
                match self.peek() {
                    Some(b'{') => {
                        let before = self.i;
                        match self.struct_body(name) {
                            Ok(v) => Ok(v),
                            Err(()) => {
                                self.i = before;
                                let raw = self.skip_balanced()?;
                                Ok(raw_node(name, raw))
                            }
                        }
                    }
                    Some(b'(') => {
                        let before = self.i;
                        match self.tuple_body(name) {
                            Ok(v) => Ok(v),
                            Err(()) => {
                                self.i = before;
                                let raw = self.skip_balanced()?;
                                Ok(raw_node(name, raw))
                            }
                        }
                    }
                    _ => {
                        self.i = save;
                        // a bare word; a flag union (`A | B`) or a path continues after spaces
                        let rest = self.atom_text();
                        Ok(Self::atom(rest))
                    }
                }
            }
            _ => {
                let text = self.atom_text();
                if text.is_empty() {
                    return Err(());
                }
                Ok(Self::atom(text))
            }
        }
    }

    fn struct_body(&mut self, name: &str) -> PResult<Value> {
        self.i += 1; // {
        let mut map = Map::new();
        map.insert("$".into(), Value::String(name.to_string()));
        loop {
            self.ws();
            if self.eat(b'}') {
                break;
            }
            self.ws();
            if !self.peek().is_some_and(is_ident_start) {
                return Err(());
            }
            let field = self.ident();
            if !self.eat(b':') {
                return Err(());
            }
            let v = self.value()?;
            if !is_excluded(name, field) {
                map.insert(field.to_string(), v);
            }
            self.eat(b',');
        }
        for (strukt, field, flag, _) in EXCLUDED_FIELDS_WHEN {
            if *strukt == name && map.get(*flag) == Some(&Value::Bool(true)) {
                map.remove(*field);
            }
        }
        Ok(Value::Object(map))
    }

    fn tuple_body(&mut self, name: &str) -> PResult<Value> {
        self.i += 1; // (
        let mut items = Vec::new();
        while !self.eat(b')') {
            items.push(self.value()?);
            if !self.eat(b',') {
                self.ws();
                if self.peek() != Some(b')') {
                    return Err(());
                }
            }
        }
        if name == "Some" && items.len() == 1 {
            return Ok(items.pop().unwrap());
        }
        if name == "Handle" && items.len() == 1 {
            return Ok(items.pop().unwrap());
        }
        let mut map = Map::new();
        map.insert("$".into(), Value::String(name.to_string()));
        for (n, v) in items.into_iter().enumerate() {
            map.insert(n.to_string(), v);
        }
        Ok(Value::Object(map))
    }
}

fn raw_node(name: &str, raw: &str) -> Value {
    let mut map = Map::new();
    map.insert("$".into(), Value::String(name.to_string()));
    map.insert("raw".into(), Value::String(raw.to_string()));
    Value::Object(map)
}

fn is_excluded(strukt: &str, field: &str) -> bool {
    EXCLUDED_FIELDS
        .iter()
        .any(|(s, f, _)| *f == field && (*s == "*" || *s == strukt))
}

/// A list of 32 or more small integers (a byte vector) becomes one hex string.
fn compact_bytes(items: Vec<Value>) -> Value {
    if items.len() >= 32
        && items
            .iter()
            .all(|v| v.as_u64().is_some_and(|n| n <= 255))
    {
        let mut hex = String::with_capacity(items.len() * 2);
        for v in &items {
            hex.push_str(&format!("{:02x}", v.as_u64().unwrap()));
        }
        return Value::String(format!("bytes:{hex}"));
    }
    Value::Array(items)
}

/// Parse the `Debug` text of a model value into a tree; text the parser does not understand is kept raw.
pub fn debug_tree(text: &str) -> Value {
    let mut p = Parser {
        s: text.as_bytes(),
        text,
        i: 0,
    };
    match p.value() {
        Ok(v) => {
            p.ws();
            if p.i == p.s.len() {
                v
            } else {
                raw_node("<unparsed>", text)
            }
        }
        Err(()) => raw_node("<unparsed>", text),
    }
}

fn tree<T: Debug>(value: &T) -> Value {
    debug_tree(&format!("{value:?}"))
}

// ---------------------------------------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------------------------------------

fn emit(out: &mut dyn Write, key: &str, value: &Value) -> io::Result<()> {
    out.write_all(b"{\"k\":")?;
    serde_json::to_writer(&mut *out, key)?;
    out.write_all(b",\"v\":")?;
    serde_json::to_writer(&mut *out, value)?;
    out.write_all(b"}\n")
}

fn emit_table<T: TableEntry + Debug>(
    out: &mut dyn Write,
    name: &str,
    table: &Table<T>,
    scratch: &mut Vec<(String, Value)>,
) -> io::Result<()> {
    scratch.clear();
    for entry in table.iter() {
        scratch.push((
            format!("{name}/entry/{}/{:016X}", entry.name(), entry.handle().value()),
            tree(entry),
        ));
    }
    // The table itself: its handle. The order of the records of a symbol table is storage order (the records are
    // looked up by name) and is not dumped.
    let mut head = Map::new();
    head.insert("handle".into(), Value::Number(table.handle().value().into()));
    scratch.push((format!("{name}/head"), Value::Object(head)));
    scratch.sort_by(|a, b| a.0.cmp(&b.0));
    for (k, v) in scratch.iter() {
        emit(out, k, v)?;
    }
    Ok(())
}

fn hex(handle: Handle) -> String {
    format!("{:016X}", handle.value())
}

/// The struct body of a record tree: a variant wraps its struct in member `"0"`.
fn inner_mut(tree: &mut Value) -> Option<&mut Map<String, Value>> {
    if tree.get("0").is_some_and(Value::is_object) {
        tree.get_mut("0").and_then(Value::as_object_mut)
    } else {
        tree.as_object_mut()
    }
}

/// The EED blocks of an entity that the DWG reader kept only as raw bytes (`ExtendedData::raw_dwg_eed`, excluded
/// from the entity record): a block whose application has a structured record is that record, which is dumped with
/// the entity; any other block (unknown application, or bytes the decoder rejects) is dumped here, decoded when it
/// can be, so XDATA a DXF cannot receive is a difference rather than nothing.
fn undecoded_entity_eed(
    doc: &CadDocument,
    data: &crate::xdata::ExtendedData,
    wide: bool,
    layer_name: &dyn Fn(u64) -> Option<String>,
) -> Option<Value> {
    let mut rest = crate::xdata::ExtendedData::new();
    for (application, bytes) in &data.raw_dwg_eed {
        let name = doc
            .app_ids
            .iter()
            .find(|app| app.handle.value() == *application)
            .map(|app| app.name.clone());
        if name.as_ref().is_some_and(|name| data.get_record(name).is_some()) {
            continue;
        }
        let decoded = name.as_ref().and_then(|_| crate::io::dwg::eed_codec::decode_values(bytes, wide, layer_name));
        let mut record =
            crate::xdata::ExtendedDataRecord::new(name.unwrap_or_else(|| format!("<application {application}>")));
        record.values = decoded.unwrap_or_else(|| vec![crate::xdata::XDataValue::BinaryData(bytes.clone())]);
        rest.add_record(record);
    }
    (!rest.is_empty()).then(|| tree(&rest))
}

/// The records the header selects by name ($CLAYER, $TEXTSTYLE, $CELTYPE, $DIMSTYLE, $CMLSTYLE, $DIMTXSTY,
/// $DIMLTYPE, $DIMLTEX1/2, $DIMBLK/1/2, $DIMLDRBLK, $UCSNAME, $UCSBASE, $PUCSNAME, $PUCSBASE). A DWG header stores
/// a handle for each (excluded as `dwg-only`) and its reader does not fill every name field, which then keeps its
/// default; a DXF header stores the name, and its reader leaves the handles at the default records. So for a
/// document read from a DWG the selected record is the one the handle points at (when it resolves), otherwise the
/// name field: a writer that drops the current record of a DWG is a difference here. The names are compared without
/// regard to case, as AutoCAD looks them up; the exact name fields are compared in `doc/header`.
fn header_references(doc: &CadDocument) -> Value {
    fn find<T: TableEntry>(table: &Table<T>, handle: Handle) -> Option<String> {
        table.iter().find(|entry| entry.handle() == handle).map(|entry| entry.name().to_string())
    }
    let h = &doc.header;
    let from_dwg = doc.dwg_source_version.is_some();
    let mline_style = |handle: Handle| {
        doc.objects.get(&handle).and_then(|object| match object {
            crate::objects::ObjectType::MLineStyle(style) => Some(style.name.clone()),
            _ => None,
        })
    };
    let refs: [(&str, Handle, &str, Option<String>); 17] = [
        ("current_layer", h.current_layer_handle, &h.current_layer_name, find(&doc.layers, h.current_layer_handle)),
        ("current_text_style", h.current_text_style_handle, &h.current_text_style_name, find(&doc.text_styles, h.current_text_style_handle)),
        ("current_linetype", h.current_linetype_handle, &h.current_linetype_name, find(&doc.line_types, h.current_linetype_handle)),
        ("current_dimstyle", h.current_dimstyle_handle, &h.current_dimstyle_name, find(&doc.dim_styles, h.current_dimstyle_handle)),
        ("current_multiline_style", h.current_multiline_style_handle, &h.multiline_style, mline_style(h.current_multiline_style_handle)),
        ("dim_text_style", h.dim_text_style_handle, &h.dim_text_style_name, find(&doc.text_styles, h.dim_text_style_handle)),
        ("dim_linetype", h.dim_linetype_handle, &h.dim_linetype_name, find(&doc.line_types, h.dim_linetype_handle)),
        ("dim_linetype1", h.dim_linetype1_handle, &h.dim_linetype1_name, find(&doc.line_types, h.dim_linetype1_handle)),
        ("dim_linetype2", h.dim_linetype2_handle, &h.dim_linetype2_name, find(&doc.line_types, h.dim_linetype2_handle)),
        ("dim_arrow_block", h.dim_arrow_block_handle, &h.dim_arrow_block, find(&doc.block_records, h.dim_arrow_block_handle)),
        ("dim_arrow_block1", h.dim_arrow_block1_handle, &h.dim_arrow_block1, find(&doc.block_records, h.dim_arrow_block1_handle)),
        ("dim_arrow_block2", h.dim_arrow_block2_handle, &h.dim_arrow_block2, find(&doc.block_records, h.dim_arrow_block2_handle)),
        ("dim_leader_arrow_block", h.dim_leader_arrow_block_handle, &h.dim_leader_arrow_block, find(&doc.block_records, h.dim_leader_arrow_block_handle)),
        ("model_space_ucs", h.model_space_ucs_handle, &h.model_space_ucs_name, find(&doc.ucss, h.model_space_ucs_handle)),
        ("model_space_ucs_base", h.model_space_ucs_base_handle, &h.ucs_base, find(&doc.ucss, h.model_space_ucs_base_handle)),
        ("paper_space_ucs", h.paper_space_ucs_handle, &h.paper_space_ucs_name, find(&doc.ucss, h.paper_space_ucs_handle)),
        ("paper_space_ucs_base", h.paper_space_ucs_base_handle, &h.paper_space_ucs_base_name, find(&doc.ucss, h.paper_space_ucs_base_handle)),
    ];
    let mut map = Map::new();
    for (key, handle, name, target) in refs {
        let selected = match target {
            Some(target) if from_dwg && !handle.is_null() => target,
            _ => name.to_string(),
        };
        // Symbol and style names are case-insensitive (a DWG record "STANDARD" is the DXF "$CMLSTYLE Standard").
        map.insert(key.into(), Value::String(selected.to_uppercase()));
    }
    Value::Object(map)
}

/// Write the native dump of `doc` (JSON lines, sorted by key).
pub fn write_native_dump(doc: &CadDocument, out: &mut dyn Write) -> io::Result<()> {
    // Exhaustive: a new document field must be dumped below or named in EXCLUDED_DOCUMENT_FIELDS.
    let CadDocument {
        version,
        maintenance_version: _,
        header,
        layers,
        line_types,
        text_styles,
        block_records,
        dim_styles,
        app_ids,
        source_app_id_order: _,
        legacy_dxf_tables: _,
        legacy_dxf_blocks: _,
        source_explicit_handle_max: _,
        source_entity_owners: _,
        views,
        vports,
        ucss,
        vx_table,
        vx_control_entries,
        classes,
        notifications: _,
        entities,
        entity_index: _,
        objects,
        block_visibility_params: _,
        context_scales: _,
        block_representations: _,
        fields: _,
        summary_info,
        dwg_summary_timestamps: _,
        source_path: _,
        dgn_ls_definitions: _,
        dgn_ls_components: _,
        eed_by_handle,
        object_xdata,
        xdic_by_handle,
        reactors_by_handle,
        block_entity_handles: _,
        dwg_source_version: _,
        preview,
        acis_sab_handles: _,
        raw_acds_data: _,
        r12_header_raw: _,
        raw_records: _,
        original_objects: _,
        raw_record_owners: _,
        raw_acds_fingerprint: _,
        dwg_data_store_handles: _,
        section_view_style: _,
        view_rep_refs: _,
        section_view_reps: _,
        next_handle: _,
    } = doc;
    let mut scratch = Vec::new();
    emit_table(out, "table.app_id", app_ids, &mut scratch)?;
    emit_table(out, "table.block_record", block_records, &mut scratch)?;
    emit_table(out, "table.dim_style", dim_styles, &mut scratch)?;
    emit_table(out, "table.layer", layers, &mut scratch)?;
    emit_table(out, "table.line_type", line_types, &mut scratch)?;
    emit_table(out, "table.text_style", text_styles, &mut scratch)?;
    emit_table(out, "table.ucs", ucss, &mut scratch)?;
    emit_table(out, "table.view", views, &mut scratch)?;
    emit_table(out, "table.vport", vports, &mut scratch)?;
    emit_table(out, "table.vx", vx_table, &mut scratch)?;
    emit(out, "doc/version", &tree(version))?;
    emit(out, "doc/header", &tree(header))?;
    // The classes of the file: the records a DXF CLASSES section had, or the DWG class list (the collection also
    // carries the writer's registry, which is not a file property).
    let file_classes: Vec<&crate::classes::DxfClass> = match classes.source_classes() {
        Some(source) => source.iter().collect(),
        None => classes.iter().collect(),
    };
    emit(out, "doc/classes", &tree(&file_classes))?;
    emit(out, "doc/summary_info", &tree(summary_info))?;
    emit(out, "doc/preview", &tree(preview))?;
    emit(out, "doc/vx_control_entries", &tree(vx_control_entries))?;
    emit(out, "doc/header_references", &header_references(doc))?;
    // BLOCK / ENDBLK markers: the DWG reader keeps them as entities, the DXF reader folds their common data into
    // the block record (`block_entity_common`, `block_end_common`). One record per block holds either.
    let mut marker_handles: std::collections::HashSet<Handle> = std::collections::HashSet::new();
    let mut markers: Vec<(Handle, Value)> = Vec::new();
    for record in block_records.iter() {
        let side = |handle: Handle, folded: &Option<Box<crate::entities::EntityCommon>>| -> Value {
            match doc.get_entity(handle) {
                Some(entity) if matches!(entity, EntityType::Block(_) | EntityType::BlockEnd(_)) => {
                    tree(entity.common())
                }
                _ => folded.as_ref().map_or(Value::Null, |common| tree(&**common)),
            }
        };
        markers.push((
            record.handle,
            serde_json::json!({
                "begin": side(record.block_entity_handle, &record.block_entity_common),
                "end": side(record.block_end_handle, &record.block_end_common),
            }),
        ));
        marker_handles.insert(record.block_entity_handle);
        marker_handles.insert(record.block_end_handle);
    }
    markers.sort_by_key(|(handle, _)| *handle);
    for (handle, value) in &markers {
        emit(out, &format!("block_marker/{}", hex(*handle)), value)?;
    }
    // entities: sorted by handle; a repeated handle gets a position suffix. Their order in the vector is storage
    // order (the order inside a block is `BlockRecord::entity_handles`); markers are dumped above.
    let mut order: Vec<(u64, usize)> = entities
        .iter()
        .enumerate()
        .filter(|(_, e)| !(matches!(&***e, EntityType::Block(_) | EntityType::BlockEnd(_)) && marker_handles.contains(&e.common().handle)))
        .map(|(i, e)| (e.common().handle.value(), i))
        .collect();
    order.sort();
    let wide = *version >= crate::types::DxfVersion::AC1021;
    let layer_name = |handle: u64| layers.iter().find(|l| l.handle.value() == handle).map(|l| l.name.clone());
    let mut previous = None;
    let mut dup = 0usize;
    for (h, i) in order {
        dup = if previous == Some(h) { dup + 1 } else { 0 };
        previous = Some(h);
        let key = if dup == 0 {
            format!("entity/{h:016X}")
        } else {
            format!("entity/{h:016X}+{dup}")
        };
        emit(out, &key, &tree(&*entities[i]))?;
        let raw = &entities[i].common().extended_data;
        if let Some(value) = undecoded_entity_eed(doc, raw, wide, &layer_name) {
            emit(out, &format!("entity_xdata_raw/{}", &key["entity/".len()..]), &value)?;
        }
    }
    // Objects. The reactors and extension dictionary of an object live in its own fields (DXF reader) or in the
    // document maps (DWG reader); both are folded into the `reactors/` and `xdictionary/` records below.
    let mut object_handles: Vec<Handle> = objects.keys().copied().collect();
    object_handles.sort();
    let mut reactors_of: HashMap<Handle, Vec<Value>> = HashMap::new();
    let mut xdictionary_of: HashMap<Handle, Value> = HashMap::new();
    for (handle, list) in reactors_by_handle {
        reactors_of.insert(*handle, list.iter().map(|h| Value::Number(h.value().into())).collect());
    }
    for (handle, dictionary) in xdic_by_handle {
        xdictionary_of.insert(*handle, Value::Number(dictionary.value().into()));
    }
    for handle in &object_handles {
        let mut value = tree(&objects[handle]);
        if let Some(inner) = inner_mut(&mut value) {
            if let Some(Value::Array(own)) = inner.remove("reactors") {
                let merged = reactors_of.entry(*handle).or_default();
                for reactor in own {
                    if !merged.contains(&reactor) {
                        merged.push(reactor);
                    }
                }
            }
            if let Some(own) = inner.remove("xdictionary_handle") {
                if !own.is_null() {
                    xdictionary_of.insert(*handle, own);
                }
            }
        }
        emit(out, &format!("object/{}", hex(*handle)), &value)?;
    }
    // XDATA of objects and table records: structured records, plus the records still held as raw DWG EED.
    let mut xdata_handles: Vec<Handle> = object_xdata.keys().chain(eed_by_handle.keys()).copied().collect();
    xdata_handles.sort();
    xdata_handles.dedup();
    for handle in xdata_handles {
        let mut data = object_xdata.get(&handle).cloned().unwrap_or_default();
        for (application, bytes) in eed_by_handle.get(&handle).into_iter().flatten() {
            let name = app_ids
                .iter()
                .find(|app| app.handle.value() == *application)
                .map(|app| app.name.clone());
            let decoded = name.as_ref().and_then(|_| crate::io::dwg::eed_codec::decode_values(bytes, wide, &layer_name));
            match (name, decoded) {
                (Some(name), Some(values)) if data.get_record(&name).is_none() => {
                    let mut record = crate::xdata::ExtendedDataRecord::new(name);
                    record.values = values;
                    data.add_record(record);
                }
                (Some(_), Some(_)) => {}
                (name, _) => {
                    // not decodable (unknown application or malformed): the bytes themselves
                    let mut record = crate::xdata::ExtendedDataRecord::new(
                        name.unwrap_or_else(|| format!("<application {application}>")),
                    );
                    record.values.push(crate::xdata::XDataValue::BinaryData(bytes.clone()));
                    data.add_record(record);
                }
            }
        }
        if !data.is_empty() {
            emit(out, &format!("object_xdata/{}", hex(handle)), &tree(&data))?;
        }
    }
    let mut reactor_handles: Vec<Handle> = reactors_of.keys().copied().collect();
    reactor_handles.sort();
    for handle in reactor_handles {
        let list = &reactors_of[&handle];
        if !list.is_empty() {
            emit(out, &format!("reactors/{}", hex(handle)), &Value::Array(list.clone()))?;
        }
    }
    let mut xdictionary_handles: Vec<Handle> = xdictionary_of.keys().copied().collect();
    xdictionary_handles.sort();
    for handle in xdictionary_handles {
        emit(out, &format!("xdictionary/{}", hex(handle)), &xdictionary_of[&handle])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::{Block, BlockEnd, EntityCommon, Line};
    use crate::objects::{Dictionary, ObjectType};
    use crate::tables::BlockRecord;
    use crate::types::{DxfVersion, Vector3};

    fn dump(doc: &CadDocument) -> HashMap<String, Value> {
        let mut bytes = Vec::new();
        write_native_dump(doc, &mut bytes).unwrap();
        String::from_utf8(bytes)
            .unwrap()
            .lines()
            .map(|line| {
                let record: Value = serde_json::from_str(line).unwrap();
                (record["k"].as_str().unwrap().to_string(), record["v"].clone())
            })
            .collect()
    }

    #[test]
    fn debug_text_becomes_a_tree() {
        let tree = debug_tree(
            r#"Thing { name: "a\"b\n\u{e9}", n: 3, f: 1.5, neg: -2, big: 18446744073709551615, flag: true, o: Some(Handle(255)), none: None, unit: ByLayer, list: [1, 2], pair: (1, "x"), nested: Inner { v: 0.1 } }"#,
        );
        assert_eq!(tree["$"], "Thing");
        assert_eq!(tree["name"], "a\"b\n\u{e9}");
        assert_eq!(tree["n"], 3);
        assert_eq!(tree["f"], 1.5);
        assert_eq!(tree["neg"], -2);
        assert_eq!(tree["big"], 18446744073709551615u64);
        assert_eq!(tree["flag"], true);
        assert_eq!(tree["o"], 255, "Some(Handle(n)) is the number");
        assert!(tree["none"].is_null());
        assert_eq!(tree["unit"], "ByLayer");
        assert_eq!(tree["list"], serde_json::json!([1, 2]));
        assert_eq!(tree["pair"], serde_json::json!([1, "x"]));
        assert_eq!(tree["nested"]["$"], "Inner");
        assert_eq!(tree["nested"]["v"], 0.1);
    }

    #[test]
    fn maps_and_sets_do_not_depend_on_their_order() {
        let a = debug_tree(r#"{"b": 2, "a": 1}"#);
        let b = debug_tree(r#"{"a": 1, "b": 2}"#);
        assert_eq!(a, b);
        assert_eq!(debug_tree("{3, 1, 2}"), debug_tree("{1, 2, 3}"));
    }

    #[test]
    fn floats_keep_their_exact_value_and_a_flag_set_is_kept_raw_not_lost() {
        let value = 3.522098985561804_f64;
        let tree = debug_tree(&format!("{value:?}"));
        assert_eq!(tree.as_f64(), Some(value));
        let flags = debug_tree("Flags(A | B)");
        assert_eq!(flags["$"], "Flags");
        assert!(flags["0"].as_str().is_some() || flags["raw"].as_str().is_some(), "{flags}");
        // Text the parser cannot read is kept whole, never dropped.
        assert_eq!(debug_tree("(((")["$"], "<unparsed>");
        // A long byte vector is one hex string.
        let bytes = format!("[{}]", (0..40).map(|n| n.to_string()).collect::<Vec<_>>().join(", "));
        assert!(debug_tree(&bytes).as_str().unwrap().starts_with("bytes:0001"));
    }

    #[test]
    fn excluded_fields_are_dropped_by_name_and_everything_else_is_kept() {
        let tree = debug_tree(
            r#"EntityProperties { layer: "0", layer_handle: Some(Handle(16)), dwg_color_source: Some((256, 0)), invisible: false, color_name: None }"#,
        );
        assert!(tree.get("dwg_color_source").is_none(), "named in EXCLUDED_FIELDS");
        assert!(tree.get("layer_handle").is_none(), "named in EXCLUDED_FIELDS");
        assert_eq!(tree["layer"], "0");
        assert!(tree.get("invisible").is_some() && tree.get("color_name").is_some());
        // A field of the same name in a struct that is not listed stays.
        let other = debug_tree("Other { dwg_color_source: 1 }");
        assert_eq!(other["dwg_color_source"], 1);
        // The conditional exclusion applies only when the flag is true.
        let complete = debug_tree("XRecord { entries_complete: true, raw_data: [1, 2] }");
        assert!(complete.get("raw_data").is_none());
        let partial = debug_tree("XRecord { entries_complete: false, raw_data: [1, 2] }");
        assert_eq!(partial["raw_data"], serde_json::json!([1, 2]));
    }

    /// Every name in the exclusion lists is a real struct field, so a rename of a model field cannot silently
    /// turn an exclusion into a no-op that compares the field again under another name.
    #[test]
    fn every_excluded_name_is_a_field_of_the_model() {
        fn sources(dir: &std::path::Path, out: &mut String) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    if path.file_name().is_some_and(|n| n != "compat") {
                        sources(&path, out);
                    }
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push_str(&std::fs::read_to_string(&path).unwrap());
                    out.push('\n');
                }
            }
        }
        let mut text = String::new();
        sources(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut text);
        let has_struct = |name: &str| {
            text.contains(&format!("pub struct {name} ")) || text.contains(&format!("pub struct {name}<")) || text.contains(&format!("pub(crate) struct {name} "))
        };
        let has_field = |name: &str| {
            text.contains(&format!("pub {name}:")) || text.contains(&format!("pub(crate) {name}:")) || text.contains(&format!("    {name}: "))
        };
        for (strukt, field, reason) in EXCLUDED_FIELDS {
            assert!(*strukt == "*" || has_struct(strukt), "no struct {strukt}");
            assert!(has_field(field), "no field {field} ({strukt})");
            assert!(reason.len() > 20, "{strukt}.{field}: no reason");
        }
        for (strukt, field, flag, reason) in EXCLUDED_FIELDS_WHEN {
            assert!(has_struct(strukt) && has_field(field) && has_field(flag), "{strukt}.{field}");
            assert!(reason.len() > 20);
        }
        for (field, reason) in EXCLUDED_DOCUMENT_FIELDS {
            assert!(has_field(field), "no document field {field}");
            assert!(reason.len() > 10);
        }
        // Each field is listed once.
        let mut seen = std::collections::HashSet::new();
        for (strukt, field, _) in EXCLUDED_FIELDS {
            assert!(seen.insert((strukt, field)), "{strukt}.{field} listed twice");
        }
    }

    #[test]
    fn the_dump_has_one_sorted_record_per_object_and_stays_the_same_when_written_twice() {
        let mut doc = CadDocument::with_version(DxfVersion::AC1032);
        let line = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 2.0, 3.0));
        doc.add_entity(EntityType::Line(line)).unwrap();
        let first = dump(&doc);
        let second = dump(&doc);
        assert_eq!(first, second);
        assert!(first.contains_key("doc/header") && first.contains_key("doc/version"));
        let entity = first.iter().find(|(k, _)| k.starts_with("entity/")).unwrap().1;
        assert_eq!(entity["$"], "Line");
        assert_eq!(entity["0"]["end"]["z"], 3.0);
    }

    #[test]
    fn reactors_and_extension_dictionaries_are_one_record_whichever_reader_stored_them() {
        // DXF reader: fields of the object.
        let mut by_field = CadDocument::with_version(DxfVersion::AC1032);
        let mut dictionary = Dictionary::new();
        dictionary.handle = Handle::new(0x50);
        dictionary.reactors = vec![Handle::new(0x51)];
        dictionary.xdictionary_handle = Some(Handle::new(0x52));
        by_field.objects.insert(dictionary.handle, ObjectType::Dictionary(dictionary.clone()));
        // DWG reader: the document maps.
        let mut by_map = CadDocument::with_version(DxfVersion::AC1032);
        dictionary.reactors.clear();
        dictionary.xdictionary_handle = None;
        by_map.objects.insert(dictionary.handle, ObjectType::Dictionary(dictionary));
        by_map.reactors_by_handle.insert(Handle::new(0x50), vec![Handle::new(0x51)]);
        by_map.xdic_by_handle.insert(Handle::new(0x50), Handle::new(0x52));
        let (a, b) = (dump(&by_field), dump(&by_map));
        for key in ["reactors/0000000000000050", "xdictionary/0000000000000050", "object/0000000000000050"] {
            assert!(a.contains_key(key), "{key}");
            assert_eq!(a[key], b[key], "{key}");
        }
        assert_eq!(a["reactors/0000000000000050"], serde_json::json!([0x51]));
    }

    #[test]
    fn block_markers_are_one_record_whether_they_are_entities_or_folded_into_the_block_record() {
        let mut common = EntityCommon::new();
        common.handle = Handle::new(0x61);
        common.owner_handle = Handle::new(0x60);
        let mut end_common = EntityCommon::new();
        end_common.handle = Handle::new(0x62);
        end_common.owner_handle = Handle::new(0x60);
        let record = |folded: bool| {
            let mut block = BlockRecord::new("B");
            block.handle = Handle::new(0x60);
            block.block_entity_handle = Handle::new(0x61);
            block.block_end_handle = Handle::new(0x62);
            if folded {
                block.block_entity_common = Some(Box::new(common.clone()));
                block.block_end_common = Some(Box::new(end_common.clone()));
            }
            block
        };
        let mut dwg_like = CadDocument::with_version(DxfVersion::AC1032);
        dwg_like.block_records.add(record(false)).unwrap();
        let mut marker = Block::new("B", Vector3::ZERO);
        marker.common = common.clone();
        dwg_like.entities.push(std::sync::Arc::new(EntityType::Block(marker)));
        let mut end = BlockEnd::new();
        end.common = end_common.clone();
        dwg_like.entities.push(std::sync::Arc::new(EntityType::BlockEnd(end)));
        dwg_like.entity_index.insert(Handle::new(0x61), 0);
        dwg_like.entity_index.insert(Handle::new(0x62), 1);
        let mut dxf_like = CadDocument::with_version(DxfVersion::AC1032);
        dxf_like.block_records.add(record(true)).unwrap();
        let (a, b) = (dump(&dwg_like), dump(&dxf_like));
        assert_eq!(a["block_marker/0000000000000060"], b["block_marker/0000000000000060"]);
        assert!(!a.keys().any(|k| k.starts_with("entity/")), "the marker entities are dumped as the marker");
    }

    #[test]
    fn xdata_still_held_as_raw_dwg_eed_is_dumped_decoded() {
        use crate::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};
        let mut structured = CadDocument::with_version(DxfVersion::AC1032);
        let mut app = crate::tables::AppId::new("MYAPP");
        app.handle = Handle::new(0x70);
        structured.app_ids.add(app.clone()).unwrap();
        let mut data = ExtendedData::new();
        let mut record = ExtendedDataRecord::new("MYAPP");
        record.values = vec![XDataValue::String("text".to_string()), XDataValue::Integer16(7)];
        data.add_record(record.clone());
        structured.object_xdata.insert(Handle::new(0x71), data);

        let mut raw = CadDocument::with_version(DxfVersion::AC1032);
        raw.app_ids.add(app).unwrap();
        let bytes = crate::io::dwg::eed_codec::encode_values_with_encoding(
            true,
            &record.values,
            encoding_rs::WINDOWS_1252.into(),
            30,
            |_| 0,
        );
        raw.eed_by_handle.insert(Handle::new(0x71), vec![(0x70, bytes)]);
        let (a, b) = (dump(&structured), dump(&raw));
        assert_eq!(a["object_xdata/0000000000000071"], b["object_xdata/0000000000000071"]);
    }

    #[test]
    fn the_current_records_of_a_dwg_header_are_the_ones_its_handles_select() {
        let mut dwg = CadDocument::with_version(DxfVersion::AC1032);
        let mut style = crate::tables::TextStyle::new("Notes");
        style.handle = Handle::new(0x90);
        dwg.text_styles.add(style).unwrap();
        dwg.header.current_text_style_handle = Handle::new(0x90);
        dwg.dwg_source_version = Some(DxfVersion::AC1032);
        // The DWG reader left the name at its default; the handle selects "Notes".
        assert_eq!(dwg.header.current_text_style_name, "Standard");
        let refs = &dump(&dwg)["doc/header_references"];
        assert_eq!(refs["current_text_style"], "NOTES");
        // A DXF header names the record; its handle is the default record's and does not count.
        let mut dxf = dwg.clone();
        dxf.dwg_source_version = None;
        assert_eq!(dump(&dxf)["doc/header_references"]["current_text_style"], "STANDARD");
        dxf.header.current_text_style_name = "Notes".to_string();
        assert_eq!(dump(&dxf)["doc/header_references"], *refs);
    }

    #[test]
    fn entity_eed_the_dwg_reader_could_not_structure_is_dumped_and_structured_eed_is_not_repeated() {
        use crate::xdata::{ExtendedDataRecord, XDataValue};
        let mut doc = CadDocument::with_version(DxfVersion::AC1032);
        let mut app = crate::tables::AppId::new("MYAPP");
        app.handle = Handle::new(0x70);
        doc.app_ids.add(app).unwrap();
        let mut line = Line::new();
        line.common.handle = Handle::new(0x80);
        let mut record = ExtendedDataRecord::new("MYAPP");
        record.values = vec![XDataValue::Integer16(7)];
        line.common.extended_data.add_record(record);
        line.common.extended_data.raw_dwg_eed.push((0x70, vec![70, 7, 0]));
        line.common.extended_data.raw_dwg_eed.push((0x99, vec![1, 2, 3]));
        doc.entities.push(std::sync::Arc::new(EntityType::Line(line)));
        let d = dump(&doc);
        let raw = d["entity_xdata_raw/0000000000000080"].to_string();
        assert!(raw.contains("<application 153>") && !raw.contains("MYAPP"), "{raw}");
    }
}
