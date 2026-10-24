//! DXF section writers
//!
//! This module contains writers for each section of a DXF file:
//! HEADER, CLASSES, TABLES, BLOCKS, ENTITIES, and OBJECTS.

mod associative;

use crate::document::CadDocument;
use crate::entities::*;
use crate::error::Result;
use crate::objects::{
    BlockEvalValue, BlockVisibilityParameter, BookColor, DataObject, DataObjectData, Dictionary,
    DictionaryVariable, DictionaryWithDefault, DimSubtype, DynamicBlockData, DynamicBlockObject,
    GeoData, Group, ImageDefinition, ImageDefinitionReactor, Layout, MLineStyle, Material,
    MaterialColor, MaterialMap, MaterialProceduralValue, MaterialTexture, MultiLeaderStyle,
    ObjectContextData, ObjectContextKind, ObjectType, PlotSettings, RasterVariables, Scale,
    SortEntitiesTable, SpatialFilter, TableStyle, VisualStyle, VisualStyleProperty,
    VisualStylePropertyValue, WipeoutVariables, XRecord, XRecordEntry,
};
use crate::tables::*;
use crate::types::{Color, DxfVersion, Handle, LineWeight, Vector3};
use crate::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};

use super::stream_writer::{DxfStreamWriter, DxfStreamWriterExt};
use super::ToDxfDegrees;
use std::collections::{HashMap, HashSet};

/// Sanitize a symbol table record name: strip control characters and
/// characters forbidden by AutoCAD (`< > / \ " : ; ? * | , = \``).
fn sanitize_symbol_name(name: &str) -> String {
    name.chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(
                    c,
                    '<' | '>' | '/' | '\\' | '"' | ':' | ';' | '?' | '*' | '|' | ',' | '=' | '`'
                )
        })
        .collect()
}

/// Whether a 2D heavy polyline can be down-saved as LWPOLYLINE.
///
/// LWPOLYLINE cannot represent 3D polylines, polygon/polyface meshes or
/// curve-/spline-fitted polylines (fit data and vertex flags have no
/// light equivalent), so those keep the legacy POLYLINE/VERTEX/SEQEND
/// form. Plain 2D polylines only carry the CLOSED (1) and PLINEGEN (128)
/// flags, which LWPOLYLINE maps 1:1.
pub(crate) fn polyline2d_downsaves_to_lwpolyline(polyline: &Polyline2D) -> bool {
    let default_common = EntityCommon::new();
    (polyline.flags.bits() & !0x81) == 0
        && polyline.smooth_surface == SmoothSurfaceType::None
        && polyline.vertices.iter().all(|v| {
            v.flags.bits() == 0
                && v.common.handle.is_null()
                && v.common == default_common
        })
        && polyline.seqend.as_deref().is_none_or(|seqend| {
            seqend.common.handle.is_null() && seqend.common == default_common
        })
}

/// The dynamic-block purge preventer, by DXF or DWG class name. A decoded `DynamicBlock` one is written (its
/// version only, without the block pointer the Representation layout carries); an `Unknown` or associative
/// record of that name has no decoded data and is not written.
fn is_unrestorable_assoc_object(type_name: &str) -> bool {
    // Normalize: the DWG class names carry underscores (e.g.
    // ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION) while the DXF records do not.
    let normalized: String = type_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    normalized.starts_with("ACDBDYNAMICBLOCKPURGEPREVENTER")
}

/// The native default application IDs (`AcadAnnotative`, `AcCmTransparency`, `AcAecLayerStandard`) of a
/// document read from a file whose APPID table did not have them, and whose XDATA does not use them.
/// A reader must not find an application the source never registered.
pub(super) fn unused_default_app_ids(document: &CadDocument) -> HashSet<String> {
    let Some(source) = document.source_app_id_order.as_ref() else {
        return HashSet::new();
    };
    let in_source: HashSet<String> = source.iter().map(|name| name.to_ascii_uppercase()).collect();
    let mut candidates: HashSet<String> = ["ACADANNOTATIVE", "ACCMTRANSPARENCY", "ACAECLAYERSTANDARD"]
        .into_iter()
        .map(String::from)
        .filter(|name| !in_source.contains(name))
        .filter(|name| document.app_ids.iter().any(|appid| appid.name().eq_ignore_ascii_case(name)))
        .collect();
    if candidates.is_empty() {
        return candidates;
    }
    // The layer writer registers these two applications' XDATA from the layer fields.
    if document.layers.iter().any(|layer| !layer.transparency.is_opaque()) {
        candidates.remove("ACCMTRANSPARENCY");
    }
    if document.layers.iter().any(|layer| !layer.description.is_empty()) {
        candidates.remove("ACAECLAYERSTANDARD");
    }
    let mut forget = |data: &ExtendedData| {
        for record in data.records() {
            candidates.remove(&record.application_name.to_ascii_uppercase());
        }
    };
    for data in document.object_xdata.values() {
        forget(data);
    }
    for entity in document.entities() {
        forget(&entity.common().extended_data);
    }
    candidates
}

/// How many sort entries of a SORTENTSTABLE need a handle allocated while writing: those without a sort handle
/// and those repeating one (see `write_sort_entities_table`).
pub(super) fn sort_entries_needing_handles(table: &SortEntitiesTable) -> usize {
    let mut used = HashSet::new();
    table
        .entries()
        .filter(|entry| entry.sort_handle.is_null() || !used.insert(entry.sort_handle))
        .count()
}

/// Writes all DXF sections
pub struct SectionWriter<'a, W: DxfStreamWriter> {
    writer: &'a mut W,
    next_handle: u64,
    handle_seed: u64,
    /// DXF version (determines SAT vs SAB format for ACIS data)
    dxf_version: DxfVersion,
    /// Writing the R12 (AC1009) structure: no CLASSES or OBJECTS section, table records and block markers without
    /// handles, the R12 header variables (the stream drops the groups R12 lacks, see `R12Stream`).
    r12: bool,
    /// R12 only: the highest handle the source stated (`source_explicit_handle_max` of a document read from an R12
    /// file, 0 when it stated none, so `$HANDLING` is 0 and no record states a handle); `u64::MAX` for a document
    /// with no such source. With handles on, every entity states its own (an entity added to the document keeps its
    /// handle); a BLOCK end marker the source did not have states one only up to this limit, since above it the
    /// reader allocated the handle and allocates it again.
    r12_handle_limit: u64,
    /// R12 only: the first handle this writer allocates (above every handle of the document).
    r12_first_allocated: u64,
    /// R12 only: the name maps of the converter of `writer/r12.rs` for the entities R12 does not have, built on the
    /// first such entity.
    r12_names: Option<super::r12::R12Names>,
    /// R12 only: the handle the source stated on each BLOCK end marker (the document keeps the one it assigned),
    /// by upper-case block name.
    r12_end_handles: HashMap<String, Handle>,
    /// R12 only: the extended data of each viewport with its `ACAD`/`MVIEW` record brought up to date. An R12
    /// VIEWPORT states its centre, size, status and number only; the rest of the view is that record.
    r12_viewport_xdata: HashMap<Handle, ExtendedData>,
    /// Collected SAB entries: (entity_handle, sab_binary_data) for ACDSDATA section
    sab_entries: Vec<(Handle, Vec<u8>)>,
    /// Whether currently writing paper space entities (for group code 67)
    writing_paper_space: bool,
    /// Viewport IDs (69) derived per paper-space block when the source does
    /// not store them, reserving explicit IDs before filling gaps.
    paper_viewport_ids: HashMap<Handle, i16>,
    /// Set of all handles that will exist in the output DXF.
    /// Used to filter reactor/xdictionary references to non-existent objects.
    valid_handles: HashSet<Handle>,
    /// The built-in `ByBlock` material, which an entity with material flag 1 (ByBlock) names in group 347.
    by_block_material: Option<Handle>,
    /// Objects the output states besides or instead of the document's own: the carriers of the modern cell styles
    /// of table styles, which the document is not changed for.
    carriers: std::rc::Rc<crate::objects::TableStyleCarriers>,
    /// DWG version the document was read from (None for a DXF source or a document built in memory): a field
    /// the DWG record stores is stated even when its value is the native default.
    dwg_source: Option<DxfVersion>,
    /// Names of the layer records by handle, for the viewport-override layers ("0 @ 1") a DWG entity names by handle.
    layer_names: HashMap<Handle, String>,
    /// Handle of the ByLayer linetype (for defaults in MLeader etc.)
    bylayer_linetype_handle: Handle,
    /// Handle of the ByBlock linetype (treated as "unset" for MLeader etc.)
    byblock_linetype_handle: Handle,
    /// Handle of the *Model_Space block record — owner fallback for entities
    /// whose original owner was dropped during conversion (e.g. an application
    /// container object with no DXF representation).
    model_space_handle: Handle,
    /// Handle of the root named-objects dictionary — owner fallback for
    /// dictionaries whose owner was dropped.
    root_dict_handle: Handle,
    /// Normal plot-style placeholder used by layer records.
    normal_plotstyle_handle: Handle,
    /// Text-style names needed by legacy TABLE records, which store names
    /// while the DWG model stores handles.
    text_style_names: HashMap<Handle, String>,
    /// Block-record names needed by legacy TABLE INSERT data.
    block_record_names: HashMap<Handle, String>,
    /// Typed inputs for loft surfaces whose native data contains references.
    loft_input_entities: HashMap<Handle, EmbeddedEntity>,
    /// Extra registration required by the loft-reference XDATA extension.
    loft_reference_appid: Option<Handle>,
    /// Reactors and extension dictionary of the OBJECTS record being written, taken from the document's side
    /// maps; consumed by `write_pending_attachments` in writers whose model type has no such fields.
    pending_attachments: Option<(Vec<Handle>, Option<Handle>)>,
    /// Header variables left out of the HEADER section (see `unchanged_absent_header_variables`).
    omitted_header_variables: HashSet<String>,
}

/// The tag as the source spelled it (a recovered multi-line attribute tag loses a numeric suffix), while the
/// native tag still is the recovered value; an edited tag is written as it is.
pub(crate) fn source_tag<'a>(source: &'a crate::entities::text::TextSource, tag: &'a str) -> &'a str {
    match &source.tag {
        Some((original, recovered)) if recovered == tag => original,
        _ => tag,
    }
}

/// Group 1 of a multiline ATTRIB/ATTDEF. The DXF reader takes the text of such an attribute from its embedded MTEXT
/// and keeps the record's own group 1 (which AutoCAD writes empty) as the source value; while the value is that
/// unedited MTEXT text, the record's own group 1 is written back. Otherwise (or when no embedded MTEXT is written,
/// before R2018) the value is.
pub(crate) fn attribute_own_value<'a>(
    value: &'a str,
    embedded: Option<&MText>,
    source: Option<&'a crate::entities::text::TextSource>,
) -> &'a str {
    let from_embedded = embedded.is_some_and(|mtext| mtext.value == value);
    match source {
        Some(source) if from_embedded && source.decoded_value.as_deref() == Some(value) => {
            source.value.as_deref().unwrap_or(value)
        }
        _ => value,
    }
}

/// A retained source spelling with the characters themselves in place of text DXF caret escapes (`^J`, `^ `):
/// a spelling read from text DXF carries the escapes, one read from binary DXF or DWG the characters. The `\U+XXXX`
/// escapes stay. `decoded` is the value the spelling stands for.
fn source_spelling_characters<'a>(spelled: &'a str, decoded: &str) -> std::borrow::Cow<'a, str> {
    if !spelled.contains('^') {
        return std::borrow::Cow::Borrowed(spelled);
    }
    let uncareted = spelled
        .replace("^J", "\n")
        .replace("^M", "\r")
        .replace("^I", "\t")
        .replace("^ ", "^");
    if crate::io::dxf::code_page::decode_legacy_escapes(&uncareted) == decoded {
        std::borrow::Cow::Owned(uncareted)
    } else {
        std::borrow::Cow::Borrowed(spelled)
    }
}

/// The highest handle an R12 file written from `document` states (see `SectionWriter::r12_handle_limit`).
pub(super) fn r12_handle_limit(document: &CadDocument) -> u64 {
    // The reader keeps the source's table and BLOCK provenance for an R12 DXF only.
    if document.legacy_dxf_tables.is_some() {
        document.source_explicit_handle_max.unwrap_or(0)
    } else {
        u64::MAX
    }
}

/// The header variables of an R12 (AC1009) DXF: those of AutoCAD's own R12 DXF (the AC1009 samples and the
/// BricsCAD `acR12_dxf` exports), `$DWGCODEPAGE`, and the pre-R13 view and drafting state that a document read
/// from an older source keeps in `HeaderVariables::legacy` (R11 kept it in the header, R12 keeps it in the
/// VPORT; the reader of this crate takes either).
const R12_HEADER_VARIABLES: &[&str] = &[
    "$ACADVER", "$DWGCODEPAGE", "$INSBASE", "$EXTMIN", "$EXTMAX", "$LIMMIN", "$LIMMAX", "$ORTHOMODE",
    "$REGENMODE", "$FILLMODE", "$QTEXTMODE", "$MIRRTEXT", "$DRAGMODE", "$LTSCALE", "$OSMODE", "$ATTMODE",
    "$TEXTSIZE", "$TRACEWID", "$TEXTSTYLE", "$CLAYER", "$CELTYPE", "$CECOLOR", "$DIMSCALE", "$DIMASZ",
    "$DIMEXO", "$DIMDLI", "$DIMRND", "$DIMDLE", "$DIMEXE", "$DIMTP", "$DIMTM", "$DIMTXT", "$DIMCEN", "$DIMTSZ",
    "$DIMTOL", "$DIMLIM", "$DIMTIH", "$DIMTOH", "$DIMSE1", "$DIMSE2", "$DIMTAD", "$DIMZIN", "$DIMBLK",
    "$DIMASO", "$DIMSHO", "$DIMPOST", "$DIMAPOST", "$DIMALT", "$DIMALTD", "$DIMALTF", "$DIMLFAC", "$DIMTOFL",
    "$DIMTVP", "$DIMTIX", "$DIMSOXD", "$DIMSAH", "$DIMBLK1", "$DIMBLK2", "$DIMSTYLE", "$DIMCLRD", "$DIMCLRE",
    "$DIMCLRT", "$DIMTFAC", "$DIMGAP", "$LUNITS", "$LUPREC", "$SKETCHINC", "$FILLETRAD", "$AUNITS", "$AUPREC",
    "$MENU", "$ELEVATION", "$PELEVATION", "$THICKNESS", "$LIMCHECK", "$BLIPMODE", "$CHAMFERA", "$CHAMFERB",
    "$SKPOLY", "$TDCREATE", "$TDUPDATE", "$TDINDWG", "$TDUSRTIMER", "$USRTIMER", "$ANGBASE", "$ANGDIR",
    "$PDMODE", "$PDSIZE", "$PLINEWID", "$COORDS", "$SPLFRAME", "$SPLINETYPE", "$SPLINESEGS", "$ATTDIA",
    "$ATTREQ", "$HANDLING", "$HANDSEED", "$SURFTAB1", "$SURFTAB2", "$SURFTYPE", "$SURFU", "$SURFV", "$UCSNAME",
    "$UCSORG", "$UCSXDIR", "$UCSYDIR", "$PUCSNAME", "$PUCSORG", "$PUCSXDIR", "$PUCSYDIR", "$USERI1", "$USERI2",
    "$USERI3", "$USERI4", "$USERI5", "$USERR1", "$USERR2", "$USERR3", "$USERR4", "$USERR5", "$WORLDVIEW",
    "$SHADEDGE", "$SHADEDIF", "$TILEMODE", "$MAXACTVP", "$PLIMCHECK", "$PEXTMIN", "$PEXTMAX", "$PLIMMIN",
    "$PLIMMAX", "$UNITMODE", "$VISRETAIN", "$PLINEGEN", "$PSLTSCALE",
    // the pre-R13 state
    "$VIEWCTR", "$VIEWDIR", "$TARGET", "$SNAPBASE", "$SNAPUNIT", "$GRIDUNIT", "$AXISUNIT", "$SNAPMODE",
    "$SNAPSTYLE", "$SNAPISOPAIR", "$GRIDMODE", "$AXISMODE", "$VIEWRES", "$FASTZOOM", "$VIEWMODE", "$FLATLAND",
    "$UCSICON", "$VIEWSIZE", "$SNAPANG", "$FRONTZ", "$BACKZ",
];

/// Which MATERIAL groups are written (see `write_material`).
struct MaterialGate<'a> {
    source: Option<&'a crate::objects::MaterialDxfSource>,
    /// A built-in material of a document that did not come from DXF: minimal form.
    minimal: bool,
}

impl MaterialGate<'_> {
    /// A group is written when the DXF source record had it, when its value differs from the native default,
    /// or (no DXF source, not minimal) always. The minimal form also carries the diffuse map source (72).
    fn writes(&self, code: usize, differs: bool) -> bool {
        match self.source {
            Some(source) => source.contains(code) || differs,
            None => !self.minimal || differs || matches!(code, 72 | 77 | 171 | 175 | 179 | 273),
        }
    }
}

impl<'a, W: DxfStreamWriter> SectionWriter<'a, W> {
    /// Create a new section writer
    pub fn new(writer: &'a mut W, handle_start: u64, handle_seed: u64) -> Self {
        Self {
            writer,
            next_handle: handle_start,
            handle_seed,
            dxf_version: DxfVersion::AC1024,
            r12: false,
            r12_handle_limit: u64::MAX,
            r12_first_allocated: u64::MAX,
            r12_names: None,
            r12_end_handles: HashMap::new(),
            r12_viewport_xdata: HashMap::new(),
            sab_entries: Vec::new(),
            writing_paper_space: false,
            paper_viewport_ids: HashMap::new(),
            valid_handles: HashSet::new(),
            by_block_material: None,
            carriers: Default::default(),
            dwg_source: None,
            layer_names: HashMap::new(),
            bylayer_linetype_handle: Handle::NULL,
            byblock_linetype_handle: Handle::NULL,
            model_space_handle: Handle::NULL,
            root_dict_handle: Handle::NULL,
            normal_plotstyle_handle: Handle::NULL,
            text_style_names: HashMap::new(),
            block_record_names: HashMap::new(),
            loft_input_entities: HashMap::new(),
            loft_reference_appid: None,
            pending_attachments: None,
            omitted_header_variables: HashSet::new(),
        }
    }

    /// Build the set of all handles that will appear in the output DXF.
    /// Call this before writing BLOCKS / ENTITIES / OBJECTS.
    pub fn build_valid_handles(&mut self, document: &CadDocument) {
        let mut set = HashSet::new();
        // Object handles — but EXCLUDE unsupported objects read from DWG that
        // have no DXF representation (Unknown with no raw_dxf_codes):
        // write_unknown_object skips those, so any reference to them would
        // dangle and must be filtered out, or strict CAD readers reject the
        // file on audit. Unknown associative-framework objects are excluded
        // as well: applications cannot restore them from the records this
        // writer produces and audit-skip them, which would leave dangling
        // dictionary entries behind.
        for (h, obj) in document.objects.iter() {
            match obj {
                ObjectType::Material(material) if material.name.eq_ignore_ascii_case("ByBlock") => {
                    self.by_block_material = Some(*h);
                }
                ObjectType::Unknown {
                    raw_dxf_codes,
                    type_name,
                    ..
                } => {
                    if raw_dxf_codes.is_none() || is_unrestorable_assoc_object(type_name) {
                        continue;
                    }
                }
                ObjectType::Associative(assoc) => {
                    if is_unrestorable_assoc_object(&assoc.dxf_name) {
                        continue;
                    }
                }
                _ => {}
            }
            set.insert(*h);
        }
        set.extend(self.carriers.objects.keys().copied());
        // Entity handles (from all block records); capture *Model_Space.
        self.block_record_names.clear();
        for br in document.block_records.iter() {
            set.insert(br.handle());
            self.block_record_names.insert(br.handle(), br.name.clone());
            if br.name.eq_ignore_ascii_case("*Model_Space") {
                self.model_space_handle = br.handle();
            }
            for eh in &br.entity_handles {
                set.insert(*eh);
            }
        }
        // Entity index (covers all entities including orphans)
        for h in document.entity_index.keys() {
            set.insert(*h);
        }
        // Compound children are inline records and are not present in
        // entity_index, but extension dictionaries may own data through them.
        for entity in &document.entities {
            match entity.as_ref() {
                EntityType::Insert(insert) => {
                    for attribute in &insert.attributes {
                        set.insert(attribute.common.handle);
                    }
                    if let Some(seqend) = &insert.seqend {
                        set.insert(seqend.common.handle);
                    }
                    if let Some(handle) = insert.seqend_handle {
                        set.insert(handle);
                    }
                }
                EntityType::Polyline2D(polyline) => {
                    for vertex in &polyline.vertices {
                        set.insert(vertex.common.handle);
                    }
                    if let Some(seqend) = &polyline.seqend {
                        set.insert(seqend.common.handle);
                    }
                }
                EntityType::Polyline3D(polyline) => {
                    for vertex in &polyline.vertices {
                        set.insert(vertex.common.handle);
                    }
                    if let Some(seqend) = &polyline.seqend {
                        set.insert(seqend.common.handle);
                    }
                }
                EntityType::PolyfaceMesh(mesh) => {
                    for vertex in &mesh.vertices {
                        set.insert(vertex.common.handle);
                    }
                    for face in &mesh.faces {
                        set.insert(face.common.handle);
                    }
                    if let Some(seqend) = &mesh.seqend {
                        set.insert(seqend.common.handle);
                    }
                    if let Some(handle) = mesh.seqend_handle {
                        set.insert(handle);
                    }
                }
                EntityType::PolygonMesh(mesh) => {
                    for vertex in &mesh.vertices {
                        set.insert(vertex.common.handle);
                    }
                    if let Some(seqend) = &mesh.seqend {
                        set.insert(seqend.common.handle);
                    }
                }
                _ => {}
            }
        }
        self.loft_input_entities.clear();
        for entity in &document.entities {
            let EntityType::Surface(surface) = entity.as_ref() else {
                continue;
            };
            let SurfaceData::Lofted {
                cross_sections,
                guide_curves,
                path_curve,
                ..
            } = &surface.surface_data
            else {
                continue;
            };
            let handles = cross_sections
                .iter()
                .chain(guide_curves)
                .chain(path_curve.iter());
            if handles.clone().next().is_some()
                && document.app_ids.get("CADCODEC_LOFT_REFERENCES").is_none()
                && self.loft_reference_appid.is_none()
            {
                let handle = self.allocate_handle();
                self.loft_reference_appid = Some(handle);
                self.handle_seed += 1;
                set.insert(handle);
            }
            for handle in handles {
                let Some(entity) = document.get_entity(*handle) else {
                    continue;
                };
                let embedded = match entity {
                    EntityType::Point(value) => EmbeddedEntity::Point(value.clone()),
                    EntityType::Line(value) => EmbeddedEntity::Line(value.clone()),
                    EntityType::Arc(value) => EmbeddedEntity::Arc(value.clone()),
                    EntityType::Circle(value) => EmbeddedEntity::Circle(value.clone()),
                    EntityType::Ellipse(value) => EmbeddedEntity::Ellipse(value.clone()),
                    EntityType::Spline(value) => EmbeddedEntity::Spline(value.clone()),
                    EntityType::LwPolyline(value) => EmbeddedEntity::LwPolyline(value.clone()),
                    EntityType::Region(value) => EmbeddedEntity::Region(value.clone()),
                    EntityType::Ray(value) => EmbeddedEntity::Ray(value.clone()),
                    EntityType::XLine(value) => EmbeddedEntity::XLine(value.clone()),
                    _ => continue,
                };
                self.loft_input_entities.insert(*handle, embedded);
            }
        }
        // Table record handles
        for r in document.layers.iter() {
            set.insert(r.handle());
        }
        for r in document.line_types.iter() {
            set.insert(r.handle());
        }
        self.text_style_names.clear();
        for r in document.text_styles.iter() {
            set.insert(r.handle());
            self.text_style_names.insert(r.handle(), r.name.clone());
        }
        for r in document.dim_styles.iter() {
            set.insert(r.handle());
        }
        for r in document.app_ids.iter() {
            set.insert(r.handle());
        }
        for r in document.views.iter() {
            set.insert(r.handle());
        }
        for r in document.vports.iter() {
            set.insert(r.handle());
        }
        for r in document.ucss.iter() {
            set.insert(r.handle());
        }
        set.insert(document.layers.handle());
        set.insert(document.line_types.handle());
        set.insert(document.text_styles.handle());
        set.insert(document.dim_styles.handle());
        set.insert(document.app_ids.handle());
        set.insert(document.views.handle());
        set.insert(document.vports.handle());
        set.insert(document.ucss.handle());
        set.insert(document.block_records.handle());
        self.root_dict_handle = Self::find_root_dict_handle(&document.objects);
        self.normal_plotstyle_handle = document
            .objects
            .values()
            .find_map(|object| {
                if let ObjectType::DictionaryWithDefault(dict) = object {
                    if dict.entries.iter().any(|(name, _)| name == "Normal") {
                        return dict.default_handle.into();
                    }
                }
                None
            })
            .unwrap_or(Handle::NULL);
        self.valid_handles = set;
        // Store ByLayer/ByBlock linetype handles for use as default in MLeader etc.
        if let Some(lt) = document.line_types.get("ByLayer") {
            self.bylayer_linetype_handle = lt.handle();
        }
        if let Some(lt) = document.line_types.get("ByBlock") {
            self.byblock_linetype_handle = lt.handle();
        }
    }

    /// Set the target DXF version
    /// Record the DWG version the document came from (see `dwg_source`).
    pub fn set_dwg_source(&mut self, version: Option<DxfVersion>) {
        self.dwg_source = version;
    }

    /// Remember the layer records by handle (see `layer_names`).
    pub fn set_layer_names(&mut self, document: &CadDocument) {
        self.layer_names = document
            .layers
            .iter()
            .filter(|layer| layer.name.contains(" @ ") && !layer.handle.is_null())
            .map(|layer| (layer.handle, layer.name.clone()))
            .collect();
    }

    /// Whether a DWG source of a version that stores the field was read.
    fn stated_by_dwg(&self, since: DxfVersion) -> bool {
        self.dwg_source.is_some_and(|version| version >= since)
    }

    /// State `carriers` in the output as objects of the document.
    pub(crate) fn set_carriers(&mut self, carriers: crate::objects::TableStyleCarriers) {
        self.carriers = std::rc::Rc::new(carriers);
    }

    pub fn set_version(&mut self, version: DxfVersion) {
        self.dxf_version = Self::writable_version(version);
    }

    /// Write the R12 (AC1009) structure. The caller wraps the stream in `R12Stream`.
    pub(crate) fn set_r12(&mut self, document: &CadDocument) {
        self.dxf_version = DxfVersion::AC1009;
        self.r12 = true;
        self.r12_handle_limit = r12_handle_limit(document);
        self.r12_first_allocated = self.next_handle;
        if let Some(blocks) = document.legacy_dxf_blocks.as_ref() {
            self.r12_end_handles = blocks
                .iter()
                .map(|block| (block.name.to_ascii_uppercase(), block.end_handle))
                .collect();
        }
        for entity in document.entities() {
            if let EntityType::Viewport(viewport) = entity {
                if viewport.common.handle.is_null() {
                    continue;
                }
                let mut data = viewport.common.extended_data.clone();
                data.upsert_record(crate::io::dwg::legacy_viewport::record_r12(viewport, document));
                self.r12_viewport_xdata.insert(viewport.common.handle, data);
            }
        }
    }

    /// Whether an R12 record states `handle`: while handles are on (`$HANDLING` 1), every handle of the document; a
    /// handle this writer allocates (a VERTEX or SEQEND the document holds without one) only when there is no R12
    /// source, since otherwise the reader allocates it again.
    fn r12_states(&self, handle: Handle) -> bool {
        !handle.is_null()
            && self.r12_handle_limit != 0
            && (self.r12_handle_limit == u64::MAX || handle.value() < self.r12_first_allocated)
    }

    /// The version this writer actually emits for a requested target.
    ///
    /// R12 (AC1009) predates the handle-based object model: its table records
    /// carry no handles, no `330` owner pointers and no `100` subclass markers,
    /// and it has no OBJECTS section. This writer emits those R13+ constructs
    /// unless it is put in its R12 mode with `set_r12` (which `DxfWriter` does
    /// for an R12 file, behind a stream that drops the groups R12 lacks), so
    /// a plain `set_version(AC1009)` writes R13, the oldest version whose
    /// structure matches what is emitted. The AC1009 DWG writer takes its
    /// records from that R13 structure.
    pub(crate) fn writable_version(version: DxfVersion) -> DxfVersion {
        match version {
            DxfVersion::AC1009 => DxfVersion::AC1012,
            other => other,
        }
    }

    /// Returns true if the target version requires SAB binary format (AC1027+)
    fn needs_sab(&self) -> bool {
        self.dxf_version >= DxfVersion::AC1027
    }

    fn allocate_handle(&mut self) -> Handle {
        let handle = Handle::new(self.next_handle);
        self.next_handle += 1;
        handle
    }

    /// Write the HEADER section
    pub fn write_header(&mut self, document: &CadDocument) -> Result<()> {
        self.omitted_header_variables = self.unchanged_absent_header_variables(document);
        // Variables a DWG header of this version does not store: their native value is a default the DWG never
        // stated (the reference then keeps its own default for the absent variable).
        if let Some(version) = document.dwg_source_version {
            if version > DxfVersion::AC1014 {
                self.omitted_header_variables.insert("$DIMFIT".to_string());
                self.omitted_header_variables.insert("$OSMODE".to_string());
            }
            if version < DxfVersion::AC1015 {
                self.omitted_header_variables.insert("$PSTYLEMODE".to_string());
            }
            if version < DxfVersion::AC1018 || document.header.dwg_template_data.is_none() {
                self.omitted_header_variables.insert("$MEASUREMENT".to_string());
            }
        }
        self.write_header_for(&document.header, document)
    }

    /// Header variables a DXF source did not have and whose value is still the native default. Writing them
    /// would assert a value the source never stated (a reader then applies its own default for an absent
    /// variable, which can differ from this crate's).
    fn unchanged_absent_header_variables(&self, document: &CadDocument) -> HashSet<String> {
        use crate::document::HeaderVariables;
        use super::text_writer::DxfTextWriter;
        let Some(source) = document.header.dxf_source_variables.as_ref() else {
            return HashSet::new();
        };
        let render = |hdr: &HeaderVariables| -> Option<HashMap<String, Vec<(String, String)>>> {
            let mut buffer = Vec::new();
            {
                let mut text = DxfTextWriter::new(&mut buffer);
                let mut scratch = SectionWriter::new(&mut text, self.next_handle, self.handle_seed);
                scratch.dxf_version = self.dxf_version;
                scratch.write_header_for(hdr, document).ok()?;
            }
            let text = String::from_utf8_lossy(&buffer);
            let mut lines = text.split("\r\n");
            let mut variables: HashMap<String, Vec<(String, String)>> = HashMap::new();
            let mut current: Option<String> = None;
            while let (Some(code), Some(value)) = (lines.next(), lines.next()) {
                if code.trim() == "9" {
                    current = Some(value.to_string());
                    variables.entry(value.to_string()).or_default();
                } else if let Some(name) = &current {
                    variables
                        .entry(name.clone())
                        .or_default()
                        .push((code.trim().to_string(), value.to_string()));
                }
            }
            Some(variables)
        };
        let defaults = HeaderVariables::default();
        let (Some(actual), Some(default)) = (render(&document.header), render(&defaults)) else {
            return HashSet::new();
        };
        actual
            .into_iter()
            .filter(|(name, pairs)| {
                !source.contains(name)
                    && !matches!(name.as_str(), "$ACADVER" | "$ACADMAINTVER" | "$HANDSEED" | "$DWGCODEPAGE")
                    && default.get(name).is_some_and(|other| other == pairs)
            })
            .map(|(name, _)| name)
            .collect()
    }

    fn write_header_for(
        &mut self,
        hdr: &crate::document::HeaderVariables,
        document: &CadDocument,
    ) -> Result<()> {
        self.writer.write_section_start("HEADER")?;

        // === Version & maintenance ===
        // `self.dxf_version` (not `document.version`) so the declared version
        // always matches the structure actually emitted - see
        // `writable_version` for the R12 case.
        let declared_version = self.dxf_version;
        self.write_header_variable("$ACADVER", |w| {
            w.write_string(1, declared_version.to_dxf_string())
        })?;
        if self.dxf_version >= DxfVersion::AC1032 {
            // The reference application writes 377 here and reads the
            // multiline text of an ATTRIB / ATTDEF only from a file whose
            // maintenance version is that recent (0, 4, 100 and 200 show the
            // value on one line with a literal `\P`).
            let maintenance = if hdr.maintenance_version != 0
                || hdr.dxf_source_variables.as_ref().is_some_and(|vars| vars.contains("$ACADMAINTVER"))
            {
                hdr.maintenance_version
            } else {
                377
            };
            self.write_header_variable("$ACADMAINTVER", |w| w.write_i32(90, maintenance))?;
        } else if self.dxf_version >= DxfVersion::AC1015 {
            // The DWG reader states the file-header byte as the variable where it is one (R2004 to R2013); the
            // byte of another version is not written as the variable.
            self.write_header_variable("$ACADMAINTVER", |w| w.write_i16(70, hdr.maintenance_version as i16))?;
        }
        // An R12 file states the code page only when its source did (the bytes are then the source's, which a
        // reader decodes the same way).
        if !self.r12 || hdr.dxf_source_variables.as_ref().is_none_or(|vars| vars.contains("$DWGCODEPAGE")) {
            self.write_header_variable("$DWGCODEPAGE", |w| w.write_string(3, &hdr.code_page))?;
        }

        // === Drawing properties (DWGPROPS) === R2004+ files carry them here,
        // right after $DWGCODEPAGE.
        if self.dxf_version >= DxfVersion::AC1018 {
            self.write_summary_info(document)?;
        }

        if self.r12 {
            // Entities, VERTEX and SEQEND records and the BLOCK end marker carry handles.
            let handling = i16::from(self.r12_handle_limit != 0);
            self.write_header_variable("$HANDLING", |w| w.write_i16(70, handling))?;
        }
        let handle_seed = self.handle_seed;
        self.write_header_variable("$HANDSEED", |w| w.write_handle(5, Handle::new(handle_seed)))?;

        // === Drawing extents & limits ===
        self.write_header_variable("$INSBASE", |w| {
            let v = &hdr.model_space_insertion_base;
            w.write_double(10, v.x)?;
            w.write_double(20, v.y)?;
            w.write_double(30, v.z)
        })?;
        self.write_header_variable("$EXTMIN", |w| {
            let v = &hdr.model_space_extents_min;
            w.write_double(10, v.x)?;
            w.write_double(20, v.y)?;
            w.write_double(30, v.z)
        })?;
        self.write_header_variable("$EXTMAX", |w| {
            let v = &hdr.model_space_extents_max;
            w.write_double(10, v.x)?;
            w.write_double(20, v.y)?;
            w.write_double(30, v.z)
        })?;
        self.write_header_variable("$LIMMIN", |w| {
            let v = &hdr.model_space_limits_min;
            w.write_double(10, v.x)?;
            w.write_double(20, v.y)
        })?;
        self.write_header_variable("$LIMMAX", |w| {
            let v = &hdr.model_space_limits_max;
            w.write_double(10, v.x)?;
            w.write_double(20, v.y)
        })?;

        // === Drawing modes ===
        self.write_header_variable("$ORTHOMODE", |w| {
            w.write_i16(70, if hdr.ortho_mode { 1 } else { 0 })
        })?;
        self.write_header_variable("$OSMODE", |w| w.write_i16(70, hdr.object_snap_mode as i16))?;
        self.write_header_variable("$REGENMODE", |w| {
            w.write_i16(70, if hdr.regen_mode { 1 } else { 0 })
        })?;
        self.write_header_variable("$FILLMODE", |w| {
            w.write_i16(70, if hdr.fill_mode { 1 } else { 0 })
        })?;
        self.write_header_variable("$QTEXTMODE", |w| {
            w.write_i16(70, if hdr.quick_text_mode { 1 } else { 0 })
        })?;
        self.write_header_variable("$MIRRTEXT", |w| {
            w.write_i16(70, if hdr.mirror_text { 1 } else { 0 })
        })?;
        self.write_header_variable("$LTSCALE", |w| w.write_double(40, hdr.linetype_scale))?;
        self.write_header_variable("$ATTMODE", |w| w.write_i16(70, hdr.attribute_visibility))?;
        self.write_header_variable("$TEXTSIZE", |w| w.write_double(40, hdr.text_height))?;
        self.write_header_variable("$TRACEWID", |w| w.write_double(40, hdr.trace_width))?;
        self.write_header_variable("$ELEVATION", |w| w.write_double(40, hdr.elevation))?;
        self.write_header_variable("$THICKNESS", |w| w.write_double(40, hdr.thickness))?;
        self.write_header_variable("$FILLETRAD", |w| w.write_double(40, hdr.fillet_radius))?;
        self.write_header_variable("$CHAMFERA", |w| w.write_double(40, hdr.chamfer_distance_a))?;
        self.write_header_variable("$CHAMFERB", |w| w.write_double(40, hdr.chamfer_distance_b))?;
        self.write_header_variable("$SKETCHINC", |w| w.write_double(40, hdr.sketch_increment))?;
        self.write_header_variable("$SKPOLY", |w| w.write_i16(70, hdr.sketch_type.clamp(0, 2)))?;
        // SKTOLERANCE is application state, not a portable DXF header variable.
        // A DWG header names the current styles by handle; the fork default name stands for "not resolved".
        let style_name = |name: &str, handle: Handle| -> String {
            if document.dwg_source_version.is_none() || name != "Standard" || handle.is_null() {
                return name.to_string();
            }
            document
                .text_styles
                .iter()
                .find(|style| style.handle == handle)
                .map_or_else(|| name.to_string(), |style| style.name.clone())
        };
        let text_style_name = style_name(&hdr.current_text_style_name, hdr.current_text_style_handle);
        self.write_header_variable("$TEXTSTYLE", |w| w.write_string(7, &text_style_name))?;
        self.write_header_variable("$CMLSTYLE", |w| w.write_string(2, &hdr.multiline_style))?;
        self.write_header_variable("$CMLJUST", |w| w.write_i16(70, hdr.multiline_justification))?;
        self.write_header_variable("$CMLSCALE", |w| w.write_double(40, hdr.multiline_scale))?;
        // $CTABLESTYLE / $CMLEADERSTYLE are only written when they differ
        // from the "Standard" default: BricsCAD (V20) does not write them
        // itself and its audit reports them as unknown system variables,
        // flagging the file for recovery (issue #51). $CANNOSCALE is always
        // skipped for the same reason.
        if !hdr.current_table_style_name.is_empty()
            && !hdr
                .current_table_style_name
                .eq_ignore_ascii_case("Standard")
        {
            self.write_header_variable("$CTABLESTYLE", |w| {
                w.write_string(2, &hdr.current_table_style_name)
            })?;
        }
        if !hdr.current_mleader_style_name.is_empty()
            && !hdr
                .current_mleader_style_name
                .eq_ignore_ascii_case("Standard")
        {
            self.write_header_variable("$CMLEADERSTYLE", |w| {
                w.write_string(2, &hdr.current_mleader_style_name)
            })?;
        }
        self.write_header_variable("$CLAYER", |w| w.write_string(8, &hdr.current_layer_name))?;
        let linetype_name = if document.dwg_source_version.is_some()
            && hdr.current_linetype_name == "ByLayer"
            && !hdr.current_linetype_handle.is_null()
        {
            document
                .line_types
                .iter()
                .find(|linetype| linetype.handle == hdr.current_linetype_handle)
                .map_or_else(|| hdr.current_linetype_name.clone(), |linetype| linetype.name.clone())
        } else {
            hdr.current_linetype_name.clone()
        };
        self.write_header_variable("$CELTYPE", |w| w.write_string(6, &linetype_name))?;
        self.write_header_variable("$CECOLOR", |w| {
            w.write_i16(62, hdr.current_entity_color.approximate_index())
        })?;
        // CELWEIGHT must be a valid lineweight (-3..-1 presets or one of the
        // fixed hundredths-of-mm values); values outside the DXF table (e.g.
        // a raw DWG lineweight index such as 29) are rejected by CAD
        // applications on load (issue #51 BricsCAD audit) — fall back to
        // BYLAYER.
        const VALID_LINEWEIGHTS: [i16; 54] = [
            0, 5, 9, 13, 14, 15, 16, 18, 20, 23, 25, 30, 35, 40, 45, 50, 53, 55, 60, 65, 70, 75,
            78, 80, 85, 90, 95, 100, 103, 105, 109, 112, 118, 120, 125, 130, 135, 140, 145, 150,
            155, 158, 160, 165, 170, 175, 180, 185, 190, 195, 200, 205, 209, 211,
        ];
        let celweight = {
            let v = hdr.current_line_weight;
            if (-3..=-1).contains(&v) || VALID_LINEWEIGHTS.contains(&v) {
                v
            } else {
                -1
            }
        };
        self.write_header_variable("$CELWEIGHT", |w| w.write_i16(370, celweight))?;
        self.write_header_variable("$CELTSCALE", |w| {
            w.write_double(40, hdr.current_entity_linetype_scale)
        })?;
        self.write_header_variable("$DISPSILH", |w| {
            w.write_i16(70, if hdr.display_silhouette { 1 } else { 0 })
        })?;
        self.write_header_variable("$PELLIPSE", |w| {
            w.write_i16(70, if hdr.create_ellipse_as_polyline { 1 } else { 0 })
        })?;
        self.write_header_variable("$LWDISPLAY", |w| w.write_bool(290, hdr.lineweight_display))?;

        // === Units ===
        self.write_header_variable("$LUNITS", |w| w.write_i16(70, hdr.linear_unit_format))?;
        self.write_header_variable("$LUPREC", |w| w.write_i16(70, hdr.linear_unit_precision))?;
        self.write_header_variable("$AUNITS", |w| w.write_i16(70, hdr.angular_unit_format))?;
        self.write_header_variable("$AUPREC", |w| w.write_i16(70, hdr.angular_unit_precision))?;
        self.write_header_variable("$ANGBASE", |w| w.write_double(50, hdr.angle_base.to_dxf_degrees()))?;
        self.write_header_variable("$ANGDIR", |w| w.write_i16(70, hdr.angle_direction))?;
        self.write_header_variable("$MEASUREMENT", |w| w.write_i16(70, hdr.measurement))?;
        self.write_header_variable("$INSUNITS", |w| w.write_i16(70, hdr.insertion_units))?;
        // The drawing property "Hyperlink base" (R2000+); omitted when empty.
        if self.dxf_version >= DxfVersion::AC1015 {
            let base = if document.summary_info.hyperlink_base.is_empty() {
                hdr.hyperlink_base.as_str()
            } else {
                document.summary_info.hyperlink_base.as_str()
            };
            if !base.is_empty()
                || hdr.dxf_source_variables.as_ref().is_some_and(|vars| vars.contains("$HYPERLINKBASE"))
                || self.stated_by_dwg(DxfVersion::AC1015)
            {
                self.write_header_variable("$HYPERLINKBASE", |w| w.write_string(1, base))?;
            }
        }

        // === Point display ===
        self.write_header_variable("$PDMODE", |w| w.write_i16(70, hdr.point_display_mode))?;
        self.write_header_variable("$PDSIZE", |w| w.write_double(40, hdr.point_display_size))?;
        self.write_header_variable("$PLINEGEN", |w| {
            w.write_i16(
                70,
                if hdr.polyline_linetype_generation {
                    1
                } else {
                    0
                },
            )
        })?;
        self.write_header_variable("$PSLTSCALE", |w| {
            w.write_i16(
                70,
                if hdr.paper_space_linetype_scaling {
                    1
                } else {
                    0
                },
            )
        })?;

        // === Dimension variables ===
        self.write_header_variable("$DIMSCALE", |w| w.write_double(40, hdr.dim_scale))?;
        self.write_header_variable("$DIMASZ", |w| w.write_double(40, hdr.dim_arrow_size))?;
        self.write_header_variable("$DIMEXO", |w| w.write_double(40, hdr.dim_ext_line_offset))?;
        self.write_header_variable("$DIMDLI", |w| w.write_double(40, hdr.dim_line_increment))?;
        self.write_header_variable("$DIMRND", |w| w.write_double(40, hdr.dim_rounding))?;
        self.write_header_variable("$DIMDLE", |w| w.write_double(40, hdr.dim_line_extension))?;
        self.write_header_variable("$DIMEXE", |w| {
            w.write_double(40, hdr.dim_ext_line_extension)
        })?;
        self.write_header_variable("$DIMTP", |w| w.write_double(40, hdr.dim_tolerance_plus))?;
        self.write_header_variable("$DIMTM", |w| w.write_double(40, hdr.dim_tolerance_minus))?;
        self.write_header_variable("$DIMTXT", |w| w.write_double(40, hdr.dim_text_height))?;
        self.write_header_variable("$DIMCEN", |w| w.write_double(40, hdr.dim_center_mark))?;
        self.write_header_variable("$DIMTSZ", |w| w.write_double(40, hdr.dim_tick_size))?;
        self.write_header_variable("$DIMTOL", |w| {
            w.write_i16(70, if hdr.dim_tolerance { 1 } else { 0 })
        })?;
        self.write_header_variable("$DIMLIM", |w| {
            w.write_i16(70, if hdr.dim_limits { 1 } else { 0 })
        })?;
        self.write_header_variable("$DIMTIH", |w| {
            w.write_i16(70, if hdr.dim_text_inside_horizontal { 1 } else { 0 })
        })?;
        self.write_header_variable("$DIMTOH", |w| {
            w.write_i16(
                70,
                if hdr.dim_text_outside_horizontal {
                    1
                } else {
                    0
                },
            )
        })?;
        self.write_header_variable("$DIMSE1", |w| {
            w.write_i16(70, if hdr.dim_suppress_ext1 { 1 } else { 0 })
        })?;
        self.write_header_variable("$DIMSE2", |w| {
            w.write_i16(70, if hdr.dim_suppress_ext2 { 1 } else { 0 })
        })?;
        self.write_header_variable("$DIMTAD", |w| w.write_i16(70, hdr.dim_text_above))?;
        self.write_header_variable("$DIMZIN", |w| w.write_i16(70, hdr.dim_zero_suppression))?;
        self.write_header_variable("$DIMCLRD", |w| {
            w.write_i16(70, hdr.dim_line_color.approximate_index())
        })?;
        self.write_header_variable("$DIMCLRE", |w| {
            w.write_i16(70, hdr.dim_ext_line_color.approximate_index())
        })?;
        self.write_header_variable("$DIMCLRT", |w| {
            w.write_i16(70, hdr.dim_text_color.approximate_index())
        })?;
        self.write_header_variable("$DIMGAP", |w| w.write_double(40, hdr.dim_line_gap))?;
        self.write_header_variable("$DIMALT", |w| {
            w.write_i16(70, if hdr.dim_alternate_units { 1 } else { 0 })
        })?;
        self.write_header_variable("$DIMALTD", |w| w.write_i16(70, hdr.dim_alt_decimal_places))?;
        self.write_header_variable("$DIMALTF", |w| w.write_double(40, hdr.dim_alt_scale))?;
        self.write_header_variable("$DIMLFAC", |w| w.write_double(40, hdr.dim_linear_scale))?;
        self.write_header_variable("$DIMTOFL", |w| {
            w.write_i16(70, if hdr.dim_force_line_inside { 1 } else { 0 })
        })?;
        self.write_header_variable("$DIMTVP", |w| w.write_double(40, hdr.dim_text_vertical_pos))?;
        self.write_header_variable("$DIMTIX", |w| {
            w.write_i16(70, if hdr.dim_force_text_inside { 1 } else { 0 })
        })?;
        self.write_header_variable("$DIMSOXD", |w| {
            w.write_i16(70, if hdr.dim_suppress_outside_ext { 1 } else { 0 })
        })?;
        self.write_header_variable("$DIMSAH", |w| {
            w.write_i16(70, if hdr.dim_separate_arrows { 1 } else { 0 })
        })?;
        self.write_header_variable("$DIMPOST", |w| w.write_string(1, &hdr.dim_post))?;
        self.write_header_variable("$DIMAPOST", |w| w.write_string(1, &hdr.dim_alt_post))?;
        // A DWG header names the current dimension style by handle only; the native name then stays at its
        // default, so the table entry the handle points at supplies it.
        let current_dimstyle_name = if hdr.current_dimstyle_name == "Standard" {
            document
                .dim_styles
                .iter()
                .find(|style| !hdr.current_dimstyle_handle.is_null() && style.handle == hdr.current_dimstyle_handle)
                .map_or(hdr.current_dimstyle_name.as_str(), |style| style.name.as_str())
        } else {
            hdr.current_dimstyle_name.as_str()
        };
        self.write_header_variable("$DIMSTYLE", |w| w.write_string(2, current_dimstyle_name))?;
        self.write_header_variable("$DIMLUNIT", |w| w.write_i16(70, hdr.dim_linear_unit_format))?;
        self.write_header_variable("$DIMDEC", |w| w.write_i16(70, hdr.dim_decimal_places))?;
        self.write_header_variable("$DIMTDEC", |w| {
            w.write_i16(70, hdr.dim_tolerance_decimal_places)
        })?;
        self.write_header_variable("$DIMALTU", |w| w.write_i16(70, hdr.dim_alt_units_format))?;
        self.write_header_variable("$DIMALTTD", |w| {
            w.write_i16(70, hdr.dim_alt_tolerance_decimal_places)
        })?;
        self.write_header_variable("$DIMAUNIT", |w| w.write_i16(70, hdr.dim_angular_units))?;
        self.write_header_variable("$DIMADEC", |w| {
            w.write_i16(70, hdr.dim_angular_decimal_places)
        })?;
        self.write_header_variable("$DIMJUST", |w| {
            w.write_i16(70, hdr.dim_horizontal_justification)
        })?;
        self.write_header_variable("$DIMSD1", |w| {
            w.write_i16(70, if hdr.dim_suppress_line1 { 1 } else { 0 })
        })?;
        self.write_header_variable("$DIMSD2", |w| {
            w.write_i16(70, if hdr.dim_suppress_line2 { 1 } else { 0 })
        })?;
        self.write_header_variable("$DIMTOLJ", |w| {
            w.write_i16(70, hdr.dim_tolerance_justification)
        })?;
        self.write_header_variable("$DIMTZIN", |w| {
            w.write_i16(70, hdr.dim_tolerance_zero_suppression)
        })?;
        self.write_header_variable("$DIMALTZ", |w| {
            w.write_i16(70, hdr.dim_alt_tolerance_zero_suppression)
        })?;
        self.write_header_variable("$DIMALTTZ", |w| {
            w.write_i16(70, hdr.dim_alt_tolerance_zero_tight)
        })?;
        self.write_header_variable("$DIMATFIT", |w| w.write_i16(70, hdr.dim_text_arrow_fit))?;
        self.write_header_variable("$DIMFIT", |w| w.write_i16(70, hdr.dim_fit))?;
        self.write_header_variable("$DIMALTMZF", |w| w.write_double(40, hdr.dim_alt_mzf))?;
        if let Some(text) = hdr.dim_alt_mzs.as_deref() {
            self.write_header_variable("$DIMALTMZS", |w| w.write_string(6, text))?;
        }
        self.write_header_variable("$DIMARCSYM", |w| {
            w.write_i16(70, hdr.dim_arc_length_symbol_position)
        })?;
        self.write_header_variable("$DIMFXLON", |w| {
            w.write_i16(70, if hdr.dim_extension_line_length_fixed { 1 } else { 0 })
        })?;
        self.write_header_variable("$DIMTXTDIRECTION", |w| w.write_i16(70, hdr.dim_text_direction))?;
        self.write_header_variable("$DIMUNIT", |w| w.write_i16(70, hdr.dim_unit))?;
        self.write_header_variable("$DIMMZF", |w| w.write_double(40, hdr.dim_mzf))?;
        if let Some(text) = hdr.dim_mzs.as_deref() {
            self.write_header_variable("$DIMMZS", |w| w.write_string(6, text))?;
        }
        self.write_header_variable("$DIMDSEP", |w| {
            w.write_i16(70, hdr.dim_decimal_separator as i16)
        })?;
        self.write_header_variable("$DIMTMOVE", |w| w.write_i16(70, hdr.dim_text_movement))?;
        self.write_header_variable("$DIMFRAC", |w| w.write_i16(70, hdr.dim_fraction_format))?;
        self.write_header_variable("$DIMLWD", |w| w.write_i16(70, hdr.dim_line_weight))?;
        self.write_header_variable("$DIMLWE", |w| w.write_i16(70, hdr.dim_ext_line_weight))?;
        self.write_header_variable("$DIMTFAC", |w| w.write_double(40, hdr.dim_tolerance_scale))?;
        let dim_text_style = style_name(&hdr.dim_text_style_name, hdr.dim_text_style_handle);
        self.write_header_variable("$DIMTXSTY", |w| w.write_string(7, &dim_text_style))?;
        self.write_header_variable("$DIMLTYPE", |w| w.write_string(6, &hdr.dim_linetype_name))?;
        self.write_header_variable("$DIMLTEX1", |w| w.write_string(6, &hdr.dim_linetype1_name))?;
        self.write_header_variable("$DIMLTEX2", |w| w.write_string(6, &hdr.dim_linetype2_name))?;
        if self.dxf_version >= DxfVersion::AC1021 {
            self.write_header_variable("$DIMFXL", |w| {
                w.write_double(40, hdr.dim_fixed_ext_line_length)
            })?;
            // DIMJOGANG valid range is 5°..90°
            self.write_header_variable("$DIMJOGANG", |w| {
                w.write_double(40, hdr.dim_jog_angle.clamp(0.0872665, std::f64::consts::FRAC_PI_2))
            })?;
            self.write_header_variable("$DIMTFILL", |w| w.write_i16(70, hdr.dim_text_fill))?;
            self.write_header_variable("$DIMTFILLCLR", |w| {
                w.write_i16(70, hdr.dim_text_fill_color.approximate_index())
            })?;
        }

        // === Misc ===
        self.write_header_variable("$SPLFRAME", |w| {
            w.write_i16(70, if hdr.spline_frame { 1 } else { 0 })
        })?;
        if self.dxf_version >= DxfVersion::AC1015 {
            self.write_header_variable("$XCLIPFRAME", |w| {
                w.write_byte(280, hdr.xclip_frame.clamp(0, 2) as u8)
            })?;
        }
        if self.dxf_version >= DxfVersion::AC1021 {
            self.write_header_variable("$DWFFRAME", |w| {
                w.write_byte(280, hdr.dwf_frame.clamp(0, 2) as u8)
            })?;
            self.write_header_variable("$DGNFRAME", |w| {
                w.write_byte(280, hdr.dgn_frame.clamp(0, 2) as u8)
            })?;
        }
        if self.dxf_version >= DxfVersion::AC1021 {
            self.write_header_variable("$SOLIDHIST", |w| {
                w.write_byte(280, u8::from(hdr.record_solid_history))
            })?;
            self.write_header_variable("$SHOWHIST", |w| {
                w.write_byte(280, hdr.show_solid_history.clamp(0, 2) as u8)
            })?;
            self.write_header_variable("$3DDWFPREC", |w| {
                w.write_double(40, hdr.dwf_3d_precision.clamp(1.0, 6.0))
            })?;
            // PSOLWIDTH / PSOLHEIGHT must be positive
            let polysolid_width = if hdr.polysolid_width > 0.0 {
                hdr.polysolid_width
            } else {
                0.25
            };
            let polysolid_height = if hdr.polysolid_height > 0.0 {
                hdr.polysolid_height
            } else {
                4.0
            };
            self.write_header_variable("$PSOLWIDTH", |w| w.write_double(40, polysolid_width))?;
            self.write_header_variable("$PSOLHEIGHT", |w| w.write_double(40, polysolid_height))?;
            self.write_header_variable("$LIGHTGLYPHDISPLAY", |w| {
                w.write_byte(280, hdr.light_glyph_display.min(1))
            })?;
            self.write_header_variable("$TILEMODELIGHTSYNCH", |w| {
                w.write_byte(280, hdr.tile_model_light_synch.min(1))
            })?;
            // The current material (a hard pointer, R2007+); null is the default and stays out.
            if !hdr.current_material_handle.is_null() {
                self.write_header_variable("$CMATERIAL", |w| {
                    w.write_handle(347, hdr.current_material_handle)
                })?;
            }
            self.write_header_variable("$REALWORLDSCALE", |w| {
                w.write_bool(290, hdr.real_world_scale)
            })?;
            self.write_header_variable("$INTERFERECOLOR", |w| {
                w.write_i16(62, hdr.interference_color.approximate_index())
            })?;
            self.write_header_variable("$CSHADOW", |w| w.write_byte(280, hdr.shadow_mode))?;
        }
        self.write_header_variable("$SPLINETYPE", |w| w.write_i16(70, hdr.spline_type))?;
        self.write_header_variable("$SPLINESEGS", |w| w.write_i16(70, hdr.spline_segments))?;
        self.write_header_variable("$SURFTAB1", |w| w.write_i16(70, hdr.surface_tab1))?;
        self.write_header_variable("$SURFTAB2", |w| w.write_i16(70, hdr.surface_tab2))?;
        self.write_header_variable("$SURFTYPE", |w| w.write_i16(70, hdr.surface_type))?;
        self.write_header_variable("$SURFU", |w| w.write_i16(70, hdr.surface_u_density))?;
        self.write_header_variable("$SURFV", |w| w.write_i16(70, hdr.surface_v_density))?;
        self.write_header_variable("$WORLDVIEW", |w| {
            w.write_i16(70, if hdr.world_view { 1 } else { 0 })
        })?;
        self.write_header_variable("$PELEVATION", |w| w.write_double(40, hdr.paper_elevation))?;
        self.write_header_variable("$PLINEWID", |w| w.write_double(40, hdr.polyline_width))?;
        self.write_header_variable("$MAXACTVP", |w| w.write_i16(70, hdr.max_active_viewports))?;
        self.write_header_variable("$TILEMODE", |w| {
            w.write_i16(70, if hdr.show_model_space { 1 } else { 0 })
        })?;
        self.write_header_variable("$PLIMCHECK", |w| {
            w.write_i16(70, if hdr.paper_space_limit_check { 1 } else { 0 })
        })?;
        self.write_header_variable("$VISRETAIN", |w| {
            w.write_i16(70, if hdr.retain_xref_visibility { 1 } else { 0 })
        })?;

        // === Plot style mode ===
        // $PSTYLEMODE is a boolean (code 290) telling the reader whether
        // plot-style references resolve against a named (1) or
        // color-dependent (0) table. Writing it with the wrong code - or
        // omitting it - makes the reader default to color-dependent mode,
        // where the layers' PlotStyleName Ids fail validation on audit
        // (issue #51 BricsCAD audit).
        self.write_header_variable("$PSTYLEMODE", |w| w.write_bool(290, hdr.plotstyle_mode))?;

        // === Time ===
        self.write_header_variable("$TDCREATE", |w| w.write_double(40, hdr.create_date_julian))?;
        // Universal times exist from R2000 on.
        let universal = self.dxf_version >= DxfVersion::AC1015;
        if universal {
            self.write_header_variable("$TDUCREATE", |w| w.write_double(40, hdr.universal_create_or_local()))?;
        }
        self.write_header_variable("$TDUPDATE", |w| w.write_double(40, hdr.update_date_julian))?;
        if universal {
            self.write_header_variable("$TDUUPDATE", |w| w.write_double(40, hdr.universal_update_or_local()))?;
        }
        self.write_header_variable("$TDINDWG", |w| w.write_double(40, hdr.total_editing_time))?;

        // === Identity === (R2000+, as in the DWG header; empty ones are
        // omitted rather than written blank)
        if self.dxf_version >= DxfVersion::AC1015 {
            if !hdr.fingerprint_guid.is_empty() {
                self.write_header_variable("$FINGERPRINTGUID", |w| {
                    w.write_string(2, &hdr.fingerprint_guid)
                })?;
            }
            if !hdr.version_guid.is_empty() {
                self.write_header_variable("$VERSIONGUID", |w| w.write_string(2, &hdr.version_guid))?;
            }
        }

        // === UCS ===
        let unit_mode = hdr.legacy.as_ref().map_or(hdr.unit_mode, |state| state.unit_mode);
        self.write_header_variable("$UNITMODE", |w| w.write_i16(70, unit_mode))?;
        self.write_header_variable("$UCSBASE", |w| w.write_string(2, &hdr.ucs_base))?;
        self.write_header_variable("$PUCSBASE", |w| {
            w.write_string(2, &hdr.paper_space_ucs_base_name)
        })?;
        self.write_header_variable("$UCSNAME", |w| {
            w.write_string(2, &hdr.model_space_ucs_name)
        })?;
        self.write_header_variable("$PUCSNAME", |w| {
            w.write_string(2, &hdr.paper_space_ucs_name)
        })?;
        self.write_header_variable("$UCSORG", |w| {
            let v = &hdr.model_space_ucs_origin;
            w.write_double(10, v.x)?;
            w.write_double(20, v.y)?;
            w.write_double(30, v.z)
        })?;
        self.write_header_variable("$UCSXDIR", |w| {
            let v = &hdr.model_space_ucs_x_axis;
            w.write_double(10, v.x)?;
            w.write_double(20, v.y)?;
            w.write_double(30, v.z)
        })?;
        self.write_header_variable("$UCSYDIR", |w| {
            let v = &hdr.model_space_ucs_y_axis;
            w.write_double(10, v.x)?;
            w.write_double(20, v.y)?;
            w.write_double(30, v.z)
        })?;
        for (name, value) in [
            ("$UCSORGTOP", &hdr.model_space_orthographic_top_origin),
            ("$UCSORGBOTTOM", &hdr.model_space_orthographic_bottom_origin),
            ("$UCSORGLEFT", &hdr.model_space_orthographic_left_origin),
            ("$UCSORGRIGHT", &hdr.model_space_orthographic_right_origin),
            ("$UCSORGFRONT", &hdr.model_space_orthographic_front_origin),
            ("$UCSORGBACK", &hdr.model_space_orthographic_back_origin),
            ("$PUCSORG", &hdr.paper_space_ucs_origin),
            ("$PUCSXDIR", &hdr.paper_space_ucs_x_axis),
            ("$PUCSYDIR", &hdr.paper_space_ucs_y_axis),
            ("$PUCSORGTOP", &hdr.paper_space_orthographic_top_origin),
            ("$PUCSORGBOTTOM", &hdr.paper_space_orthographic_bottom_origin),
            ("$PUCSORGLEFT", &hdr.paper_space_orthographic_left_origin),
            ("$PUCSORGRIGHT", &hdr.paper_space_orthographic_right_origin),
            ("$PUCSORGFRONT", &hdr.paper_space_orthographic_front_origin),
            ("$PUCSORGBACK", &hdr.paper_space_orthographic_back_origin),
        ] {
            self.write_header_variable(name, |w| {
                w.write_double(10, value.x)?;
                w.write_double(20, value.y)?;
                w.write_double(30, value.z)
            })?;
        }
        self.write_header_variable("$UCSORTHOVIEW", |w| {
            w.write_i16(70, hdr.ucs_ortho_view)
        })?;
        self.write_header_variable("$PUCSORTHOVIEW", |w| {
            w.write_i16(70, hdr.paper_ucs_ortho_view)
        })?;

        self.write_optional_header_variables(hdr, document)?;
        if self.r12 {
            self.write_legacy_header_state(hdr)?;
        }

        self.writer.write_section_end()?;
        Ok(())
    }

    /// The pre-R13 current view and drafting state (`HeaderVariables::legacy`), as the header variables the reader of
    /// this crate takes it from. A variable is written when it differs from the default of the state: the source
    /// stated the others only when it was an R11-style file, and the default is what a missing variable reads as.
    fn write_legacy_header_state(&mut self, hdr: &crate::document::HeaderVariables) -> Result<()> {
        let Some(state) = hdr.legacy.as_ref() else {
            return Ok(());
        };
        let d = crate::document::LegacyHeaderState::default();
        macro_rules! point3 {
            ($name:literal, $value:expr, $default:expr) => {
                if $value != $default {
                    self.write_header_variable($name, |w| {
                        w.write_double(10, $value.x)?;
                        w.write_double(20, $value.y)?;
                        w.write_double(30, $value.z)
                    })?;
                }
            };
        }
        macro_rules! point2 {
            ($name:literal, $value:expr, $default:expr) => {
                if $value != $default {
                    self.write_header_variable($name, |w| {
                        w.write_double(10, $value.x)?;
                        w.write_double(20, $value.y)?;
                        w.write_double(30, 0.0)
                    })?;
                }
            };
        }
        macro_rules! int {
            ($name:literal, $value:expr, $default:expr) => {
                if $value != $default {
                    self.write_header_variable($name, |w| w.write_i16(70, $value))?;
                }
            };
        }
        macro_rules! real {
            ($name:literal, $code:literal, $value:expr, $default:expr) => {
                if $value != $default {
                    self.write_header_variable($name, |w| w.write_double($code, $value))?;
                }
            };
        }
        point3!("$VIEWCTR", state.view_center, d.view_center);
        point3!("$VIEWDIR", state.view_direction, d.view_direction);
        point3!("$TARGET", state.view_target, d.view_target);
        point2!("$SNAPBASE", state.snap_base, d.snap_base);
        point2!("$SNAPUNIT", state.snap_spacing, d.snap_spacing);
        point2!("$GRIDUNIT", state.grid_spacing, d.grid_spacing);
        point2!("$AXISUNIT", state.axis_spacing, d.axis_spacing);
        int!("$SNAPMODE", state.snap_mode, d.snap_mode);
        int!("$SNAPSTYLE", state.snap_style, d.snap_style);
        int!("$SNAPISOPAIR", state.snap_isopair, d.snap_isopair);
        int!("$GRIDMODE", state.grid_mode, d.grid_mode);
        int!("$AXISMODE", state.axis_mode, d.axis_mode);
        int!("$VIEWRES", state.circle_zoom, d.circle_zoom);
        int!("$FASTZOOM", state.fast_zoom, d.fast_zoom);
        int!("$VIEWMODE", state.view_mode, d.view_mode);
        int!("$FLATLAND", state.flatland, d.flatland);
        int!("$UCSICON", state.ucs_icon, d.ucs_icon);
        real!("$VIEWSIZE", 40, state.view_height, d.view_height);
        real!("$SNAPANG", 50, state.snap_angle.to_dxf_degrees(), d.snap_angle.to_dxf_degrees());
        real!("$FRONTZ", 40, state.front_clip, d.front_clip);
        real!("$BACKZ", 40, state.back_clip, d.back_clip);
        Ok(())
    }

    /// System variables the reader keeps that no always-written group covers. One is written when the DXF
    /// source had it, when a DWG source of a version that stores it was read (the value is stated even when it
    /// equals the native default), or when its value differs from the native default (an edit); so a file never
    /// gains a variable it did not state and a stated one is never dropped.
    fn write_optional_header_variables(
        &mut self,
        hdr: &crate::document::HeaderVariables,
        document: &CadDocument,
    ) -> Result<()> {
        let d = crate::document::HeaderVariables::default();
        let source = hdr.dxf_source_variables.as_ref();
        let dwg = document.dwg_source_version;
        let wanted = |name: &str, since: DxfVersion, differs: bool| {
            differs
                || source.is_some_and(|vars| vars.contains(name))
                || (source.is_none() && dwg.is_some_and(|version| version >= since))
        };
        macro_rules! var {
            ($name:literal, $since:ident, $differs:expr, $write:expr) => {
                if wanted($name, DxfVersion::$since, $differs) {
                    self.write_header_variable($name, $write)?;
                }
            };
        }
        macro_rules! point3 {
            ($name:literal, $since:ident, $field:ident) => {
                var!($name, $since, hdr.$field != d.$field, |w| {
                    w.write_double(10, hdr.$field.x)?;
                    w.write_double(20, hdr.$field.y)?;
                    w.write_double(30, hdr.$field.z)
                });
            };
        }
        macro_rules! flag {
            ($name:literal, $since:ident, $field:ident) => {
                var!($name, $since, hdr.$field != d.$field, |w| w.write_i16(70, hdr.$field as i16));
            };
        }
        macro_rules! bool290 {
            ($name:literal, $since:ident, $field:ident) => {
                var!($name, $since, hdr.$field != d.$field, |w| w.write_bool(290, hdr.$field));
            };
        }
        macro_rules! int {
            ($name:literal, $since:ident, $code:literal, $field:ident) => {
                var!($name, $since, hdr.$field != d.$field, |w| w.write_i16($code, hdr.$field));
            };
        }
        macro_rules! real {
            ($name:literal, $since:ident, $code:literal, $field:ident) => {
                var!($name, $since, hdr.$field != d.$field, |w| w.write_double($code, hdr.$field));
            };
        }
        macro_rules! text {
            ($name:literal, $since:ident, $field:ident) => {
                var!($name, $since, hdr.$field != d.$field, |w| w.write_string(1, &hdr.$field));
            };
        }
        point3!("$PINSBASE", AC1012, paper_space_insertion_base);
        point3!("$PEXTMIN", AC1012, paper_space_extents_min);
        point3!("$PEXTMAX", AC1012, paper_space_extents_max);
        var!("$PLIMMIN", AC1012, hdr.paper_space_limits_min != d.paper_space_limits_min, |w| {
            w.write_double(10, hdr.paper_space_limits_min.x)?;
            w.write_double(20, hdr.paper_space_limits_min.y)
        });
        var!("$PLIMMAX", AC1012, hdr.paper_space_limits_max != d.paper_space_limits_max, |w| {
            w.write_double(10, hdr.paper_space_limits_max.x)?;
            w.write_double(20, hdr.paper_space_limits_max.y)
        });
        flag!("$LIMCHECK", AC1012, limit_check);
        flag!("$DIMASO", AC1012, associate_dimensions);
        flag!("$DIMSHO", AC1012, update_dimensions_while_dragging);
        flag!("$USRTIMER", AC1012, user_timer);
        real!("$TDUSRTIMER", AC1012, 40, user_elapsed_time);
        flag!("$ATTREQ", AC1012, attribute_request);
        flag!("$ATTDIA", AC1012, attribute_dialog);
        flag!("$BLIPMODE", AC1012, blip_mode);
        flag!("$DELOBJ", AC1012, delete_objects);
        int!("$COORDS", AC1012, 70, coords_mode);
        int!("$DRAGMODE", AC1012, 70, drag_mode);
        int!("$ISOLINES", AC1012, 70, isolines);
        int!("$TEXTQLTY", AC1012, 70, text_quality);
        int!("$PICKSTYLE", AC1012, 70, pick_style);
        int!("$OBSCOLOR", AC1018, 70, obscured_color);
        int!("$OBSLTYPE", AC1018, 280, obscured_linetype);
        int!("$INTERSECTIONCOLOR", AC1018, 70, intersection_color);
        int!("$INTERSECTIONDISPLAY", AC1018, 280, intersection_display);
        bool290!("$OLESTARTUP", AC1015, ole_startup);
        bool290!("$XEDIT", AC1015, xedit);
        bool290!("$EXTNAMES", AC1015, extended_names);
        bool290!("$CAMERADISPLAY", AC1021, camera_display);
        int!("$HIDETEXT", AC1018, 280, hide_text);
        int!("$SORTENTS", AC1018, 280, sort_entities);
        int!("$INDEXCTL", AC1018, 280, index_control);
        int!("$HALOGAP", AC1018, 280, halo_gap);
        int!("$ENDCAPS", AC1015, 280, end_caps);
        int!("$JOINSTYLE", AC1015, 280, join_style);
        int!("$DIMASSOC", AC1018, 280, dimension_associativity);
        int!("$DIMAZIN", AC1015, 70, dim_alt_zero_suppression);
        flag!("$DIMUPT", AC1012, dim_user_positioned_text);
        real!("$DIMALTRND", AC1015, 40, dim_alt_rounding);
        // A DWG header names the arrow blocks by handle; the DXF variable carries the block name.
        let block_name = |name: &str, handle: Handle| -> String {
            if !name.is_empty() || handle.is_null() {
                return name.to_string();
            }
            document
                .block_records
                .iter()
                .find(|record| record.handle == handle)
                .map(|record| record.name.clone())
                .unwrap_or_default()
        };
        for (name, since, value, default) in [
            ("$DIMBLK", DxfVersion::AC1012, block_name(&hdr.dim_arrow_block, hdr.dim_arrow_block_handle), &d.dim_arrow_block),
            ("$DIMBLK1", DxfVersion::AC1012, block_name(&hdr.dim_arrow_block1, hdr.dim_arrow_block1_handle), &d.dim_arrow_block1),
            ("$DIMBLK2", DxfVersion::AC1012, block_name(&hdr.dim_arrow_block2, hdr.dim_arrow_block2_handle), &d.dim_arrow_block2),
            ("$DIMLDRBLK", DxfVersion::AC1012, block_name(&hdr.dim_leader_arrow_block, hdr.dim_leader_arrow_block_handle), &d.dim_leader_arrow_block),
        ] {
            if wanted(name, since, value != *default)
                && (!value.is_empty() || source.is_some() || dwg.is_some_and(|version| version <= DxfVersion::AC1014))
            {
                self.write_header_variable(name, |w| w.write_string(1, &value))?;
            }
        }
        text!("$PROJECTNAME", AC1018, project_name);
        var!("$STYLESHEET", AC1015, hdr.stylesheet != d.stylesheet, |w| w.write_string(1, &hdr.stylesheet));
        text!("$MENU", AC1012, menu_name);
        real!("$LENSLENGTH", AC1021, 40, lens_length);
        real!("$CAMERAHEIGHT", AC1021, 40, camera_height);
        real!("$LOFTANG1", AC1021, 40, loft_angle1);
        real!("$LOFTANG2", AC1021, 40, loft_angle2);
        real!("$LOFTMAG1", AC1021, 40, loft_magnitude1);
        real!("$LOFTMAG2", AC1021, 40, loft_magnitude2);
        int!("$LOFTNORMALS", AC1021, 280, loft_normals);
        int!("$LOFTPARAM", AC1021, 70, loft_param);
        var!("$TIMEZONE", AC1021, hdr.timezone != d.timezone, |w| w.write_i16(70, hdr.timezone as i16));
        real!("$LATITUDE", AC1015, 40, latitude);
        real!("$LONGITUDE", AC1015, 40, longitude);
        real!("$NORTHDIRECTION", AC1015, 40, north_direction);
        real!("$CHAMFERC", AC1012, 40, chamfer_length);
        real!("$CHAMFERD", AC1012, 40, chamfer_angle);
        real!("$FACETRES", AC1012, 40, facet_resolution);
        int!("$PROXYGRAPHICS", AC1015, 70, proxy_graphics);
        real!("$PSVPSCALE", AC1012, 40, viewport_scale_factor);
        int!("$SHADEDGE", AC1012, 70, shade_edge);
        int!("$SHADEDIF", AC1012, 70, shade_diffuse);
        real!("$SHADOWPLANELOCATION", AC1021, 40, shadow_plane_location);
        real!("$STEPSIZE", AC1021, 40, step_size);
        real!("$STEPSPERSEC", AC1021, 40, steps_per_second);
        int!("$TREEDEPTH", AC1012, 70, tree_depth);
        int!("$USERI1", AC1012, 70, user_int1);
        int!("$USERI2", AC1012, 70, user_int2);
        int!("$USERI3", AC1012, 70, user_int3);
        int!("$USERI4", AC1012, 70, user_int4);
        int!("$USERI5", AC1012, 70, user_int5);
        real!("$USERR1", AC1012, 40, user_real1);
        real!("$USERR2", AC1012, 40, user_real2);
        real!("$USERR3", AC1012, 40, user_real3);
        real!("$USERR4", AC1012, 40, user_real4);
        real!("$USERR5", AC1012, 40, user_real5);
        Ok(())
    }

    /// Write the drawing properties (`SummaryInfo`, the DWGPROPS dialog) as
    /// R2004+ header variables: `$TITLE`, `$SUBJECT`, `$AUTHOR`, `$KEYWORDS`,
    /// `$COMMENTS`, `$LASTSAVEDBY` and `$REVISIONNUMBER` in that order, empty
    /// ones omitted, then a `$CUSTOMPROPERTYTAG` / `$CUSTOMPROPERTY` pair per
    /// custom property with a name. Custom properties are only accepted after
    /// `$LASTSAVEDBY`, so that variable is written whenever there are any.
    fn write_summary_info(&mut self, document: &CadDocument) -> Result<()> {
        let info = &document.summary_info;
        for (name, value) in [
            ("$TITLE", &info.title),
            ("$SUBJECT", &info.subject),
            ("$AUTHOR", &info.author),
            ("$KEYWORDS", &info.keywords),
            ("$COMMENTS", &info.comments),
        ] {
            if !value.is_empty() {
                self.write_header_variable(name, |w| w.write_string(1, value))?;
            }
        }
        let custom: Vec<&(String, String)> = info
            .custom_properties
            .iter()
            .filter(|(name, _)| !name.is_empty())
            .collect();
        let last_saved_by = if info.last_saved_by.is_empty() {
            document.header.last_saved_by.as_str()
        } else {
            info.last_saved_by.as_str()
        };
        if !last_saved_by.is_empty() || !custom.is_empty() {
            self.write_header_variable("$LASTSAVEDBY", |w| w.write_string(1, last_saved_by))?;
        }
        if !info.revision_number.is_empty() {
            self.write_header_variable("$REVISIONNUMBER", |w| {
                w.write_string(1, &info.revision_number)
            })?;
        }
        for (name, value) in custom {
            self.write_header_variable("$CUSTOMPROPERTYTAG", |w| w.write_string(1, name))?;
            self.write_header_variable("$CUSTOMPROPERTY", |w| w.write_string(1, value))?;
        }
        Ok(())
    }

    /// Write a header variable
    fn write_header_variable<F>(&mut self, name: &str, write_value: F) -> Result<()>
    where
        F: FnOnce(&mut W) -> Result<()>,
    {
        if self.omitted_header_variables.contains(name) || (self.r12 && !R12_HEADER_VARIABLES.contains(&name)) {
            return Ok(());
        }
        self.writer.write_string(9, name)?;
        write_value(self.writer)
    }

    /// Write the CLASSES section
    pub fn write_classes(&mut self, document: &CadDocument) -> Result<()> {
        self.writer.write_section_start("CLASSES")?;

        let mut classes = Self::classes_to_write(document);
        // The class of a carrier the document does not declare (a source's own CLASSES section is as it was read).
        if let (None, Some(class)) = (document.classes.source_classes(), self.carriers.class.as_ref()) {
            if !classes.iter().any(|written| written.dxf_name.eq_ignore_ascii_case(&class.dxf_name)) {
                classes.push(class);
            }
        }
        for class in classes {
            self.writer.write_string(0, "CLASS")?;
            self.writer.write_string(1, &class.dxf_name)?;
            self.writer.write_string(2, &class.cpp_class_name)?;
            self.writer.write_string(3, &class.application_name)?;
            self.writer.write_i32(90, class.proxy_flags.0 as i32)?;
            if self.dxf_version >= DxfVersion::AC1018 {
                self.writer.write_i32(91, class.instance_count)?;
            }
            self.writer
                .write_byte(280, if class.was_zombie { 1 } else { 0 })?;
            self.writer
                .write_byte(281, if class.is_an_entity { 1 } else { 0 })?;
        }

        self.writer.write_section_end()?;
        Ok(())
    }

    /// The CLASS records of the output. A document read from DXF keeps its CLASSES section as read (flags,
    /// application names and instance counts included); the registry of default classes it also carries is not
    /// written, only the registry classes that a written entity or object needs and the section lacked. Any
    /// other document (DWG source, built in memory) writes its class registry.
    fn classes_to_write(document: &CadDocument) -> Vec<&crate::classes::DxfClass> {
        let Some(source) = document.classes.source_classes() else {
            return document.classes.iter().collect();
        };
        let mut out: Vec<&crate::classes::DxfClass> = source.iter().collect();
        let mut present: HashSet<String> = source.iter().map(|class| class.dxf_name.to_ascii_uppercase()).collect();
        // Only records added after the file was read can need a class the section lacks: everything the file
        // holds is declared as its own writer declared it (a DXF built-in such as MLINE has no class). Those
        // records have no handle yet or one at or above the source's handle seed. Every name a record may be
        // declared under counts: it is covered when the section has any of them.
        let seed = document.header.source_handle_seed;
        let added = |handle: Handle| handle.is_null() || seed.is_some_and(|seed| handle.value() >= seed);
        let entity_names = document
            .entities()
            .filter(|entity| added(entity.common().handle))
            .map(|entity| vec![std::borrow::Cow::Borrowed(crate::io::dwg::dwg_writer::entity_class_name(entity))]);
        let object_names = document
            .objects
            .iter()
            .filter(|(handle, _)| added(**handle))
            .map(|(_, object)| crate::io::dwg::dwg_stream_writers::object_writer::objects::object_class_names(object));
        let field_names: Vec<Vec<std::borrow::Cow<str>>> = if document.version >= DxfVersion::AC1018
            && document.fields.keys().any(|handle| added(*handle))
        {
            vec![vec!["FIELD".into()], vec!["FIELDLIST".into()]]
        } else {
            Vec::new()
        };
        for names in entity_names.chain(object_names).chain(field_names) {
            // DXF declares some data-table classes with the ACDB prefix the DWG class table leaves off.
            let covered = |name: &str| {
                let upper = name.to_ascii_uppercase();
                present.contains(&upper) || present.contains(&format!("ACDB{upper}"))
            };
            if names.is_empty() || names.iter().any(|name| covered(name)) {
                continue;
            }
            if let Some(class) = names.iter().find_map(|name| document.classes.get_by_name(name)) {
                present.insert(class.dxf_name.to_ascii_uppercase());
                out.push(class);
            }
        }
        out
    }

    /// Write the TABLES section
    pub fn write_tables(&mut self, document: &CadDocument) -> Result<()> {
        self.writer.write_section_start("TABLES")?;

        if self.r12 {
            return self.write_r12_tables(document);
        }

        // Write tables in the standard order
        self.write_vport_table(document)?;
        self.write_ltype_table(document)?;
        self.write_layer_table(document)?;
        self.write_style_table(document)?;
        self.write_view_table(document)?;
        self.write_ucs_table(document)?;
        self.write_appid_table(document)?;
        self.write_dimstyle_table(document)?;
        // R12 has no BLOCK_RECORD table: the blocks are the BLOCKS section.
        if !self.r12 {
            self.write_block_record_table(document)?;
        }

        self.writer.write_section_end()?;
        Ok(())
    }

    /// The TABLES section of an R12 file: the tables of the source in the order it had them (an R12 reader creates
    /// the records of a missing table itself), then the standard ones that hold records and were not among them.
    fn write_r12_tables(&mut self, document: &CadDocument) -> Result<()> {
        const STANDARD: [&str; 8] = ["VPORT", "LTYPE", "LAYER", "STYLE", "VIEW", "UCS", "APPID", "DIMSTYLE"];
        let mut order: Vec<&str> = Vec::new();
        for table in document.legacy_dxf_tables.iter().flatten() {
            if let Some(name) = STANDARD.iter().find(|name| table.name.eq_ignore_ascii_case(name)) {
                if !order.contains(name) {
                    order.push(name);
                }
            }
        }
        let has_source = !order.is_empty();
        for name in STANDARD {
            let holds_records = match name {
                "VPORT" => document.vports.len() > 0,
                "LTYPE" => document.line_types.len() > 0,
                "LAYER" => document.layers.len() > 0,
                "STYLE" => document.text_styles.len() > 0,
                "VIEW" => document.views.len() > 0,
                "UCS" => document.ucss.len() > 0,
                "APPID" => document.app_ids.len() > 0,
                _ => document.dim_styles.len() > 0,
            };
            // Without a source every table is written, as AutoCAD does, empty or not.
            if !order.contains(&name) && (!has_source || holds_records) {
                order.push(name);
            }
        }
        for name in order {
            match name {
                "VPORT" => self.write_vport_table(document)?,
                "LTYPE" => self.write_ltype_table(document)?,
                "LAYER" => self.write_layer_table(document)?,
                "STYLE" => self.write_style_table(document)?,
                "VIEW" => self.write_view_table(document)?,
                "UCS" => self.write_ucs_table(document)?,
                "APPID" => self.write_appid_table(document)?,
                _ => self.write_dimstyle_table(document)?,
            }
        }
        self.writer.write_section_end()?;
        Ok(())
    }

    /// Write VPORT table
    fn write_vport_table(&mut self, document: &CadDocument) -> Result<()> {
        let table_handle = document.vports.handle();
        self.write_table_header("VPORT", document.vports.len(), table_handle, document)?;

        for vport in document.vports.iter() {
            self.write_vport_entry(vport, table_handle, document)?;
            self.write_table_xdata(document, vport.handle, &[])?;
        }

        self.write_table_end()?;
        Ok(())
    }

    fn write_vport_entry(
        &mut self,
        vport: &VPort,
        owner: Handle,
        document: &CadDocument,
    ) -> Result<()> {
        self.writer.write_string(0, "VPORT")?;
        self.write_common_table_data(vport.handle(), owner, document)?;
        self.writer.write_subclass("AcDbSymbolTableRecord")?;
        self.writer.write_subclass("AcDbViewportTableRecord")?;
        self.writer.write_string(2, vport.name())?;
        let mut flags = 0;
        if vport.xref_dependent {
            flags |= 0x10;
        }
        if vport.xref_resolved {
            flags |= 0x20;
        }
        if vport.xref_reference {
            flags |= 0x40;
        }
        self.writer.write_i16(70, flags)?;

        // Lower-left corner
        self.writer.write_double(10, vport.lower_left.x)?;
        self.writer.write_double(20, vport.lower_left.y)?;

        // Upper-right corner
        self.writer.write_double(11, vport.upper_right.x)?;
        self.writer.write_double(21, vport.upper_right.y)?;

        // View center
        self.writer.write_double(12, vport.view_center.x)?;
        self.writer.write_double(22, vport.view_center.y)?;

        // Snap base point
        self.writer.write_double(13, vport.snap_base.x)?;
        self.writer.write_double(23, vport.snap_base.y)?;

        // Snap spacing
        self.writer.write_double(14, vport.snap_spacing.x)?;
        self.writer.write_double(24, vport.snap_spacing.y)?;

        // Grid spacing
        self.writer.write_double(15, vport.grid_spacing.x)?;
        self.writer.write_double(25, vport.grid_spacing.y)?;

        // View direction
        self.writer.write_double(16, vport.view_direction.x)?;
        self.writer.write_double(26, vport.view_direction.y)?;
        self.writer.write_double(36, vport.view_direction.z)?;

        // View target
        self.writer.write_double(17, vport.view_target.x)?;
        self.writer.write_double(27, vport.view_target.y)?;
        self.writer.write_double(37, vport.view_target.z)?;

        // View height
        self.writer.write_double(40, vport.view_height)?;

        // Aspect ratio
        self.writer.write_double(41, vport.aspect_ratio)?;

        // Lens length
        self.writer.write_double(42, vport.lens_length)?;

        // Front clipping plane
        self.writer.write_double(43, vport.front_clip)?;

        // Back clipping plane
        self.writer.write_double(44, vport.back_clip)?;

        // Snap rotation
        self.writer
            .write_double(50, vport.snap_rotation.to_dxf_degrees())?;

        // View twist angle
        self.writer
            .write_double(51, vport.view_twist.to_dxf_degrees())?;

        let mut view_mode = 0;
        if vport.perspective {
            view_mode |= 1;
        }
        if vport.front_clipping {
            view_mode |= 2;
        }
        if vport.back_clipping {
            view_mode |= 4;
        }
        if vport.ucsfollow {
            view_mode |= 8;
        }
        if vport.front_clip_at_eye {
            view_mode |= 16;
        }
        self.writer.write_i16(71, view_mode)?;

        // Circle zoom
        self.writer.write_i16(72, vport.circle_zoom)?;

        // Fast zoom
        self.writer
            .write_i16(73, if vport.fast_zoom { 1 } else { 0 })?;

        let mut ucsicon = 0;
        if vport.ucsicon_lower {
            ucsicon |= 1;
        }
        if vport.ucsicon_origin {
            ucsicon |= 2;
        }
        self.writer.write_i16(74, ucsicon)?;

        // Snap on
        self.writer
            .write_i16(75, if vport.snap_on { 1 } else { 0 })?;

        // Grid on
        self.writer
            .write_i16(76, if vport.grid_on { 1 } else { 0 })?;

        // Snap style
        self.writer
            .write_i16(77, if vport.snap_style { 1 } else { 0 })?;

        // Snap isopair
        self.writer.write_i16(78, vport.snap_isopair)?;

        if self.dxf_version >= DxfVersion::AC1015 {
            self.writer.write_i16(281, vport.render_mode.to_value())?;
            self.writer
                .write_i16(65, if vport.ucs_per_viewport { 1 } else { 0 })?;
            self.writer.write_double(110, vport.ucs_origin.x)?;
            self.writer.write_double(120, vport.ucs_origin.y)?;
            self.writer.write_double(130, vport.ucs_origin.z)?;
            self.writer.write_double(111, vport.ucs_x_axis.x)?;
            self.writer.write_double(121, vport.ucs_x_axis.y)?;
            self.writer.write_double(131, vport.ucs_x_axis.z)?;
            self.writer.write_double(112, vport.ucs_y_axis.x)?;
            self.writer.write_double(122, vport.ucs_y_axis.y)?;
            self.writer.write_double(132, vport.ucs_y_axis.z)?;
            self.writer.write_i16(79, vport.ucs_ortho_type)?;
            self.writer.write_double(146, vport.ucs_elevation)?;
            if !vport.named_ucs_handle.is_null() {
                self.writer.write_handle(345, vport.named_ucs_handle)?;
            }
            if !vport.base_ucs_handle.is_null() {
                self.writer.write_handle(346, vport.base_ucs_handle)?;
            }
        }
        if self.dxf_version >= DxfVersion::AC1021 {
            self.writer.write_i16(60, vport.grid_flags.to_bits())?;
            self.writer.write_i16(61, vport.grid_major)?;
            if !vport.background_handle.is_null() {
                self.writer.write_handle(332, vport.background_handle)?;
            }
            if !vport.visual_style_handle.is_null() {
                self.writer.write_handle(348, vport.visual_style_handle)?;
            }
            if !vport.sun_handle.is_null() {
                self.writer.write_handle(361, vport.sun_handle)?;
            }
            self.writer.write_bool(292, vport.use_default_lights)?;
            self.writer.write_i16(282, vport.default_lighting_type)?;
            self.writer.write_double(141, vport.brightness)?;
            self.writer.write_double(142, vport.contrast)?;
            self.writer.write_color(63, vport.ambient_color)?;
            if let Some(rgb) = vport.ambient_color.to_true_color_value() {
                self.writer.write_i32(421, rgb)?;
            }
        }

        Ok(())
    }

    /// Write LTYPE table
    fn write_ltype_table(&mut self, document: &CadDocument) -> Result<()> {
        let table_handle = document.line_types.handle();
        // BYLAYER and BYBLOCK are keywords of the entity records in R12, not LTYPE records: the document holds them
        // as defaults unless the R12 file it was read from had such a record.
        let r12 = self.r12;
        let written = |ltype: &LineType| {
            !r12
                || !(ltype.name.eq_ignore_ascii_case("ByLayer") || ltype.name.eq_ignore_ascii_case("ByBlock"))
                || document.legacy_dxf_tables.iter().flatten().any(|table| {
                    table.name == "LTYPE" && table.entries.iter().any(|entry| entry.name.eq_ignore_ascii_case(&ltype.name))
                })
        };
        let count = document.line_types.iter().filter(|ltype| written(ltype)).count();
        self.write_table_header("LTYPE", count, table_handle, document)?;

        for ltype in document.line_types.iter().filter(|ltype| written(ltype)) {
            self.write_ltype_entry(ltype, table_handle, document)?;
            self.write_table_xdata(document, ltype.handle, &[])?;
        }

        self.write_table_end()?;
        Ok(())
    }

    fn write_ltype_entry(
        &mut self,
        ltype: &LineType,
        owner: Handle,
        document: &CadDocument,
    ) -> Result<()> {
        self.writer.write_string(0, "LTYPE")?;
        self.write_common_table_data(ltype.handle(), owner, document)?;
        self.writer.write_subclass("AcDbSymbolTableRecord")?;
        self.writer.write_subclass("AcDbLinetypeTableRecord")?;
        self.writer.write_string(2, ltype.name())?;
        let mut flags: i16 = 0;
        if ltype.xref_dependent {
            flags |= 0x10;
        }
        flags |= ltype.source_flags & !0x10;
        self.writer.write_i16(70, flags)?;
        self.writer.write_string(3, &ltype.description)?;
        self.writer.write_i16(72, 65)?; // Alignment code (always 65)
        self.writer.write_i16(73, ltype.elements.len() as i16)?;
        self.writer.write_double(40, ltype.pattern_length)?;

        for element in &ltype.elements {
            self.writer.write_double(49, element.length)?;
            // DXF LTYPE per-element codes: 74 = element-type FLAGS
            // (0x01=abs rot, 0x02=text, 0x04=shape; 0 = plain dash),
            // 75 = shape number. These used to be emitted swapped — AutoCAD
            // then read every element as complex (OCS#314).
            if let Some(c) = &element.complex {
                let mut flags: i16 = c.source_flags as i16;
                flags &= !(0x01 | 0x02 | 0x04);
                if c.is_absolute_rotation() {
                    flags |= 0x01;
                }
                if c.is_shape() {
                    flags |= 0x04;
                } else if c.is_text() {
                    flags |= 0x02;
                }

                self.writer.write_i16(74, flags)?;
                self.writer.write_i16(75, c.shape_number().unwrap_or(0))?;
                // AutoCAD's DXF reader is order-sensitive here: the STYLE
                // pointer (340) must follow 75 directly and must be present
                // for every complex element, otherwise the whole file is
                // rejected with "Missing group code 340 in complex linetype".
                // Order per the DXF reference: 49, 74, 75, 340, 46, 50, 44, 45, 9.
                let style_handle = if c.style_handle.is_null() {
                    document
                        .text_styles
                        .get("Standard")
                        .map(|s| s.handle)
                        .filter(|h| !h.is_null())
                        .or_else(|| document.text_styles.iter().map(|s| s.handle).find(|h| !h.is_null()))
                        .unwrap_or(Handle::NULL)
                } else {
                    c.style_handle
                };
                self.writer.write_handle(340, style_handle)?;
                self.writer.write_double(46, c.scale)?;
                // `rotation` is stored in radians; DXF code 50 is in degrees.
                self.writer.write_double(50, c.rotation.to_dxf_degrees())?;
                self.writer.write_double(44, c.offset[0])?;
                self.writer.write_double(45, c.offset[1])?;
                if let Some(t) = c.text() {
                    if !t.is_empty() {
                        self.writer.write_string(9, t)?;
                    }
                }
            } else {
                // Plain dash element — AutoCAD emits the zero flag word (R12 has no such group).
                if !self.r12 {
                    self.writer.write_i16(74, 0)?;
                }
            }
        }

        Ok(())
    }

    /// Write LAYER table
    fn write_layer_table(&mut self, document: &CadDocument) -> Result<()> {
        let table_handle = document.layers.handle();
        self.write_table_header("LAYER", document.layers.len(), table_handle, document)?;

        for layer in document.layers.iter() {
            let handle = if layer.handle.is_null() {
                self.allocate_handle()
            } else {
                layer.handle
            };
            self.write_layer_entry(layer, table_handle, handle, document)?;
        }

        self.write_table_end()?;
        Ok(())
    }

    fn write_layer_entry(
        &mut self,
        layer: &Layer,
        owner: Handle,
        handle: Handle,
        document: &CadDocument,
    ) -> Result<()> {
        self.writer.write_string(0, "LAYER")?;
        self.write_common_table_data(handle, owner, document)?;
        self.writer.write_subclass("AcDbSymbolTableRecord")?;
        self.writer.write_subclass("AcDbLayerTableRecord")?;
        self.writer.write_string(2, layer.name())?;

        // Flags
        let mut flags: i16 = 0;
        if layer.is_frozen() {
            flags |= 1;
        }
        if layer.flags.frozen_in_new_viewport {
            flags |= 2;
        }
        if layer.is_locked() {
            flags |= 4;
        }
        if layer.flags.xref_dependent {
            flags |= 0x10;
        }
        // Source bits without a typed flag (64 = referenced, 32 = resolved xref) are carried through.
        flags |= layer.source_flags & !(1 | 2 | 4 | 0x10);
        self.writer.write_i16(70, flags)?;

        // Color (negative if layer is off)
        let color_index = match layer.color {
            Color::Index(i) => i as i16,
            Color::ByLayer => 7,
            Color::None => 7,
            Color::ByBlock => 0,
            Color::Rgb { .. } => layer.color.approximate_index(),
        };
        if !layer.is_off() {
            self.writer.write_i16(62, color_index)?;
        } else {
            self.writer.write_i16(62, -color_index)?;
        }
        // Preserve the exact RGB value alongside the nearest ACI fallback (R2004 and later; R12 has the index).
        if let Some(tc) = layer.color.to_true_color_value().filter(|_| !self.r12) {
            self.writer.write_i32(420, tc)?;
        }
        if self.dxf_version >= DxfVersion::AC1018 {
            if let Some(name) = crate::io::dxf::join_color_book_name(
                layer.book_name.as_deref(),
                layer.color_name.as_deref(),
            ) {
                self.writer.write_string(430, &name)?;
            }
        }

        // Linetype name
        self.writer.write_string(6, &layer.line_type)?;

        // Lineweight (R2002 and later)
        if !self.r12 {
            self.writer.write_i16(370, layer.line_weight.value())?;
        }

        // R2000+ layer records require a hard pointer to a plot-style object.
        // NOTE (issue #51): BricsCAD's audit only accepts the reference when
        // the NOD's ACAD_PLOTSTYLENAME entry is a soft pointer (350) and the
        // plot style dictionary matches its export form; see
        // is_canonical_hard_owner_key.
        if self.dxf_version >= DxfVersion::AC1015 {
            // A layer without a plot style gets the Normal one only in a document built in memory; a layer read
            // from a file keeps what the file had.
            let from_file = document.dwg_source_version.is_some()
                || document.header.dxf_source_variables.is_some();
            let plotstyle_handle = if layer.plotstyle_handle.is_null() {
                if from_file {
                    Handle::NULL
                } else {
                    self.normal_plotstyle_handle
                }
            } else {
                layer.plotstyle_handle
            };
            if !plotstyle_handle.is_null() {
                self.writer.write_handle(390, plotstyle_handle)?;
            }
        }

        // Plot flag (code 290 is Bool type - single byte in binary); R2000 and later
        if !self.r12 {
            self.writer.write_bool(290, layer.is_plottable)?;
        }
        // R2007+ layer material (hard pointer).
        if self.dxf_version >= DxfVersion::AC1021 && !layer.material.is_null() {
            self.writer.write_handle(347, layer.material)?;
        }

        let mut generated = Vec::new();
        // AutoCAD stores layer transparency as AcCmTransparency XDATA.
        let source_transparency = document
            .object_xdata
            .get(&layer.handle)
            .and_then(|data| data.get_record("AcCmTransparency"));
        // A retained record that still decodes to the layer's transparency is written as it was (a packed zero
        // is the opaque default, which would otherwise come back as the explicit 33554687).
        let source_unchanged = source_transparency.is_some_and(|record| {
            record.values.iter().any(|value| {
                matches!(value, XDataValue::Integer32(packed)
                    if crate::types::Transparency::from_layer_alpha_value(*packed as u32)
                        == Some(layer.transparency))
            })
        });
        if !source_unchanged
            && (!layer.transparency.is_opaque() || source_transparency.is_some())
        {
            let mut record = crate::xdata::ExtendedDataRecord::new("AcCmTransparency");
            record.values.push(XDataValue::Integer32(layer.transparency.to_dxf_value()));
            generated.push(record);
        }

        // ... and the description as AcAecLayerStandard XDATA, the text being
        // the second of two strings.
        // A source record that still carries the layer's description is written as it was (it may have one
        // string only).
        let source_description = document
            .object_xdata
            .get(&layer.handle)
            .and_then(|data| data.get_record("AcAecLayerStandard"))
            .map(|record| {
                record
                    .values
                    .iter()
                    .filter_map(|value| if let XDataValue::String(text) = value { Some(text.as_str()) } else { None })
                    .nth(1)
                    .unwrap_or("")
                    .to_string()
            });
        let description_unchanged = source_description.as_deref() == Some(layer.description.as_str());
        if !description_unchanged && (!layer.description.is_empty() || source_description.is_some()) {
            let mut record = crate::xdata::ExtendedDataRecord::new("AcAecLayerStandard");
            record.values = vec![XDataValue::String(String::new()), XDataValue::String(layer.description.clone())];
            generated.push(record);
        }
        self.write_table_xdata(document, layer.handle, &generated)?;

        Ok(())
    }

    /// Write STYLE table (text styles)
    fn write_style_table(&mut self, document: &CadDocument) -> Result<()> {
        let table_handle = document.text_styles.handle();
        self.write_table_header("STYLE", document.text_styles.len(), table_handle, document)?;

        for style in document.text_styles.iter() {
            self.write_style_entry(style, table_handle, document)?;
        }

        self.write_table_end()?;
        Ok(())
    }

    /// Persist the annotative flag the standard way: XDATA under the
    /// `AcadAnnotative` application, in the form
    /// `AnnotativeData { <version=1> <flag> }`. Written only when the record
    /// is annotative; its absence on read means non-annotative. This matches
    /// how AutoCAD stores annotative on STYLE / DIMSTYLE / TABLESTYLE records.
    fn write_annotative_xdata(&mut self, annotative: bool) -> Result<()> {
        if !annotative {
            return Ok(());
        }
        self.writer.write_string(1001, "AcadAnnotative")?;
        self.writer.write_string(1000, "AnnotativeData")?;
        self.writer.write_string(1002, "{")?;
        self.writer.write_i16(1070, 1)?;
        self.writer.write_i16(1070, 1)?;
        self.writer.write_string(1002, "}")?;
        Ok(())
    }

    fn write_style_entry(
        &mut self,
        style: &TextStyle,
        owner: Handle,
        document: &CadDocument,
    ) -> Result<()> {
        self.writer.write_string(0, "STYLE")?;
        self.write_common_table_data(style.handle(), owner, document)?;
        self.writer.write_subclass("AcDbSymbolTableRecord")?;
        self.writer.write_subclass("AcDbTextStyleTableRecord")?;
        // Shape files have an empty DXF STYLE name; SHAPE resolves group 2
        // by searching the shape files, not a text-style name.
        self.writer.write_string(
            2,
            if style.is_shape_file {
                ""
            } else {
                style.name()
            },
        )?;
        let mut flags: i16 = 0;
        if style.is_shape_file {
            flags |= 0x01;
        }
        if style.is_vertical {
            flags |= 0x04;
        }
        if style.xref_dependent {
            flags |= 0x10;
        }
        flags |= style.source_flags & !(0x01 | 0x04 | 0x10);
        self.writer.write_i16(70, flags)?;
        self.writer.write_double(40, style.height)?;
        self.writer.write_double(41, style.width_factor)?;
        // Group 50 is in degrees; the entity stores radians.
        self.writer.write_double(50, style.oblique_angle.to_dxf_degrees())?;
        // Text generation flags: 2 = backward, 4 = upside down.
        let mut generation: i16 = 0;
        if style.flags.backward {
            generation |= 2;
        }
        if style.flags.upside_down {
            generation |= 4;
        }
        self.writer.write_i16(71, generation)?;
                                       // Last height used — must be > 0 for CAD validation
        self.writer
            .write_double(42, style.effective_last_height())?;
        self.writer.write_string(3, &style.font_file)?;
        self.writer.write_string(4, &style.big_font_file)?;
        // The font record is merged into the style's retained XDATA: a style read from a file keeps its
        // `ACAD` record there, and a second `1001 ACAD` block is a duplicate application for readers.
        let mut generated = Vec::new();
        if !style.true_type_font.trim().is_empty() {
            let mut record = crate::xdata::ExtendedDataRecord::new("ACAD");
            record.values = vec![
                XDataValue::String(style.true_type_font.trim().to_string()),
                XDataValue::Integer32(style.true_type_font_flags),
            ];
            generated.push(record);
        }
        self.write_table_annotative_xdata_with(document, style.handle, style.annotative, generated)?;

        Ok(())
    }

    /// Write VIEW table
    fn write_view_table(&mut self, document: &CadDocument) -> Result<()> {
        let table_handle = document.views.handle();
        self.write_table_header("VIEW", document.views.len(), table_handle, document)?;

        for view in document.views.iter() {
            self.write_view_entry(view, table_handle, document)?;
            self.write_table_xdata(document, view.handle, &[])?;
        }

        self.write_table_end()?;
        Ok(())
    }

    fn write_view_entry(
        &mut self,
        view: &View,
        owner: Handle,
        document: &CadDocument,
    ) -> Result<()> {
        self.writer.write_string(0, "VIEW")?;
        self.write_common_table_data(view.handle(), owner, document)?;
        self.writer.write_subclass("AcDbSymbolTableRecord")?;
        self.writer.write_subclass("AcDbViewTableRecord")?;
        self.writer.write_string(2, view.name())?;
        let mut flags = 0;
        if view.paper_space {
            flags |= 1;
        }
        if view.xref_dependent {
            flags |= 0x10;
        }
        if view.xref_resolved {
            flags |= 0x20;
        }
        if view.xref_reference {
            flags |= 0x40;
        }
        self.writer.write_i16(70, flags)?;
        self.writer.write_double(40, view.height)?;
        self.writer.write_double(10, view.center.x)?;
        self.writer.write_double(20, view.center.y)?;
        self.writer.write_double(41, view.width)?;
        self.writer.write_double(11, view.direction.x)?;
        self.writer.write_double(21, view.direction.y)?;
        self.writer.write_double(31, view.direction.z)?;
        self.writer.write_double(12, view.target.x)?;
        self.writer.write_double(22, view.target.y)?;
        self.writer.write_double(32, view.target.z)?;
        self.writer.write_double(42, view.lens_length)?;
        self.writer.write_double(43, view.front_clip)?;
        self.writer.write_double(44, view.back_clip)?;
        self.writer
            .write_double(50, view.twist_angle.to_dxf_degrees())?;
        let mut view_mode = 0;
        if view.perspective {
            view_mode |= 1;
        }
        if view.front_clipping {
            view_mode |= 2;
        }
        if view.back_clipping {
            view_mode |= 4;
        }
        if view.front_clip_at_eye {
            view_mode |= 16;
        }
        self.writer.write_i16(71, view_mode)?;

        if self.dxf_version >= DxfVersion::AC1015 {
            self.writer
                .write_i16(72, if view.ucs_associated { 1 } else { 0 })?;
            self.writer.write_i16(281, view.render_mode.to_value())?;
            if view.ucs_associated {
                self.writer.write_double(110, view.ucs_origin.x)?;
                self.writer.write_double(120, view.ucs_origin.y)?;
                self.writer.write_double(130, view.ucs_origin.z)?;
                self.writer.write_double(111, view.ucs_x_axis.x)?;
                self.writer.write_double(121, view.ucs_x_axis.y)?;
                self.writer.write_double(131, view.ucs_x_axis.z)?;
                self.writer.write_double(112, view.ucs_y_axis.x)?;
                self.writer.write_double(122, view.ucs_y_axis.y)?;
                self.writer.write_double(132, view.ucs_y_axis.z)?;
                self.writer.write_double(146, view.ucs_elevation)?;
                self.writer.write_i16(79, view.ucs_ortho_type)?;
                if !view.named_ucs_handle.is_null() {
                    self.writer.write_handle(345, view.named_ucs_handle)?;
                }
                if !view.base_ucs_handle.is_null() {
                    self.writer.write_handle(346, view.base_ucs_handle)?;
                }
            }
        }
        if self.dxf_version >= DxfVersion::AC1021 {
            self.writer
                .write_i16(73, if view.camera_plottable { 1 } else { 0 })?;
            if !view.background_handle.is_null() {
                self.writer.write_handle(332, view.background_handle)?;
            }
            if !view.live_section_handle.is_null() {
                self.writer.write_handle(334, view.live_section_handle)?;
            }
            if !view.visual_style_handle.is_null() {
                self.writer.write_handle(348, view.visual_style_handle)?;
            }
            if !view.sun_handle.is_null() {
                self.writer.write_handle(361, view.sun_handle)?;
            }
            self.writer.write_bool(292, view.use_default_lights)?;
            self.writer.write_i16(282, view.default_lighting_type)?;
            self.writer.write_double(141, view.brightness)?;
            self.writer.write_double(142, view.contrast)?;
            self.writer.write_color(63, view.ambient_color)?;
            if let Some(rgb) = view.ambient_color.to_true_color_value() {
                self.writer.write_i32(421, rgb)?;
            }
        }
        Ok(())
    }

    /// Write UCS table
    fn write_ucs_table(&mut self, document: &CadDocument) -> Result<()> {
        let table_handle = document.ucss.handle();
        self.write_table_header("UCS", document.ucss.len(), table_handle, document)?;

        for ucs in document.ucss.iter() {
            self.write_ucs_entry(ucs, table_handle, document)?;
            self.write_table_xdata(document, ucs.handle, &[])?;
        }

        self.write_table_end()?;
        Ok(())
    }

    fn write_ucs_entry(&mut self, ucs: &Ucs, owner: Handle, document: &CadDocument) -> Result<()> {
        self.writer.write_string(0, "UCS")?;
        self.write_common_table_data(ucs.handle(), owner, document)?;
        self.writer.write_subclass("AcDbSymbolTableRecord")?;
        self.writer.write_subclass("AcDbUCSTableRecord")?;
        self.writer.write_string(2, ucs.name())?;
        let mut flags = 0;
        if ucs.xref_dependent {
            flags |= 0x10;
        }
        if ucs.xref_resolved {
            flags |= 0x20;
        }
        if ucs.xref_reference {
            flags |= 0x40;
        }
        self.writer.write_i16(70, flags)?;
        self.writer.write_double(10, ucs.origin.x)?;
        self.writer.write_double(20, ucs.origin.y)?;
        self.writer.write_double(30, ucs.origin.z)?;
        self.writer.write_double(11, ucs.x_axis.x)?;
        self.writer.write_double(21, ucs.x_axis.y)?;
        self.writer.write_double(31, ucs.x_axis.z)?;
        self.writer.write_double(12, ucs.y_axis.x)?;
        self.writer.write_double(22, ucs.y_axis.y)?;
        self.writer.write_double(32, ucs.y_axis.z)?;

        if self.dxf_version >= DxfVersion::AC1015 {
            self.writer.write_i16(71, ucs.ortho_type)?;
            self.writer.write_i16(79, ucs.ortho_view_type)?;
            self.writer.write_double(146, ucs.elevation)?;
            if !ucs.base_ucs_handle.is_null() {
                self.writer.write_handle(346, ucs.base_ucs_handle)?;
            }
        }
        Ok(())
    }

    /// Write APPID table
    fn write_appid_table(&mut self, document: &CadDocument) -> Result<()> {
        let table_handle = document.app_ids.handle();
        let unused = unused_default_app_ids(document);
        let written = document
            .app_ids
            .iter()
            .filter(|appid| !unused.contains(&appid.name().to_ascii_uppercase()))
            .count();
        self.write_table_header(
            "APPID",
            written + usize::from(self.loft_reference_appid.is_some()),
            table_handle,
            document,
        )?;

        // The ACAD RegApp record must be the table's first entry: CAD
        // applications map XDATA applications by table index and require
        // ACAD at index 0 (issue #51 BricsCAD audit: "RegApp ACAD has
        // invalid index 1").
        let mut wrote_acad = false;
        if self.r12 {
            // An R12 file is written in the order of its source: a reader registers the applications in that order.
            let position = |appid: &AppId| {
                document
                    .source_app_id_order
                    .iter()
                    .flatten()
                    .position(|name| name.eq_ignore_ascii_case(appid.name()))
                    .unwrap_or(usize::MAX)
            };
            let mut ordered: Vec<&AppId> = document
                .app_ids
                .iter()
                .filter(|appid| !unused.contains(&appid.name().to_ascii_uppercase()))
                .collect();
            ordered.sort_by_key(|appid| position(appid));
            for appid in ordered {
                self.write_appid_entry(appid, table_handle, document)?;
            }
        } else {
            for appid in document.app_ids.iter() {
                if appid.name().eq_ignore_ascii_case("ACAD") {
                    self.write_appid_entry(appid, table_handle, document)?;
                    wrote_acad = true;
                    break;
                }
            }
            for appid in document.app_ids.iter() {
                if wrote_acad && appid.name().eq_ignore_ascii_case("ACAD") {
                    continue;
                }
                if unused.contains(&appid.name().to_ascii_uppercase()) {
                    continue;
                }
                self.write_appid_entry(appid, table_handle, document)?;
            }
        }
        if let Some(handle) = self.loft_reference_appid {
            let mut appid = AppId::new("CADCODEC_LOFT_REFERENCES");
            appid.handle = handle;
            self.write_appid_entry(&appid, table_handle, document)?;
        }

        self.write_table_end()?;
        Ok(())
    }

    fn write_appid_entry(
        &mut self,
        appid: &AppId,
        owner: Handle,
        document: &CadDocument,
    ) -> Result<()> {
        // Sanitize name: strip control chars and characters forbidden in symbol table names
        let name = sanitize_symbol_name(appid.name());
        if name.is_empty() {
            return Ok(());
        }
        self.writer.write_string(0, "APPID")?;
        self.write_common_table_data(appid.handle(), owner, document)?;
        self.writer.write_subclass("AcDbSymbolTableRecord")?;
        self.writer.write_subclass("AcDbRegAppTableRecord")?;
        self.writer.write_string(2, &name)?;
        self.writer.write_i16(70, appid.source_flags)?;
        self.write_table_xdata(document, appid.handle, &[])?;
        Ok(())
    }

    /// Write DIMSTYLE table
    fn write_dimstyle_table(&mut self, document: &CadDocument) -> Result<()> {
        let table_handle = document.dim_styles.handle();
        // The Standard style the document starts with is not a record of an R12 file that did not have it.
        let r12_source = self.r12 && document.legacy_dxf_tables.is_some();
        let written = |style: &DimStyle| {
            !(r12_source && !style.source_present && style.name.eq_ignore_ascii_case("Standard"))
        };
        let count = document.dim_styles.iter().filter(|style| written(style)).count();
        self.write_table_header("DIMSTYLE", count, table_handle, document)?;
        if self.dxf_version >= DxfVersion::AC1015 {
            self.writer.write_subclass("AcDbDimStyleTable")?;
            self.writer
                .write_i16(71, document.dim_styles.len() as i16)?;
        }

        for dimstyle in document.dim_styles.iter().filter(|style| written(style)) {
            self.write_dimstyle_entry(dimstyle, table_handle, document)?;
        }

        self.write_table_end()?;
        Ok(())
    }

    fn write_dimstyle_entry(
        &mut self,
        dimstyle: &DimStyle,
        owner: Handle,
        document: &CadDocument,
    ) -> Result<()> {
        self.writer.write_string(0, "DIMSTYLE")?;
        self.writer.write_handle(105, dimstyle.handle())?;
        self.write_table_entry_reactors(dimstyle.handle(), document)?;
        self.write_table_entry_xdictionary(dimstyle.handle(), document)?;
        self.writer.write_handle(330, owner)?;
        self.writer.write_subclass("AcDbSymbolTableRecord")?;
        self.writer.write_subclass("AcDbDimStyleTableRecord")?;
        self.writer.write_string(2, dimstyle.name())?;
        let mut flags = 0;
        if dimstyle.xref_dependent {
            flags |= 0x10;
        }
        if dimstyle.xref_resolved {
            flags |= 0x20;
        }
        if dimstyle.xref_reference {
            flags |= 0x40;
        }
        self.writer.write_i16(70, flags)?;

        // A group the source record left out (the reference reader then applies its own default) stays out
        // unless the value was changed: writing the native default would assert a value the source never had.
        let defaults = DimStyle::dxf_defaults("");
        let from_dxf = dimstyle.dxf_present_codes.is_some();
        macro_rules! put {
            ($code:expr, $field:ident, $write:expr) => {
                if !from_dxf
                    || dimstyle.dxf_code_present($code)
                    || dimstyle.$field != defaults.$field
                {
                    $write
                }
            };
        }

        // Postfix / suffix
        // The suffix is always stated: a record read from a DXF without group 3 has an empty suffix (AutoCAD omits
        // it), so "<>" must be written to read back as "<>".
        self.writer.write_string(3, &dimstyle.dimpost)?;
        // An R12 record states the suffix and the three arrow block names whether they are empty or not.
        if self.r12 || !dimstyle.dimapost.is_empty() {
            self.writer.write_string(4, &dimstyle.dimapost)?;
        }
        if self.dxf_version <= DxfVersion::AC1014 {
            if self.r12 || !dimstyle.dimblk_name.is_empty() {
                self.writer.write_string(5, &dimstyle.dimblk_name)?;
            }
            if self.r12 || !dimstyle.dimblk1_name.is_empty() {
                self.writer.write_string(6, &dimstyle.dimblk1_name)?;
            }
            if self.r12 || !dimstyle.dimblk2_name.is_empty() {
                self.writer.write_string(7, &dimstyle.dimblk2_name)?;
            }
        }

        // Scale / floats (codes 40-50)
        put! { 40, dimscale, self.writer.write_double(40, dimstyle.dimscale)? }
        put! { 41, dimasz, self.writer.write_double(41, dimstyle.dimasz)? }
        put! { 42, dimexo, self.writer.write_double(42, dimstyle.dimexo)? }
        put! { 43, dimdli, self.writer.write_double(43, dimstyle.dimdli)? }
        put! { 44, dimexe, self.writer.write_double(44, dimstyle.dimexe)? }
        put! { 45, dimrnd, self.writer.write_double(45, dimstyle.dimrnd)? }
        put! { 46, dimdle, self.writer.write_double(46, dimstyle.dimdle)? }
        put! { 47, dimtp, self.writer.write_double(47, dimstyle.dimtp)? }
        put! { 48, dimtm, self.writer.write_double(48, dimstyle.dimtm)? }
        // DIMFXL and DIMJOGANG exist from R2007; a record read from an older DWG holds zero placeholders for them.
        let has_r2007_dim_vars = !document
            .dwg_source_version
            .is_some_and(|version| version < DxfVersion::AC1021);
        if has_r2007_dim_vars && dimstyle.dimfxl != 1.0 {
            put! { 49, dimfxl, self.writer.write_double(49, dimstyle.dimfxl)? }
        }
        if has_r2007_dim_vars && dimstyle.dimjogang != std::f64::consts::FRAC_PI_4 {
            // Clamp to valid range [5°..90°]
            put! { 50, dimjogang, self.writer
                .write_double(50, dimstyle.dimjogang.to_dxf_degrees().clamp(5.0, 90.0))? }
        }

        // Floats 140-148
        put! { 140, dimtxt, self.writer.write_double(140, dimstyle.dimtxt)? }
        put! { 141, dimcen, self.writer.write_double(141, dimstyle.dimcen)? }
        put! { 142, dimtsz, self.writer.write_double(142, dimstyle.dimtsz)? }
        put! { 143, dimaltf, self.writer.write_double(143, dimstyle.dimaltf)? }
        put! { 144, dimlfac, self.writer.write_double(144, dimstyle.dimlfac)? }
        put! { 145, dimtvp, self.writer.write_double(145, dimstyle.dimtvp)? }
        put! { 146, dimtfac, self.writer.write_double(146, dimstyle.dimtfac)? }
        put! { 147, dimgap, self.writer.write_double(147, dimstyle.dimgap)? }
        if dimstyle.dimaltrnd != 0.0 {
            put! { 148, dimaltrnd, self.writer.write_double(148, dimstyle.dimaltrnd)? }
        }

        // Int16 flags (69-79)
        if self.dxf_version >= DxfVersion::AC1021 && dimstyle.dimtfill != 0 {
            put! { 69, dimtfill, self.writer.write_i16(69, dimstyle.dimtfill)? }
            put! { 70, dimtfillclr, self.writer.write_i16(70, dimstyle.dimtfillclr)? }
        }
        put! { 71, dimtol, self.writer
            .write_i16(71, if dimstyle.dimtol { 1 } else { 0 })? }
        put! { 72, dimlim, self.writer
            .write_i16(72, if dimstyle.dimlim { 1 } else { 0 })? }
        put! { 73, dimtih, self.writer
            .write_i16(73, if dimstyle.dimtih { 1 } else { 0 })? }
        put! { 74, dimtoh, self.writer
            .write_i16(74, if dimstyle.dimtoh { 1 } else { 0 })? }
        put! { 75, dimse1, self.writer
            .write_i16(75, if dimstyle.dimse1 { 1 } else { 0 })? }
        put! { 76, dimse2, self.writer
            .write_i16(76, if dimstyle.dimse2 { 1 } else { 0 })? }
        put! { 77, dimtad, self.writer.write_i16(77, dimstyle.dimtad)? }
        put! { 78, dimzin, self.writer.write_i16(78, dimstyle.dimzin)? }
        put! { 79, dimazin, self.writer.write_i16(79, dimstyle.dimazin)? }

        // Int16 / Int32 (90, 170-179)
        if dimstyle.dimarcsym != 0 {
            put! { 90, dimarcsym, self.writer.write_i32(90, dimstyle.dimarcsym as i32)? }
        }
        put! { 170, dimalt, self.writer
            .write_i16(170, if dimstyle.dimalt { 1 } else { 0 })? }
        put! { 171, dimaltd, self.writer.write_i16(171, dimstyle.dimaltd)? }
        put! { 172, dimtofl, self.writer
            .write_i16(172, if dimstyle.dimtofl { 1 } else { 0 })? }
        put! { 173, dimsah, self.writer
            .write_i16(173, if dimstyle.dimsah { 1 } else { 0 })? }
        put! { 174, dimtix, self.writer
            .write_i16(174, if dimstyle.dimtix { 1 } else { 0 })? }
        put! { 175, dimsoxd, self.writer
            .write_i16(175, if dimstyle.dimsoxd { 1 } else { 0 })? }
        put! { 176, dimclrd, self.writer.write_i16(176, dimstyle.dimclrd)? }
        put! { 177, dimclre, self.writer.write_i16(177, dimstyle.dimclre)? }
        put! { 178, dimclrt, self.writer.write_i16(178, dimstyle.dimclrt)? }
        put! { 179, dimadec, self.writer.write_i16(179, dimstyle.dimadec)? }

        // Int16 (270-290)
        if self.dxf_version <= DxfVersion::AC1014 && !self.r12 {
            self.writer.write_i16(270, dimstyle.dimunit)?;
        }
        put! { 271, dimdec, self.writer.write_i16(271, dimstyle.dimdec)? }
        put! { 272, dimtdec, self.writer.write_i16(272, dimstyle.dimtdec)? }
        put! { 273, dimaltu, self.writer.write_i16(273, dimstyle.dimaltu)? }
        put! { 274, dimalttd, self.writer.write_i16(274, dimstyle.dimalttd)? }
        put! { 275, dimaunit, self.writer.write_i16(275, dimstyle.dimaunit)? }
        put! { 276, dimfrac, self.writer.write_i16(276, dimstyle.dimfrac)? }
        put! { 277, dimlunit, self.writer.write_i16(277, dimstyle.dimlunit)? }
        put! { 278, dimdsep, self.writer.write_i16(278, dimstyle.dimdsep)? }
        put! { 279, dimtmove, self.writer.write_i16(279, dimstyle.dimtmove)? }
        put! { 280, dimjust, self.writer.write_i16(280, dimstyle.dimjust)? }
        put! { 281, dimsd1, self.writer
            .write_i16(281, if dimstyle.dimsd1 { 1 } else { 0 })? }
        put! { 282, dimsd2, self.writer
            .write_i16(282, if dimstyle.dimsd2 { 1 } else { 0 })? }
        put! { 283, dimtolj, self.writer.write_i16(283, dimstyle.dimtolj)? }
        put! { 284, dimtzin, self.writer.write_i16(284, dimstyle.dimtzin)? }
        put! { 285, dimaltz, self.writer.write_i16(285, dimstyle.dimaltz)? }
        put! { 286, dimalttz, self.writer.write_i16(286, dimstyle.dimalttz)? }
        put! { 288, dimupt, self.writer
            .write_i16(288, if dimstyle.dimupt { 1 } else { 0 })? }
        if self.dxf_version <= DxfVersion::AC1014 {
            put! { 287, dimfit, self.writer.write_i16(287, dimstyle.dimfit)? }
        } else {
            put! { 289, dimatfit, self.writer.write_i16(289, dimstyle.dimatfit)? }
        }
        if self.dxf_version >= DxfVersion::AC1021 && dimstyle.dimfxlon {
            self.writer.write_bool(290, true)?;
        }
        if self.dxf_version >= DxfVersion::AC1024 && dimstyle.dimtxtdirection {
            self.writer.write_bool(295, true)?;
        }

        // Handle references
        if !dimstyle.dimtxsty_handle.is_null() {
            self.writer.write_handle(340, dimstyle.dimtxsty_handle)?;
        }
        if self.dxf_version >= DxfVersion::AC1015 {
            if !dimstyle.dimldrblk.is_null() {
                self.writer.write_handle(341, dimstyle.dimldrblk)?;
            }
            if !dimstyle.dimblk.is_null() {
                self.writer.write_handle(342, dimstyle.dimblk)?;
            }
            if !dimstyle.dimblk1.is_null() {
                self.writer.write_handle(343, dimstyle.dimblk1)?;
            }
            if !dimstyle.dimblk2.is_null() {
                self.writer.write_handle(344, dimstyle.dimblk2)?;
            }
        }
        if self.dxf_version >= DxfVersion::AC1021 {
            if !dimstyle.dimltex_handle.is_null() {
                self.writer.write_handle(345, dimstyle.dimltex_handle)?;
            }
            if !dimstyle.dimltex1_handle.is_null() {
                self.writer.write_handle(346, dimstyle.dimltex1_handle)?;
            }
            if !dimstyle.dimltex2_handle.is_null() {
                self.writer.write_handle(347, dimstyle.dimltex2_handle)?;
            }
        }

        // Line weights
        if self.dxf_version >= DxfVersion::AC1015 {
            self.writer.write_i16(371, dimstyle.dimlwd)?;
            self.writer.write_i16(372, dimstyle.dimlwe)?;
        }
        self.write_table_annotative_xdata(document, dimstyle.handle, dimstyle.annotative)?;

        Ok(())
    }

    /// Write BLOCK_RECORD table
    fn write_block_record_table(&mut self, document: &CadDocument) -> Result<()> {
        let table_handle = document.block_records.handle();
        self.write_table_header(
            "BLOCK_RECORD",
            document.block_records.len(),
            table_handle,
            document,
        )?;

        for block_record in document.block_records.iter() {
            self.write_block_record_entry(block_record, table_handle, document)?;
        }

        self.write_table_end()?;
        Ok(())
    }

    /// Whether a drawing before R2000 holds the description or the preview image of a block record in the record `application` of
    /// its extended data (`autocad_conversions`), which then stands for the group the DXF of R2000 and later has.
    fn states_block_content_in_record(&self, document: &CadDocument, block: Handle, application: &str) -> bool {
        self.dxf_version < DxfVersion::AC1015
            && document.object_xdata.get(&block).is_some_and(|data| data.get_record(application).is_some())
    }

    fn write_block_record_entry(
        &mut self,
        block_record: &BlockRecord,
        owner: Handle,
        document: &CadDocument,
    ) -> Result<()> {
        self.writer.write_string(0, "BLOCK_RECORD")?;
        self.write_common_table_data(block_record.handle(), owner, document)?;
        self.writer.write_subclass("AcDbSymbolTableRecord")?;
        self.writer.write_subclass("AcDbBlockTableRecord")?;
        self.writer.write_string(2, block_record.name())?;
        self.writer.write_handle(340, block_record.layout)?;
        let references: Vec<Handle> = block_record
            .insert_handles
            .iter()
            .copied()
            .filter(|handle| self.valid_handles.is_empty() || self.valid_handles.contains(handle))
            .collect();
        if !references.is_empty() {
            self.writer.write_string(102, "{BLKREFS")?;
            for handle in references {
                self.writer.write_handle(331, handle)?;
            }
            self.writer.write_string(102, "}")?;
        }
        // Before R2000 the image is the record `CONTENTBLOCKICON` of the block record's extended data, where the drawing states one.
        if !self.states_block_content_in_record(document, block_record.handle, "CONTENTBLOCKICON") {
            for chunk in block_record.preview_data.chunks(127) {
                self.writer.write_binary(310, chunk)?;
            }
        }
        if self.dxf_version >= DxfVersion::AC1021 {
            // A record read from DXF writes only the groups its source had (or whose value changed): an
            // absent group stands for the reader's own default, not for the native one.
            let present = block_record.dxf_present;
            let keep = |bit: u8, default_value: bool| {
                present.map_or(true, |present| present & bit != 0 || !default_value)
            };
            if keep(32, block_record.units == 0) {
                self.writer.write_i16(70, block_record.units)?;
            }
            if keep(1, block_record.explodable) {
                self.writer
                    .write_byte(280, if block_record.explodable { 1 } else { 0 })?;
            }
            if keep(2, !block_record.scale_uniformly) {
                self.writer
                    .write_i16(281, if block_record.scale_uniformly { 1 } else { 0 })?;
            }
        }
        let mut generated = Vec::new();
        if self.dxf_version < DxfVersion::AC1021 && (block_record.units != 0 || document.object_xdata.get(&block_record.handle).and_then(|data| data.get_record("ACAD")).is_some_and(|record| record.values.iter().any(|value| matches!(value, XDataValue::String(value) if value == "DesignCenter Data")))) {
            // Older files keep the units in ACAD `DesignCenter Data` xdata.
            let mut units = crate::xdata::ExtendedDataRecord::new("ACAD");
            units.values = crate::tables::block_record::design_center_units_values(block_record.units);
            generated.push(units);
        }
        self.write_table_xdata(document, block_record.handle, &generated)?;

        Ok(())
    }

    /// Write table header
    fn write_table_header(
        &mut self,
        name: &str,
        count: usize,
        table_handle: Handle,
        document: &CadDocument,
    ) -> Result<()> {
        self.writer.write_string(0, "TABLE")?;
        self.writer.write_string(2, name)?;
        if self.r12 {
            // An R12 table header has its name and the entry count only.
            self.writer.write_i16(70, count as i16)?;
            return Ok(());
        }
        self.writer.write_handle(5, table_handle)?;
        if let Some(handle) = document.extension_dictionary_handle(table_handle) {
            self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
            self.writer.write_handle(360, handle)?;
            self.writer.write_string(102, "}")?;
        }
        if let Some(reactors) = document.reactors_by_handle.get(&table_handle) {
            if !reactors.is_empty() {
                self.writer.write_string(102, "{ACAD_REACTORS")?;
                for reactor in reactors {
                    self.writer.write_handle(330, *reactor)?;
                }
                self.writer.write_string(102, "}")?;
            }
        }
        self.writer.write_handle(330, Handle::new(0))?; // Tables owned by document root (handle 0)
        self.writer.write_subclass("AcDbSymbolTable")?;
        self.writer.write_i16(70, count as i16)?;
        self.write_table_xdata(document, table_handle, &[])?;
        Ok(())
    }

    /// Write table end
    fn write_table_end(&mut self) -> Result<()> {
        self.writer.write_string(0, "ENDTAB")
    }

    /// Write common table entry data
    fn write_table_entry_xdictionary(
        &mut self,
        handle: Handle,
        document: &CadDocument,
    ) -> Result<()> {
        if let Some(xdictionary) = document.extension_dictionary_handle(handle) {
            if !xdictionary.is_null()
                && (self.valid_handles.is_empty() || self.valid_handles.contains(&xdictionary))
            {
                self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
                self.writer.write_handle(360, xdictionary)?;
                self.writer.write_string(102, "}")?;
            }
        }
        Ok(())
    }

    /// Write common table entry data
    fn write_common_table_data(
        &mut self,
        handle: Handle,
        owner: Handle,
        document: &CadDocument,
    ) -> Result<()> {
        if self.r12 {
            // R12 table records are not handled objects.
            return Ok(());
        }
        self.writer.write_handle(5, handle)?;
        self.write_table_entry_reactors(handle, document)?;
        self.write_table_entry_xdictionary(handle, document)?;
        self.writer.write_handle(330, owner)?;
        Ok(())
    }

    /// Reactors kept from the source (an associative array's dependencies on its anonymous blocks, the
    /// dimensions using a dimension style, ...). They precede the extension dictionary, as on every other object.
    fn write_table_entry_reactors(&mut self, handle: Handle, document: &CadDocument) -> Result<()> {
        let reactors: Vec<Handle> = document
            .reactors_by_handle
            .get(&handle)
            .into_iter()
            .flatten()
            .copied()
            .filter(|reactor| {
                !reactor.is_null()
                    && (self.valid_handles.is_empty() || self.valid_handles.contains(reactor))
            })
            .collect();
        if !reactors.is_empty() {
            self.writer.write_string(102, "{ACAD_REACTORS")?;
            for reactor in reactors {
                self.writer.write_handle(330, reactor)?;
            }
            self.writer.write_string(102, "}")?;
        }
        Ok(())
    }

    /// Write the BLOCKS section
    pub fn write_blocks(&mut self, document: &CadDocument) -> Result<()> {
        self.writer.write_section_start("BLOCKS")?;

        for block_record in document.block_records.iter() {
            self.write_block_definition(block_record, document)?;
        }

        self.writer.write_section_end()?;
        Ok(())
    }

    fn number_paper_space_viewports(&mut self, block: &BlockRecord, document: &CadDocument) {
        let viewports = block
            .entity_handles
            .iter()
            .filter_map(|handle| match document.get_entity(*handle) {
                Some(EntityType::Viewport(viewport)) => Some(viewport),
                _ => None,
            });
        let used_ids: HashSet<i16> = viewports
            .clone()
            .map(|viewport| viewport.id)
            .filter(|id| *id > 0)
            .collect();
        let mut available_ids = (1..=i16::MAX).filter(|id| !used_ids.contains(id));
        for viewport in viewports {
            if viewport.id == 0 {
                self.paper_viewport_ids
                    .insert(viewport.common.handle, available_ids.next().unwrap_or(0));
            }
        }
    }

    /// Write a complete block definition (BLOCK...entities...ENDBLK)
    /// Appearance, attachments and XDATA of a BLOCK (or ENDBLK) marker: the retained DXF state, else the
    /// marker entity of a DWG read, else the defaults.
    fn block_marker_common(
        &self,
        block_record: &BlockRecord,
        document: &CadDocument,
        end: bool,
    ) -> EntityCommon {
        let (retained, handle) = if end {
            (&block_record.block_end_common, block_record.block_end_handle)
        } else {
            (&block_record.block_entity_common, block_record.block_entity_handle)
        };
        let mut common = if let Some(common) = retained {
            (**common).clone()
        } else if let Some(entity) = document.get_entity(handle) {
            entity.common().clone()
        } else {
            let mut common = EntityCommon::new();
            common.layer = "0".to_string();
            common
        };
        common.handle = handle;
        common
    }

    /// The names R12 gives the two space blocks.
    fn r12_block_name(block_record: &BlockRecord) -> &str {
        if block_record.is_model_space() {
            "$MODEL_SPACE"
        } else if block_record.name().eq_ignore_ascii_case("*Paper_Space") {
            "$PAPER_SPACE"
        } else {
            block_record.name()
        }
    }

    fn write_block_definition(
        &mut self,
        block_record: &BlockRecord,
        document: &CadDocument,
    ) -> Result<()> {
        let owner = block_record.handle();
        // Block names are case-insensitive: drawings written by other tools
        // use "*MODEL_SPACE" / "*PAPER_SPACE".
        let is_paper_space = block_record.is_paper_space();
        if is_paper_space {
            self.number_paper_space_viewports(block_record, document);
        }

        // Determine block flags from stored BlockFlags
        let mut flags: i16 = 0;
        // Anonymous flag: use stored flag, or infer for truly anonymous blocks
        // (e.g. *D1, *U2, *T3, *X4, *E5, *A6) but NOT system blocks
        // (*Model_Space, *Paper_Space*)
        let name = block_record.name();
        let is_anonymous_block = block_record.flags.anonymous
            || (name.starts_with('*')
                && !block_record.is_model_space()
                && !block_record.is_paper_space());
        if is_anonymous_block {
            flags |= 1; // anonymous
        }
        if block_record.flags.has_attributes {
            flags |= 2; // has attribute definitions
        }
        if block_record.flags.is_xref {
            flags |= 4; // xref
        }
        if block_record.flags.is_xref_overlay {
            flags |= 8; // xref overlay
        }
        if block_record.flags.is_external {
            flags |= 16; // externally dependent
        }
        flags |= block_record.source_flags & !31;
        // A block keeps its anonymous bit apart from the typed flag: native hydration renames the anonymous
        // table name of a dynamic definition (DWG) and the DXF reader takes a public name for not anonymous.
        // The marker written for it must still say what its source said.
        flags |= block_record.source_flags & 1;

        // Write BLOCK entity: its own appearance, attachments and XDATA when the source had them.
        let mut begin_common = self.block_marker_common(block_record, document, false);
        if self.r12 {
            // The BLOCK record of an R12 file has no handle (its ENDBLK has one).
            begin_common.handle = Handle::NULL;
        }
        let block_name = if self.r12 {
            Self::r12_block_name(block_record)
        } else {
            block_record.name()
        };
        self.writer.write_string(0, "BLOCK")?;
        let prev_ps = self.writing_paper_space;
        self.writing_paper_space = is_paper_space;
        self.write_common_entity_data(&begin_common, owner)?;
        self.writing_paper_space = prev_ps;
        self.writer.write_subclass("AcDbBlockBegin")?;
        self.writer.write_string(2, block_name)?;
        self.writer.write_i16(70, flags)?;
        self.writer.write_double(10, block_record.base_point.x)?;
        self.writer.write_double(20, block_record.base_point.y)?;
        self.writer.write_double(30, block_record.base_point.z)?;
        self.writer.write_string(3, block_name)?;
        // Group code 1 is XRef path (empty for normal blocks)
        self.writer.write_string(1, &block_record.xref_path)?;
        // Group code 4 is the block description, written when it has text or the source had the field (every
        // R2000+ DWG block record carries one, empty or not).
        let description_present = block_record.dxf_present.is_some_and(|present| present & 8 != 0)
            || document
                .dwg_source_version
                .is_some_and(|version| version >= DxfVersion::AC1015);
        // Before R2000 the description is the record `CONTENTBLOCKDESCRIPTION` of the block record's extended data, where the
        // drawing states one.
        if (!block_record.description.is_empty() || description_present)
            && !self.r12
            && !self.states_block_content_in_record(document, block_record.handle, "CONTENTBLOCKDESCRIPTION")
        {
            self.writer.write_string(4, &block_record.description)?;
        }
        self.write_xdata(&begin_common.extended_data)?;

        // Write entities inside block definition:
        // - Model space entities go to ENTITIES section (not here)
        // - Active paper space (*Paper_Space) entities go to ENTITIES section (not here)
        // - Non-active paper spaces (*Paper_Space0, *Paper_Space1, ...) write entities here
        // - Other blocks (inserts etc.) also write entities here
        // - Xref blocks (see below) write no owned entities at all
        //
        // An xref block record references an external drawing: its contents
        // live in that file, not here. Some hosts merge the resolved xref
        // geometry into the block record for display; serializing those as
        // owned entities would bind/explode the xref into the host file on
        // the next open. Skip the entity loop entirely, leaving only the
        // BLOCK/ENDBLK markers — matching the DWG writer (see
        // `DwgObjectWriter::write_block_entities`).
        let is_xref = block_record.flags.is_xref || block_record.flags.is_xref_overlay;
        if !is_xref
            && !block_record.is_model_space()
            && !block_record.name().eq_ignore_ascii_case("*Paper_Space")
        {
            // Set paper space flag so entities inside non-active paper
            // space blocks get code 67=1 (same as active paper space).
            let prev_ps = self.writing_paper_space;
            if is_paper_space {
                self.writing_paper_space = true;
            }
            for eh in &block_record.entity_handles {
                if let Some(&idx) = document.entity_index.get(eh) {
                    self.write_owned_entity(&document.entities[idx], owner, document)?;
                }
            }
            self.writing_paper_space = prev_ps;
        }

        // Write ENDBLK entity
        let mut end_common = self.block_marker_common(block_record, document, true);
        if self.r12 {
            if let Some(handle) = self.r12_end_handles.get(&block_record.name().to_ascii_uppercase()) {
                end_common.handle = *handle;
            } else if end_common.handle.value() > self.r12_handle_limit {
                // The marker of a block the source did not have (the space blocks of an R12 file without them):
                // the reader allocated its handle and allocates it again.
                end_common.handle = Handle::NULL;
            }
        }
        self.writer.write_string(0, "ENDBLK")?;
        self.writing_paper_space = is_paper_space;
        self.write_common_entity_data(&end_common, owner)?;
        self.writing_paper_space = prev_ps;
        self.writer.write_subclass("AcDbBlockEnd")?;
        self.write_xdata(&end_common.extended_data)?;

        Ok(())
    }

    /// Write the ENTITIES section
    pub fn write_entities(&mut self, document: &CadDocument) -> Result<()> {
        self.writer.write_section_start("ENTITIES")?;

        // Write entities from model space block record
        self.writing_paper_space = false;
        if let Some(model_space) = document.block_records.get("*Model_Space") {
            let owner = model_space.handle();
            for eh in &model_space.entity_handles {
                if let Some(&idx) = document.entity_index.get(eh) {
                    self.write_owned_entity(&document.entities[idx], owner, document)?;
                }
            }
        }

        // Write entities from the active paper space (*Paper_Space) only.
        // Non-active paper spaces (*Paper_Space0, *Paper_Space1, ...) have their
        // entities written inside their BLOCK definitions in the BLOCKS section.
        self.writing_paper_space = true;
        if let Some(paper_space) = document.block_records.get("*Paper_Space") {
            let owner = paper_space.handle();
            self.number_paper_space_viewports(paper_space, document);
            for eh in &paper_space.entity_handles {
                if let Some(&idx) = document.entity_index.get(eh) {
                    self.write_owned_entity(&document.entities[idx], owner, document)?;
                }
            }
        }
        self.writing_paper_space = false;

        self.writer.write_section_end()?;
        Ok(())
    }

    /// Write an entity of a block or a space. An R12 file states an entity R12 does not have in its R12 form (the
    /// conversion rules of `writer/r12.rs`: R12 entities without handles, or nothing for what has no R12 form); the
    /// rest of the document keeps its R12 records, handles and extended data.
    fn write_owned_entity(&mut self, entity: &EntityType, owner: Handle, document: &CadDocument) -> Result<()> {
        if self.r12 && !super::r12::is_r12_entity(entity) {
            let paper = self.writing_paper_space;
            return super::r12::write_r12_entity(&mut *self.writer, document, entity, paper, &mut self.r12_names);
        }
        self.write_entity_with_owner(entity, owner)
    }

    /// Write an entity with explicit owner, followed by its XDATA.
    ///
    /// XDATA must sit at the end of the entity's own group codes, before any
    /// child records (VERTEX / ATTRIB / SEQEND). Writers that emit such inline
    /// children — the polylines, INSERT-with-attributes, the mesh forms — and
    /// ATTRIB (emitted inline by INSERT) write their own XDATA at the right
    /// spot; every other entity gets it here. `Unknown` entities re-emit their
    /// captured bytes verbatim, so they are skipped.
    fn write_entity_with_owner(&mut self, entity: &EntityType, owner: Handle) -> Result<()> {
        self.write_entity_body(entity, owner)?;
        if !matches!(
            entity,
            EntityType::Polyline(_)
                | EntityType::Polyline2D(_)
                | EntityType::Polyline3D(_)
                | EntityType::Insert(_)
                | EntityType::PolyfaceMesh(_)
                | EntityType::PolygonMesh(_)
                | EntityType::AttributeEntity(_)
                | EntityType::Unknown(_)
        ) {
            let lofted = match entity {
                EntityType::Surface(surface) => Some(&surface.surface_data),
                _ => None,
            };
            if let Some(SurfaceData::Lofted {
                cross_sections,
                guide_curves,
                path_curve,
                ..
            }) = lofted
            {
                let mut data = entity.common().extended_data.clone();
                data.remove_record("CADCODEC_LOFT_REFERENCES");
                if !cross_sections.is_empty() || !guide_curves.is_empty() || path_curve.is_some() {
                    // Database references are separate from native embedded
                    // curve bodies. Keep their roles in registered XDATA,
                    // using real handle values so insert/clone remapping works.
                    let mut record = ExtendedDataRecord::new("CADCODEC_LOFT_REFERENCES");
                    record.values = vec![
                        XDataValue::Integer16(1),
                        XDataValue::Integer32(cross_sections.len() as i32),
                        XDataValue::Integer32(guide_curves.len() as i32),
                        XDataValue::Integer32(i32::from(path_curve.is_some())),
                    ];
                    record.values.extend(
                        cross_sections
                            .iter()
                            .chain(guide_curves)
                            .chain(path_curve.iter())
                            .copied()
                            .map(XDataValue::Handle),
                    );
                    data.add_record(record);
                }
                self.write_xdata(&data)?;
            } else if let Some(data) = self.r12_viewport_xdata.remove(&entity.common().handle) {
                self.write_xdata(&data)?;
            } else {
                self.write_xdata(&entity.common().extended_data)?;
            }
        }
        Ok(())
    }

    /// Dispatch to the per-type entity writer (no XDATA — see
    /// [`write_entity_with_owner`](Self::write_entity_with_owner)).
    fn write_entity_body(&mut self, entity: &EntityType, owner: Handle) -> Result<()> {
        match entity {
            EntityType::Point(e) => self.write_point(e, owner),
            EntityType::Line(e) => self.write_line(e, owner),
            EntityType::Circle(e) => self.write_circle(e, owner),
            EntityType::Arc(e) => self.write_arc(e, owner),
            EntityType::Ellipse(e) => self.write_ellipse(e, owner),
            EntityType::Polyline(e) => self.write_polyline(e, owner),
            EntityType::Polyline2D(e) => self.write_polyline2d(e, owner),
            EntityType::Polyline3D(e) => self.write_polyline3d(e, owner),
            EntityType::LwPolyline(e) => self.write_lwpolyline(e, owner),
            EntityType::Text(e) => self.write_text(e, owner),
            EntityType::MText(e) => self.write_mtext(e, owner),
            EntityType::Spline(e) => self.write_spline(e, owner),
            EntityType::Helix(e) => self.write_helix(e, owner),
            EntityType::Dimension(dim) => self.write_dimension(dim, owner),
            EntityType::Hatch(e) => self.write_hatch(e, owner),
            EntityType::Solid(e) => self.write_solid(e, owner),
            EntityType::Face3D(e) => self.write_face3d(e, owner),
            EntityType::Insert(e) => self.write_insert(e, owner),
            EntityType::Block(e) => self.write_block_entity(e, owner),
            EntityType::BlockEnd(e) => self.write_block_end(e, owner),
            EntityType::Ray(e) => self.write_ray(e, owner),
            EntityType::XLine(e) => self.write_xline(e, owner),
            EntityType::Viewport(e) => self.write_viewport(e, owner),
            EntityType::AttributeDefinition(e) => self.write_attdef(e, owner),
            EntityType::AttributeEntity(e) => self.write_attrib(e, owner),
            EntityType::Leader(e) => self.write_leader(e, owner),
            EntityType::MultiLeader(e) => self.write_multileader(e, owner),
            EntityType::MLine(e) => self.write_mline(e, owner),
            EntityType::Mesh(e) => self.write_mesh(e, owner),
            EntityType::RasterImage(e) => self.write_raster_image(e, owner),
            EntityType::Solid3D(e) => self.write_solid3d(e, owner),
            EntityType::Region(e) => self.write_region(e, owner),
            EntityType::Body(e) => self.write_body(e, owner),
            EntityType::Surface(e) => self.write_surface(e, owner),
            EntityType::Light(e) => self.write_light(e, owner),
            EntityType::SectionSymbol(e) => self.write_section_symbol_dxf(e, owner),
            EntityType::ViewBorder(e) => self.write_view_border_dxf(e, owner),
            EntityType::Table(e) => self.write_acad_table(e, owner),
            EntityType::Tolerance(e) => self.write_tolerance(e, owner),
            EntityType::PolyfaceMesh(e) => self.write_polyface_mesh(e, owner),
            EntityType::Wipeout(e) => self.write_wipeout(e, owner),
            EntityType::Shape(e) => self.write_shape(e, owner),
            EntityType::Underlay(e) => self.write_underlay(e, owner),
            EntityType::Seqend(e) => self.write_seqend(e, owner),
            EntityType::Ole2Frame(e) => self.write_ole2frame(e, owner),
            EntityType::PolygonMesh(e) => self.write_polygon_mesh(e, owner),
            EntityType::Extended(e) => self.write_extended_entity(e, owner),
            EntityType::Unknown(e) => self.write_unknown_entity(e, owner),
        }
    }

    fn write_extended_entity(&mut self, e: &ExtendedEntity, owner: Handle) -> Result<()> {
        if let ExtendedEntityData::Format(value) = &e.data {
            if let Some(raw) = &value.raw_dxf_codes {
                self.writer.write_entity_type(e.class_name())?;
                for (code, item) in raw {
                    self.writer.write_string(*code, item)?;
                }
                return Ok(());
            }
        }
        if let ExtendedEntityData::RegisteredClass(data) = &e.data {
            if data.payload.bit_count != 0 {
                self.writer.write_entity_type("ACAD_PROXY_ENTITY")?;
                // Proxy graphics belong to AcDbProxyEntity, not AcDbEntity.
                let mut common = e.common.clone();
                let graphics = common.graphic_data.take().unwrap_or_default();
                self.write_common_entity_data(&common, owner)?;
                self.writer.write_subclass("AcDbProxyEntity")?;
                let payload = crate::objects::semantic_property::encode_registered_class_envelope(
                    &data.dxf_name,
                    &data.cpp_class_name,
                    &data.properties,
                    &data.payload,
                );
                self.write_proxy_body_dxf(
                    498,
                    499,
                    0,
                    0,
                    false,
                    Some(&graphics),
                    &payload,
                    &crate::objects::ProxyPayload::default(),
                    &data.object_ids,
                )?;
                return Ok(());
            }
        }
        self.writer.write_entity_type(e.class_name())?;
        if let ExtendedEntityData::Proxy(data) = &e.data {
            // The proxy graphics are written once, inside AcDbProxyEntity.
            // `common.graphic_data` holds the same bytes read from the DWG
            // entity preamble; writing them in AcDbEntity as well makes
            // AutoCAD reject the entity.
            let mut common = e.common.clone();
            let preamble_graphics = common.graphic_data.take().unwrap_or_default();
            self.write_common_entity_data(&common, owner)?;
            self.writer.write_subclass("AcDbProxyEntity")?;
            let graphics = data.graphics.data();
            let graphics = if graphics.is_empty() { preamble_graphics } else { graphics };
            return self.write_proxy_body_dxf(
                498,
                data.class_id,
                data.dwg_version,
                data.maintenance_version,
                data.from_dxf,
                Some(&graphics),
                &data.payload,
                &data.text_payload,
                &data.object_ids,
            );
        }
        self.write_common_entity_data(&e.common, owner)?;
        match &e.data {
            ExtendedEntityData::Camera { view_handle } => {
                self.writer.write_subclass("AcDbCamera")?;
                self.writer.write_handle(340, *view_handle)?;
            }
            ExtendedEntityData::SectionObject(data) => {
                self.writer.write_subclass("AcDbSection")?;
                self.writer.write_i32(90, data.state)?;
                self.writer.write_i32(91, data.flags)?;
                self.writer.write_string(1, &data.name)?;
                self.writer.write_point3d(10, data.vertical_direction)?;
                self.writer.write_double(40, data.top_height)?;
                self.writer.write_double(41, data.bottom_height)?;
                self.writer.write_i16(70, data.indicator_alpha)?;
                self.writer.write_color(62, data.indicator_color)?;
                self.writer.write_i32(92, data.vertices.len() as i32)?;
                for point in &data.vertices {
                    self.writer.write_point3d(11, *point)?;
                }
                self.writer
                    .write_i32(93, data.back_line_vertices.len() as i32)?;
                for point in &data.back_line_vertices {
                    self.writer.write_point3d(12, *point)?;
                }
                self.writer.write_handle(360, data.settings_handle)?;
            }
            ExtendedEntityData::ArcAlignedText(data) => {
                self.writer.write_subclass("AcDbArcAlignedText")?;
                self.writer.write_string(1, &data.text)?;
                self.writer.write_string(2, &data.font_name)?;
                self.writer.write_string(3, &data.big_font_name)?;
                self.writer.write_string(7, &data.style_name)?;
                self.writer.write_point3d(10, data.center)?;
                self.writer.write_double(40, data.radius)?;
                self.writer.write_double(41, data.x_scale)?;
                self.writer.write_double(42, data.text_size)?;
                self.writer.write_double(43, data.character_spacing)?;
                self.writer.write_double(44, data.offset_from_arc)?;
                self.writer.write_double(45, data.right_offset)?;
                self.writer.write_double(46, data.left_offset)?;
                self.writer
                    .write_double(50, data.start_angle.to_dxf_degrees())?;
                self.writer.write_double(51, data.end_angle.to_dxf_degrees())?;
                self.writer.write_i16(70, data.reverse as i16)?;
                self.writer.write_i16(71, data.text_direction)?;
                self.writer.write_i16(72, data.alignment)?;
                self.writer.write_i16(73, data.text_position)?;
                self.writer.write_i16(74, data.bold as i16)?;
                self.writer.write_i16(75, data.italic as i16)?;
                self.writer.write_i16(76, data.underlined as i16)?;
                self.writer.write_i16(77, data.character_set)?;
                self.writer.write_i16(78, data.pitch_and_family)?;
                self.writer.write_i16(79, data.is_shx as i16)?;
                self.writer.write_i32(90, data.text_color)?;
                self.writer.write_point3d(210, data.normal)?;
                self.writer.write_bool(280, data.wizard_flag)?;
                self.writer.write_handle(330, data.arc_handle)?;
            }
            ExtendedEntityData::RemoteText(data) => {
                // Express Tools writes its subclass marker as "RText"; AutoCAD
                // rejects the entity otherwise ("Class separator for class
                // RText expected").
                self.writer.write_subclass("RText")?;
                self.writer.write_point3d(10, data.position)?;
                self.writer.write_point3d(210, data.normal)?;
                self.writer.write_double(50, data.rotation)?;
                self.writer.write_double(40, data.height)?;
                self.writer.write_string(7, &data.style_name)?;
                self.writer.write_i16(70, data.flags)?;
                self.writer.write_string(1, &data.text)?;
            }
            ExtendedEntityData::GeoPositionMarker(data) => {
                self.writer.write_subclass("AcDbGeoPositionMarker")?;
                self.writer.write_i32(90, data.class_version)?;
                self.writer.write_point3d(10, data.position)?;
                self.writer.write_double(40, data.radius)?;
                self.writer.write_string(1, &data.notes)?;
                self.writer.write_double(40, data.landing_gap)?;
                self.writer.write_bool(290, data.mtext_visible)?;
                self.writer.write_byte(280, data.text_alignment)?;
                self.writer.write_bool(290, data.enable_frame_text)?;
                if let Some(mtext) = &data.embedded_mtext {
                    self.writer.write_subclass("AcDbMTextObjectEmbedded")?;
                    self.writer.write_point3d(10, mtext.insertion_point)?;
                    self.writer.write_point3d(210, mtext.normal)?;
                    self.writer.write_double(40, mtext.height)?;
                    self.writer.write_double(41, mtext.rectangle_width)?;
                    self.writer.write_string(1, &mtext.value)?;
                    self.writer.write_string(7, &mtext.style)?;
                }
            }
            ExtendedEntityData::CoordinationModel(data) => {
                self.writer.write_subclass("AcDbNavisworksModel")?;
                self.writer.write_i16(70, data.flags)?;
                self.writer.write_handle(340, data.definition_handle)?;
                for value in data.transform {
                    self.writer.write_double(40, value)?;
                }
                self.writer.write_double(40, data.unit_factor)?;
            }
            ExtendedEntityData::PointCloud(data) => {
                self.write_point_cloud_dxf(data)?;
            }
            ExtendedEntityData::PointCloudEx(data) => {
                self.write_point_cloud_ex_dxf(data)?;
            }
            ExtendedEntityData::Proxy(_) => unreachable!("written by the early return above"),
            ExtendedEntityData::OleFrame(data) => {
                self.writer.write_subclass("AcDbOleFrame")?;
                self.writer.write_i16(70, data.flag)?;
                self.writer.write_i16(71, data.mode)?;
                let bytes = data.storage.encode();
                self.writer.write_i32(90, bytes.len() as i32)?;
                for chunk in bytes.chunks(127) {
                    self.writer.write_binary(310, chunk)?;
                }
            }
            ExtendedEntityData::LayoutPrintConfig(data) => {
                self.writer.write_subclass("CAcLayoutPrintConfig")?;
                self.writer.write_i16(93, data.flag)?;
            }
            ExtendedEntityData::Format(_) => {
                self.writer.write_subclass("mcsDbObjectFormat")?;
            }
            ExtendedEntityData::Legacy(LegacyEntityData::Repeat) => {}
            ExtendedEntityData::Legacy(LegacyEntityData::EndRepeat {
                columns,
                rows,
                column_spacing,
                row_spacing,
            }) => {
                self.writer.write_i16(70, *columns)?;
                self.writer.write_i16(71, *rows)?;
                self.writer.write_double(40, *column_spacing)?;
                self.writer.write_double(41, *row_spacing)?;
            }
            ExtendedEntityData::Legacy(LegacyEntityData::Load { filename }) => {
                self.writer.write_string(1, filename)?;
            }
            ExtendedEntityData::Legacy(LegacyEntityData::Jump { address }) => {
                self.writer.write_i32(90, *address as i32)?;
            }
            ExtendedEntityData::DynamicBlock(data) => {
                if let crate::objects::DynamicBlockData::AngularConstraintParameterEntity(value) =
                    data
                {
                    self.write_dynamic_constraint_dxf(&value.constraint)?;
                    self.writer
                        .write_subclass("AcDbBlockAngularConstraintParameterEntity")?;
                    self.writer.write_point3d(1011, value.center_point)?;
                    self.writer.write_point3d(1012, value.label_point)?;
                    self.writer.write_string(305, &value.expression_name)?;
                    self.writer
                        .write_string(306, &value.expression_description)?;
                    self.writer.write_double(140, value.angle)?;
                    self.writer
                        .write_bool(280, value.orientation_on_both_grips)?;
                    self.write_dynamic_value_set_dxf(&value.value_set, 96, 128, 175, 307)?;
                } else if let Some(cpp_name) = data.entity_cpp_name() {
                    self.writer.write_subclass(cpp_name)?;
                }
            }
            ExtendedEntityData::RegisteredClass(data) => {
                self.write_semantic_properties(&data.properties)?;
                self.write_proxy_references_dxf(&data.object_ids)?;
            }
        }
        Ok(())
    }

    fn write_section_symbol_dxf(&mut self, entity: &SectionSymbol, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("SECTIONLINE")?;
        self.write_common_entity_data(&entity.common, owner)?;
        self.writer.write_subclass("AcDbViewSymbol")?;
        self.writer.write_i16(70, entity.view_symbol_version)?;
        self.writer.write_handle(340, entity.style_handle)?;
        self.writer.write_double(40, entity.symbol_scale)?;
        self.writer.write_handle(330, entity.view_rep_handle)?;
        self.writer.write_i16(70, entity.raw_view_symbol_70)?;
        self.writer.write_subclass("AcDbSectionSymbol")?;
        self.writer.write_i16(70, entity.version)?;
        self.writer.write_i32(90, entity.raw_point_count_90)?;
        self.writer.write_i32(90, entity.raw_flags_90)?;
        self.writer.write_i32(90, entity.raw_point_record_count)?;
        for point in &entity.points {
            self.writer.write_double(10, point.point.x)?;
            self.writer.write_double(20, point.point.y)?;
            self.writer.write_double(30, point.point.z)?;
            self.writer.write_double(40, point.bulge)?;
            self.writer.write_string(1, &point.label)?;
            self.writer.write_double(11, point.label_offset.x)?;
            self.writer.write_double(21, point.label_offset.y)?;
            self.writer.write_double(31, point.label_offset.z)?;
            self.writer.write_i16(280, point.raw_flag_280 as i16)?;
        }
        Ok(())
    }

    fn write_view_border_dxf(&mut self, entity: &ViewBorder, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("DRAWINGVIEW")?;
        self.write_common_entity_data(&entity.common, owner)?;
        self.writer.write_subclass("AcDbViewBorder")?;
        self.writer.write_i16(70, entity.version)?;
        self.writer.write_double(10, entity.min[0])?;
        self.writer.write_double(20, entity.min[1])?;
        self.writer.write_double(10, entity.max[0])?;
        self.writer.write_double(20, entity.max[1])?;
        self.writer.write_handle(330, entity.active_viewport)?;
        self.writer.write_double(40, entity.scale)?;
        self.writer.write_double(40, entity.rotation_angle)?;
        self.writer.write_double(40, entity.center[0])?;
        self.writer.write_double(40, entity.center[1])?;
        self.writer.write_handle(340, entity.scale_handle)?;
        Ok(())
    }

    fn write_semantic_properties(
        &mut self,
        properties: &[crate::objects::SemanticProperty],
    ) -> Result<()> {
        use crate::objects::SemanticPropertyValue as Value;
        let mut subclass = "";
        for property in properties {
            if property.subclass != subclass {
                subclass = &property.subclass;
                if !subclass.is_empty() {
                    self.writer.write_subclass(subclass)?;
                }
            }
            match &property.value {
                Value::Text(value) => {
                    self.writer.write_string(property.code, value)?;
                }
                Value::Bool(value) => {
                    self.writer.write_bool(property.code, *value)?;
                }
                Value::Byte(value) => {
                    self.writer.write_byte(property.code, *value)?;
                }
                Value::Int16(value) => {
                    self.writer.write_i16(property.code, *value)?;
                }
                Value::Int32(value) => {
                    self.writer.write_i32(property.code, *value)?;
                }
                Value::Int64(value) => {
                    self.writer.write_i64(property.code, *value)?;
                }
                Value::Double(value) => {
                    self.writer.write_double(property.code, *value)?;
                }
                Value::Handle(value) => {
                    self.writer.write_handle(property.code, *value)?;
                }
                Value::Binary(value) => {
                    self.writer.write_binary(property.code, value)?;
                }
            }
        }
        Ok(())
    }

    fn write_dynamic_eval_dxf(
        &mut self,
        value: &crate::objects::BlockEvalExpression,
    ) -> Result<()> {
        self.writer.write_subclass("AcDbEvalExpr")?;
        self.writer.write_i32(90, value.node_id)?;
        self.writer.write_i32(98, value.major)?;
        self.writer.write_i32(99, value.minor)?;
        // -9999 = no value: AutoCAD writes nothing after 99 (a "70 -9999"
        // group makes it reject the object). Otherwise it writes an (empty)
        // expression string, the value type, then the value.
        if value.value_code == -9999 {
            return Ok(());
        }
        self.writer.write_string(1, "")?;
        self.writer.write_i16(70, value.value_code)?;
        match &value.value {
            BlockEvalValue::Real(item) => self.writer.write_double(140, *item)?,
            BlockEvalValue::Point(item) => {
                let code = if matches!(value.value_code, 10 | 11) {
                    value.value_code as i32
                } else {
                    10
                };
                self.writer.write_double(code, item[0])?;
                self.writer.write_double(code + 10, item[1])?;
            }
            BlockEvalValue::Text(item) => self.writer.write_string(1, item)?,
            BlockEvalValue::Long(item) => self.writer.write_i32(90, *item)?,
            BlockEvalValue::Handle(item) => self.writer.write_handle(91, *item)?,
            BlockEvalValue::Short(item) => self.writer.write_i16(70, *item)?,
            BlockEvalValue::None => {}
        }
        Ok(())
    }

    fn write_dynamic_element_dxf(&mut self, value: &crate::objects::BlockElement) -> Result<()> {
        self.write_dynamic_eval_dxf(&value.eval)?;
        self.writer.write_subclass("AcDbBlockElement")?;
        self.writer.write_string(300, &value.name)?;
        self.writer.write_i32(98, value.major)?;
        self.writer.write_i32(99, value.minor)?;
        self.writer.write_i32(1071, value.eed_1071)?;
        Ok(())
    }

    fn write_dynamic_parameter_dxf(
        &mut self,
        value: &crate::objects::BlockParameter,
    ) -> Result<()> {
        self.write_dynamic_element_dxf(&value.element)?;
        self.writer.write_subclass("AcDbBlockParameter")?;
        self.writer.write_bool(280, value.show_properties)?;
        self.writer.write_bool(281, value.chain_actions)?;
        Ok(())
    }

    fn write_dynamic_property_dxf(
        &mut self,
        value: &crate::objects::BlockParameterProperty,
        count_code: i32,
        value_code: i32,
        name_code: i32,
    ) -> Result<()> {
        self.writer
            .write_i32(count_code, value.connections.len() as i32)?;
        for connection in &value.connections {
            self.writer.write_i32(value_code, connection.code)?;
            self.writer.write_string(name_code, &connection.name)?;
        }
        Ok(())
    }

    fn write_dynamic_two_point_dxf(
        &mut self,
        value: &crate::objects::BlockTwoPointParameter,
    ) -> Result<()> {
        self.write_dynamic_parameter_dxf(&value.parameter)?;
        self.writer.write_subclass("AcDbBlock2PtParameter")?;
        self.writer
            .write_point3d(1010, value.definition_base_point)?;
        self.writer
            .write_point3d(1011, value.definition_end_point)?;
        self.writer.write_i32(170, 4)?;
        for state in value.property_states {
            self.writer.write_i32(91, state)?;
        }
        for (index, property) in value.properties.iter().enumerate() {
            self.write_dynamic_property_dxf(
                property,
                171 + index as i32,
                92 + index as i32,
                301 + index as i32,
            )?;
        }
        self.writer.write_i16(177, value.parameter_base_location)?;
        Ok(())
    }

    fn write_dynamic_constraint_dxf(
        &mut self,
        value: &crate::objects::BlockConstraintParameter,
    ) -> Result<()> {
        self.write_dynamic_two_point_dxf(&value.parameter)?;
        self.writer.write_subclass("AcDbBlockConstraintParameter")?;
        self.writer.write_handle(330, value.dependency)?;
        Ok(())
    }

    fn write_dynamic_value_set_dxf(
        &mut self,
        value: &crate::objects::BlockParameterValueSet,
        flags_code: i32,
        double_code: i32,
        count_code: i32,
        description_code: i32,
    ) -> Result<()> {
        self.writer
            .write_string(description_code, &value.description)?;
        self.writer.write_i32(flags_code, value.flags)?;
        self.writer.write_double(double_code, value.minimum)?;
        self.writer.write_double(double_code + 1, value.maximum)?;
        self.writer.write_double(double_code + 2, value.increment)?;
        self.writer
            .write_i16(count_code, value.values.len() as i16)?;
        for item in &value.values {
            self.writer.write_double(double_code + 3, *item)?;
        }
        Ok(())
    }

    fn write_dynamic_one_point_dxf(
        &mut self,
        value: &crate::objects::BlockOnePointParameter,
    ) -> Result<()> {
        self.write_dynamic_parameter_dxf(&value.parameter)?;
        self.writer.write_subclass("AcDbBlock1PtParameter")?;
        self.writer.write_point3d(1010, value.definition_point)?;
        self.writer.write_i32(93, value.property_count)?;
        for (index, property) in value.properties.iter().enumerate() {
            self.write_dynamic_property_dxf(
                property,
                170 + index as i32,
                91 + index as i32,
                301 + index as i32,
            )?;
        }
        Ok(())
    }

    fn write_dynamic_grip_dxf(&mut self, value: &crate::objects::BlockGrip) -> Result<()> {
        self.write_dynamic_element_dxf(&value.element)?;
        self.writer.write_subclass("AcDbBlockGrip")?;
        self.writer.write_i32(91, value.flags_91)?;
        self.writer.write_i32(92, value.flags_92)?;
        self.writer.write_point3d(1010, value.location)?;
        self.writer.write_bool(280, value.insert_cycling)?;
        self.writer.write_i32(93, value.insert_cycling_weight)?;
        Ok(())
    }

    fn write_dynamic_action_dxf(&mut self, value: &crate::objects::BlockAction) -> Result<()> {
        self.write_dynamic_element_dxf(&value.element)?;
        self.writer.write_subclass("AcDbBlockAction")?;
        self.writer
            .write_i32(70, value.action_ids.len() as i32)?;
        for id in &value.action_ids {
            self.writer.write_i32(91, *id)?;
        }
        self.writer.write_i32(71, value.dependencies.len() as i32)?;
        for handle in &value.dependencies {
            self.writer.write_handle(330, *handle)?;
        }
        self.writer.write_point3d(1010, value.display_location)?;
        Ok(())
    }

    fn write_dynamic_connections_dxf(
        &mut self,
        values: &[crate::objects::BlockConnection],
        code: i32,
        name_code: i32,
    ) -> Result<()> {
        // Each connection is a code and its name, one after the other (92, 301, 93, 302, ...): the reference
        // reader takes a name for the connection it has just read, so grouped codes lose all but the first.
        for (index, value) in values.iter().enumerate() {
            self.writer.write_i32(code + index as i32, value.code)?;
            self.writer
                .write_string(name_code + index as i32, &value.name)?;
        }
        Ok(())
    }

    fn write_dynamic_action_with_base_dxf(
        &mut self,
        value: &crate::objects::BlockActionWithBasePoint,
    ) -> Result<()> {
        self.write_dynamic_action_dxf(&value.action)?;
        self.writer.write_subclass("AcDbBlockActionWithBasePt")?;
        self.write_dynamic_connections_dxf(&value.connections, 92, 301)?;
        self.writer.write_point3d(1011, value.offset)?;
        self.writer.write_bool(280, value.dependent)?;
        self.writer.write_point3d(1012, value.base_point)?;
        Ok(())
    }

    fn write_dynamic_linear_constraint_dxf(
        &mut self,
        value: &crate::objects::BlockLinearConstraintParameter,
    ) -> Result<()> {
        self.write_dynamic_constraint_dxf(&value.constraint)?;
        self.writer
            .write_subclass("AcDbBlockLinearConstraintParameter")?;
        self.writer.write_string(305, &value.expression_name)?;
        self.writer
            .write_string(306, &value.expression_description)?;
        self.writer.write_double(140, value.value)?;
        self.write_dynamic_value_set_dxf(&value.value_set, 96, 128, 175, 307)?;
        Ok(())
    }

    fn write_solid_history_base_dxf(
        &mut self,
        value: &crate::objects::SolidHistoryNodeBase,
    ) -> Result<()> {
        self.write_dynamic_eval_dxf(&value.saved_eval())?;
        self.writer.write_subclass("AcDbShHistoryNode")?;
        self.writer.write_i32(90, value.major)?;
        self.writer.write_i32(91, value.minor)?;
        // Groups 50..55 are angle codes: the reference application stores
        // those matrix elements in degrees.
        for (index, item) in crate::entities::surface::transpose_matrix(value.transform).iter().enumerate() {
            let item = if index >= 10 { item.to_dxf_degrees() } else { *item };
            self.writer.write_double(40 + index as i32, item)?;
        }
        self.writer.write_color(62, value.color)?;
        if let Some(true_color) = value.color.to_true_color_value() {
            self.writer.write_i32(420, true_color)?;
        }
        self.writer.write_i32(92, value.step_id)?;
        self.writer.write_handle(347, value.material)?;
        Ok(())
    }

    fn write_solid_history_sweep_dxf(
        &mut self,
        value: &crate::objects::SolidHistorySweep,
        subclass: &str,
    ) -> Result<()> {
        self.write_solid_history_base_dxf(&value.base)?;
        self.writer.write_subclass("AcDbShPrimitive")?;
        self.writer.write_subclass("AcDbShSweepBase")?;
        self.writer.write_i32(90, value.operation_major)?;
        self.writer.write_i32(91, value.operation_minor)?;
        self.writer.write_point3d(10, value.direction)?;
        self.write_history_entity_dxf(92, 90, value.sweep_entity.as_ref())?;
        self.write_history_entity_dxf(93, 90, value.path_entity.as_ref())?;
        self.writer.write_double(42, value.draft_angle)?;
        self.writer.write_double(43, value.start_draft_distance)?;
        self.writer.write_double(44, value.end_draft_distance)?;
        self.writer.write_double(45, value.twist_angle)?;
        self.writer.write_double(48, value.scale_factor)?;
        self.writer.write_double(49, value.align_angle)?;
        // The reference application repeats 46 and 47 once per element.
        for item in crate::entities::surface::transpose_matrix(value.sweep_entity_transform) {
            self.writer.write_double(46, item)?;
        }
        for item in crate::entities::surface::transpose_matrix(value.path_entity_transform) {
            self.writer.write_double(47, item)?;
        }
        self.writer.write_bool(290, value.has_align_start)?;
        self.writer.write_i16(70, value.align_option.into())?;
        self.writer.write_i16(71, value.miter_option.into())?;
        self.writer.write_bool(292, value.align_start)?;
        self.writer.write_bool(293, value.bank)?;
        self.writer.write_bool(294, value.flags_294_296[0])?;
        self.writer.write_bool(295, value.flags_294_296[1])?;
        self.writer.write_bool(296, value.flags_294_296[2])?;
        self.writer.write_point3d(11, value.reference_point)?;
        self.writer.write_subclass(subclass)?;
        Ok(())
    }

    fn write_solid_history_operation_dxf(
        &mut self,
        value: &crate::objects::SolidHistoryOperation,
    ) -> Result<()> {
        use crate::objects::SolidHistoryOperation;

        match value {
            SolidHistoryOperation::Unknown => {}
            SolidHistoryOperation::Box(value) => {
                self.write_solid_history_base_dxf(&value.base)?;
                self.writer.write_subclass("AcDbShPrimitive")?;
                self.writer.write_subclass("AcDbShBox")?;
                self.writer.write_i32(90, value.operation_major)?;
                self.writer.write_i32(91, value.operation_minor)?;
                self.writer.write_double(40, value.length)?;
                self.writer.write_double(41, value.width)?;
                self.writer.write_double(42, value.height)?;
            }
            SolidHistoryOperation::Wedge(value) => {
                self.write_solid_history_base_dxf(&value.base)?;
                self.writer.write_subclass("AcDbShPrimitive")?;
                self.writer.write_subclass("AcDbShWedge")?;
                self.writer.write_i32(90, value.operation_major)?;
                self.writer.write_i32(91, value.operation_minor)?;
                self.writer.write_double(40, value.length)?;
                self.writer.write_double(41, value.width)?;
                self.writer.write_double(42, value.height)?;
            }
            SolidHistoryOperation::Sphere(value) => {
                self.write_solid_history_base_dxf(&value.base)?;
                self.writer.write_subclass("AcDbShPrimitive")?;
                self.writer.write_subclass("AcDbShSphere")?;
                self.writer.write_i32(90, value.operation_major)?;
                self.writer.write_i32(91, value.operation_minor)?;
                self.writer.write_double(40, value.radius)?;
            }
            SolidHistoryOperation::Cylinder(value) => {
                self.write_solid_history_base_dxf(&value.base)?;
                self.writer.write_subclass("AcDbShPrimitive")?;
                self.writer.write_subclass("AcDbShCylinder")?;
                self.writer.write_i32(90, value.operation_major)?;
                self.writer.write_i32(91, value.operation_minor)?;
                self.writer.write_double(40, value.height)?;
                self.writer.write_double(41, value.major_radius)?;
                self.writer.write_double(42, value.minor_radius)?;
                self.writer.write_double(43, value.x_radius)?;
            }
            SolidHistoryOperation::Cone(value) => {
                self.write_solid_history_base_dxf(&value.base)?;
                self.writer.write_subclass("AcDbShPrimitive")?;
                // A cone is a cylinder subclass: its fields belong to
                // AcDbShCylinder and AcDbShCone itself is empty.
                self.writer.write_subclass("AcDbShCylinder")?;
                self.writer.write_i32(90, value.operation_major)?;
                self.writer.write_i32(91, value.operation_minor)?;
                self.writer.write_double(40, value.height)?;
                self.writer.write_double(41, value.base_x_radius)?;
                self.writer.write_double(42, value.base_y_radius)?;
                self.writer.write_double(43, value.top_radius)?;
                self.writer.write_subclass("AcDbShCone")?;
            }
            SolidHistoryOperation::Pyramid(value) => {
                self.write_solid_history_base_dxf(&value.base)?;
                self.writer.write_subclass("AcDbShPrimitive")?;
                self.writer.write_subclass("AcDbShPyramid")?;
                self.writer.write_i32(90, value.operation_major)?;
                self.writer.write_i32(91, value.operation_minor)?;
                self.writer.write_double(40, value.height)?;
                self.writer.write_i32(92, value.sides)?;
                self.writer.write_double(41, value.radius)?;
                self.writer.write_double(42, value.top_radius)?;
            }
            SolidHistoryOperation::Torus(value) => {
                self.write_solid_history_base_dxf(&value.base)?;
                self.writer.write_subclass("AcDbShPrimitive")?;
                self.writer.write_subclass("AcDbShTorus")?;
                self.writer.write_i32(90, value.operation_major)?;
                self.writer.write_i32(91, value.operation_minor)?;
                self.writer.write_double(40, value.major_radius)?;
                self.writer.write_double(41, value.minor_radius)?;
            }
            SolidHistoryOperation::Boolean(value) => {
                self.write_solid_history_base_dxf(&value.base)?;
                self.writer.write_subclass("AcDbShBoolean")?;
                self.writer.write_i32(90, value.operation_major)?;
                self.writer.write_i32(91, value.operation_minor)?;
                self.writer.write_byte(280, value.operation)?;
                self.writer.write_i32(92, value.first_operand)?;
                self.writer.write_i32(93, value.second_operand)?;
            }
            SolidHistoryOperation::Brep(value) => {
                self.write_solid_history_base_dxf(&value.base)?;
                self.writer.write_subclass("AcDbShPrimitive")?;
                self.writer.write_subclass("AcDbShBrep")?;
                self.writer.write_i32(90, value.operation_major)?;
                self.writer.write_i32(91, value.operation_minor)?;
                // The modeler data follows in the AcDbShBrep subclass.
                self.writer.write_i16(70, 1)?;
                self.write_acis_data(&value.acis_data)?;
            }
            SolidHistoryOperation::Fillet(value) => {
                self.write_solid_history_base_dxf(&value.base)?;
                self.writer.write_subclass("AcDbShFillet")?;
                self.writer.write_i32(90, value.operation_major)?;
                self.writer.write_i32(91, value.operation_minor)?;
                self.writer.write_i32(92, value.method)?;
                self.writer.write_i32(93, value.edges.len() as i32)?;
                for item in &value.edges {
                    self.writer.write_i32(94, *item)?;
                }
                self.writer.write_i32(95, value.radii.len() as i32)?;
                for item in &value.radii {
                    self.writer.write_double(41, *item)?;
                }
                self.writer
                    .write_i32(96, value.start_setbacks.len() as i32)?;
                for item in &value.start_setbacks {
                    self.writer.write_double(42, *item)?;
                }
                self.writer.write_i32(97, value.end_setbacks.len() as i32)?;
                for item in &value.end_setbacks {
                    self.writer.write_double(43, *item)?;
                }
            }
            SolidHistoryOperation::Chamfer(value) => {
                self.write_solid_history_base_dxf(&value.base)?;
                self.writer.write_subclass("AcDbShChamfer")?;
                self.writer.write_i32(90, value.operation_major)?;
                self.writer.write_i32(91, value.operation_minor)?;
                self.writer.write_i32(92, value.method)?;
                self.writer.write_double(41, value.base_distance)?;
                self.writer.write_double(42, value.other_distance)?;
                self.writer.write_i32(93, value.edges.len() as i32)?;
                for item in &value.edges {
                    self.writer.write_i32(94, *item)?;
                }
                self.writer.write_i32(95, value.base_face)?;
            }
            SolidHistoryOperation::Sweep(value) => {
                self.write_solid_history_sweep_dxf(value, "AcDbShSweep")?;
            }
            SolidHistoryOperation::Extrusion(value) => {
                self.write_solid_history_sweep_dxf(value, "AcDbShExtrusion")?;
            }
            SolidHistoryOperation::Loft(value) => {
                self.write_solid_history_base_dxf(&value.base)?;
                self.writer.write_subclass("AcDbShPrimitive")?;
                self.writer.write_subclass("AcDbShLoft")?;
                self.writer.write_i32(90, value.operation_major)?;
                self.writer.write_i32(91, value.operation_minor)?;
                self.writer
                    .write_i32(92, value.cross_sections.len() as i32)?;
                for entity in &value.cross_sections {
                    self.write_history_entity_dxf(93, 94, Some(entity))?;
                }
                self.writer.write_i32(95, value.guides.len() as i32)?;
                for entity in &value.guides {
                    self.write_history_entity_dxf(96, 97, Some(entity))?;
                }
                let (path, options) = value.native();
                self.write_history_entity_dxf(98, 99, path)?;
                self.writer.write_i16(70, options.surface_option as i16)?;
                self.writer.write_double(41, options.start_draft_angle)?;
                self.writer.write_double(42, options.end_draft_angle)?;
                self.writer.write_double(43, options.start_magnitude)?;
                self.writer.write_double(44, options.end_magnitude)?;
                for (index, flag) in options.flags.into_iter().enumerate() {
                    self.writer.write_bool(290 + index as i32, flag)?;
                }
            }
            SolidHistoryOperation::Revolve(value) => {
                self.write_solid_history_base_dxf(&value.base)?;
                self.writer.write_subclass("AcDbShPrimitive")?;
                self.writer.write_subclass("AcDbShRevolve")?;
                self.writer.write_i32(90, value.operation_major)?;
                self.writer.write_i32(91, value.operation_minor)?;
                self.writer.write_point3d(10, value.axis_point)?;
                self.writer.write_point3d(11, value.direction)?;
                self.writer.write_double(40, value.revolve_angle)?;
                self.writer.write_double(41, value.start_angle)?;
                self.write_history_entity_dxf(92, 90, value.sweep_entity.as_ref())?;
                self.writer.write_double(43, value.draft_angle)?;
                self.writer.write_double(44, value.field_44)?;
                self.writer.write_double(45, value.field_45)?;
                self.writer.write_double(46, value.twist_angle)?;
                self.writer.write_bool(290, value.flag_290)?;
                self.writer.write_bool(291, value.close_to_axis)?;
            }
        }
        Ok(())
    }

    fn write_history_entity_dxf(
        &mut self,
        type_code: i32,
        size_code: i32,
        entity: Option<&crate::entities::EmbeddedEntity>,
    ) -> Result<()> {
        let Some(entity) = entity else {
            return self.writer.write_i32(type_code, 0);
        };
        if let crate::entities::EmbeddedEntity::Body { type_code: body_type, acis_data } = entity {
            // A polyline kept as a wire body: SAT version and text, as in
            // surface records.
            self.writer.write_i32(type_code, *body_type)?;
            self.writer.write_i16(70, 1)?;
            return self.write_acis_data(acis_data);
        }
        let encoded = crate::io::dwg::embedded_entity::encode_embedded_entity(
            entity,
            crate::io::dwg::DwgVersion::from_dxf_version(self.dxf_version)
                .unwrap_or(crate::io::dwg::DwgVersion::AC24),
            self.dxf_version,
        );
        self.writer.write_i32(type_code, encoded.type_code)?;
        self.writer
            .write_i32(size_code, (encoded.bytes.len() * 8) as i32)?;
        for chunk in encoded.bytes.chunks(127) {
            self.writer.write_binary(310, chunk)?;
        }
        Ok(())
    }

    fn write_dynamic_block_object_dxf(&mut self, object: &DynamicBlockObject) -> Result<()> {
        if let DynamicBlockData::VisibilityParameter(value) = &object.data {
            return self.write_block_visibility_parameter(value);
        }
        self.writer.write_string(0, &object.dxf_name)?;
        self.writer.write_handle(5, object.handle)?;
        // Reactors referencing objects dropped from the output (e.g.
        // unrestorable associative-framework dependencies) must be filtered,
        // otherwise the record dangles and the file is flagged for recovery.
        if !object.reactors.is_empty() {
            self.writer.write_string(102, "{ACAD_REACTORS")?;
            for reactor in &object.reactors {
                if !self.valid_handles.is_empty() && !self.valid_handles.contains(reactor) {
                    continue;
                }
                self.writer.write_handle(330, *reactor)?;
            }
            self.writer.write_string(102, "}")?;
        }
        if let Some(xdictionary) = object.xdictionary_handle {
            self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
            self.writer.write_handle(360, xdictionary)?;
            self.writer.write_string(102, "}")?;
        }
        self.writer.write_handle(330, object.owner)?;
        match &object.data {
            DynamicBlockData::Unknown => {}
            DynamicBlockData::Representation(value) => {
                self.writer.write_subclass(&object.cpp_class_name)?;
                self.writer.write_i16(70, value.flags)?;
                // The purge preventer (AutoCAD's own DXF) has the version only, no block pointer.
                if !is_unrestorable_assoc_object(&object.dxf_name) {
                    self.writer.write_handle(340, value.block)?;
                }
            }
            DynamicBlockData::ProxyNode(value) => {
                self.write_dynamic_eval_dxf(value)?;
                self.writer.write_subclass("AcDbDynamicBlockProxyNode")?;
            }
            DynamicBlockData::GripLocationComponent(value) => {
                self.write_dynamic_eval_dxf(&value.eval)?;
                self.writer.write_subclass("AcDbBlockGripExpr")?;
                self.writer.write_i32(91, value.grip_type)?;
                self.writer.write_string(300, &value.expression)?;
            }
            DynamicBlockData::AlignmentGrip(value) => {
                self.write_dynamic_grip_dxf(&value.grip)?;
                self.writer.write_subclass("AcDbBlockAlignmentGrip")?;
                // Orientation vector groups are 140/141/142, not 140/150/160.
                self.writer.write_double(140, value.orientation.x)?;
                self.writer.write_double(141, value.orientation.y)?;
                self.writer.write_double(142, value.orientation.z)?;
            }
            DynamicBlockData::FlipGrip(value) => {
                self.write_dynamic_grip_dxf(&value.grip)?;
                self.writer.write_subclass("AcDbBlockFlipGrip")?;
                // Orientation vector groups are 140/141/142, not 140/150/160.
                self.writer.write_double(140, value.orientation.x)?;
                self.writer.write_double(141, value.orientation.y)?;
                self.writer.write_double(142, value.orientation.z)?;
                self.writer.write_i32(93, value.combined_state)?;
            }
            DynamicBlockData::LinearGrip(value) => {
                self.write_dynamic_grip_dxf(&value.grip)?;
                self.writer.write_subclass("AcDbBlockLinearGrip")?;
                self.writer.write_double(140, value.orientation.x)?;
                self.writer.write_double(141, value.orientation.y)?;
                self.writer.write_double(142, value.orientation.z)?;
            }
            DynamicBlockData::LookupGrip(value)
            | DynamicBlockData::PolarGrip(value)
            | DynamicBlockData::RotationGrip(value)
            | DynamicBlockData::VisibilityGrip(value)
            | DynamicBlockData::XYGrip(value)
            | DynamicBlockData::PropertiesTableGrip(value) => {
                self.write_dynamic_grip_dxf(value)?;
                self.writer.write_subclass(&object.cpp_class_name)?;
            }
            DynamicBlockData::AlignmentParameter(value) => {
                self.write_dynamic_two_point_dxf(&value.parameter)?;
                self.writer.write_subclass("AcDbBlockAlignmentParameter")?;
                self.writer.write_bool(280, value.align_perpendicular)?;
            }
            DynamicBlockData::BasePointParameter(value) => {
                self.write_dynamic_one_point_dxf(&value.parameter)?;
                self.writer.write_subclass("AcDbBlockBasepointParameter")?;
                self.writer.write_point3d(1011, value.point)?;
                self.writer.write_point3d(1012, value.base_point)?;
            }
            DynamicBlockData::FlipParameter(value) => {
                self.write_dynamic_two_point_dxf(&value.parameter)?;
                self.writer.write_subclass("AcDbBlockFlipParameter")?;
                self.writer.write_string(305, &value.flip_label)?;
                self.writer
                    .write_string(306, &value.flip_label_description)?;
                self.writer.write_string(307, &value.base_state_label)?;
                self.writer.write_string(308, &value.flipped_state_label)?;
                self.writer
                    .write_point3d(1012, value.definition_label_point)?;
                self.writer.write_string(309, &value.tooltip)?;
                self.writer.write_i32(96, value.flags_96)?;
            }
            DynamicBlockData::LinearParameter(value) => {
                self.write_dynamic_two_point_dxf(&value.parameter)?;
                self.writer.write_subclass("AcDbBlockLinearParameter")?;
                self.writer.write_string(305, &value.distance_name)?;
                self.writer.write_string(306, &value.distance_description)?;
                self.writer.write_double(140, value.distance)?;
                self.write_dynamic_value_set_dxf(&value.value_set, 96, 141, 175, 307)?;
            }
            DynamicBlockData::LookupParameter(value) => {
                self.write_dynamic_one_point_dxf(&value.parameter)?;
                self.writer.write_subclass("AcDbBlockLookupParameter")?;
                self.writer.write_i32(94, value.index)?;
                self.writer.write_string(303, &value.lookup_name)?;
                self.writer.write_string(304, &value.lookup_description)?;
            }
            DynamicBlockData::PointParameter(value) => {
                self.write_dynamic_one_point_dxf(&value.parameter)?;
                self.writer.write_subclass("AcDbBlockPointParameter")?;
                self.writer.write_string(303, &value.position_name)?;
                self.writer.write_string(304, &value.position_description)?;
                self.writer
                    .write_point3d(1011, value.definition_label_point)?;
            }
            DynamicBlockData::PolarParameter(value) => {
                self.write_dynamic_two_point_dxf(&value.parameter)?;
                self.writer.write_subclass("AcDbBlockPolarParameter")?;
                self.writer.write_string(305, &value.distance_name)?;
                self.writer.write_string(306, &value.distance_description)?;
                self.writer.write_string(307, &value.angle_name)?;
                self.writer.write_string(308, &value.angle_description)?;
                self.writer.write_double(140, value.offset)?;
                self.write_dynamic_value_set_dxf(&value.distance_value_set, 96, 141, 175, 309)?;
                self.write_dynamic_value_set_dxf(&value.angle_value_set, 97, 145, 176, 410)?;
            }
            DynamicBlockData::RotationParameter(value) => {
                self.write_dynamic_two_point_dxf(&value.parameter)?;
                self.writer.write_subclass("AcDbBlockRotationParameter")?;
                // AutoCAD order: labels (305, 306) before the base-angle point.
                self.writer.write_string(305, &value.angle_name)?;
                self.writer.write_string(306, &value.angle_description)?;
                self.writer
                    .write_point3d(1011, value.definition_base_angle_point)?;
                self.writer.write_double(140, value.angle)?;
                self.write_dynamic_value_set_dxf(&value.value_set, 96, 141, 175, 307)?;
            }
            DynamicBlockData::XYParameter(value) => {
                self.write_dynamic_two_point_dxf(&value.parameter)?;
                self.writer.write_subclass("AcDbBlockXYParameter")?;
                self.writer.write_string(305, &value.y_label)?;
                self.writer.write_string(306, &value.x_label)?;
                self.writer.write_string(307, &value.y_label_description)?;
                self.writer.write_string(308, &value.x_label_description)?;
                self.writer.write_double(141, value.x_value)?;
                self.writer.write_double(140, value.y_value)?;
                self.write_dynamic_value_set_dxf(&value.y_value_set, 97, 146, 176, 309)?;
                self.write_dynamic_value_set_dxf(&value.x_value_set, 96, 142, 175, 410)?;
            }
            DynamicBlockData::UserParameter(value) => {
                self.write_dynamic_one_point_dxf(&value.parameter)?;
                self.writer.write_subclass("AcDbBlockUserParameter")?;
                self.writer.write_i16(90, value.flags)?;
                self.writer.write_handle(330, value.associated_variable)?;
                self.writer.write_string(301, &value.expression)?;
                self.writer.write_i16(70, value.value_code)?;
                match &value.value {
                    BlockEvalValue::Real(item) => self.writer.write_double(40, *item)?,
                    BlockEvalValue::Text(item) => self.writer.write_string(1, item)?,
                    BlockEvalValue::Long(item) => self.writer.write_i32(90, *item)?,
                    BlockEvalValue::Handle(item) => self.writer.write_handle(91, *item)?,
                    BlockEvalValue::Short(item) => self.writer.write_i16(70, *item)?,
                    BlockEvalValue::Point(item) => {
                        self.writer.write_double(10, item[0])?;
                        self.writer.write_double(20, item[1])?;
                    }
                    BlockEvalValue::None => {}
                }
                self.writer.write_i16(170, value.value_type)?;
            }
            DynamicBlockData::VisibilityParameter(_) => unreachable!(),
            DynamicBlockData::AngularConstraintParameter(value) => {
                self.write_dynamic_constraint_dxf(&value.constraint)?;
                self.writer
                    .write_subclass("AcDbBlockAngularConstraintParameter")?;
                self.writer.write_point3d(1011, value.center_point)?;
                self.writer.write_point3d(1012, value.end_point)?;
                self.writer.write_string(305, &value.expression_name)?;
                self.writer
                    .write_string(306, &value.expression_description)?;
                self.writer.write_double(140, value.angle)?;
                self.writer
                    .write_bool(280, value.orientation_on_both_grips)?;
                self.write_dynamic_value_set_dxf(&value.value_set, 96, 128, 175, 307)?;
            }
            DynamicBlockData::DiametricConstraintParameter(value)
            | DynamicBlockData::RadialConstraintParameter(value) => {
                self.write_dynamic_constraint_dxf(&value.constraint)?;
                self.writer.write_subclass(&object.cpp_class_name)?;
                self.writer.write_string(305, &value.expression_name)?;
                self.writer
                    .write_string(306, &value.expression_description)?;
                self.writer.write_double(140, value.distance)?;
                self.write_dynamic_value_set_dxf(&value.value_set, 96, 128, 175, 307)?;
            }
            DynamicBlockData::AlignedConstraintParameter(value)
            | DynamicBlockData::LinearConstraintParameter(value)
            | DynamicBlockData::HorizontalConstraintParameter(value)
            | DynamicBlockData::VerticalConstraintParameter(value) => {
                self.write_dynamic_linear_constraint_dxf(value)?;
                if object.dxf_name != "BLOCKLINEARCONSTRAINTPARAMETER" {
                    self.writer.write_subclass(&object.cpp_class_name)?;
                }
            }
            DynamicBlockData::ParameterDependencyBody(value) => {
                self.writer.write_subclass("AcDbAssocDependencyBody")?;
                self.writer.write_i16(90, value.dependency_body_version)?;
                self.writer
                    .write_subclass("AcDbImpAssocDimDependencyBodyBase")?;
                self.writer.write_i16(90, value.dimension_base_version)?;
                self.writer.write_string(1, &value.name)?;
                self.writer
                    .write_subclass("AcDbBlockParameterDependencyBody")?;
                self.writer.write_i16(90, value.class_version)?;
            }
            DynamicBlockData::MoveAction(value) => {
                self.write_dynamic_action_dxf(&value.action)?;
                self.writer.write_subclass("AcDbBlockMoveAction")?;
                self.write_dynamic_connections_dxf(&value.connections, 92, 301)?;
                self.writer
                    .write_double(140, value.offsets.distance_multiplier)?;
                self.writer.write_double(141, value.offsets.angle_offset)?;
                self.writer.write_byte(280, value.offsets.flags)?;
            }
            DynamicBlockData::FlipAction(value) => {
                self.write_dynamic_action_dxf(&value.action)?;
                self.writer.write_subclass("AcDbBlockFlipAction")?;
                self.write_dynamic_connections_dxf(&value.connections, 92, 301)?;
            }
            DynamicBlockData::RotateAction(value) | DynamicBlockData::ScaleAction(value) => {
                self.write_dynamic_action_with_base_dxf(&value.action)?;
                // The DXF subclass marker is not the C++ class name for the
                // rotate action (AcDbBlockRotateAction is written as
                // AcDbBlockRotationAction); AutoCAD rejects the object otherwise.
                let subclass = if matches!(object.data, DynamicBlockData::RotateAction(_)) {
                    "AcDbBlockRotationAction"
                } else {
                    "AcDbBlockScaleAction"
                };
                self.writer.write_subclass(subclass)?;
                self.write_dynamic_connections_dxf(&value.connections, 94, 303)?;
            }
            DynamicBlockData::ArrayAction(value) => {
                self.write_dynamic_action_dxf(&value.action)?;
                self.writer.write_subclass("AcDbBlockArrayAction")?;
                self.write_dynamic_connections_dxf(&value.connections, 92, 301)?;
                self.writer.write_double(140, value.row_offset)?;
                self.writer.write_double(141, value.column_offset)?;
            }
            DynamicBlockData::LookupAction(value) => {
                self.write_dynamic_action_dxf(&value.action)?;
                self.writer.write_subclass("AcDbBlockLookupAction")?;
                self.writer.write_i32(92, value.row_count)?;
                self.writer.write_i32(93, value.column_count)?;
                self.writer.write_string(301, "")?;
                for expression in &value.expressions {
                    self.writer.write_string(302, expression)?;
                }
                for column in &value.columns {
                    self.writer.write_string(303, "")?;
                    self.writer.write_i32(94, column.node_id)?;
                    self.writer.write_i32(95, column.value_type)?;
                    self.writer.write_i32(96, column.property_type)?;
                    self.writer.write_bool(282, column.lookup_property)?;
                    self.writer.write_string(305, &column.unmatched_name)?;
                    self.writer.write_bool(281, column.writable)?;
                    self.writer.write_string(304, &column.connection_name)?;
                }
                self.writer.write_bool(280, value.flag_280)?;
            }
            DynamicBlockData::StretchAction(value) => {
                self.write_dynamic_action_dxf(&value.action)?;
                self.writer.write_subclass("AcDbBlockStretchAction")?;
                self.write_dynamic_connections_dxf(&value.connections, 92, 301)?;
                self.writer.write_i32(72, value.points.len() as i32)?;
                for point in &value.points {
                    self.writer.write_point2d(1011, *point)?;
                }
                self.writer.write_i32(73, value.handles.len() as i32)?;
                for item in &value.handles {
                    self.writer.write_handle(331, item.handle)?;
                    self.writer.write_i16(74, item.indexes.len() as i16)?;
                    for index in &item.indexes {
                        self.writer.write_i32(94, *index)?;
                    }
                }
                self.writer.write_i32(75, value.codes.len() as i32)?;
                for item in &value.codes {
                    self.writer.write_i32(95, item.code)?;
                    self.writer.write_i16(76, item.indexes.len() as i16)?;
                    for index in &item.indexes {
                        self.writer.write_i32(94, *index)?;
                    }
                }
                self.writer
                    .write_double(140, value.offsets.distance_multiplier)?;
                self.writer.write_double(141, value.offsets.angle_offset)?;
                self.writer.write_byte(280, value.offsets.flags)?;
            }
            DynamicBlockData::PolarStretchAction(value) => {
                // ponytail: the trailing `extra` ids have no known DXF group
                // code, so DXF drops them (count 77 = 0) rather than failing
                // the whole save; DWG keeps them. Map them once a native DXF
                // sample shows their codes.
                self.write_dynamic_action_dxf(&value.action)?;
                self.writer.write_subclass("AcDbBlockPolarStretchAction")?;
                self.write_dynamic_connections_dxf(&value.connections, 92, 301)?;
                self.writer.write_i32(73, value.points.len() as i32)?;
                for point in &value.points {
                    self.writer.write_point2d(1011, *point)?;
                }
                self.writer.write_i32(72, value.handles.len() as i32)?;
                for handle in &value.handles {
                    self.writer.write_handle(331, *handle)?;
                }
                self.writer.write_i32(74, value.bindings.len() as i32)?;
                for item in &value.bindings {
                    self.writer.write_handle(332, item.handle)?;
                    self.writer.write_i32(75, item.indexes.len() as i32)?;
                    for index in &item.indexes {
                        self.writer.write_i32(76, *index)?;
                    }
                }
                self.writer.write_i32(78, value.codes.len() as i32)?;
                for item in &value.codes {
                    self.writer.write_i32(98, item.code)?;
                    self.writer.write_i32(79, item.indexes.len() as i32)?;
                    for index in &item.indexes {
                        self.writer.write_i32(76, *index)?;
                    }
                }
                self.writer.write_double(141, value.distance_multiplier)?;
                self.writer.write_double(140, value.angle_offset)?;
                self.writer.write_i32(77, 0)?;
            }
            DynamicBlockData::EvaluationGraph(value) => {
                self.writer.write_subclass("AcDbEvalGraph")?;
                self.writer.write_i32(96, value.first_node_id)?;
                self.writer.write_i32(97, value.first_node_id_copy)?;
                for node in &value.nodes {
                    self.writer.write_i32(91, node.id)?;
                    self.writer.write_i32(93, node.edge_flags)?;
                    self.writer.write_i32(95, node.next_id)?;
                    self.writer.write_handle(360, node.expression)?;
                    for item in node.node_data {
                        self.writer.write_i32(92, item)?;
                    }
                    if let Some(active) = node.active_cycles {
                        self.writer.write_bool(290, active)?;
                    }
                }
                for edge in &value.edges {
                    self.writer.write_i32(92, edge.id)?;
                    self.writer.write_i32(93, edge.next_id)?;
                    self.writer.write_i32(94, edge.incoming_edge)?;
                    self.writer.write_i32(91, edge.source_node)?;
                    self.writer.write_i32(91, edge.destination_node)?;
                    for item in edge.outgoing_edges {
                        self.writer.write_i32(92, item)?;
                    }
                }
            }
            DynamicBlockData::AlignmentParameterEntity
            | DynamicBlockData::BasePointParameterEntity
            | DynamicBlockData::FlipParameterEntity
            | DynamicBlockData::LinearParameterEntity
            | DynamicBlockData::PointParameterEntity
            | DynamicBlockData::RotationParameterEntity
            | DynamicBlockData::VisibilityParameterEntity
            | DynamicBlockData::XYParameterEntity
            | DynamicBlockData::FlipGripEntity
            | DynamicBlockData::LinearGripEntity
            | DynamicBlockData::PolarGripEntity
            | DynamicBlockData::RotationGripEntity
            | DynamicBlockData::VisibilityGripEntity
            | DynamicBlockData::XYGripEntity
            | DynamicBlockData::AngularConstraintParameterEntity(_) => {}
            DynamicBlockData::SolidHistory(value) => {
                self.writer.write_subclass("AcDbShHistory")?;
                self.writer.write_i32(90, value.major)?;
                self.writer.write_i32(91, value.minor)?;
                self.writer.write_handle(360, value.owner)?;
                self.writer.write_i32(92, value.history_node_id)?;
                self.writer.write_bool(280, value.show_history)?;
                self.writer.write_bool(281, value.record_history)?;
            }
            DynamicBlockData::SolidHistoryNode(value) => {
                self.write_solid_history_operation_dxf(value)?;
            }
        }
        Ok(())
    }

    fn write_point_cloud_dxf(&mut self, data: &PointCloudData) -> Result<()> {
        self.writer.write_subclass("AcDbPointCloud")?;
        self.writer.write_i16(70, data.class_version)?;
        self.writer.write_point3d(10, data.origin)?;
        self.writer.write_string(1, &data.saved_filename)?;
        self.writer.write_i32(90, data.source_files.len() as i32)?;
        if data.source_files.is_empty() {
            self.writer.write_point3d(11, data.extents_min)?;
            self.writer.write_point3d(12, data.extents_max)?;
            self.writer.write_i64(92, data.point_count)?;
            self.writer.write_string(3, &data.ucs_name)?;
            self.writer.write_point3d(13, data.ucs_origin)?;
            self.writer.write_point3d(210, data.ucs_x_direction)?;
            self.writer.write_point3d(211, data.ucs_y_direction)?;
            self.writer.write_point3d(212, data.ucs_z_direction)?;
            self.writer.write_handle(330, data.definition_handle)?;
            self.writer.write_handle(360, data.reactor_handle)?;
            self.writer.write_bool(290, data.show_intensity)?;
            self.writer.write_i16(71, data.intensity_scheme)?;
            self.writer.write_double(40, data.minimum_intensity)?;
            self.writer.write_double(41, data.maximum_intensity)?;
            self.writer.write_double(42, data.low_intensity_threshold)?;
            self.writer
                .write_double(43, data.high_intensity_threshold)?;
        }
        for source_file in &data.source_files {
            self.writer.write_string(2, source_file)?;
        }
        Ok(())
    }

    fn write_point_cloud_ex_dxf(&mut self, data: &PointCloudExData) -> Result<()> {
        self.writer.write_subclass("AcDbPointCloudEx")?;
        self.writer.write_i16(70, data.class_version)?;
        self.writer.write_point3d(10, data.extents_min)?;
        self.writer.write_point3d(11, data.extents_max)?;
        self.writer.write_point3d(12, data.ucs_origin)?;
        self.writer.write_point3d(210, data.ucs_x_direction)?;
        self.writer.write_point3d(211, data.ucs_y_direction)?;
        self.writer.write_point3d(212, data.ucs_z_direction)?;
        self.writer.write_bool(290, data.locked)?;
        self.writer.write_handle(340, data.definition_handle)?;
        self.writer.write_handle(360, data.reactor_handle)?;
        self.writer.write_string(1, &data.name)?;
        self.writer.write_bool(291, data.show_intensity)?;
        self.writer.write_i16(71, data.stylization_type)?;
        self.writer.write_string(1, &data.intensity_color_scheme)?;
        self.writer.write_string(1, &data.current_color_scheme)?;
        self.writer
            .write_string(1, &data.classification_color_scheme)?;
        self.writer.write_double(40, data.elevation_min)?;
        self.writer.write_double(41, data.elevation_max)?;
        self.writer.write_i32(90, data.intensity_min)?;
        self.writer.write_i32(91, data.intensity_max)?;
        self.writer
            .write_i16(71, data.intensity_out_of_range_behavior)?;
        self.writer
            .write_i16(72, data.elevation_out_of_range_behavior)?;
        self.writer
            .write_bool(292, data.elevation_apply_to_fixed_range)?;
        self.writer.write_bool(293, data.intensity_as_gradient)?;
        self.writer.write_bool(294, data.elevation_as_gradient)?;
        self.writer.write_bool(295, data.show_cropping)?;
        self.writer.write_i32(92, data.croppings.len() as i32)?;
        for crop in &data.croppings {
            self.writer.write_i16(280, crop.crop_type)?;
            self.writer.write_bool(290, crop.inside)?;
            self.writer.write_bool(290, crop.inverted)?;
            self.writer.write_point3d(13, crop.plane)?;
            self.writer.write_point3d(213, crop.x_direction)?;
            self.writer.write_point3d(213, crop.y_direction)?;
            self.writer.write_i32(93, crop.points.len() as i32)?;
            for point in &crop.points {
                self.writer.write_point3d(13, *point)?;
            }
        }
        // The scans and the regions turned off.
        self.writer.write_i32(93, data.hidden_scans.len() as i32)?;
        for scan in &data.hidden_scans {
            self.writer.write_string(1, scan)?;
        }
        self.writer.write_i32(93, data.hidden_regions.len() as i32)?;
        for region in &data.hidden_regions {
            self.writer.write_i32(93, *region)?;
        }
        Ok(())
    }

    /// Return `owner` if it will be present in the output, else fall back to the
    /// *Model_Space record. An entity whose original owner (e.g. a dropped
    /// application container with no DXF form) is gone would otherwise emit a
    /// dangling 330 reference that strict CAD readers reject on audit.
    fn safe_entity_owner(&self, owner: Handle) -> Handle {
        if owner == Handle::NULL
            || self.valid_handles.is_empty()
            || self.valid_handles.contains(&owner)
            || self.model_space_handle == Handle::NULL
        {
            owner
        } else {
            self.model_space_handle
        }
    }

    /// Write common entity data with owner
    fn write_common_entity_data(&mut self, common: &EntityCommon, owner: Handle) -> Result<()> {
        let owner = self.safe_entity_owner(owner);
        // An R12 record states a handle only when its source did.
        if !self.r12 || self.r12_states(common.handle) {
            self.writer.write_handle(5, common.handle)?;
        }
        self.writer.write_handle(330, owner)?;

        // Write xdictionary group
        if let Some(xdict) = common.xdictionary_handle {
            if xdict != Handle::NULL
                && (self.valid_handles.is_empty() || self.valid_handles.contains(&xdict))
            {
                self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
                self.writer.write_handle(360, xdict)?;
                self.writer.write_string(102, "}")?;
            }
        }

        // Write reactor group (filter out reactors pointing to non-existent objects)
        if !common.reactors.is_empty() {
            let valid_reactors: Vec<Handle> = if self.valid_handles.is_empty() {
                common.reactors.clone()
            } else {
                common
                    .reactors
                    .iter()
                    .copied()
                    .filter(|r| self.valid_handles.contains(r))
                    .collect()
            };
            if !valid_reactors.is_empty() {
                self.writer.write_string(102, "{ACAD_REACTORS")?;
                for reactor in &valid_reactors {
                    self.writer.write_handle(330, *reactor)?;
                }
                self.writer.write_string(102, "}")?;
            }
        }

        self.writer.write_subclass("AcDbEntity")?;

        // Paper space flag (code 67) — required for entities in paper space
        if self.writing_paper_space {
            self.writer.write_i16(67, 1)?;
        }

        // The DWG reader names an entity on a viewport-override layer by its base layer and keeps the handle;
        // the DXF names the override record ("0 @ 1").
        let override_name = common
            .layer_handle
            .and_then(|handle| self.layer_names.get(&handle))
            .filter(|name| name.starts_with(&format!("{} @ ", common.layer)));
        self.writer.write_string(8, override_name.unwrap_or(&common.layer))?;

        if let Some(graphics) = common.graphic_data.as_deref() {
            if self.dxf_version >= DxfVersion::AC1024 {
                self.writer.write_i64(160, graphics.len() as i64)?;
            } else {
                self.writer.write_i32(92, graphics.len() as i32)?;
            }
            for chunk in graphics.chunks(127) {
                self.writer.write_binary(310, chunk)?;
            }
        }

        // Write linetype if not default (ByLayer)
        if common.has_linetype() {
            self.writer.write_string(6, &common.linetype)?;
        }

        // Write color only if not ByLayer (default)
        if common.color != Color::ByLayer {
            self.writer.write_color(62, common.color)?;
        }

        // True color (code 420) — only for AC1018+ (AutoCAD 2004+)
        if self.dxf_version >= DxfVersion::AC1018 {
            if let Some(tc) = common.color.to_true_color_value() {
                self.writer.write_i32(420, tc)?;
            }
            if let Some(name) = &common.color_name {
                self.writer.write_string(430, name)?;
            }
        }

        // Write linetype scale if not 1.0
        if (common.linetype_scale - 1.0).abs() > 1e-12 && !self.r12 {
            self.writer.write_double(48, common.linetype_scale)?;
        }

        // Write lineweight if not default (an R14 drawing has none: AutoCAD's RTLwPs record holds it)
        if common.line_weight != crate::types::LineWeight::ByLayer && !self.r12 && self.dxf_version >= DxfVersion::AC1015 {
            self.writer.write_i16(370, common.line_weight.value())?;
        }

        // Write visibility
        if common.invisible {
            self.writer.write_i16(60, 1)?;
        }

        // Transparency (code 440) — ByLayer is the implicit default.
        if self.dxf_version >= DxfVersion::AC1018 && !common.transparency.is_by_layer() {
            self.writer
                .write_i32(440, common.transparency.to_dxf_value())?;
        }

        // Material (code 347, R2007+), only when the entity carries its own (the flags say "handle follows").
        if self.dxf_version >= DxfVersion::AC1021 && common.material_flags == 0b11 {
            if let Some(handle) = common.material_handle.filter(|handle| {
                !handle.is_null() && (self.valid_handles.is_empty() || self.valid_handles.contains(handle))
            }) {
                self.writer.write_handle(347, handle)?;
            }
        } else if self.dxf_version >= DxfVersion::AC1021 && common.material_flags == 1 {
            // A DWG says "ByBlock" in two bits; AutoCAD's DXF names the built-in ByBlock material.
            if let Some(handle) = self.by_block_material {
                self.writer.write_handle(347, handle)?;
            }
        }

        if self.dxf_version >= DxfVersion::AC1024 {
            for handle in [
                common.full_visual_style_handle,
                common.face_visual_style_handle,
                common.edge_visual_style_handle,
            ]
            .into_iter()
            .flatten()
            .filter(|handle| {
                !handle.is_null()
                    && (self.valid_handles.is_empty() || self.valid_handles.contains(handle))
            }) {
                self.writer.write_handle(348, handle)?;
            }
        }

        Ok(())
    }

    /// Write extrusion direction (normal vector, codes 210/220/230) if not default (0,0,1).
    fn write_normal(&mut self, normal: Vector3) -> Result<()> {
        if normal != Vector3::UNIT_Z {
            self.writer.write_double(210, normal.x)?;
            self.writer.write_double(220, normal.y)?;
            self.writer.write_double(230, normal.z)?;
        }
        Ok(())
    }

    /// Write an unknown entity, preserving raw group codes if available.
    fn write_unknown_entity(
        &mut self,
        entity: &crate::entities::UnknownEntity,
        owner: Handle,
    ) -> Result<()> {
        if let Some(ref codes) = entity.raw_dxf_codes {
            // Write the original DXF type name (e.g. "ACAD_PROXY_ENTITY")
            self.writer.write_entity_type(&entity.dxf_name)?;
            self.write_common_entity_data(&entity.common, owner)?;
            // Write preserved entity-specific codes
            for (code, value) in codes {
                self.writer.write_string(*code, value)?;
            }
            Ok(())
        } else {
            // No raw data — skip this entity
            Ok(())
        }
    }

    /// Write POINT entity
    fn write_point(&mut self, point: &Point, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("POINT")?;
        self.write_common_entity_data(&point.common, owner)?;
        self.writer.write_subclass("AcDbPoint")?;
        self.writer.write_point3d(10, point.location)?;
        if point.thickness != 0.0 {
            self.writer.write_double(39, point.thickness)?;
        }
        self.write_normal(point.normal)?;
        if point.x_axis_angle != 0.0 {
            self.writer.write_double(50, point.x_axis_angle)?;
        }
        Ok(())
    }

    /// Write LINE entity
    fn write_line(&mut self, line: &Line, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("LINE")?;
        self.write_common_entity_data(&line.common, owner)?;
        self.writer.write_subclass("AcDbLine")?;
        self.writer.write_point3d(10, line.start)?;
        self.writer.write_point3d(11, line.end)?;
        if line.thickness != 0.0 {
            self.writer.write_double(39, line.thickness)?;
        }
        self.write_normal(line.normal)?;
        Ok(())
    }

    /// Write CIRCLE entity
    fn write_circle(&mut self, circle: &Circle, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("CIRCLE")?;
        self.write_common_entity_data(&circle.common, owner)?;
        self.writer.write_subclass("AcDbCircle")?;
        self.writer.write_point3d(10, circle.center)?;
        self.writer.write_double(40, circle.radius)?;
        if circle.thickness != 0.0 {
            self.writer.write_double(39, circle.thickness)?;
        }
        self.write_normal(circle.normal)?;
        Ok(())
    }

    /// Write ARC entity
    fn write_arc(&mut self, arc: &Arc, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("ARC")?;
        self.write_common_entity_data(&arc.common, owner)?;
        self.writer.write_subclass("AcDbCircle")?;
        self.writer.write_point3d(10, arc.center)?;
        self.writer.write_double(40, arc.radius)?;
        if arc.thickness != 0.0 {
            self.writer.write_double(39, arc.thickness)?;
        }
        self.write_normal(arc.normal)?;
        self.writer.write_subclass("AcDbArc")?;
        self.writer.write_double(50, arc.start_angle.to_dxf_degrees())?;
        self.writer.write_double(51, arc.end_angle.to_dxf_degrees())?;
        Ok(())
    }

    /// Write ELLIPSE entity
    fn write_ellipse(&mut self, ellipse: &Ellipse, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("ELLIPSE")?;
        self.write_common_entity_data(&ellipse.common, owner)?;
        self.writer.write_subclass("AcDbEllipse")?;
        self.writer.write_point3d(10, ellipse.center)?;
        self.writer.write_point3d(11, ellipse.major_axis)?;
        self.writer.write_double(40, ellipse.minor_axis_ratio)?;
        self.write_normal(ellipse.normal)?;
        if ellipse.thickness != 0.0 {
            self.writer.write_double(39, ellipse.thickness)?;
        }
        self.writer.write_double(41, ellipse.start_parameter)?;
        self.writer.write_double(42, ellipse.end_parameter)?;
        Ok(())
    }

    /// Write POLYLINE entity (3D polyline)
    fn write_polyline(&mut self, polyline: &Polyline, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("POLYLINE")?;
        self.write_common_entity_data(&polyline.common, owner)?;
        self.writer.write_subclass("AcDb3dPolyline")?;

        // Entities follow flag (VERTEX records follow)
        self.writer.write_i16(66, 1)?;

        // Dummy point (required by DXF spec)
        self.writer.write_double(10, 0.0)?;
        self.writer.write_double(20, 0.0)?;
        self.writer.write_double(30, 0.0)?;

        let mut flags: i16 = 8; // 3D polyline flag
        if polyline.is_closed() {
            flags |= 1;
        }
        self.writer.write_i16(70, flags)?;

        // XDATA precedes the child VERTEX/SEQEND records.
        self.write_xdata(&polyline.common.extended_data)?;

        // VERTEX and SEQEND are owned by the polyline entity
        let polyline_handle = polyline.common.handle;

        // Write vertices with proper subclass markers
        for vertex in polyline.vertices.iter() {
            let vertex_handle = self.allocate_handle();
            self.writer.write_entity_type("VERTEX")?;
            self.writer.write_handle(5, vertex_handle)?;
            self.writer.write_handle(330, polyline_handle)?;
            self.writer.write_subclass("AcDbEntity")?;
            self.writer.write_string(8, &polyline.common.layer)?;
            // Propagate parent color to vertex so CAD doesn't flag mismatch
            if polyline.common.color != Color::ByLayer {
                self.writer.write_color(62, polyline.common.color)?;
            }
            self.writer.write_subclass("AcDbVertex")?;
            self.writer.write_subclass("AcDb3dPolylineVertex")?;
            self.writer.write_point3d(10, vertex.location)?;
            self.writer.write_i16(70, 32)?; // 3D polyline vertex
        }

        // Write SEQEND
        let seqend_handle = self.allocate_handle();
        self.writer.write_entity_type("SEQEND")?;
        self.writer.write_handle(5, seqend_handle)?;
        self.writer.write_handle(330, polyline_handle)?;
        self.writer.write_subclass("AcDbEntity")?;
        if self.writing_paper_space {
            self.writer.write_i16(67, 1)?;
        }
        self.writer.write_string(8, &polyline.common.layer)?;

        Ok(())
    }

    /// Write POLYLINE entity (2D polyline)
    fn write_polyline2d(&mut self, polyline: &Polyline2D, owner: Handle) -> Result<()> {
        // Down-save plain 2D polylines to LWPOLYLINE for R2000+ output.
        // Writing every heavy polyline back as POLYLINE/VERTEX/SEQEND is a
        // large structural divergence from the source drawing, and some CAD
        // applications flag the resurrected VERTEX owner wiring on save.
        if self.dxf_version >= DxfVersion::AC1015 && polyline2d_downsaves_to_lwpolyline(polyline) {
            return self.write_polyline2d_as_lwpolyline(polyline, owner);
        }
        self.writer.write_entity_type("POLYLINE")?;
        self.write_common_entity_data(&polyline.common, owner)?;
        self.writer.write_subclass("AcDb2dPolyline")?;

        // Entities follow flag (VERTEX records follow)
        self.writer.write_i16(66, 1)?;

        // Dummy origin point (required by DXF spec for POLYLINE entity)
        self.writer.write_double(10, 0.0)?;
        self.writer.write_double(20, 0.0)?;
        self.writer.write_double(30, polyline.elevation)?;

        self.writer.write_i16(70, polyline.flags.bits() as i16)?;

        if polyline.thickness != 0.0 {
            self.writer.write_double(39, polyline.thickness)?;
        }
        // The default widths of the record. Once the widths live on the vertices the polyline's own are zero;
        // the defaults the source stated are still written, so a second write is the first.
        let (default_start, default_end) = if polyline.start_width != 0.0 || polyline.end_width != 0.0 {
            (polyline.start_width, polyline.end_width)
        } else {
            polyline.source_widths.unwrap_or((0.0, 0.0))
        };
        if default_start != 0.0 {
            self.writer.write_double(40, default_start)?;
        }
        if default_end != 0.0 {
            self.writer.write_double(41, default_end)?;
        }
        self.write_normal(polyline.normal)?;

        // XDATA precedes the child VERTEX/SEQEND records.
        self.write_xdata(&polyline.common.extended_data)?;

        // VERTEX and SEQEND are owned by the polyline entity
        let polyline_handle = polyline.common.handle;

        // Write vertices with proper subclass markers
        for vertex in polyline.vertices.iter() {
            let mut common = vertex.common.clone();
            let vertex_handle = if common.handle.is_null() {
                // A newly constructed vertex has no independent identity; keep
                // the historical inherited layer/color behaviour.
                Self::inherit_compound_child_defaults(&mut common, &polyline.common);
                self.allocate_handle()
            } else {
                common.handle
            };
            common.handle = vertex_handle;
            common.owner_handle = polyline_handle;
            self.writer.write_entity_type("VERTEX")?;
            self.write_common_entity_data(&common, polyline_handle)?;
            self.writer.write_subclass("AcDbVertex")?;
            self.writer.write_subclass("AcDb2dVertex")?;
            self.writer.write_point3d(10, vertex.location)?;
            // A VERTEX without these groups has the width of the POLYLINE record. A width that differs from it is
            // stated, and so is a zero width read from a file that stated it (a vertex built in memory with width 0
            // means "the default").
            let from_source = vertex.source_widths.is_some();
            if vertex.start_width != default_start && (vertex.start_width != 0.0 || from_source) {
                self.writer.write_double(40, vertex.start_width)?;
            }
            if vertex.end_width != default_end && (vertex.end_width != 0.0 || from_source) {
                self.writer.write_double(41, vertex.end_width)?;
            }
            if vertex.bulge != 0.0 {
                self.writer.write_double(42, vertex.bulge)?;
            }
            self.writer.write_i16(70, vertex.flags.bits() as i16)?;
            if vertex.id != 0 && !self.r12 { self.writer.write_i32(91, vertex.id)?; }
            self.writer.write_double(50, vertex.curve_tangent.to_dxf_degrees())?;
            self.write_xdata(&common.extended_data)?;
        }

        // Write SEQEND
        let seqend_handle = polyline
            .seqend
            .as_ref()
            .map(|seqend| seqend.common.handle)
            .filter(|handle| !handle.is_null())
            .unwrap_or_else(|| self.allocate_handle());
        self.write_owned_seqend(
            polyline.seqend.as_deref(),
            &polyline.common,
            seqend_handle,
            polyline_handle,
        )?;

        Ok(())
    }

    /// Down-save a plain 2D heavy polyline as LWPOLYLINE (R2000+ output).
    ///
    /// Per-vertex widths are baked in: a VERTEX without codes 40/41 inherits
    /// the POLYLINE default width, and LWPOLYLINE has no default-width slot —
    /// the same baking the DXF reader applies when it reads a heavy polyline.
    fn write_polyline2d_as_lwpolyline(
        &mut self,
        polyline: &Polyline2D,
        owner: Handle,
    ) -> Result<()> {
        self.writer.write_entity_type("LWPOLYLINE")?;
        self.write_common_entity_data(&polyline.common, owner)?;
        self.writer.write_subclass("AcDbPolyline")?;
        self.writer.write_i32(90, polyline.vertices.len() as i32)?;

        let mut flags: i16 = 0;
        if polyline.flags.is_closed() {
            flags |= 1;
        }
        if polyline.flags.bits() & 128 != 0 {
            flags |= 128;
        }
        self.writer.write_i16(70, flags)?;

        self.writer.write_double(38, polyline.elevation)?;
        if polyline.thickness != 0.0 {
            self.writer.write_double(39, polyline.thickness)?;
        }

        for vertex in &polyline.vertices {
            self.writer.write_double(10, vertex.location.x)?;
            self.writer.write_double(20, vertex.location.y)?;
            let start_width = if vertex.start_width != 0.0 {
                vertex.start_width
            } else {
                polyline.start_width
            };
            let end_width = if vertex.end_width != 0.0 {
                vertex.end_width
            } else {
                polyline.end_width
            };
            self.writer.write_double(40, start_width)?;
            self.writer.write_double(41, end_width)?;
            self.writer.write_double(42, vertex.bulge)?;
            if vertex.id != 0 {
                self.writer.write_i32(91, vertex.id)?;
            }
        }

        self.write_normal(polyline.normal)?;

        // XDATA precedes the child records in the legacy form; LWPOLYLINE has
        // no child records, so it goes at the end of the entity's own codes.
        self.write_xdata(&polyline.common.extended_data)?;
        Ok(())
    }

    /// Write LWPOLYLINE entity
    fn write_lwpolyline(&mut self, lwpoly: &LwPolyline, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("LWPOLYLINE")?;
        self.write_common_entity_data(&lwpoly.common, owner)?;
        self.writer.write_subclass("AcDbPolyline")?;
        self.writer.write_i32(90, lwpoly.vertices.len() as i32)?;

        let mut flags: i16 = 0;
        if lwpoly.is_closed {
            flags |= 1;
        }
        if lwpoly.plinegen {
            flags |= 128;
        }
        self.writer.write_i16(70, flags)?;

        self.writer.write_double(38, lwpoly.elevation)?;
        if lwpoly.thickness != 0.0 {
            self.writer.write_double(39, lwpoly.thickness)?;
        }
        if lwpoly.constant_width != 0.0 {
            self.writer.write_double(43, lwpoly.constant_width)?;
        }

        for vertex in &lwpoly.vertices {
            self.writer.write_double(10, vertex.location.x)?;
            self.writer.write_double(20, vertex.location.y)?;
            // Always write start width, end width, and bulge (default to 0.0 if not set)
            self.writer.write_double(40, vertex.start_width)?;
            self.writer.write_double(41, vertex.end_width)?;
            self.writer.write_double(42, vertex.bulge)?;
            if vertex.vertex_id != 0 {
                self.writer.write_i32(91, vertex.vertex_id)?;
            }
        }

        self.write_normal(lwpoly.normal)?;
        Ok(())
    }

    /// Write TEXT entity
    fn write_text(&mut self, text: &Text, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("TEXT")?;
        self.write_common_entity_data(&text.common, owner)?;
        self.writer.write_subclass("AcDbText")?;
        if text.thickness != 0.0 {
            self.writer.write_double(39, text.thickness)?;
        }
        self.writer.write_point3d(10, text.insertion_point)?;
        self.writer.write_double(40, text.height)?;
        self.writer.write_string(1, &text.value)?;
        if text.rotation != 0.0 {
            self.writer.write_double(50, text.rotation.to_dxf_degrees())?;
        }
        if text.width_factor != 1.0 {
            self.writer.write_double(41, text.width_factor)?;
        }
        if text.oblique_angle != 0.0 {
            self.writer.write_double(51, text.oblique_angle.to_dxf_degrees())?;
        }
        self.writer.write_string(7, &text.style)?;
        if text.generation_flags != 0 {
            self.writer.write_i16(71, text.generation_flags)?;
        }
        self.writer
            .write_i16(72, text.horizontal_alignment as i16)?;
        if let Some(align_pt) = text.alignment_point {
            self.writer.write_point3d(11, align_pt)?;
        }
        self.write_normal(text.normal)?;
        self.writer.write_subclass("AcDbText")?;
        self.writer.write_i16(73, text.vertical_alignment as i16)?;
        Ok(())
    }

    /// Write MTEXT entity
    fn write_mtext(&mut self, mtext: &MText, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("MTEXT")?;
        self.write_common_entity_data(&mtext.common, owner)?;
        self.writer.write_subclass("AcDbMText")?;
        self.writer.write_point3d(10, mtext.insertion_point)?;
        self.writer.write_double(40, mtext.height)?;
        self.writer.write_double(41, mtext.rectangle_width)?;
        self.writer.write_i16(71, mtext.attachment_point as i16)?;
        self.writer.write_i16(72, mtext.drawing_direction as i16)?;
        self.write_mtext_value(&mtext.value, Self::mtext_lexical_value(mtext))?;

        self.writer.write_string(7, &mtext.style)?;
        // The X-axis direction (group 11) is stated when the text has one or is rotated; an unrotated text without
        // one leaves the group out, as AutoCAD does, so the reader does not gain a direction the source never had.
        if mtext.dwg_x_direction.is_some() || mtext.rotation != 0.0 {
            let x_direction = mtext
                .dwg_x_direction
                .unwrap_or_else(|| Vector3::new(mtext.rotation.cos(), mtext.rotation.sin(), 0.0));
            self.writer.write_point3d(11, x_direction)?;
        }
        // Laid-out text extents (output-only). Skipped when never computed so
        // files authored without layout info round-trip unchanged.
        if mtext.extents_width > 0.0 {
            self.writer.write_double(42, mtext.extents_width)?;
        }
        if mtext.extents_height > 0.0 {
            self.writer.write_double(43, mtext.extents_height)?;
        }
        if self.dxf_version >= DxfVersion::AC1015 {
            self.writer.write_i16(73, mtext.line_spacing_style as i16)?;
            self.writer.write_double(44, mtext.line_spacing_factor)?;
        }
        if let Some(h) = mtext.rectangle_height {
            self.writer.write_double(46, h)?;
        }
        // Background fill — only when enabled by the flags.
        // A pre-R2004 drawing keeps the fill in the round-trip XDATA it was saved with (the reader restored it
        // into the native fields); those versions predate the native groups, so only the XDATA is written then.
        let legacy_fill_xdata = self.dxf_version < DxfVersion::AC1018
            && mtext.common.extended_data.get_record("ACAD").is_some_and(|record| {
                record
                    .values
                    .iter()
                    .any(|value| matches!(value, XDataValue::String(text) if text == "ACAD_MTEXT_BBRT"))
            });
        if mtext.background_fill_flags != 0 && !legacy_fill_xdata {
            self.writer.write_i32(90, mtext.background_fill_flags)?;
            self.writer.write_double(45, mtext.background_scale)?;
            if let Some(tc) = mtext.background_color.to_true_color_value() {
                self.writer.write_i32(421, tc)?;
            } else if let Some(idx) = mtext.background_color.index() {
                self.writer.write_i16(63, idx as i16)?;
            }
            // Written even when zero (AutoCAD does, and a reader that finds the group absent applies its own
            // default transparency), unless the DXF source itself left it out.
            if mtext
                .dxf_source
                .as_ref()
                .map_or(true, |source| source.has_background_transparency)
                || mtext.background_transparency != 0
            {
                self.writer.write_i32(441, mtext.background_transparency)?;
            }
        }
        // R2018+ keeps the column layout in the MTEXT's embedded object, as
        // the reference application writes it (it misreads the standard
        // column codes in an R2018 file and lays every line out at once).
        if mtext.column_data.column_type != 0 && self.dxf_version >= DxfVersion::AC1032 {
            self.write_normal(mtext.normal)?;
            let columns = &mtext.column_data;
            let manual_heights = columns.column_type == 2 && !columns.auto_height;
            self.writer.write_string(101, "Embedded Object")?;
            self.writer.write_i16(70, mtext.attachment_point as i16)?;
            self.writer
                .write_point3d(10, Vector3::new(mtext.rotation.cos(), mtext.rotation.sin(), 0.0))?;
            self.writer.write_point3d(11, mtext.insertion_point)?;
            self.writer.write_double(40, mtext.rectangle_width)?;
            self.writer.write_double(41, mtext.rectangle_height.unwrap_or(0.0))?;
            self.writer.write_double(42, mtext.extents_width)?;
            self.writer.write_double(43, mtext.extents_height)?;
            self.writer.write_i16(71, columns.column_type)?;
            let count = if manual_heights { columns.heights.len() as i32 } else { columns.column_count };
            self.writer.write_i16(72, count.clamp(0, i16::MAX as i32) as i16)?;
            self.writer.write_double(44, columns.width)?;
            self.writer.write_double(45, columns.gutter)?;
            self.writer.write_i16(73, i16::from(columns.auto_height))?;
            self.writer.write_i16(74, i16::from(columns.flow_reversed))?;
            if manual_heights {
                for height in &columns.heights {
                    self.writer.write_double(46, *height)?;
                }
            }
            return Ok(());
        }
        // Standard DXF MTEXT column layout. Rotation is emitted above before
        // the first column marker because code 50 is reused for column heights.
        if mtext.column_data.column_type != 0 {
            self.writer.write_i16(75, mtext.column_data.column_type)?;
            let manual_heights =
                mtext.column_data.column_type == 2 && !mtext.column_data.auto_height;
            let column_count = if manual_heights {
                mtext.column_data.heights.len().min(i16::MAX as usize) as i16
            } else {
                mtext.column_data.column_count.clamp(0, i16::MAX as i32) as i16
            };
            self.writer.write_i16(76, column_count)?;
            self.writer
                .write_i16(78, i16::from(mtext.column_data.flow_reversed))?;
            self.writer
                .write_i16(79, i16::from(mtext.column_data.auto_height))?;
            self.writer.write_double(48, mtext.column_data.width)?;
            self.writer.write_double(49, mtext.column_data.gutter)?;
            if manual_heights {
                for height in mtext.column_data.heights.iter().take(column_count as usize) {
                    self.writer.write_double(50, *height)?;
                }
            }
        }
        self.write_normal(mtext.normal)?;
        Ok(())
    }

    /// The MTEXT text as the source spelled it (`\U+00B0`, `^J`) while that still decodes to the value.
    fn mtext_lexical_value(mtext: &MText) -> Option<&str> {
        mtext.unchanged_source_value()
    }

    /// Largest split point at or below `limit` that is a character boundary and does not cut a `^X` caret
    /// escape or a `\U+XXXX` / `\M+XXXXX` escape in two (each chunk is decoded on its own by the reader).
    fn lexical_split_point(text: &str, limit: usize) -> usize {
        let mut split = limit.min(text.len());
        while split > 0 && !text.is_char_boundary(split) {
            split -= 1;
        }
        let bytes = text.as_bytes();
        let start = split.saturating_sub(8);
        for position in start..split {
            if bytes[position] == b'\\'
                && matches!(bytes.get(position + 1), Some(b'U') | Some(b'M'))
                && bytes.get(position + 2) == Some(&b'+')
            {
                let length = if bytes[position + 1] == b'U' { 7 } else { 8 };
                if position + length > split {
                    return position;
                }
            }
        }
        if split > 0 && bytes[split - 1] == b'^' {
            split -= 1;
        }
        split
    }

    fn write_mtext_value(&mut self, value: &str, lexical: Option<&str>) -> Result<()> {
        // A line feed in the text is not a paragraph break (`\P`): it is written as AutoCAD spells it, `^J` (a
        // carriage return `^M`), and a binary DXF string holds it as is.
        let lexical = lexical.or_else(|| value.contains(['\n', '\r']).then_some(value));
        // A value that still equals its decoded source spelling is written as the source spelled it (a line
        // break as ^J, a degree sign as \U+00B0), chunked like any other text.
        if let Some(lexical) = lexical.filter(|_| self.writer.writes_source_spelling()) {
            // The retained spelling has its caret escapes decoded and its \U+XXXX escapes intact; the caret
            // escapes go back on.
            let encoded = lexical
                .replace('^', "^ ")
                .replace("\r\n", "^M^J")
                .replace('\r', "^M")
                .replace('\n', "^J")
                .replace('\t', "^I");
            let mut remaining = encoded.as_str();
            while remaining.len() > 250 {
                let split = Self::lexical_split_point(remaining, 250);
                if split == 0 {
                    break;
                }
                let (chunk, rest) = remaining.split_at(split);
                self.writer.write_source_spelling(3, chunk)?;
                remaining = rest;
            }
            return self.writer.write_source_spelling(1, remaining);
        }
        // A binary DXF string may hold a source line break as is.
        if let Some(lexical) = lexical.filter(|_| !self.writer.writes_source_spelling()) {
            let mut remaining = lexical;
            while remaining.len() > 250 {
                // Each chunk is decoded on its own, so a \U+XXXX escape must not be cut in two.
                let mut split = Self::lexical_split_point(remaining, 250);
                if split == 0 {
                    split = remaining.len();
                }
                let (chunk, rest) = remaining.split_at(split);
                self.writer.write_xrecord_string(3, chunk)?;
                remaining = rest;
            }
            return self.writer.write_xrecord_string(1, remaining);
        }
        // DXF text format is line-based, so literal \n / \r in the value would
        // corrupt the file.  Replace them with the MText paragraph mark \P.
        let sanitized;
        let text: &str = if value.contains('\n') || value.contains('\r') {
            sanitized = value
                .replace("\r\n", "\\P")
                .replace('\r', "\\P")
                .replace('\n', "\\P");
            &sanitized
        } else {
            value
        };
        if text.len() > 250 {
            // Split into chunks at char boundaries
            let mut remaining = text;
            while remaining.len() > 250 {
                // Find a valid char boundary at or before byte 250
                let mut split_pos = 250;
                while split_pos > 0 && !remaining.is_char_boundary(split_pos) {
                    split_pos -= 1;
                }
                if split_pos == 0 {
                    split_pos = remaining.len();
                }
                let (chunk, rest) = remaining.split_at(split_pos);
                self.writer.write_string(3, chunk)?;
                remaining = rest;
            }
            self.writer.write_string(1, remaining)?;
        } else {
            self.writer.write_string(1, text)?;
        }
        Ok(())
    }

    /// Write SPLINE entity
    fn write_spline(&mut self, spline: &Spline, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("SPLINE")?;
        self.write_common_entity_data(&spline.common, owner)?;
        self.write_spline_body(spline)?;
        Ok(())
    }

    /// Write the AcDbSpline subclass block (marker + all spline group codes).
    /// Shared by SPLINE and HELIX, which carries a spline as its geometry.
    fn write_spline_body(&mut self, spline: &Spline) -> Result<()> {
        self.writer.write_subclass("AcDbSpline")?;

        // Normal vector
        self.write_normal(spline.normal)?;

        // Flags
        let flags = crate::io::dxf::spline_flags::write(spline);
        self.writer.write_i16(70, flags)?;

        self.writer.write_i16(71, spline.degree as i16)?;
        self.writer.write_i16(72, spline.knots.len() as i16)?;
        self.writer
            .write_i16(73, spline.control_points.len() as i16)?;
        self.writer.write_i16(74, spline.fit_points.len() as i16)?;

        // Knot / control-point / fit tolerances (round-trip the stored values). A DWG fit-point record (scenario 2)
        // stores no knot or control tolerances: the groups are left out, so a DXF reader applies its default
        // (1e-7 in the DXF reference).
        let record_without_tolerances = spline.dwg_scenario == Some(2) && spline.knot_tolerance == 0.0 && spline.control_tolerance == 0.0;
        if !record_without_tolerances {
            self.writer.write_double(42, spline.knot_tolerance)?;
            self.writer.write_double(43, spline.control_tolerance)?;
        }
        self.writer.write_double(44, spline.fit_tolerance)?;

        // Start / end tangents (only when set).
        if spline.begin_tangent != Vector3::ZERO {
            self.writer.write_point3d(12, spline.begin_tangent)?;
        }
        if spline.end_tangent != Vector3::ZERO {
            self.writer.write_point3d(13, spline.end_tangent)?;
        }

        // Knots
        for knot in &spline.knots {
            self.writer.write_double(40, *knot)?;
        }

        // Control points (with optional weights for rational splines)
        for (i, point) in spline.control_points.iter().enumerate() {
            self.writer.write_point3d(10, *point)?;
            if spline.flags.rational {
                let w = spline.weights.get(i).copied().unwrap_or(1.0);
                self.writer.write_double(41, w)?;
            }
        }

        // Fit points
        for point in &spline.fit_points {
            self.writer.write_point3d(11, *point)?;
        }

        Ok(())
    }

    /// Write a HELIX entity: the AcDbSpline geometry block followed by the
    /// AcDbHelix parameters.
    fn write_helix(&mut self, helix: &Helix, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("HELIX")?;
        self.write_common_entity_data(&helix.common, owner)?;
        self.write_spline_body(&helix.spline)?;

        self.writer.write_subclass("AcDbHelix")?;
        self.writer.write_i32(90, helix.major_version)?;
        self.writer.write_i32(91, helix.maintenance_version)?;
        self.writer.write_point3d(10, helix.axis_base_point)?;
        self.writer.write_point3d(11, helix.start_point)?;
        self.writer.write_point3d(12, helix.axis_vector)?;
        self.writer.write_double(40, helix.radius)?;
        self.writer.write_double(41, helix.turns)?;
        self.writer.write_double(42, helix.turn_height)?;
        self.writer.write_bool(290, helix.handedness)?;
        self.writer.write_byte(280, helix.constraint.to_code())?;
        Ok(())
    }

    /// Write DIMENSION entity
    fn write_dimension(&mut self, dimension: &Dimension, owner: Handle) -> Result<()> {
        match dimension {
            Dimension::Aligned(dim) => self.write_dimension_aligned(dim, owner),
            Dimension::Linear(dim) => self.write_dimension_linear(dim, owner),
            Dimension::Radius(dim) => self.write_dimension_radius(dim, owner),
            Dimension::Diameter(dim) => self.write_dimension_diameter(dim, owner),
            Dimension::Angular2Ln(dim) => self.write_dimension_angular_2line(dim, owner),
            Dimension::Angular3Pt(dim) => self.write_dimension_angular_3point(dim, owner),
            Dimension::Ordinate(dim) => self.write_dimension_ordinate(dim, owner),
            Dimension::Arc(dim) => self.write_dimension_arc(dim, owner),
            Dimension::LargeRadial(dim) => self.write_dimension_large_radial(dim, owner),
        }
    }

    fn write_dimension_base(
        &mut self,
        base: &DimensionBase,
        definition_point: Vector3,
        type_flags: i16,
        owner: Handle,
    ) -> Result<()> {
        self.write_common_entity_data(&base.common, owner)?;
        self.writer.write_subclass("AcDbDimension")?;
        // Omit an absent geometry cache so the CAD host can regenerate it.
        // An explicit empty group 2 is an invalid block-table reference.
        if !base.block_name.is_empty() {
            self.writer.write_string(2, &base.block_name)?;
        }
        self.writer.write_point3d(10, definition_point)?;
        self.writer.write_point3d(11, base.text_middle_point)?;
        if base.insertion_point != Vector3::ZERO {
            self.writer.write_point3d(12, base.insertion_point)?;
        }
        // Bit 0x80 marks text positioned at a user-defined location.
        let type_flags = if base.text_user_positioned {
            type_flags | 0x80
        } else {
            type_flags
        };
        // Bit 0x20 (the geometry block is an anonymous dimension block) has no typed field. A DXF source word
        // carries it; a record without one (read from DWG, built in memory) has it exactly when it has a block,
        // as every AutoCAD dimension does.
        let source = base.dxf_source.as_ref();
        let block_bit = match source.and_then(|s| s.raw_flags) {
            Some(raw) => raw & 0x20,
            None if base.block_name.is_empty() => 0,
            None => 0x20,
        };
        let type_flags = type_flags | block_bit;
        self.writer.write_i16(70, type_flags)?;
        if !self.r12 {
            self.writer.write_double(42, base.actual_measurement)?;
        }
        // DXF angles are in degrees; internal representation is radians.
        self.writer
            .write_double(53, base.text_rotation.to_dxf_degrees())?;
        if base.horizontal_direction.abs() > 1e-12 {
            self.writer
                .write_double(51, base.horizontal_direction.to_dxf_degrees())?;
        }
        // Groups the DXF source left out stay out (the reference reader then uses its own defaults) unless the
        // value was changed from the native default.
        // Groups 71, 72 and 41 are R2000 additions: an R14 record has none, and the native defaults must not
        // stand in for them.
        let r2000 = self.dxf_version >= DxfVersion::AC1015;
        if r2000
            && (source.map_or(true, |s| s.attachment_point.is_some())
                || base.attachment_point != AttachmentPointType::MiddleCenter)
        {
            self.writer.write_i16(71, base.attachment_point as i16)?;
        }
        if r2000
            && (source.map_or(true, |s| s.line_spacing_style.is_some()) || base.line_spacing_style != 1)
        {
            self.writer.write_i16(72, base.line_spacing_style)?;
        }
        self.writer.write_string(3, &base.style_name)?;
        if let Some(text) = base.text_override() {
            self.writer.write_string(1, text)?;
        }
        // A zero factor is the unset value of a record that has no such field (an older DWG), not a spacing.
        if r2000
            && base.line_spacing_factor > 0.0
            && ((base.line_spacing_factor - 1.0).abs() > 1e-10
                || source.map_or_else(|| self.stated_by_dwg(DxfVersion::AC1015), |s| s.line_spacing_factor.is_some()))
        {
            self.writer.write_double(41, base.line_spacing_factor)?;
        }
        // Normal vector (extrusion direction) — only write if not default (0,0,1)
        let n = base.normal;
        if (n.x).abs() > 1e-12 || (n.y).abs() > 1e-12 || (n.z - 1.0).abs() > 1e-12 {
            self.writer.write_double(210, n.x)?;
            self.writer.write_double(220, n.y)?;
            self.writer.write_double(230, n.z)?;
        }
        Ok(())
    }

    fn write_dimension_aligned(&mut self, dim: &DimensionAligned, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("DIMENSION")?;
        self.write_dimension_base(&dim.base, dim.definition_point, 1, owner)?; // Aligned = 1
        self.writer.write_subclass("AcDbAlignedDimension")?;
        self.writer.write_point3d(13, dim.first_point)?;
        self.writer.write_point3d(14, dim.second_point)?;
        if dim.ext_line_rotation.abs() > 1e-12 {
            self.writer
                .write_double(52, dim.ext_line_rotation.to_dxf_degrees())?;
        }
        Ok(())
    }

    fn write_dimension_linear(&mut self, dim: &DimensionLinear, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("DIMENSION")?;
        self.write_dimension_base(&dim.base, dim.definition_point, 0, owner)?; // Linear = 0
        self.writer.write_subclass("AcDbAlignedDimension")?;
        self.writer.write_point3d(13, dim.first_point)?;
        self.writer.write_point3d(14, dim.second_point)?;
        // DXF dimension-line rotation is in degrees.
        self.writer.write_double(50, dim.rotation.to_dxf_degrees())?;
        if dim.ext_line_rotation.abs() > 1e-12 {
            self.writer
                .write_double(52, dim.ext_line_rotation.to_dxf_degrees())?;
        }
        self.writer.write_subclass("AcDbRotatedDimension")?;
        Ok(())
    }

    fn write_dimension_radius(&mut self, dim: &DimensionRadius, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("DIMENSION")?;
        // Group 10 is the centre; group 15 is the chord point.
        self.write_dimension_base(&dim.base, dim.angle_vertex, 4, owner)?; // Radius = 4
        self.writer.write_subclass("AcDbRadialDimension")?;
        self.writer.write_point3d(15, dim.definition_point)?;
        self.writer.write_double(40, dim.leader_length)?;
        Ok(())
    }

    fn write_dimension_diameter(&mut self, dim: &DimensionDiameter, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("DIMENSION")?;
        self.write_dimension_base(&dim.base, dim.definition_point, 3, owner)?; // Diameter = 3
        self.writer.write_subclass("AcDbDiametricDimension")?;
        self.writer.write_point3d(15, dim.angle_vertex)?;
        self.writer.write_double(40, dim.leader_length)?;
        Ok(())
    }

    fn write_dimension_angular_2line(
        &mut self,
        dim: &DimensionAngular2Ln,
        owner: Handle,
    ) -> Result<()> {
        self.writer.write_entity_type("DIMENSION")?;
        self.write_dimension_base(&dim.base, dim.definition_point, 2, owner)?; // Angular = 2
        self.writer.write_subclass("AcDb2LineAngularDimension")?;
        self.writer.write_point3d(13, dim.first_point)?;
        self.writer.write_point3d(14, dim.second_point)?;
        self.writer.write_point3d(15, dim.angle_vertex)?;
        self.writer.write_point3d(16, dim.dimension_arc)?;
        Ok(())
    }

    fn write_dimension_angular_3point(
        &mut self,
        dim: &DimensionAngular3Pt,
        owner: Handle,
    ) -> Result<()> {
        self.writer.write_entity_type("DIMENSION")?;
        self.write_dimension_base(&dim.base, dim.definition_point, 5, owner)?; // 3-point angular = 5
        self.writer.write_subclass("AcDb3PointAngularDimension")?;
        self.writer.write_point3d(13, dim.first_point)?;
        self.writer.write_point3d(14, dim.second_point)?;
        self.writer.write_point3d(15, dim.angle_vertex)?;
        Ok(())
    }

    fn write_dimension_ordinate(&mut self, dim: &DimensionOrdinate, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("DIMENSION")?;
        // Bit 0x40 marks the X datum; clear = Y. (0x80 is reserved for the
        // text-user-positioned flag and must not be reused here.)
        let type_flags = if dim.is_ordinate_type_x { 0x40 } else { 0 };
        let mut base = dim.base.clone();
        base.actual_measurement = dim.measurement();
        self.write_dimension_base(&base, dim.definition_point, 6 | type_flags, owner)?;
        self.writer.write_subclass("AcDbOrdinateDimension")?;
        self.writer.write_point3d(13, dim.feature_location)?;
        self.writer.write_point3d(14, dim.leader_endpoint)?;
        Ok(())
    }

    fn write_dimension_arc(&mut self, dim: &DimensionArc, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("ARC_DIMENSION")?;
        self.write_dimension_base(&dim.base, dim.definition_point, 8, owner)?;
        self.writer.write_subclass("AcDbArcDimension")?;
        self.writer.write_point3d(13, dim.first_extension_point)?;
        self.writer.write_point3d(14, dim.second_extension_point)?;
        self.writer.write_point3d(15, dim.center_point)?;
        self.writer.write_bool(70, dim.is_partial)?;
        self.writer.write_double(40, dim.arc_start_parameter)?;
        self.writer.write_double(41, dim.arc_end_parameter)?;
        self.writer.write_bool(71, dim.has_leader)?;
        self.writer.write_point3d(16, dim.first_leader_point)?;
        self.writer.write_point3d(17, dim.second_leader_point)?;
        Ok(())
    }

    fn write_dimension_large_radial(
        &mut self,
        dim: &DimensionLargeRadial,
        owner: Handle,
    ) -> Result<()> {
        self.writer.write_entity_type("LARGE_RADIAL_DIMENSION")?;
        self.write_dimension_base(&dim.base, dim.definition_point, 4, owner)?;
        self.writer.write_subclass("AcDbRadialDimensionLarge")?;
        self.writer.write_point3d(13, dim.jog_point)?;
        self.writer.write_point3d(14, dim.override_center)?;
        self.writer.write_point3d(15, dim.chord_point)?;
        self.writer.write_double(40, dim.jog_angle)?;
        Ok(())
    }

    /// Write HATCH entity
    fn write_hatch(&mut self, hatch: &Hatch, owner: Handle) -> Result<()> {
        if hatch.is_mpolygon {
            return self.write_mpolygon_dxf(hatch, owner);
        }
        self.writer.write_entity_type("HATCH")?;
        self.write_common_entity_data(&hatch.common, owner)?;
        self.writer.write_subclass("AcDbHatch")?;

        // Elevation point
        self.writer.write_double(10, 0.0)?;
        self.writer.write_double(20, 0.0)?;
        self.writer.write_double(30, hatch.elevation)?;

        // Normal vector
        self.writer.write_double(210, hatch.normal.x)?;
        self.writer.write_double(220, hatch.normal.y)?;
        self.writer.write_double(230, hatch.normal.z)?;

        // Pattern name
        self.writer.write_string(2, &hatch.pattern.name)?;

        // Solid fill flag
        self.writer
            .write_i16(70, if hatch.is_solid { 1 } else { 0 })?;

        // Associative flag — clear if boundary handles are all missing / invalid
        let effective_associative = hatch.is_associative
            && hatch.paths.iter().any(|p| {
                if self.valid_handles.is_empty() {
                    !p.boundary_handles.is_empty()
                } else {
                    p.boundary_handles
                        .iter()
                        .any(|h| *h != Handle::NULL && self.valid_handles.contains(h))
                }
            });
        self.writer
            .write_i16(71, if effective_associative { 1 } else { 0 })?;

        // Number of boundary paths
        self.writer.write_i32(91, hatch.paths.len() as i32)?;

        // Write boundary paths
        for path in &hatch.paths {
            self.write_hatch_boundary_path(path)?;
        }

        // Pattern style
        self.writer.write_i16(75, hatch.style as i16)?;
        self.writer.write_i16(76, hatch.pattern_type as i16)?;

        if !hatch.is_solid {
            self.writer
                .write_double(52, hatch.pattern_angle.to_dxf_degrees())?;
            self.writer.write_double(41, hatch.pattern_scale)?;
            self.writer
                .write_i16(77, if hatch.is_double { 1 } else { 0 })?;

            // Pattern definition lines
            self.writer
                .write_i16(78, hatch.pattern.lines.len() as i16)?;
            for line in &hatch.pattern.lines {
                self.writer.write_double(53, line.angle.to_dxf_degrees())?;
                self.writer.write_double(43, line.base_point.x)?;
                self.writer.write_double(44, line.base_point.y)?;
                self.writer.write_double(45, line.offset.x)?;
                self.writer.write_double(46, line.offset.y)?;
                self.writer.write_i16(79, line.dash_lengths.len() as i16)?;
                for dash in &line.dash_lengths {
                    self.writer.write_double(49, *dash)?;
                }
            }
        }

        // Pixel size: present exactly when a boundary path is derived (the
        // same rule as the DWG record); AutoCAD requires it before 98.
        if hatch.paths.iter().any(|path| path.flags.is_derived()) {
            self.writer.write_double(47, hatch.pixel_size)?;
        }

        // Seed points
        self.writer.write_i32(98, hatch.seed_points.len() as i32)?;
        for seed in &hatch.seed_points {
            self.writer.write_double(10, seed.x)?;
            self.writer.write_double(20, seed.y)?;
        }

        self.write_hatch_gradient(hatch)?;

        // XDATA (e.g. HATCHBACKGROUNDCOLOR) is emitted by write_entity_with_owner
        // after this returns — HATCH has no child records to precede.
        Ok(())
    }

    fn write_mpolygon_dxf(&mut self, hatch: &Hatch, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("MPOLYGON")?;
        self.write_common_entity_data(&hatch.common, owner)?;
        self.writer.write_subclass("AcDbMPolygon")?;
        self.writer.write_i16(75, hatch.style as i16)?;
        self.writer.write_double(10, 0.0)?;
        self.writer.write_double(20, 0.0)?;
        self.writer.write_double(30, hatch.elevation)?;
        self.writer.write_point3d(210, hatch.normal)?;
        self.writer.write_string(2, &hatch.pattern.name)?;
        self.writer
            .write_i16(70, if hatch.is_solid { 1 } else { 0 })?;
        let effective_associative = hatch.is_associative
            && hatch.paths.iter().any(|path| {
                path.boundary_handles
                    .iter()
                    .any(|handle| *handle != Handle::NULL)
            });
        self.writer
            .write_i16(71, if effective_associative { 1 } else { 0 })?;
        self.writer.write_i32(91, hatch.paths.len() as i32)?;
        for path in &hatch.paths {
            self.write_hatch_boundary_path(path)?;
        }
        self.writer.write_i16(75, hatch.style as i16)?;
        self.writer.write_i16(76, hatch.pattern_type as i16)?;
        if !hatch.is_solid {
            self.writer
                .write_double(52, hatch.pattern_angle.to_dxf_degrees())?;
            self.writer.write_double(41, hatch.pattern_scale)?;
            self.writer
                .write_i16(77, if hatch.is_double { 1 } else { 0 })?;
            self.writer
                .write_i16(78, hatch.pattern.lines.len() as i16)?;
            for line in &hatch.pattern.lines {
                self.writer.write_double(53, line.angle.to_dxf_degrees())?;
                self.writer.write_double(43, line.base_point.x)?;
                self.writer.write_double(44, line.base_point.y)?;
                self.writer.write_double(45, line.offset.x)?;
                self.writer.write_double(46, line.offset.y)?;
                self.writer.write_i16(79, line.dash_lengths.len() as i16)?;
                for dash in &line.dash_lengths {
                    self.writer.write_double(49, *dash)?;
                }
            }
        }
        self.writer.write_color(62, hatch.mpolygon_hatch_color)?;
        self.writer.write_double(11, hatch.mpolygon_x_direction.x)?;
        self.writer.write_double(21, hatch.mpolygon_x_direction.y)?;
        // Invalid loops are not written to DXF.
        self.writer.write_i32(99, 0)?;
        self.write_hatch_gradient(hatch)?;
        Ok(())
    }

    fn write_hatch_gradient(&mut self, hatch: &Hatch) -> Result<()> {
        // A pattern hatch of an R2004+ drawing still carries its (disabled) gradient definition, which AutoCAD
        // writes with group 450 = 0; it is kept when the model has one.
        let gradient = &hatch.gradient_color;
        let stored_definition =
            self.dxf_version >= DxfVersion::AC1018 && (!gradient.colors.is_empty() || !gradient.name.is_empty());
        if !gradient.enabled && !stored_definition {
            return Ok(());
        }
        // A gradient the reader restored from the round-trip data of a pre-R2004 save is carried by that data
        // (written back with the entity XDATA); the native groups belong to R2004 and later.
        if self.dxf_version < DxfVersion::AC1018 && hatch.gradient_recovery_source.is_some() {
            return Ok(());
        }
        self.writer.write_i32(450, i32::from(gradient.enabled))?;
        self.writer.write_i32(451, gradient.reserved)?;
        self.writer.write_double(460, gradient.angle)?;
        self.writer.write_double(461, gradient.shift)?;
        self.writer
            .write_i32(452, if gradient.is_single_color { 1 } else { 0 })?;
        self.writer.write_double(462, gradient.color_tint)?;
        self.writer.write_i32(453, gradient.colors.len() as i32)?;
        for entry in &gradient.colors {
            self.writer.write_double(463, entry.value)?;
            self.writer.write_color(63, entry.color)?;
            if let Color::Rgb { r, g, b } = entry.color {
                let rgb = ((r as i32) << 16) | ((g as i32) << 8) | b as i32;
                self.writer.write_i32(421, rgb)?;
            }
        }
        self.writer.write_string(470, &gradient.name)?;
        Ok(())
    }

    fn write_hatch_boundary_path(&mut self, path: &BoundaryPath) -> Result<()> {
        self.writer
            .write_i32(92, get_boundary_path_bits(&path.flags) as i32)?;

        if !path.flags.is_polyline() {
            self.writer.write_i32(93, path.edges.len() as i32)?;
        }

        for edge in &path.edges {
            self.write_hatch_edge(edge)?;
        }

        // Associated entities (boundary handles)
        // Filter to only valid handles if valid_handles is populated
        let valid_boundary_handles: Vec<Handle> = if self.valid_handles.is_empty() {
            path.boundary_handles.clone()
        } else {
            path.boundary_handles
                .iter()
                .copied()
                .filter(|h| *h != Handle::NULL && self.valid_handles.contains(h))
                .collect()
        };
        self.writer
            .write_i32(97, valid_boundary_handles.len() as i32)?;
        for h in &valid_boundary_handles {
            self.writer.write_handle(330, *h)?;
        }

        Ok(())
    }

    fn write_hatch_edge(&mut self, edge: &BoundaryEdge) -> Result<()> {
        match edge {
            BoundaryEdge::Line(line_edge) => {
                self.writer.write_i16(72, 1)?; // Line type
                self.writer.write_double(10, line_edge.start.x)?;
                self.writer.write_double(20, line_edge.start.y)?;
                self.writer.write_double(11, line_edge.end.x)?;
                self.writer.write_double(21, line_edge.end.y)?;
            }
            BoundaryEdge::CircularArc(arc) => {
                self.writer.write_i16(72, 2)?; // Arc type
                self.writer.write_double(10, arc.center.x)?;
                self.writer.write_double(20, arc.center.y)?;
                self.writer.write_double(40, arc.radius)?;
                self.writer.write_double(50, arc.start_angle.to_dxf_degrees())?;
                self.writer.write_double(51, arc.end_angle.to_dxf_degrees())?;
                self.writer
                    .write_i16(73, if arc.counter_clockwise { 1 } else { 0 })?;
            }
            BoundaryEdge::EllipticArc(ellipse) => {
                self.writer.write_i16(72, 3)?; // Ellipse type
                self.writer.write_double(10, ellipse.center.x)?;
                self.writer.write_double(20, ellipse.center.y)?;
                self.writer
                    .write_double(11, ellipse.major_axis_endpoint.x)?;
                self.writer
                    .write_double(21, ellipse.major_axis_endpoint.y)?;
                self.writer.write_double(40, ellipse.minor_axis_ratio)?;
                self.writer.write_double(50, ellipse.start_angle.to_dxf_degrees())?;
                self.writer.write_double(51, ellipse.end_angle.to_dxf_degrees())?;
                self.writer
                    .write_i16(73, if ellipse.counter_clockwise { 1 } else { 0 })?;
            }
            BoundaryEdge::Spline(spline) => {
                self.writer.write_i16(72, 4)?; // Spline type
                // AutoCAD requires the degree (94) before the flags.
                self.writer.write_i32(94, spline.degree)?;
                self.writer
                    .write_i16(73, if spline.rational { 1 } else { 0 })?;
                self.writer
                    .write_i16(74, if spline.periodic { 1 } else { 0 })?;
                self.writer.write_i32(95, spline.knots.len() as i32)?;
                self.writer
                    .write_i32(96, spline.control_points.len() as i32)?;
                for knot in &spline.knots {
                    self.writer.write_double(40, *knot)?;
                }
                for point in &spline.control_points {
                    self.writer.write_double(10, point.x)?;
                    self.writer.write_double(20, point.y)?;
                    if spline.rational {
                        self.writer.write_double(42, point.z)?; // z stores weight
                    }
                }
                // Fit data (R2010+)
                if self.dxf_version >= DxfVersion::AC1024 {
                    self.writer.write_i32(97, spline.fit_points.len() as i32)?;
                    for fp in &spline.fit_points {
                        self.writer.write_double(11, fp.x)?;
                        self.writer.write_double(21, fp.y)?;
                    }
                    if !spline.fit_points.is_empty() {
                        self.writer.write_double(12, spline.start_tangent.x)?;
                        self.writer.write_double(22, spline.start_tangent.y)?;
                        self.writer.write_double(13, spline.end_tangent.x)?;
                        self.writer.write_double(23, spline.end_tangent.y)?;
                    }
                }
            }
            BoundaryEdge::Polyline(poly) => {
                let has_bulge = poly.has_bulge();
                self.writer.write_i16(72, if has_bulge { 1 } else { 0 })?;
                self.writer
                    .write_i16(73, if poly.is_closed { 1 } else { 0 })?;
                self.writer.write_i32(93, poly.vertices.len() as i32)?;
                for vertex in &poly.vertices {
                    self.writer.write_double(10, vertex.x)?;
                    self.writer.write_double(20, vertex.y)?;
                    if has_bulge {
                        self.writer.write_double(42, vertex.z)?; // z stores bulge
                    }
                }
            }
        }
        Ok(())
    }

    /// Write SOLID entity
    fn write_solid(&mut self, solid: &Solid, owner: Handle) -> Result<()> {
        self.writer
            .write_entity_type(if solid.is_trace { "TRACE" } else { "SOLID" })?;
        self.write_common_entity_data(&solid.common, owner)?;
        self.writer.write_subclass("AcDbTrace")?;
        self.writer.write_point3d(10, solid.first_corner)?;
        self.writer.write_point3d(11, solid.second_corner)?;
        self.writer.write_point3d(12, solid.third_corner)?;
        self.writer.write_point3d(13, solid.fourth_corner)?;
        if solid.thickness != 0.0 {
            self.writer.write_double(39, solid.thickness)?;
        }
        self.write_normal(solid.normal)?;
        Ok(())
    }

    /// Write 3DFACE entity
    fn write_face3d(&mut self, face: &Face3D, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("3DFACE")?;
        self.write_common_entity_data(&face.common, owner)?;
        self.writer.write_subclass("AcDbFace")?;
        self.writer.write_point3d(10, face.first_corner)?;
        self.writer.write_point3d(11, face.second_corner)?;
        self.writer.write_point3d(12, face.third_corner)?;
        self.writer.write_point3d(13, face.fourth_corner)?;
        if face.invisible_edges != InvisibleEdgeFlags::NONE {
            let edge_bits = get_invisible_edge_bits(&face.invisible_edges);
            self.writer.write_i16(70, edge_bits as i16)?;
        }
        Ok(())
    }

    /// Write INSERT entity
    fn write_insert(&mut self, insert: &Insert, owner: Handle) -> Result<()> {
        let has_sequence = insert.has_attributes()
            || insert.seqend.is_some()
            || insert.seqend_handle.is_some();
        let is_view_rep = insert.view_rep_handle.is_some();
        self.writer.write_entity_type(if is_view_rep {
            "ACDBVIEWREPBLOCKREFERENCE"
        } else {
            "INSERT"
        })?;
        self.write_common_entity_data(&insert.common, owner)?;
        self.writer.write_subclass(insert.subclass_marker())?;
        // Has-attributes flag (group code 66)
        if has_sequence {
            self.writer.write_i16(66, 1)?;
        }
        self.writer.write_string(2, &insert.block_name)?;
        self.writer.write_point3d(10, insert.insert_point)?;
        if insert.x_scale() != 1.0 {
            self.writer.write_double(41, insert.x_scale())?;
        }
        if insert.y_scale() != 1.0 {
            self.writer.write_double(42, insert.y_scale())?;
        }
        if insert.z_scale() != 1.0 {
            self.writer.write_double(43, insert.z_scale())?;
        }
        if insert.rotation != 0.0 {
            self.writer.write_double(50, insert.rotation.to_dxf_degrees())?;
        }
        if insert.column_count > 1 {
            self.writer.write_i16(70, insert.column_count as i16)?;
        }
        if insert.row_count > 1 {
            self.writer.write_i16(71, insert.row_count as i16)?;
        }
        if insert.column_spacing != 0.0 {
            self.writer.write_double(44, insert.column_spacing)?;
        }
        if insert.row_spacing != 0.0 {
            self.writer.write_double(45, insert.row_spacing)?;
        }
        self.write_normal(insert.normal)?;
        if let Some(view_rep_handle) = insert.view_rep_handle {
            self.writer.write_subclass("AcDbViewRepBlockReference")?;
            self.writer.write_handle(330, view_rep_handle)?;
        }

        // XDATA precedes the child ATTRIB/SEQEND records.
        self.write_xdata(&insert.common.extended_data)?;

        // Write child ATTRIB entities + SEQEND when the source had an
        // attribute sequence. A retained empty sequence is still meaningful.
        if has_sequence {
            let insert_handle = insert.handle();
            for att in &insert.attributes {
                if att.common.handle.is_null() {
                    let mut child = att.clone();
                    child.common.handle = self.allocate_handle();
                    self.write_attrib(&child, insert_handle)?;
                } else {
                    self.write_attrib(att, insert_handle)?;
                }
            }
            // SEQEND terminates the attribute sequence
            let seqend_handle = insert
                .seqend
                .as_ref()
                .map(|seqend| seqend.common.handle)
                .filter(|handle| !handle.is_null())
                .or_else(|| insert.seqend_handle.filter(|handle| !handle.is_null()))
                .unwrap_or_else(|| self.allocate_handle());
            self.write_owned_seqend(
                insert.seqend.as_deref(),
                &insert.common,
                seqend_handle,
                insert_handle,
            )?;
        }
        Ok(())
    }

    /// Write BLOCK entity
    fn write_block_entity(&mut self, block: &Block, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("BLOCK")?;
        self.write_common_entity_data(&block.common, owner)?;
        self.writer.write_subclass("AcDbBlockBegin")?;
        self.writer.write_string(2, &block.name)?;
        self.writer.write_i16(70, 0)?; // Block flags
        self.writer.write_point3d(10, block.base_point)?;
        self.writer.write_string(3, &block.name)?;
        if !block.xref_path.is_empty() {
            self.writer.write_string(1, &block.xref_path)?;
        }
        if !block.description.is_empty() {
            self.writer.write_string(4, &block.description)?;
        }
        Ok(())
    }

    /// Write ENDBLK entity
    fn write_block_end(&mut self, block_end: &BlockEnd, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("ENDBLK")?;
        self.write_common_entity_data(&block_end.common, owner)?;
        self.writer.write_subclass("AcDbBlockEnd")?;
        Ok(())
    }

    /// Write RAY entity
    fn write_ray(&mut self, ray: &Ray, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("RAY")?;
        self.write_common_entity_data(&ray.common, owner)?;
        self.writer.write_subclass("AcDbRay")?;
        self.writer.write_point3d(10, ray.base_point)?;
        self.writer.write_point3d(11, ray.direction)?;
        Ok(())
    }

    /// Write XLINE entity
    fn write_xline(&mut self, xline: &XLine, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("XLINE")?;
        self.write_common_entity_data(&xline.common, owner)?;
        self.writer.write_subclass("AcDbXline")?;
        self.writer.write_point3d(10, xline.base_point)?;
        self.writer.write_point3d(11, xline.direction)?;
        Ok(())
    }

    /// Write POLYLINE (3D) entity
    fn write_polyline3d(&mut self, polyline: &Polyline3D, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("POLYLINE")?;
        self.write_common_entity_data(&polyline.common, owner)?;
        self.writer.write_subclass("AcDb3dPolyline")?;

        // Entities follow flag (VERTEX records follow)
        self.writer.write_i16(66, 1)?;

        // Dummy point with elevation (reference pattern)
        self.writer.write_double(10, 0.0)?;
        self.writer.write_double(20, 0.0)?;
        self.writer.write_double(30, polyline.elevation)?;
        if polyline.thickness != 0.0 {
            self.writer.write_double(39, polyline.thickness)?;
        }

        // Polyline flags (bit 8 = 3D polyline)
        self.writer.write_i16(70, polyline.flags.to_bits() as i16)?;
        // Curve type of a spline-fit polyline (5 quadratic, 6 cubic).
        if polyline.smooth_type != crate::entities::polyline3d::SmoothSurfaceType::None {
            self.writer.write_i16(75, polyline.smooth_type.to_value())?;
        }

        // XDATA precedes the child VERTEX/SEQEND records.
        self.write_xdata(&polyline.common.extended_data)?;

        // Write vertices with proper subclass markers
        let polyline_handle = polyline.handle();
        for vertex in polyline.vertices.iter() {
            let mut common = vertex.common.clone();
            let vertex_handle = if common.handle.is_null() {
                Self::inherit_compound_child_defaults(&mut common, &polyline.common);
                self.allocate_handle()
            } else {
                common.handle
            };
            common.handle = vertex_handle;
            common.owner_handle = polyline_handle;
            self.writer.write_entity_type("VERTEX")?;
            self.write_common_entity_data(&common, polyline_handle)?;
            self.writer.write_subclass("AcDbVertex")?;
            self.writer.write_subclass("AcDb3dPolylineVertex")?;
            self.writer.write_point3d(10, vertex.position)?;
            self.writer.write_i16(70, vertex.flags as i16)?;
            self.write_xdata(&common.extended_data)?;
        }

        // SEQEND
        let seqend_handle = polyline
            .seqend
            .as_ref()
            .map(|seqend| seqend.common.handle)
            .filter(|handle| !handle.is_null())
            .unwrap_or_else(|| self.allocate_handle());
        self.write_owned_seqend(
            polyline.seqend.as_deref(),
            &polyline.common,
            seqend_handle,
            polyline_handle,
        )?;

        Ok(())
    }

    /// Write VIEWPORT entity
    fn write_viewport(&mut self, viewport: &Viewport, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("VIEWPORT")?;
        self.write_common_entity_data(&viewport.common, owner)?;
        self.writer.write_subclass("AcDbViewport")?;

        // Center point
        self.writer.write_point3d(10, viewport.center)?;

        // Width and height
        self.writer.write_double(40, viewport.width)?;
        self.writer.write_double(41, viewport.height)?;

        // Status / stacking order (68) and viewport ID (69). Every paper-space
        // layout needs IDs: an on-screen viewport with ID 0 would emit 68=0
        // and be interpreted as switched off.
        let id = if viewport.id != 0 {
            viewport.id
        } else {
            self.paper_viewport_ids
                .get(&viewport.common.handle)
                .copied()
                .unwrap_or(0)
        };
        let status = viewport.dxf_status.unwrap_or_else(|| {
            match (viewport.is_on(), viewport.off_screen) {
                (false, _) => 0,
                (true, true) => -1,
                (true, false) => id,
            }
        });
        self.writer.write_i16(68, status)?;
        self.writer.write_i16(69, id)?;
        if self.r12 {
            // The rest of the view is the MVIEW record of the extended data.
            return Ok(());
        }

        // Status
        let grid_word = i32::from(viewport.grid_flags.to_bits()) << 18;
        // Before R2007 the grid bits are not part of the format; a source word that carried them (and still
        // agrees with the grid flags) is written as it was.
        let source_has_grid = viewport
            .source
            .as_ref()
            .and_then(|source| source.status_flags)
            .is_some_and(|source| source & (0xFF << 18) == grid_word);
        let grid_bits = if self.dxf_version >= DxfVersion::AC1021 || source_has_grid {
            grid_word
        } else {
            0
        };
        self.writer
            .write_i32(90, viewport.status.to_bits() | grid_bits)?;

        // View center
        self.writer.write_double(12, viewport.view_center.x)?;
        self.writer.write_double(22, viewport.view_center.y)?;

        // Snap base
        self.writer.write_double(13, viewport.snap_base.x)?;
        self.writer.write_double(23, viewport.snap_base.y)?;

        // Snap spacing
        self.writer.write_double(14, viewport.snap_spacing.x)?;
        self.writer.write_double(24, viewport.snap_spacing.y)?;

        // Grid spacing
        self.writer.write_double(15, viewport.grid_spacing.x)?;
        self.writer.write_double(25, viewport.grid_spacing.y)?;

        // View direction
        self.writer.write_double(16, viewport.view_direction.x)?;
        self.writer.write_double(26, viewport.view_direction.y)?;
        self.writer.write_double(36, viewport.view_direction.z)?;

        // View target
        self.writer.write_double(17, viewport.view_target.x)?;
        self.writer.write_double(27, viewport.view_target.y)?;
        self.writer.write_double(37, viewport.view_target.z)?;

        // Lens length
        self.writer.write_double(42, viewport.lens_length)?;

        // Front and back clipping
        self.writer.write_double(43, viewport.front_clip_z)?;
        self.writer.write_double(44, viewport.back_clip_z)?;

        // View height
        self.writer.write_double(45, viewport.view_height)?;

        // Snap and twist angles
        self.writer.write_double(50, viewport.snap_angle.to_dxf_degrees())?;
        self.writer.write_double(51, viewport.twist_angle.to_dxf_degrees())?;

        // Circle sides
        self.writer.write_i16(72, viewport.circle_sides)?;

        // Frozen layers (code 331)
        for frozen_layer in &viewport.frozen_layers {
            if !frozen_layer.is_null() {
                self.writer.write_handle(331, *frozen_layer)?;
            }
        }
        // Boundary presence and clipping activation are independent.
        if !viewport.clip_boundary_handle.is_null() {
            self.writer.write_handle(340, viewport.clip_boundary_handle)?;
        }

        // Render mode
        self.writer
            .write_byte(281, viewport.render_mode.to_value() as u8)?;

        // UCS per viewport
        if viewport.ucs_per_viewport {
            self.writer.write_i16(71, 1)?;
        }

        // UCS origin, axes
        if viewport.ucs_origin != Vector3::ZERO {
            self.writer.write_double(110, viewport.ucs_origin.x)?;
            self.writer.write_double(120, viewport.ucs_origin.y)?;
            self.writer.write_double(130, viewport.ucs_origin.z)?;
        }
        if viewport.ucs_x_axis != Vector3::ZERO {
            self.writer.write_double(111, viewport.ucs_x_axis.x)?;
            self.writer.write_double(121, viewport.ucs_x_axis.y)?;
            self.writer.write_double(131, viewport.ucs_x_axis.z)?;
        }
        if viewport.ucs_y_axis != Vector3::ZERO {
            self.writer.write_double(112, viewport.ucs_y_axis.x)?;
            self.writer.write_double(122, viewport.ucs_y_axis.y)?;
            self.writer.write_double(132, viewport.ucs_y_axis.z)?;
        }

        // Elevation
        if viewport.elevation != 0.0 {
            self.writer.write_double(146, viewport.elevation)?;
        }

        // Grid major (R2007+); a DXF source without the group, or a DWG older than R2007 (no such field in its
        // record), keeps it out unless the value was changed.
        let grid_major_absent = viewport
            .source
            .as_ref()
            .is_some_and(|source| source.dxf_codes.is_some() && !source.contains(61))
            || self.dwg_source.is_some_and(|version| version < DxfVersion::AC1021);
        if viewport.grid_major != 0 && !(grid_major_absent && viewport.grid_major == Viewport::new().grid_major) {
            self.writer.write_i16(61, viewport.grid_major)?;
        }

        self.write_viewport_extra_groups(viewport)?;

        // R2004+: per-viewport ShadePlot mode, independent of render mode.
        if self.dxf_version >= DxfVersion::AC1018 {
            self.writer.write_i16(170, viewport.shade_plot_mode)?;
        }

        Ok(())
    }

    /// The VIEWPORT groups beyond the geometry: sheet style, UCS references, plot/background/visual style and
    /// sun objects, lighting and the ambient colour. Each is written when the DXF source had it or the value
    /// differs from a new viewport's, so a source without the group never gains one.
    fn write_viewport_extra_groups(&mut self, viewport: &Viewport) -> Result<()> {
        let source = viewport.source.as_deref();
        let has = |code: usize| source.is_some_and(|source| source.contains(code));
        let defaults = Viewport::new();
        let valid = |this: &Self, handle: Handle| {
            !handle.is_null() && (this.valid_handles.is_empty() || this.valid_handles.contains(&handle))
        };
        let dwg_record = self.stated_by_dwg(DxfVersion::AC1015)
            && viewport.source.as_deref().map_or(true, |source| source.dxf_codes.is_none());
        let dwg_lighting = dwg_record && self.stated_by_dwg(DxfVersion::AC1021);
        if self.dxf_version >= DxfVersion::AC1015 && (!viewport.style_sheet.is_empty() || has(1) || dwg_record) {
            self.writer.write_string(1, &viewport.style_sheet)?;
        }
        if self.dxf_version >= DxfVersion::AC1015 && (viewport.ucs_at_origin || has(74)) {
            self.writer.write_i16(74, i16::from(viewport.ucs_at_origin))?;
        }
        if self.dxf_version >= DxfVersion::AC1015 && (viewport.ucs_ortho_type != 0 || has(79)) {
            self.writer.write_i16(79, viewport.ucs_ortho_type)?;
        }
        for (code, handle) in [
            (345, viewport.ucs_handle),
            (346, viewport.base_ucs_handle),
            (332, viewport.background_handle),
            (333, viewport.shade_plot_handle),
            (348, viewport.visual_style_handle),
            (361, viewport.sun_handle),
        ] {
            if self.dxf_version >= DxfVersion::AC1015 && valid(self, handle) {
                self.writer.write_handle(code, handle)?;
            }
        }
        if self.dxf_version >= DxfVersion::AC1021 {
            if viewport.brightness != defaults.brightness || has(141) {
                self.writer.write_double(141, viewport.brightness)?;
            }
            if viewport.contrast != defaults.contrast || has(142) {
                self.writer.write_double(142, viewport.contrast)?;
            }
            if viewport.ambient_color != defaults.ambient_color || has(63) || has(421) {
                self.writer.write_color(63, viewport.ambient_color)?;
                if let Some(true_color) = viewport.ambient_color.to_true_color_value() {
                    self.writer.write_i32(421, true_color)?;
                }
            }
            if viewport.default_lighting != defaults.default_lighting || has(292) || dwg_lighting {
                self.writer.write_bool(292, viewport.default_lighting)?;
            }
            if viewport.default_lighting_type != defaults.default_lighting_type || has(282) || dwg_lighting {
                self.writer.write_i16(282, viewport.default_lighting_type)?;
            }
        }
        Ok(())
    }

    /// Write ATTDEF entity
    fn write_attdef(&mut self, attdef: &AttributeDefinition, owner: Handle) -> Result<()> {
        let multiline =
            self.attribute_is_multiline(attdef.is_multiline, attdef.embedded_mtext.as_deref());
        self.writer.write_entity_type("ATTDEF")?;
        self.write_common_entity_data(&attdef.common, owner)?;
        self.writer.write_subclass("AcDbText")?;
        if attdef.thickness != 0.0 {
            self.writer.write_double(39, attdef.thickness)?;
        }

        // Insertion point
        self.writer.write_point3d(10, attdef.insertion_point)?;

        // Text height
        self.writer.write_double(40, attdef.height)?;

        // Default value (a multiline one lives in the embedded MTEXT)
        self.write_attribute_text_value(attribute_own_value(
            &attdef.default_value,
            attdef.embedded_mtext.as_deref().filter(|_| multiline),
            attdef.source.as_deref(),
        ))?;

        // Rotation
        self.writer.write_double(50, attdef.rotation.to_dxf_degrees())?;

        // Width factor
        self.writer.write_double(41, attdef.width_factor)?;

        // Oblique angle
        self.writer
            .write_double(51, attdef.oblique_angle.to_dxf_degrees())?;

        // Text style
        self.writer.write_string(7, &attdef.text_style)?;

        // Text generation flags: omitted when zero, as for TEXT. The reference reader maps group 71 to the
        // attribute type, so a written zero would replace its single-line default.
        if attdef.text_generation_flags != 0 {
            self.writer.write_i16(71, attdef.text_generation_flags)?;
        }

        // Horizontal alignment
        self.writer
            .write_i16(72, attdef.horizontal_alignment.to_value())?;

        // Alignment point (base code 11 → writes 11, 21, 31)
        self.writer.write_point3d(11, attdef.alignment_point)?;

        // Normal
        self.writer.write_point3d(210, attdef.normal)?;

        self.writer.write_subclass("AcDbAttributeDefinition")?;

        // R2010+ version byte; the prompt precedes the tag.
        if self.dxf_version >= DxfVersion::AC1024 {
            self.writer.write_byte(280, attdef.version)?;
        }

        // Prompt
        self.writer.write_string(3, &attdef.prompt)?;

        // Tag
        self.writer.write_string(
            2,
            attdef.source.as_deref().map_or(&attdef.tag, |source| source_tag(source, &attdef.tag)),
        )?;

        // Attribute flags
        self.writer.write_i16(70, attdef.flags.to_bits() as i16)?;

        // Field length
        self.writer.write_i16(73, attdef.field_length)?;

        // Vertical alignment
        self.writer
            .write_i16(74, attdef.vertical_alignment.to_value())?;

        // R2007+ lock-position flag
        if self.dxf_version >= DxfVersion::AC1021 {
            self.writer.write_byte(280, u8::from(attdef.lock_position))?;
        }

        // Multiline definitions carry MTEXT flag 4 (constant or not).
        if multiline {
            self.write_attribute_mtext(
                4,
                attdef.alignment_point,
                attdef.embedded_mtext.as_deref(),
                &attdef.default_value,
                attdef.insertion_point,
                attdef.normal,
                attdef.rotation,
                attdef.height,
                &attdef.text_style,
            )?;
        }

        Ok(())
    }

    fn attribute_is_multiline(&self, is_multiline: bool, embedded: Option<&MText>) -> bool {
        self.dxf_version >= DxfVersion::AC1032 && (is_multiline || embedded.is_some())
    }

    #[allow(clippy::too_many_arguments)]
    fn write_attribute_mtext(
        &mut self,
        mtext_flag: i16,
        alignment_point: Vector3,
        embedded: Option<&MText>,
        value: &str,
        insertion_point: Vector3,
        normal: Vector3,
        rotation: f64,
        height: f64,
        style: &str,
    ) -> Result<()> {
        let mut fallback = MText::with_value(value, insertion_point);
        fallback.normal = normal;
        fallback.rotation = rotation;
        fallback.height = height;
        fallback.style = style.to_string();
        let mtext = embedded.unwrap_or(&fallback);

        self.writer.write_i16(71, mtext_flag)?;
        self.writer.write_i16(72, 0)?;
        self.writer.write_point3d(11, alignment_point)?;
        self.writer.write_string(101, "Embedded Object")?;
        self.writer.write_point3d(10, mtext.insertion_point)?;
        self.writer.write_double(40, mtext.height)?;
        self.writer.write_double(41, mtext.rectangle_width)?;
        self.writer
            .write_double(46, mtext.rectangle_height.unwrap_or(0.0))?;
        self.writer.write_i16(71, mtext.attachment_point as i16)?;
        self.writer.write_i16(72, mtext.drawing_direction as i16)?;
        self.write_mtext_value(&mtext.value, Self::mtext_lexical_value(mtext))?;
        self.writer.write_string(7, &mtext.style)?;
        // Only what differs from an unrotated text in the XY plane: the
        // reference application drops an attribute whose embedded object
        // carries the default normal, direction and extents.
        if mtext.normal != Vector3::new(0.0, 0.0, 1.0) {
            self.writer.write_point3d(210, mtext.normal)?;
        }
        if mtext.rotation != 0.0 {
            let x_direction = mtext
                .dwg_x_direction
                .filter(|direction| direction.y.atan2(direction.x) == mtext.rotation)
                .unwrap_or_else(|| Vector3::new(mtext.rotation.cos(), mtext.rotation.sin(), 0.0));
            self.writer.write_point3d(11, x_direction)?;
        }
        if mtext.extents_width > 0.0 {
            self.writer.write_double(42, mtext.extents_width)?;
        }
        if mtext.extents_height > 0.0 {
            self.writer.write_double(43, mtext.extents_height)?;
        }
        if mtext.rotation != 0.0 {
            self.writer.write_double(50, mtext.rotation.to_dxf_degrees())?;
        }
        self.writer.write_i16(73, mtext.line_spacing_style as i16)?;
        self.writer.write_double(44, mtext.line_spacing_factor)?;
        Ok(())
    }

    /// Group 1 of an ATTRIB/ATTDEF is the record's own text value. A multiline attribute keeps its text in the
    /// embedded MTEXT, and AutoCAD writes group 1 empty then: the MTEXT text is not copied here.
    fn write_attribute_text_value(&mut self, value: &str) -> Result<()> {
        // DXF is line based: a line break in the value would end the group.
        if value.contains(['\n', '\r']) {
            let value = value.replace("\r\n", "\\P").replace(['\r', '\n'], "\\P");
            return self.writer.write_string(1, &value);
        }
        self.writer.write_string(1, value)
    }

    /// Write ATTRIB entity
    fn write_attrib(&mut self, attrib: &AttributeEntity, owner: Handle) -> Result<()> {
        let multiline =
            self.attribute_is_multiline(attrib.is_multiline, attrib.embedded_mtext.as_deref());
        self.writer.write_entity_type("ATTRIB")?;
        self.write_common_entity_data(&attrib.common, owner)?;
        self.writer.write_subclass("AcDbText")?;
        if attrib.thickness != 0.0 {
            self.writer.write_double(39, attrib.thickness)?;
        }

        // Insertion point
        self.writer.write_point3d(10, attrib.insertion_point)?;

        // Text height
        self.writer.write_double(40, attrib.height)?;

        // Value (a multiline one lives in the embedded MTEXT)
        self.write_attribute_text_value(attribute_own_value(
            &attrib.value,
            attrib.embedded_mtext.as_deref().filter(|_| multiline),
            attrib.source.as_deref(),
        ))?;

        // Rotation
        self.writer.write_double(50, attrib.rotation.to_dxf_degrees())?;

        // Width factor
        self.writer.write_double(41, attrib.width_factor)?;

        // Oblique angle
        self.writer
            .write_double(51, attrib.oblique_angle.to_dxf_degrees())?;

        // Text style
        self.writer.write_string(7, &attrib.text_style)?;

        // Text generation flags
        if attrib.text_generation_flags != 0 {
            self.writer.write_i16(71, attrib.text_generation_flags)?;
        }

        // Horizontal alignment
        self.writer
            .write_i16(72, attrib.horizontal_alignment.to_value())?;

        // Alignment point (base code 11 → writes 11, 21, 31)
        self.writer.write_point3d(11, attrib.alignment_point)?;

        // Normal
        self.writer.write_point3d(210, attrib.normal)?;

        self.writer.write_subclass("AcDbAttribute")?;

        // R2010+ version byte
        if self.dxf_version >= DxfVersion::AC1024 {
            self.writer.write_byte(280, attrib.version)?;
        }

        // Tag
        self.writer.write_string(
            2,
            attrib.source.as_deref().map_or(&attrib.tag, |source| source_tag(source, &attrib.tag)),
        )?;

        // Attribute flags
        self.writer.write_i16(70, attrib.flags.to_bits() as i16)?;

        // Field length
        self.writer.write_i16(73, attrib.field_length)?;

        // Vertical alignment
        self.writer
            .write_i16(74, attrib.vertical_alignment.to_value())?;

        // R2007+ lock-position flag
        if self.dxf_version >= DxfVersion::AC1021 {
            self.writer.write_byte(280, u8::from(attrib.lock_position))?;
        }

        // Multiline attributes carry MTEXT flag 2.
        if multiline {
            self.write_attribute_mtext(
                2,
                attrib.alignment_point,
                attrib.embedded_mtext.as_deref(),
                &attrib.value,
                attrib.insertion_point,
                attrib.normal,
                attrib.rotation,
                attrib.height,
                &attrib.text_style,
            )?;
        }

        // XDATA precedes the parent INSERT's child SEQEND record.
        self.write_xdata(&attrib.common.extended_data)?;

        Ok(())
    }

    /// Write LEADER entity
    fn write_leader(&mut self, leader: &Leader, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("LEADER")?;
        self.write_common_entity_data(&leader.common, owner)?;
        self.writer.write_subclass("AcDbLeader")?;

        // Dimension style
        self.writer.write_string(3, &leader.dimension_style)?;

        // A source that left a group out keeps it out (the reference reader then uses its own default) unless
        // the value was changed from the native default.
        let defaults = Leader::new();
        let source = &leader.dxf_source;
        let keep = |code: usize, at_default: bool| {
            !source.is_dxf() || source.contains(code) || !at_default
        };

        // Arrow head flag
        if keep(71, leader.arrow_enabled == defaults.arrow_enabled) {
            self.writer
                .write_i16(71, if leader.arrow_enabled { 1 } else { 0 })?;
        }

        // Path type
        self.writer.write_i16(72, leader.path_type.to_value())?;

        // Creation type
        if keep(73, leader.creation_type == defaults.creation_type) {
            self.writer.write_i16(73, leader.creation_type.to_value())?;
        }

        // Hookline direction
        self.writer
            .write_i16(74, leader.hookline_direction.to_value())?;

        // Hookline flag
        self.writer
            .write_i16(75, if leader.hookline_enabled { 1 } else { 0 })?;

        // Text height
        // An R2010+ DWG leader stores no box size; the native 2.5 is not a stated value.
        let no_box = !source.is_dxf() && self.dwg_source.is_some_and(|version| version >= DxfVersion::AC1024);
        if !no_box && keep(40, leader.text_height == defaults.text_height) {
            self.writer.write_double(40, leader.text_height)?;
        }

        // Text width
        if !no_box && keep(41, leader.text_width == defaults.text_width) {
            self.writer.write_double(41, leader.text_width)?;
        }

        // Number of vertices
        self.writer.write_i16(76, leader.vertices.len() as i16)?;

        // Vertices
        for vertex in &leader.vertices {
            self.writer.write_point3d(10, *vertex)?;
        }

        // Normal
        self.writer.write_point3d(210, leader.normal)?;

        // Horizontal direction
        self.writer
            .write_point3d(211, leader.horizontal_direction)?;

        // Block offset
        self.writer.write_point3d(212, leader.block_offset)?;

        // Annotation offset
        self.writer.write_point3d(213, leader.annotation_offset)?;

        // Associated annotation (text, tolerance or block reference)
        if !leader.annotation_handle.is_null() {
            self.writer.write_handle(340, leader.annotation_handle)?;
        }

        // Colour used when the leader's DIMCLRD is BYBLOCK
        if let Some(idx) = leader.override_color.index() {
            self.writer.write_i16(77, idx as i16)?;
        }

        Ok(())
    }

    fn write_field_cell_value_dxf(&mut self, value: &CellValue) -> Result<()> {
        self.write_cell_value_dxf_masked(value, 1, true)
    }

    fn write_cell_value_dxf_masked(
        &mut self,
        value: &CellValue,
        body_mask: i32,
        point_size: bool,
    ) -> Result<()> {
        if self.dxf_version >= DxfVersion::AC1021 {
            self.writer.write_i32(93, value.flags)?;
        }
        let type_code = if self.dxf_version >= DxfVersion::AC1021 {
            value.type_code()
        } else {
            value.type_code() & !0x200
        };
        self.writer.write_i32(90, type_code)?;
        if self.dxf_version < DxfVersion::AC1021 || (value.flags & body_mask) == 0 {
            match type_code {
                0 | 1 => {
                    self.writer.write_i32(91, value.numeric_value as i32)?;
                }
                2 => {
                    self.writer.write_double(140, value.numeric_value)?;
                }
                4 | 0x200 => {
                    self.writer.write_string(1, &value.text)?;
                }
                8 => {
                    let size = if value.data_size > 0
                        && value.data_size as usize == value.binary_value.len()
                    {
                        value.data_size
                    } else {
                        value.binary_value.len() as i32
                    };
                    self.writer.write_i32(92, size)?;
                    for chunk in value.binary_value.chunks(127) {
                        self.writer.write_binary(310, chunk)?;
                    }
                }
                0x10 => {
                    // AutoCAD's own files carry no size before a point; one is written only when the source had it.
                    if point_size && value.data_size != 0 {
                        self.writer.write_i32(92, value.data_size)?;
                    }
                    self.writer.write_point2d(
                        11,
                        crate::types::Vector2::new(value.point_value.x, value.point_value.y),
                    )?;
                }
                0x20 => {
                    if point_size && value.data_size != 0 {
                        self.writer.write_i32(92, value.data_size)?;
                    }
                    self.writer.write_point3d(11, value.point_value)?;
                }
                0x40 => {
                    self.writer
                        .write_handle(330, value.handle_value.unwrap_or(Handle::NULL))?;
                }
                0x80 | 0x100 => {}
                _ => {}
            }
        }
        if self.dxf_version >= DxfVersion::AC1021 {
            let unit_code = value.unit_type_code();
            self.writer.write_i32(94, unit_code)?;
            self.writer.write_string(300, &value.format)?;
            if unit_code != 12 {
                self.writer.write_string(302, &value.formatted_value)?;
            }
        }
        Ok(())
    }

    fn write_field_object_dxf(&mut self, value: &crate::objects::Field) -> Result<()> {
        self.writer.write_string(0, "FIELD")?;
        self.writer.write_handle(5, value.handle)?;
        self.write_pending_attachments()?;
        self.writer.write_handle(330, value.owner)?;
        self.writer.write_subclass("AcDbField")?;
        self.writer.write_string(1, &value.evaluator_id)?;
        self.writer.write_string(2, &value.code)?;
        self.writer.write_i32(90, value.child_fields.len() as i32)?;
        for handle in &value.child_fields {
            self.writer.write_handle(360, *handle)?;
        }
        self.writer
            .write_i32(97, value.referenced_objects.len() as i32)?;
        for handle in &value.referenced_objects {
            self.writer.write_handle(331, *handle)?;
        }
        if self.dxf_version < DxfVersion::AC1021 {
            self.writer.write_string(4, value.pre2007_format())?;
        }
        self.writer.write_i32(91, value.evaluation_option)?;
        self.writer.write_i32(92, value.filing_option)?;
        self.writer.write_i32(94, value.state)?;
        self.writer.write_i32(95, value.evaluation_status)?;
        self.writer.write_i32(96, value.evaluation_error_code)?;
        self.writer
            .write_string(300, &value.evaluation_error_message)?;
        // Reference layout: child values (`6` key + value), then the field's
        // own value behind the `7` key, each value closed by ACVALUE_END.
        self.writer.write_i32(93, value.child_values.len() as i32)?;
        for item in &value.child_values {
            self.writer.write_string(6, &item.key)?;
            self.write_field_value_dxf(&item.value)?;
        }
        self.writer.write_string(7, "ACFD_FIELD_VALUE")?;
        self.write_field_value_dxf(&value.value)?;
        self.writer.write_string(301, &value.value_string)?;
        self.writer.write_i32(98, value.value_string_length)?;
        self.write_xdata(&value.xdata)
    }

    fn write_field_value_dxf(&mut self, value: &CellValue) -> Result<()> {
        self.write_cell_value_dxf_masked(value, 1, false)?;
        if self.dxf_version >= DxfVersion::AC1021 {
            self.writer.write_string(304, "ACVALUE_END")?;
        }
        Ok(())
    }

    fn write_field_list_dxf(&mut self, value: &crate::objects::FieldList) -> Result<()> {
        self.writer.write_string(0, "FIELDLIST")?;
        self.writer.write_handle(5, value.handle)?;
        self.write_pending_attachments()?;
        self.writer.write_handle(330, value.owner)?;
        self.writer.write_subclass("AcDbIdSet")?;
        self.writer.write_i32(90, value.fields.len() as i32)?;
        // The reference application does not write the id-set flag (290).
        if value.unknown {
            self.writer.write_bool(290, value.unknown)?;
        }
        for handle in &value.fields {
            self.writer.write_handle(330, *handle)?;
        }
        self.writer.write_subclass("AcDbFieldList")?;
        Ok(())
    }

    /// Write the OBJECTS section
    /// Capture the side-map reactors and extension dictionary of `handle` for the object writer that follows.
    fn stage_attachments(&mut self, document: &CadDocument, handle: Handle) {
        let valid = |h: &Handle| {
            !h.is_null() && (self.valid_handles.is_empty() || self.valid_handles.contains(h))
        };
        let reactors: Vec<Handle> = document
            .reactors_by_handle
            .get(&handle)
            .into_iter()
            .flatten()
            .copied()
            .filter(valid)
            .collect();
        let xdictionary = self
            .carriers
            .extension_dictionaries
            .get(&handle)
            .copied()
            .or_else(|| document.extension_dictionary_handle(handle))
            .filter(valid);
        self.pending_attachments = Some((reactors, xdictionary));
    }

    /// Write the staged `{ACAD_REACTORS` and `{ACAD_XDICTIONARY` groups; call right after the handle (code 5).
    fn write_pending_attachments(&mut self) -> Result<()> {
        if let Some((reactors, xdictionary)) = self.pending_attachments.take() {
            if !reactors.is_empty() {
                self.writer.write_string(102, "{ACAD_REACTORS")?;
                for reactor in reactors {
                    self.writer.write_handle(330, reactor)?;
                }
                self.writer.write_string(102, "}")?;
            }
            if let Some(xdictionary) = xdictionary {
                self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
                self.writer.write_handle(360, xdictionary)?;
                self.writer.write_string(102, "}")?;
            }
        }
        Ok(())
    }

    pub fn write_objects(&mut self, document: &CadDocument) -> Result<()> {
        self.writer.write_section_start("OBJECTS")?;

        // DXF spec requires the root named object dictionary to be the
        // very first object in the OBJECTS section.
        // The header handle may be NULL/invalid (e.g. after DWG reading where
        // handle references aren't fully resolved), so fall back to scanning
        // objects for the root dictionary (owner == NULL).
        let mut root_handle = document.header.named_objects_dict_handle;
        if root_handle.is_null()
            || !matches!(
                document.objects.get(&root_handle),
                Some(ObjectType::Dictionary(_))
            )
        {
            root_handle = Self::find_root_dict_handle(&document.objects);
        }
        if let Some(object @ ObjectType::Dictionary(root_dict)) = document.objects.get(&root_handle) {
            self.write_dictionary(root_dict, document)?;
            self.write_object_xdata(document, root_handle, object)?;
        }

        // Write remaining objects (skip the root dictionary already written).
        // `objects` is a HashMap: iterate in handle order so the same
        // document writes the same bytes on every call.
        // The carriers of table styles replace or join the document's objects.
        let carriers = std::rc::Rc::clone(&self.carriers);
        let mut remaining: Vec<(&Handle, &ObjectType)> = document
            .objects
            .iter()
            .filter(|(handle, _)| !carriers.objects.contains_key(handle))
            .chain(carriers.objects.iter())
            .collect();
        remaining.sort_by_key(|(handle, _)| handle.value());
        self.write_object_records(document, remaining, root_handle)?;

        self.writer.write_section_end()?;
        Ok(())
    }

    fn write_object_records(
        &mut self,
        document: &CadDocument,
        remaining: Vec<(&Handle, &ObjectType)>,
        root_handle: Handle,
    ) -> Result<()> {
        for (handle, object) in remaining {
            if *handle == root_handle {
                continue;
            }
            let object = object;
            self.stage_attachments(document, *handle);
            match object {
                ObjectType::Dictionary(dict) => self.write_dictionary(dict, document)?,
                ObjectType::Layout(layout) => self.write_layout(layout)?,
                ObjectType::XRecord(xrecord) => self.write_xrecord(xrecord, document)?,
                ObjectType::Group(group) => self.write_group(group)?,
                ObjectType::MLineStyle(mlinestyle) => self.write_mlinestyle(mlinestyle)?,
                ObjectType::ImageDefinition(imagedef) => self.write_image_definition(imagedef)?,
                ObjectType::UnderlayDefinition(def) => self.write_underlay_definition(def, document)?,
                ObjectType::PlotSettings(plotsettings) => self.write_plot_settings(plotsettings)?,
                ObjectType::MultiLeaderStyle(style) => self.write_multileader_style(style)?,
                ObjectType::TableStyle(style) => self.write_table_style(style)?,
                ObjectType::TableContent(table) => self.write_table_content_object_dxf(table)?,
                ObjectType::Scale(scale) => self.write_scale(scale)?,
                ObjectType::ObjectContextData(ctx) => self.write_object_context_data(ctx)?,
                ObjectType::SortEntitiesTable(table) => self.write_sort_entities_table(table)?,
                ObjectType::DictionaryVariable(var) => self.write_dictionary_variable(var)?,
                ObjectType::VisualStyle(obj) => self.write_visualstyle(obj)?,
                ObjectType::Material(obj) => self.write_material(obj)?,
                ObjectType::ImageDefinitionReactor(obj) => self.write_imagedef_reactor(obj)?,
                ObjectType::GeoData(obj) => self.write_geodata(obj)?,
                ObjectType::SpatialFilter(obj) => self.write_spatial_filter(obj, document)?,
                ObjectType::RasterVariables(obj) => self.write_raster_variables(obj)?,
                ObjectType::BookColor(obj) => self.write_bookcolor(obj)?,
                ObjectType::PlaceHolder(obj) => {
                    self.write_stub_handle_only("ACDBPLACEHOLDER", obj.handle, obj.owner)?
                }
                ObjectType::DictionaryWithDefault(obj) => {
                    self.write_dict_with_default(obj, &document.objects)?
                }
                ObjectType::WipeoutVariables(obj) => self.write_wipeout_variables(obj)?,
                ObjectType::BlockVisibilityParameter(obj) => {
                    self.write_block_visibility_parameter(obj)?
                }
                ObjectType::DynamicBlock(obj) => self.write_dynamic_block_object_dxf(obj)?,
                ObjectType::Associative(obj) => {
                    // Unrestorable associative-framework objects are not
                    // written (see is_unrestorable_assoc_object); they were
                    // already excluded from valid_handles.
                    if is_unrestorable_assoc_object(&obj.dxf_name) {
                        continue;
                    }
                    self.write_associative_object_dxf(obj)?
                }
                ObjectType::ClassObject(obj) => self.write_class_object_dxf(obj)?,
                ObjectType::DataObject(obj) => self.write_data_object_dxf(obj)?,
                ObjectType::Field(obj) => self.write_field_object_dxf(obj)?,
                ObjectType::FieldList(obj) => self.write_field_list_dxf(obj)?,
                ObjectType::RegisteredClass(obj) => self.write_registered_class_object_dxf(obj)?,
                ObjectType::DgnLineStyle(obj) => self.write_dgn_line_style_object_dxf(obj)?,
                ObjectType::ProxyObject(obj) => self.write_proxy_object_dxf(obj)?,
                ObjectType::Unknown {
                    type_name,
                    handle,
                    owner,
                    raw_dxf_codes,
                    ..
                } => {
                    // Unrestorable associative-framework objects are not
                    // written (see is_unrestorable_assoc_object); they were
                    // already excluded from valid_handles.
                    if is_unrestorable_assoc_object(type_name) {
                        continue;
                    }
                    self.write_unknown_object(
                        type_name,
                        *handle,
                        *owner,
                        raw_dxf_codes.as_deref(),
                    )?;
                }
            }
            self.write_object_xdata(document, *handle, object)?;
        }
        Ok(())
    }

    fn write_registered_class_object_dxf(
        &mut self,
        object: &crate::objects::RegisteredClassObject,
    ) -> Result<()> {
        if object.payload.bit_count != 0 {
            self.writer.write_string(0, "ACAD_PROXY_OBJECT")?;
            self.writer.write_handle(5, object.handle)?;
            if !object.reactors.is_empty() {
                self.writer.write_string(102, "{ACAD_REACTORS")?;
                for reactor in &object.reactors {
                    self.writer.write_handle(330, *reactor)?;
                }
                self.writer.write_string(102, "}")?;
            }
            if let Some(xdictionary) = object.xdictionary_handle {
                self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
                self.writer.write_handle(360, xdictionary)?;
                self.writer.write_string(102, "}")?;
            }
            self.writer.write_handle(330, object.owner)?;
            self.writer.write_subclass("AcDbProxyObject")?;
            let payload = crate::objects::semantic_property::encode_registered_class_envelope(
                &object.dxf_name,
                &object.cpp_class_name,
                &object.properties,
                &object.payload,
            );
            return self.write_proxy_body_dxf(
                499,
                499,
                0,
                0,
                false,
                None,
                &payload,
                &crate::objects::ProxyPayload::default(),
                &object.object_ids,
            );
        }
        self.writer.write_string(0, &object.dxf_name)?;
        self.writer.write_handle(5, object.handle)?;
        if !object.reactors.is_empty() {
            self.writer.write_string(102, "{ACAD_REACTORS")?;
            for reactor in &object.reactors {
                self.writer.write_handle(330, *reactor)?;
            }
            self.writer.write_string(102, "}")?;
        }
        if let Some(xdictionary) = object.xdictionary_handle {
            self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
            self.writer.write_handle(360, xdictionary)?;
            self.writer.write_string(102, "}")?;
        }
        self.writer.write_handle(330, object.owner)?;
        self.write_semantic_properties(&object.properties)?;
        self.write_proxy_references_dxf(&object.object_ids)
    }

    fn write_dgn_line_style_object_dxf(
        &mut self,
        object: &crate::objects::DgnLineStyleObject,
    ) -> Result<()> {
        use crate::objects::DgnLineStyleData;
        self.writer.write_string(0, object.dxf_name())?;
        self.writer.write_handle(5, object.handle)?;
        if !object.reactors.is_empty() {
            self.writer.write_string(102, "{ACAD_REACTORS")?;
            for reactor in &object.reactors {
                self.writer.write_handle(330, *reactor)?;
            }
            self.writer.write_string(102, "}")?;
        }
        if let Some(xdictionary) = object.xdictionary_handle {
            self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
            self.writer.write_handle(360, xdictionary)?;
            self.writer.write_string(102, "}")?;
        }
        self.writer.write_handle(330, object.owner)?;
        match &object.data {
            DgnLineStyleData::Definition {
                description,
                version,
                style_number,
                component_uid,
                is_continuous,
                unit_definition,
                unit_scale,
                units_type,
                is_element,
                is_physical,
                is_scale_independent,
                is_snappable,
                root_component,
                properties,
            } => {
                self.writer.write_subclass("AcDbLSDefinition")?;
                self.writer.write_string(1, description)?;
                self.writer.write_i32(90, *version)?;
                self.writer.write_i32(91, *style_number)?;
                self.writer.write_binary(310, component_uid)?;
                self.writer.write_bool(290, *is_continuous)?;
                self.writer.write_double(40, *unit_definition)?;
                self.writer.write_double(41, *unit_scale)?;
                self.writer.write_i32(92, *units_type)?;
                self.writer.write_bool(291, *is_element)?;
                self.writer.write_bool(292, *is_physical)?;
                self.writer.write_bool(293, *is_scale_independent)?;
                self.writer.write_bool(294, *is_snappable)?;
                self.writer.write_handle(340, *root_component)?;
                self.write_semantic_properties(properties)
            }
            DgnLineStyleData::Component {
                kind,
                description,
                version,
                component_uid,
                scale,
                property_flags,
                component,
                properties,
            } => {
                let subclass = match kind {
                    crate::objects::DgnLsComponentType::Symbol => "AcDbLSSymbolComponent",
                    crate::objects::DgnLsComponentType::Compound => "AcDbLSCompoundComponent",
                    crate::objects::DgnLsComponentType::Stroke => "AcDbLSStrokePatternComponent",
                    crate::objects::DgnLsComponentType::Point => "AcDbLSPointComponent",
                    crate::objects::DgnLsComponentType::Internal => "AcDbLSInternalComponent",
                };
                self.writer.write_subclass(subclass)?;
                self.writer.write_string(1, description)?;
                self.writer.write_i32(90, *version)?;
                self.writer.write_i32(91, kind.code())?;
                self.writer.write_binary(310, component_uid)?;
                self.writer.write_double(40, *scale)?;
                self.writer.write_byte(280, *property_flags)?;
                match component {
                    crate::objects::DgnLsComponentData::Symbol(value) => {
                        self.writer.write_double(41, value.stored_unit_scale)?;
                        self.writer.write_double(42, value.unit_scale)?;
                        self.writer.write_bool(290, value.has_unit_scale)?;
                        self.writer.write_bool(291, value.is_3d)?;
                        self.writer.write_handle(340, value.block)?;
                    }
                    crate::objects::DgnLsComponentData::Compound(value) => {
                        for entry in &value.entries {
                            self.writer.write_double(41, entry.offset)?;
                            self.writer.write_handle(340, entry.component)?;
                        }
                    }
                    crate::objects::DgnLsComponentData::Stroke(value) => {
                        self.write_dgn_stroke_pattern_dxf(value)?;
                    }
                    crate::objects::DgnLsComponentData::Point(value) => {
                        self.writer.write_i32(93, value.symbols.len() as i32)?;
                        self.writer.write_handle(340, value.stroke_component)?;
                        for symbol in &value.symbols {
                            self.writer.write_handle(341, symbol.symbol_component)?;
                            self.writer.write_bool(290, symbol.partial_strokes)?;
                            self.writer.write_bool(291, symbol.clip_partial)?;
                            self.writer.write_bool(292, symbol.allow_stretch)?;
                            self.writer.write_bool(293, symbol.partial_projected)?;
                            self.writer.write_bool(294, symbol.use_symbol_color)?;
                            self.writer.write_bool(295, symbol.use_symbol_lineweight)?;
                            self.writer.write_i32(92, symbol.justify)?;
                            self.writer.write_i32(94, symbol.rotation_type)?;
                            self.writer.write_i32(95, symbol.vertex_mask)?;
                            self.writer.write_double(41, symbol.x_offset)?;
                            self.writer.write_double(42, symbol.y_offset)?;
                            self.writer.write_double(43, symbol.angle)?;
                            self.writer.write_i32(96, symbol.stroke_number)?;
                        }
                    }
                    crate::objects::DgnLsComponentData::Internal(value) => {
                        self.write_dgn_stroke_pattern_dxf(&value.pattern)?;
                        self.writer.write_i32(96, value.internal_version)?;
                        self.writer.write_i32(97, value.hardware_style)?;
                        self.writer.write_bool(297, value.is_hardware_style)?;
                        self.writer.write_i32(98, value.line_code)?;
                    }
                }
                self.write_semantic_properties(properties)
            }
            DgnLineStyleData::Registered {
                properties,
                object_ids,
                ..
            } => {
                self.write_semantic_properties(properties)?;
                self.write_proxy_references_dxf(object_ids)
            }
        }
    }

    fn write_dgn_stroke_pattern_dxf(
        &mut self,
        pattern: &crate::objects::DgnLsStrokePattern,
    ) -> Result<()> {
        self.writer.write_bool(290, pattern.has_iteration_limit)?;
        self.writer.write_bool(291, pattern.is_single_segment)?;
        self.writer.write_i32(92, pattern.iteration_limit)?;
        self.writer.write_double(41, pattern.auto_phase)?;
        self.writer.write_double(42, pattern.phase)?;
        self.writer.write_byte(281, pattern.phase_mode.code())?;
        self.writer.write_i32(93, pattern.strokes.len() as i32)?;
        for stroke in &pattern.strokes {
            self.writer.write_bool(292, stroke.is_dash)?;
            self.writer.write_bool(293, stroke.bypass_corner)?;
            self.writer.write_bool(294, stroke.can_be_scaled)?;
            self.writer.write_bool(295, stroke.invert_at_origin)?;
            self.writer.write_bool(296, stroke.invert_at_end)?;
            self.writer.write_double(43, stroke.length)?;
            self.writer.write_double(44, stroke.start_width)?;
            self.writer.write_double(45, stroke.end_width)?;
            self.writer.write_i32(94, stroke.width_mode)?;
            self.writer.write_i32(95, stroke.cap_mode)?;
        }
        Ok(())
    }

    /// DXF group code of a proxy object id: 330 soft pointer, 340 hard
    /// pointer, 350 soft owner, 360 hard owner (the DXF reference-group
    /// convention; AutoCAD writes a proxy's hard pointers as 340).
    fn proxy_reference_code(kind: crate::objects::ProxyReferenceKind) -> i32 {
        match kind {
            crate::objects::ProxyReferenceKind::Undefined
            | crate::objects::ProxyReferenceKind::SoftPointer => 330,
            crate::objects::ProxyReferenceKind::HardPointer => 340,
            crate::objects::ProxyReferenceKind::SoftOwnership => 350,
            crate::objects::ProxyReferenceKind::HardOwnership => 360,
        }
    }

    fn write_proxy_references_dxf(
        &mut self,
        references: &[crate::objects::ProxyObjectReference],
    ) -> Result<()> {
        for reference in references {
            self.writer
                .write_handle(Self::proxy_reference_code(reference.kind), reference.handle)?;
        }
        if !references.is_empty() {
            self.writer.write_i32(94, 0)?;
        }
        Ok(())
    }

    /// Body of an ACAD_PROXY_ENTITY / ACAD_PROXY_OBJECT after its subclass
    /// marker, in the layout AutoCAD itself writes for each DXF version:
    ///
    /// ```text
    /// R2000/R2004: 90 91 95 70 [92 310*] 93 310* refs 94
    /// R2007:       90 91 95 70 [92 310*] [96] 93 310* refs 94
    /// R2010/R2013: 90 91 95 70 [160 310*] [162] 161 310* refs 94
    /// R2018+:      90 91 71 97 70 [160 310*] [162] 161 310* refs 94
    /// ```
    ///
    /// `[92/160 310*]` is the proxy graphics (entities only); `[96/162]` is the
    /// size of the R2007+ string stream in bits, present only when the proxied
    /// object itself was saved in an R2007+ format (DHL code 27 and up).
    /// 94 always terminates the object-id list, even when it is empty.
    #[allow(clippy::too_many_arguments)]
    fn write_proxy_body_dxf(
        &mut self,
        proxy_id: i32,
        class_id: i32,
        dwg_version: i32,
        maintenance_version: i32,
        from_dxf: bool,
        graphics: Option<&[u8]>,
        payload: &crate::objects::ProxyPayload,
        text_payload: &crate::objects::ProxyPayload,
        references: &[crate::objects::ProxyObjectReference],
    ) -> Result<()> {
        let v2010 = self.dxf_version >= DxfVersion::AC1024;
        self.writer.write_i32(90, proxy_id)?;
        self.writer.write_i32(91, class_id)?;
        if self.dxf_version >= DxfVersion::AC1032 {
            self.writer.write_i32(71, dwg_version)?;
            self.writer.write_i32(97, maintenance_version)?;
        } else {
            self.writer
                .write_i32(95, (maintenance_version << 16) | (dwg_version & 0xffff))?;
        }
        self.writer.write_bool(70, from_dxf)?;
        if let Some(graphics) = graphics {
            if v2010 {
                self.writer.write_i64(160, graphics.len() as i64)?;
            } else {
                self.writer.write_i32(92, graphics.len() as i32)?;
            }
            for chunk in graphics.chunks(127) {
                self.writer.write_binary(310, chunk)?;
            }
        }
        const DHL_1021: i32 = 27;
        if self.dxf_version >= DxfVersion::AC1021 && dwg_version >= DHL_1021 {
            let text_bits = text_payload.bit_count as i64;
            if v2010 {
                self.writer.write_i64(162, text_bits)?;
            } else {
                self.writer.write_i32(96, text_bits as i32)?;
            }
        }
        if v2010 {
            self.writer.write_i64(161, payload.bit_count as i64)?;
        } else {
            self.writer.write_i32(93, payload.bit_count as i32)?;
        }
        let mut data = payload.data();
        // No sample in the wild carries a non-empty string stream; keep it
        // lossless by appending it after the main data (split again on read).
        data.extend_from_slice(&text_payload.data());
        for chunk in data.chunks(127) {
            self.writer.write_binary(310, chunk)?;
        }
        for reference in references {
            self.writer
                .write_handle(Self::proxy_reference_code(reference.kind), reference.handle)?;
        }
        self.writer.write_i32(94, 0)?;
        Ok(())
    }

    fn write_proxy_object_dxf(&mut self, object: &crate::objects::ProxyObject) -> Result<()> {
        self.writer.write_string(0, "ACAD_PROXY_OBJECT")?;
        self.writer.write_handle(5, object.handle)?;
        if !object.reactors.is_empty() {
            self.writer.write_string(102, "{ACAD_REACTORS")?;
            for reactor in &object.reactors {
                self.writer.write_handle(330, *reactor)?;
            }
            self.writer.write_string(102, "}")?;
        }
        if let Some(xdictionary) = object.xdictionary_handle {
            self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
            self.writer.write_handle(360, xdictionary)?;
            self.writer.write_string(102, "}")?;
        }
        self.writer.write_handle(330, object.owner)?;
        self.writer.write_subclass("AcDbProxyObject")?;
        self.write_proxy_body_dxf(
            499,
            object.class_id,
            object.dwg_version,
            object.maintenance_version,
            object.from_dxf,
            None,
            &object.payload,
            &object.text_payload,
            &object.object_ids,
        )
    }

    fn write_class_object_header(
        &mut self,
        object: &crate::objects::ClassObject,
        subclass: &str,
    ) -> Result<()> {
        self.writer.write_string(0, object.dxf_name())?;
        self.writer.write_handle(5, object.handle)?;
        if !object.reactors.is_empty() {
            self.writer.write_string(102, "{ACAD_REACTORS")?;
            for reactor in &object.reactors {
                self.writer.write_handle(330, *reactor)?;
            }
            self.writer.write_string(102, "}")?;
        }
        if let Some(xdictionary) = object.xdictionary_handle {
            self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
            self.writer.write_handle(360, xdictionary)?;
            self.writer.write_string(102, "}")?;
        }
        self.writer.write_handle(330, object.owner)?;
        if !subclass.is_empty() {
            self.writer.write_subclass(subclass)?;
        }
        Ok(())
    }

    /// `write_has_predefined`: only plain RENDERSETTINGS carries the R2013+
    /// flag after `display_index` (MENTALRAYRENDERSETTINGS has none there;
    /// RAPIDRT writes it after its own fields). The class version is stored
    /// the same way as in DWG (see the DWG class-object reader/writer).
    fn write_render_settings_dxf(
        &mut self,
        value: &crate::objects::RenderSettings,
        write_has_predefined: bool,
        rapid_rt: bool,
    ) -> Result<()> {
        let class_version = if rapid_rt && self.dxf_version == DxfVersion::AC1027 {
            value.class_version - 1
        } else if !rapid_rt && self.dxf_version >= DxfVersion::AC1027 {
            value.class_version + 1
        } else {
            value.class_version
        };
        self.writer.write_i32(90, class_version)?;
        self.writer.write_string(1, &value.name)?;
        self.writer.write_bool(290, value.fog_enabled)?;
        self.writer.write_bool(290, value.fog_background_enabled)?;
        self.writer.write_bool(290, value.backfaces_enabled)?;
        self.writer
            .write_bool(290, value.environment_image_enabled)?;
        self.writer
            .write_string(1, &value.environment_image_filename)?;
        self.writer.write_string(1, &value.description)?;
        self.writer.write_i32(90, value.display_index)?;
        if write_has_predefined && self.dxf_version >= DxfVersion::AC1027 {
            self.writer.write_bool(290, value.has_predefined)?;
        }
        Ok(())
    }

    fn write_point_cloud_definition_dxf(
        &mut self,
        value: &crate::objects::PointCloudDefinition,
    ) -> Result<()> {
        self.writer.write_i32(90, value.class_version)?;
        self.writer.write_string(1, &value.source_filename)?;
        self.writer.write_bool(280, value.is_loaded)?;
        self.writer.write_i64(160, value.point_count)?;
        self.writer.write_point3d(10, value.extents_min)?;
        self.writer.write_point3d(11, value.extents_max)?;
        Ok(())
    }

    fn write_point_cloud_ramps_dxf(
        &mut self,
        ramps: &[crate::objects::PointCloudColorRamp],
    ) -> Result<()> {
        self.writer.write_i32(90, ramps.len() as i32)?;
        for ramp in ramps {
            self.writer.write_string(1, &ramp.id)?;
            self.writer.write_i16(70, ramp.class_version)?;
            self.writer.write_i32(90, ramp.colors.len() as i32)?;
            for color in &ramp.colors {
                self.writer.write_i32(91, color.color)?;
                self.writer.write_bool(290, color.visible)?;
            }
            self.writer.write_string(1, &ramp.name)?;
        }
        Ok(())
    }

    fn write_view_rep_dxf(&mut self, value: &crate::objects::ViewRep) -> Result<()> {
        for item in value.header_values {
            self.writer.write_i32(90, item)?;
        }
        self.writer.write_string(1, &value.name)?;
        self.writer.write_i32(90, value.scale)?;
        self.writer.write_i32(90, value.header_status)?;
        self.writer.write_string(1, &value.description)?;
        self.writer.write_i64(160, value.source_id)?;
        self.writer.write_bool(290, value.source_enabled)?;
        self.writer.write_i32(90, value.source_version)?;
        self.writer.write_i64(160, value.model_id)?;
        self.writer.write_i32(90, value.guid.data1)?;
        self.writer.write_i16(70, value.guid.data2)?;
        self.writer.write_i16(70, value.guid.data3)?;
        for item in value.guid.data4 {
            self.writer.write_byte(280, item)?;
        }
        self.writer.write_byte(280, value.marker)?;
        for item in value.transform {
            self.writer.write_double(40, item)?;
        }
        self.writer.write_i32(90, value.transform_version)?;
        self.writer.write_i64(160, value.database_id)?;
        self.writer.write_i32(90, value.geometry_version)?;
        self.writer.write_i32(90, value.geometry_marker)?;
        self.writer.write_i32(90, value.sketches.len() as i32)?;
        for sketch in &value.sketches {
            self.writer.write_i32(90, sketch.id)?;
            self.writer.write_i32(90, sketch.version)?;
            self.writer.write_i32(90, sketch.references.len() as i32)?;
            for reference in &sketch.references {
                self.writer.write_handle(330, reference.object)?;
                self.writer.write_bool(290, reference.flag)?;
            }
            self.writer.write_i32(90, sketch.reserved)?;
            self.writer.write_bool(290, sketch.enabled)?;
            match &sketch.geometry {
                crate::objects::ViewRepSketchGeometry::None => {
                    self.writer.write_i32(90, 0)?;
                }
                crate::objects::ViewRepSketchGeometry::Line {
                    type_code,
                    first,
                    second,
                } => {
                    self.writer.write_i32(90, *type_code)?;
                    self.writer.write_point3d(10, *first)?;
                    self.writer.write_point3d(10, *second)?;
                }
                crate::objects::ViewRepSketchGeometry::Circle {
                    type_code,
                    center,
                    normal,
                    direction,
                    radius,
                    start_parameter,
                    end_parameter,
                    reserved,
                } => {
                    self.writer.write_i32(90, *type_code)?;
                    self.writer.write_point3d(10, *center)?;
                    self.writer.write_point3d(10, *normal)?;
                    self.writer.write_point3d(10, *direction)?;
                    self.writer.write_double(40, *radius)?;
                    self.writer.write_double(40, *start_parameter)?;
                    self.writer.write_double(40, *end_parameter)?;
                    self.writer.write_double(40, *reserved)?;
                }
                crate::objects::ViewRepSketchGeometry::Nurb {
                    type_code,
                    flags,
                    degree,
                    tolerance,
                    knot_header,
                    knots,
                    weight_header,
                    weights,
                    point_header,
                    control_points,
                } => {
                    self.writer.write_i32(90, *type_code)?;
                    self.writer.write_i16(70, i16::from(flags[0]))?;
                    self.writer.write_i16(70, i16::from(flags[1]))?;
                    self.writer.write_i32(90, *degree)?;
                    self.writer.write_double(40, *tolerance)?;
                    for item in knot_header {
                        self.writer.write_i32(90, *item)?;
                    }
                    for item in knots {
                        self.writer.write_double(40, *item)?;
                    }
                    for item in weight_header {
                        self.writer.write_i32(90, *item)?;
                    }
                    for item in weights {
                        self.writer.write_double(40, *item)?;
                    }
                    for item in point_header {
                        self.writer.write_i32(90, *item)?;
                    }
                    for item in control_points {
                        self.writer.write_point3d(10, *item)?;
                    }
                }
            }
            self.writer.write_bool(290, sketch.final_flag)?;
        }
        for handle in value.related_objects {
            self.writer.write_handle(330, handle)?;
        }
        self.writer.write_handle(340, value.source_manager)?;
        for handle in value.owned_objects {
            self.writer.write_handle(360, handle)?;
        }
        for handle in value.optional_objects {
            self.writer.write_handle(330, handle)?;
        }
        self.writer.write_point2d(10, value.position)?;
        self.writer.write_double(40, value.rotation)?;
        self.writer.write_handle(340, value.orientation)?;
        self.writer.write_bool(290, value.is_active)?;
        self.writer.write_i16(70, value.projection)?;
        for handle in value.linked_views {
            self.writer.write_handle(330, handle)?;
        }
        self.writer
            .write_i32(90, value.section_sketches.len() as i32)?;
        for path in &value.section_sketches {
            self.writer.write_string(1, &path.class_name)?;
            self.writer
                .write_i16(70, path.objects.len().saturating_sub(1) as i16)?;
            for handle in &path.objects {
                self.writer.write_handle(330, *handle)?;
            }
        }
        self.writer.write_i32(90, value.action_mode)?;
        if let Some(action) = value.action {
            self.writer.write_handle(360, action)?;
        }
        self.writer.write_bool(290, value.has_parent)?;
        self.writer.write_handle(330, value.parent)?;
        self.writer.write_i32(90, value.tail_version)?;
        self.writer.write_i32(90, value.tail_state)?;
        self.writer.write_i64(160, value.tail_id)?;
        self.writer.write_i32(90, value.path_count)?;
        self.writer.write_i32(90, value.path_version)?;
        self.writer.write_i64(160, value.path_id)?;
        self.writer.write_bool(290, value.has_block_path)?;
        if let Some(path) = &value.block_path {
            self.writer.write_string(1, &path.class_name)?;
            self.writer.write_i32(90, path.version)?;
            self.writer.write_i32(91, path.entries.len() as i32)?;
            for entry in &path.entries {
                self.writer.write_byte(281, entry.flag)?;
                self.writer.write_byte(280, entry.kind)?;
                self.writer.write_handle(332, entry.object)?;
            }
        }
        self.writer.write_handle(340, value.style)?;
        Ok(())
    }

    fn write_class_object_dxf(&mut self, object: &crate::objects::ClassObject) -> Result<()> {
        use crate::objects::{ClassObjectData as Data, DataTableCellType};
        match &object.data {
            Data::Empty => self.write_class_object_header(object, ""),
            Data::ViewRepModelSpaceSource(value) => {
                self.write_class_object_header(object, "AcDbViewRepSource")?;
                self.writer.write_subclass("AcDbViewRepModelSpaceSource")?;
                self.writer.write_bool(290, value.enabled)?;
                for item in value.header_values {
                    self.writer.write_i32(90, item)?;
                }
                for item in value.transform {
                    self.writer.write_double(40, item)?;
                }
                self.writer.write_i32(90, value.source_version)?;
                self.writer.write_i32(90, value.source_status)?;
                self.writer.write_handle(5, value.model)?;
                self.writer.write_i32(90, value.guid.data1)?;
                self.writer.write_i16(70, value.guid.data2)?;
                self.writer.write_i16(70, value.guid.data3)?;
                for item in value.guid.data4 {
                    self.writer.write_byte(280, item)?;
                }
                for handle in value.references {
                    self.writer.write_handle(330, handle)?;
                }
                for item in value.tail_values {
                    self.writer.write_i32(90, item)?;
                }
                self.writer.write_handle(350, value.orientation)?;
                Ok(())
            }
            Data::ViewRep(value) => {
                self.write_class_object_header(object, "AcDbViewRep")?;
                self.write_view_rep_dxf(value)
            }
            Data::SpatialIndex(value) => {
                self.write_class_object_header(object, "AcDbIndex")?;
                self.writer.write_double(
                    40,
                    value.last_updated_julian_day as f64
                        + value.last_updated_milliseconds as f64 / 86_400_000.0,
                )?;
                // Autodesk's DXF contract deliberately carries no spatial
                // index parameters. Hosts rebuild them from drawing entities.
                self.writer.write_subclass("AcDbSpatialIndex")
            }
            Data::LayerFilter(value) => {
                self.write_class_object_header(object, "AcDbLayerFilter")?;
                self.writer.write_i32(90, value.names.len() as i32)?;
                for name in &value.names {
                    self.writer.write_string(8, name)?;
                }
                Ok(())
            }
            Data::PartialViewingIndex(value) => {
                self.write_class_object_header(object, "OdDbPartialViewingIndex")?;
                self.writer.write_i32(90, value.entries.len() as i32)?;
                self.writer.write_bool(290, value.has_entries)?;
                for entry in &value.entries {
                    self.writer.write_point3d(10, entry.extents_min)?;
                    self.writer.write_point3d(11, entry.extents_max)?;
                    self.writer.write_handle(330, entry.object)?;
                }
                Ok(())
            }
            Data::VbaProject(value) => {
                self.write_class_object_header(object, "AcDbVbaProject")?;
                let data = value.storage.encode();
                self.writer.write_i32(90, data.len() as i32)?;
                for chunk in data.chunks(127) {
                    self.writer.write_binary(310, chunk)?;
                }
                Ok(())
            }
            Data::SectionManager(value) => {
                self.write_class_object_header(object, "AcDbSectionManager")?;
                self.writer.write_bool(70, value.is_live)?;
                self.writer.write_i32(90, value.sections.len() as i32)?;
                for section in &value.sections {
                    self.writer.write_handle(330, *section)?;
                }
                Ok(())
            }
            Data::SectionSettings(value) => {
                self.write_class_object_header(object, "AcDbSectionSettings")?;
                self.writer.write_i32(90, value.current_type)?;
                self.writer.write_i32(91, value.types.len() as i32)?;
                for section_type in &value.types {
                    self.writer.write_string(1, "SectionTypeSettings")?;
                    self.writer.write_i32(90, section_type.section_type)?;
                    self.writer.write_i32(91, section_type.generation)?;
                    self.writer
                        .write_i32(92, section_type.sources.len() as i32)?;
                    for source in &section_type.sources {
                        self.writer.write_handle(330, *source)?;
                    }
                    self.writer
                        .write_handle(331, section_type.destination_block)?;
                    self.writer
                        .write_string(1, &section_type.destination_file)?;
                    self.writer
                        .write_i32(93, section_type.geometry.len() as i32)?;
                    for geometry in &section_type.geometry {
                        self.writer.write_string(2, "SectionGeometrySettings")?;
                        self.writer.write_i32(90, geometry.geometry_count)?;
                        self.writer.write_i32(91, geometry.index)?;
                        self.writer.write_i32(92, geometry.flags)?;
                        self.writer.write_color(62, geometry.color)?;
                        self.writer.write_string(8, &geometry.layer)?;
                        self.writer.write_string(6, &geometry.linetype)?;
                        self.writer.write_double(40, geometry.linetype_scale)?;
                        self.writer.write_string(1, &geometry.plot_style)?;
                        self.writer.write_i32(370, geometry.lineweight)?;
                        self.writer.write_i16(70, geometry.face_transparency)?;
                        self.writer.write_i16(71, geometry.edge_transparency)?;
                        self.writer.write_i16(72, geometry.hatch_type)?;
                        self.writer.write_string(2, &geometry.hatch_pattern)?;
                        self.writer.write_double(41, geometry.hatch_angle)?;
                        self.writer.write_double(42, geometry.hatch_spacing)?;
                        self.writer.write_double(43, geometry.hatch_scale)?;
                        self.writer.write_string(3, "SectionGeometrySettingsEnd")?;
                    }
                    self.writer.write_string(3, "SectionTypeSettingsEnd")?;
                }
                Ok(())
            }
            Data::LightList(value) => {
                self.write_class_object_header(object, "AcDbLightList")?;
                self.writer.write_i32(90, value.class_version)?;
                self.writer.write_i32(90, value.lights.len() as i32)?;
                for light in &value.lights {
                    self.writer.write_handle(5, light.handle)?;
                    self.writer.write_string(1, &light.name)?;
                }
                Ok(())
            }
            Data::Sun(value) => {
                self.write_class_object_header(object, "AcDbSun")?;
                self.writer.write_i32(90, value.class_version)?;
                self.writer.write_bool(290, value.is_on)?;
                self.writer.write_color(63, value.color)?;
                if let Some(rgb) = value.color.to_true_color_value() {
                    self.writer.write_i32(421, rgb)?;
                }
                self.writer.write_double(40, value.intensity)?;
                self.writer.write_bool(291, value.has_shadow)?;
                self.writer.write_i32(91, value.julian_day)?;
                self.writer.write_i32(92, value.milliseconds)?;
                self.writer.write_bool(292, value.is_daylight_savings_on)?;
                self.writer.write_i32(70, value.shadow_type)?;
                self.writer.write_i16(71, value.shadow_map_size)?;
                self.writer.write_byte(280, value.shadow_softness)?;
                Ok(())
            }
            Data::RenderSettings(value) => {
                self.write_class_object_header(object, "AcDbRenderSettings")?;
                self.write_render_settings_dxf(value, true, false)
            }
            Data::MentalRayRenderSettings(value) => {
                self.write_class_object_header(object, "AcDbRenderSettings")?;
                self.write_render_settings_dxf(&value.base, false, false)?;
                self.writer.write_subclass("AcDbMentalRayRenderSettings")?;
                self.writer.write_i32(90, value.version)?;
                self.writer.write_i32(90, value.sampling_min)?;
                self.writer.write_i32(90, value.sampling_max)?;
                self.writer.write_i16(70, value.sampling_filter)?;
                self.writer.write_double(40, value.sampling_filter_width)?;
                self.writer.write_double(40, value.sampling_filter_height)?;
                for component in value.sampling_contrast {
                    self.writer.write_double(40, component)?;
                }
                self.writer.write_i16(70, value.shadow_mode)?;
                self.writer.write_bool(290, value.shadow_maps_enabled)?;
                self.writer.write_bool(290, value.ray_tracing_enabled)?;
                for depth in value.ray_trace_depth {
                    self.writer.write_i32(90, depth)?;
                }
                self.writer
                    .write_bool(290, value.global_illumination_enabled)?;
                self.writer
                    .write_i32(90, value.global_illumination_sample_count)?;
                self.writer
                    .write_bool(290, value.global_illumination_sample_radius_enabled)?;
                self.writer
                    .write_double(40, value.global_illumination_sample_radius)?;
                self.writer.write_i32(90, value.photons_per_light)?;
                for depth in value.photon_trace_depth {
                    self.writer.write_i32(90, depth)?;
                }
                self.writer.write_bool(290, value.final_gathering_enabled)?;
                self.writer.write_i32(90, value.final_gathering_ray_count)?;
                for state in value.final_gathering_sample_radius_state {
                    self.writer.write_bool(290, state)?;
                }
                for radius in value.final_gathering_sample_radius {
                    self.writer.write_double(40, radius)?;
                }
                self.writer.write_double(40, value.light_luminance_scale)?;
                self.writer.write_i16(70, value.diagnostics_mode)?;
                self.writer.write_i16(70, value.diagnostics_grid_mode)?;
                self.writer.write_double(40, value.diagnostics_grid_size)?;
                self.writer.write_i16(70, value.diagnostics_photon_mode)?;
                self.writer.write_i16(70, value.diagnostics_bsp_mode)?;
                self.writer.write_bool(290, value.export_mi_enabled)?;
                self.writer.write_string(1, &value.description)?;
                self.writer.write_i32(90, value.tile_size)?;
                self.writer.write_i16(70, value.tile_order)?;
                self.writer.write_i32(90, value.memory_limit)?;
                self.writer
                    .write_bool(290, value.diagnostics_samples_mode)?;
                self.writer.write_double(40, value.energy_multiplier)?;
                Ok(())
            }
            Data::RapidRtRenderSettings(value) => {
                self.write_class_object_header(object, "AcDbRenderSettings")?;
                self.write_render_settings_dxf(&value.base, false, true)?;
                self.writer.write_subclass("AcDbRapidRTRenderSettings")?;
                self.writer.write_i32(90, value.version)?;
                self.writer.write_i32(70, value.render_target)?;
                self.writer.write_i32(90, value.render_level)?;
                self.writer.write_i32(90, value.render_time)?;
                self.writer.write_i32(70, value.lighting_model)?;
                self.writer.write_i32(70, value.filter_type)?;
                self.writer.write_double(40, value.filter_width)?;
                self.writer.write_double(40, value.filter_height)?;
                self.writer.write_bool(290, value.base.has_predefined)?;
                Ok(())
            }
            Data::GradientBackground(value) => {
                self.write_class_object_header(object, "AcDbGradientBackground")?;
                self.writer.write_i32(90, value.class_version)?;
                self.writer.write_i32(90, value.color_top as i32)?;
                self.writer.write_i32(91, value.color_middle as i32)?;
                self.writer.write_i32(92, value.color_bottom as i32)?;
                self.writer.write_double(140, value.horizon)?;
                self.writer.write_double(141, value.height)?;
                self.writer.write_double(142, value.rotation)?;
                Ok(())
            }
            Data::GroundPlaneBackground(value) => {
                self.write_class_object_header(object, "AcDbGroundPlaneBackground")?;
                self.writer.write_i32(90, value.class_version)?;
                self.writer.write_i32(90, value.color_sky_zenith as i32)?;
                self.writer.write_i32(91, value.color_sky_horizon as i32)?;
                self.writer
                    .write_i32(92, value.color_underground_horizon as i32)?;
                self.writer
                    .write_i32(93, value.color_underground_azimuth as i32)?;
                self.writer.write_i32(94, value.color_near as i32)?;
                self.writer.write_i32(95, value.color_far as i32)?;
                Ok(())
            }
            Data::IblBackground(value) => {
                self.write_class_object_header(object, "AcDbIBLBackground")?;
                self.writer.write_i32(90, value.class_version)?;
                self.writer.write_bool(290, value.enabled)?;
                self.writer.write_string(1, &value.name)?;
                self.writer.write_double(40, value.rotation)?;
                self.writer.write_bool(290, value.display_image)?;
                self.writer.write_handle(340, value.secondary_background)?;
                Ok(())
            }
            Data::ImageBackground(value) => {
                self.write_class_object_header(object, "AcDbImageBackground")?;
                self.writer.write_i32(90, value.class_version)?;
                self.writer.write_string(300, &value.filename)?;
                self.writer.write_bool(290, value.fit_to_screen)?;
                self.writer.write_bool(291, value.maintain_aspect_ratio)?;
                self.writer.write_bool(292, value.use_tiling)?;
                self.writer.write_double(140, value.offset.x)?;
                self.writer.write_double(141, value.offset.y)?;
                self.writer.write_double(142, value.scale.x)?;
                self.writer.write_double(143, value.scale.y)?;
                Ok(())
            }
            Data::SkyLightBackground(value) => {
                self.write_class_object_header(object, "AcDbSkyBackground")?;
                self.writer.write_i32(90, value.class_version)?;
                self.writer.write_handle(340, value.sun)?;
                Ok(())
            }
            Data::SolidBackground(value) => {
                self.write_class_object_header(object, "AcDbSolidBackground")?;
                self.writer.write_i32(90, value.class_version)?;
                self.writer.write_i32(90, value.color as i32)?;
                Ok(())
            }
            Data::RenderEntry(value) => {
                self.write_class_object_header(object, "AcDbRenderEntry")?;
                self.writer.write_i32(90, value.class_version)?;
                self.writer.write_string(1, &value.image_filename)?;
                self.writer.write_string(1, &value.preset_name)?;
                self.writer.write_string(1, &value.view_name)?;
                self.writer.write_i32(90, value.width)?;
                self.writer.write_i32(90, value.height)?;
                self.writer.write_i16(70, value.start_year)?;
                self.writer.write_i16(70, value.start_month)?;
                self.writer.write_i16(70, value.start_day)?;
                self.writer.write_i16(70, value.start_hour)?;
                self.writer.write_i16(70, value.start_minute)?;
                self.writer.write_i16(70, value.start_second)?;
                self.writer.write_i16(70, value.start_millisecond)?;
                self.writer.write_i16(70, value.end_year)?;
                self.writer.write_i16(70, value.end_month)?;
                self.writer.write_i16(70, value.end_day)?;
                self.writer.write_i16(70, value.end_hour)?;
                self.writer.write_i16(70, value.end_minute)?;
                self.writer.write_i16(70, value.end_second)?;
                self.writer.write_i16(70, value.end_millisecond)?;
                self.writer.write_double(40, value.render_time)?;
                self.writer.write_i32(90, value.memory_amount)?;
                self.writer.write_i32(90, value.material_count)?;
                self.writer.write_i32(90, value.light_count)?;
                self.writer.write_i32(90, value.triangle_count)?;
                self.writer.write_i32(90, value.display_index)?;
                Ok(())
            }
            Data::RenderEnvironment(value) => {
                self.write_class_object_header(object, "AcDbRenderEnvironment")?;
                self.writer.write_i32(90, value.class_version)?;
                self.writer.write_bool(290, value.fog_enabled)?;
                self.writer.write_bool(290, value.fog_background_enabled)?;
                self.writer.write_byte(280, value.fog_color[0])?;
                self.writer.write_byte(280, value.fog_color[1])?;
                self.writer.write_byte(280, value.fog_color[2])?;
                self.writer.write_double(40, value.fog_density_near)?;
                self.writer.write_double(40, value.fog_density_far)?;
                self.writer.write_double(40, value.fog_distance_near)?;
                self.writer.write_double(40, value.fog_distance_far)?;
                self.writer
                    .write_bool(290, value.environment_image_enabled)?;
                self.writer
                    .write_string(1, &value.environment_image_filename)?;
                Ok(())
            }
            Data::RenderGlobal(value) => {
                self.write_class_object_header(object, "AcDbRenderGlobal")?;
                self.writer.write_i32(90, value.class_version)?;
                self.writer.write_i32(90, value.procedure)?;
                self.writer.write_i32(90, value.destination)?;
                self.writer.write_bool(290, value.save_enabled)?;
                self.writer.write_string(1, &value.save_filename)?;
                self.writer.write_i32(90, value.image_width)?;
                self.writer.write_i32(90, value.image_height)?;
                self.writer
                    .write_bool(290, value.predefined_presets_first)?;
                self.writer.write_bool(290, value.high_level_info)?;
                Ok(())
            }
            Data::MotionPath(value) => {
                self.write_class_object_header(object, "AcDbMotionPath")?;
                self.writer.write_i32(90, value.class_version)?;
                self.writer.write_handle(340, value.camera_path)?;
                self.writer.write_handle(340, value.target_path)?;
                self.writer.write_handle(340, value.view)?;
                self.writer.write_i32(90, value.frames as i32)?;
                self.writer.write_i32(90, value.frame_rate as i32)?;
                self.writer.write_bool(290, value.corner_deceleration)?;
                Ok(())
            }
            Data::CurvePath(value) => {
                self.write_class_object_header(object, "AcDbCurvePath")?;
                self.writer.write_i32(90, value.class_version)?;
                self.writer.write_handle(340, value.entity)?;
                Ok(())
            }
            Data::PointPath(value) => {
                self.write_class_object_header(object, "AcDbPointPath")?;
                self.writer.write_i16(90, value.class_version)?;
                self.writer.write_point3d(10, value.point)?;
                Ok(())
            }
            Data::TvDeviceProperties(value) => {
                self.write_class_object_header(object, "AcDbTvDeviceProperties")?;
                self.writer.write_i32(90, value.flags as i32)?;
                self.writer.write_i16(70, value.max_regen_threads)?;
                self.writer.write_i32(91, value.use_lut_palette)?;
                self.writer.write_i64(160, value.alternate_highlight)?;
                self.writer
                    .write_i64(161, value.alternate_highlight_color)?;
                self.writer.write_i64(162, value.geometry_shader_usage)?;
                self.writer.write_i32(92, value.blending_mode)?;
                self.writer.write_double(40, value.antialiasing_level)?;
                self.writer.write_double(41, value.reserved_double)?;
                Ok(())
            }
            Data::PointCloudDefinition(value) => {
                self.write_class_object_header(object, "AcDbPointCloudDef")?;
                self.write_point_cloud_definition_dxf(value)
            }
            Data::PointCloudDefinitionEx(value) => {
                self.write_class_object_header(object, "AcDbPointCloudDefEx")?;
                self.write_point_cloud_definition_dxf(value)
            }
            Data::PointCloudDefinitionReactor(value) => {
                self.write_class_object_header(object, "AcDbPointCloudDefReactor")?;
                self.writer.write_i32(90, value.class_version)
            }
            Data::PointCloudDefinitionReactorEx(value) => {
                self.write_class_object_header(object, "AcDbPointCloudDefReactorEx")?;
                self.writer.write_i32(90, value.class_version)
            }
            Data::PointCloudColorMap(value) => {
                self.write_class_object_header(object, "AcDbPointCloudColorMap")?;
                self.writer.write_i16(70, value.class_version)?;
                self.writer
                    .write_string(1, &value.default_intensity_scheme)?;
                self.writer
                    .write_string(1, &value.default_elevation_scheme)?;
                self.writer
                    .write_string(1, &value.default_classification_scheme)?;
                self.write_point_cloud_ramps_dxf(&value.color_ramps)?;
                self.write_point_cloud_ramps_dxf(&value.classification_color_ramps)
            }
            Data::NavisworksModelDefinition(value) => {
                self.write_class_object_header(object, "AcDbNavisworksModelDef")?;
                self.writer.write_i16(70, value.flags)?;
                self.writer.write_string(1, &value.path)?;
                self.writer.write_bool(290, value.status)?;
                self.writer.write_point3d(10, value.extents_min)?;
                self.writer.write_point3d(11, value.extents_max)?;
                self.writer.write_bool(290, value.host_drawing_visibility)?;
                Ok(())
            }
            Data::ContextDataManager(value) => {
                self.write_class_object_header(object, "AcDbContextDataManager")?;
                self.writer.write_handle(340, value.object_context)?;
                self.writer.write_i32(90, value.sub_managers.len() as i32)?;
                for manager in &value.sub_managers {
                    self.writer.write_handle(340, manager.handle)?;
                    self.writer.write_i32(91, manager.entries.len() as i32)?;
                    for entry in &manager.entries {
                        self.writer.write_handle(350, entry.item)?;
                        self.writer.write_string(3, &entry.name)?;
                    }
                }
                Ok(())
            }
            Data::SunStudy(value) => {
                self.write_class_object_header(object, "AcDbSunStudy")?;
                self.writer.write_i32(90, value.class_version)?;
                self.writer.write_string(1, &value.setup_name)?;
                self.writer.write_string(2, &value.description)?;
                self.writer.write_i32(70, value.output_type)?;
                if value.output_type == 0 {
                    self.writer.write_bool(290, value.use_subset)?;
                    self.writer.write_string(3, &value.sheet_set_name)?;
                    self.writer.write_string(4, &value.sheet_subset_name)?;
                }
                self.writer
                    .write_bool(291, value.select_dates_from_calendar)?;
                self.writer.write_i32(91, value.dates.len() as i32)?;
                for date in &value.dates {
                    self.writer.write_i32(90, date.julian_day)?;
                    self.writer.write_i32(90, date.milliseconds)?;
                }
                self.writer.write_bool(292, value.select_range_of_dates)?;
                if value.select_range_of_dates {
                    self.writer.write_i32(93, value.start_time)?;
                    self.writer.write_i32(94, value.end_time)?;
                    self.writer.write_i32(95, value.interval)?;
                }
                self.writer.write_i32(91, value.hours.len() as i32)?;
                for hour in &value.hours {
                    self.writer.write_bool(290, *hour)?;
                }
                self.writer.write_i32(74, value.shade_plot_type)?;
                self.writer.write_i32(75, value.viewport_count)?;
                self.writer.write_i32(76, value.rows)?;
                self.writer.write_i32(77, value.columns)?;
                self.writer.write_double(40, value.spacing)?;
                self.writer.write_bool(293, value.lock_viewports)?;
                self.writer.write_bool(294, value.label_viewports)?;
                self.writer.write_handle(340, value.page_setup_wizard)?;
                self.writer.write_handle(341, value.view)?;
                self.writer.write_handle(342, value.visual_style)?;
                self.writer.write_handle(343, value.text_style)?;
                Ok(())
            }
            Data::DataTable(value) => {
                value.validate()?;
                self.write_class_object_header(object, "AcDbDataTable")?;
                self.writer.write_i16(70, value.flags)?;
                self.writer.write_i32(90, value.columns.len() as i32)?;
                self.writer.write_i32(91, value.row_count)?;
                self.writer.write_string(1, &value.name)?;
                for column in &value.columns {
                    self.writer.write_i32(92, column.value_type)?;
                    self.writer.write_string(2, &column.name)?;
                    let cell_type = column.cell_type().expect("DATATABLE validated above");
                    for row in &column.rows {
                        match cell_type {
                            DataTableCellType::Integer => self.writer.write_i32(93, row.integer)?,
                            DataTableCellType::Double => self.writer.write_double(40, row.real)?,
                            DataTableCellType::Text => self.writer.write_string(3, &row.text)?,
                            DataTableCellType::Point => {
                                self.writer.write_double(10, row.point.x)?;
                                self.writer.write_double(20, row.point.y)?;
                                self.writer.write_double(30, row.point.z)?;
                            }
                            DataTableCellType::ObjectId => self.writer.write_handle(331, row.handle)?,
                            DataTableCellType::HardOwnerId => self.writer.write_handle(360, row.handle)?,
                            _ => unreachable!("DATATABLE validated above"),
                        }
                    }
                }
                Ok(())
            }
            Data::DataLink(value) => {
                self.write_class_object_header(object, "AcDbDataLink")?;
                self.writer.write_string(1, &value.data_adapter)?;
                self.writer.write_string(300, &value.description)?;
                self.writer.write_string(301, &value.tooltip)?;
                self.writer.write_string(302, &value.connection_string)?;
                self.writer.write_i32(90, value.option)?;
                self.writer.write_i32(91, value.update_option)?;
                self.writer.write_i32(92, value.flags)?;
                self.writer.write_i16(170, value.year)?;
                self.writer.write_i16(171, value.month)?;
                self.writer.write_i16(172, value.day)?;
                self.writer.write_i16(173, value.hour)?;
                self.writer.write_i16(174, value.minute)?;
                self.writer.write_i16(175, value.second)?;
                self.writer.write_i16(176, value.millisecond)?;
                self.writer.write_i16(177, value.path_option)?;
                self.writer.write_i32(93, value.status_flags)?;
                self.writer.write_string(304, &value.update_status)?;
                self.writer.write_i32(94, value.custom_data.len() as i32)?;
                self.writer.write_string(305, "CUSTOMDATA")?;
                self.writer.write_string(1, "DATAMAP_BEGIN")?;
                for item in &value.custom_data {
                    self.writer.write_handle(330, item.target)?;
                    self.writer.write_string(304, &item.value)?;
                }
                self.writer.write_string(309, "DATAMAP_END")?;
                self.writer.write_handle(360, value.hard_owner)?;
                Ok(())
            }
            Data::PersistentSubentityManager(value) => {
                self.write_class_object_header(object, "AcDbPersSubentManager")?;
                self.writer.write_i32(90, value.class_version)?;
                self.writer.write_i32(90, value.reserved_zero)?;
                self.writer.write_i32(90, value.reserved_two)?;
                self.writer.write_i32(90, value.associated_step_count)?;
                self.writer
                    .write_i32(90, value.associated_subentity_count)?;
                self.writer.write_i32(90, value.steps.len() as i32)?;
                for step in &value.steps {
                    self.writer.write_i32(90, *step)?;
                }
                self.writer.write_i32(90, value.subentities.len() as i32)?;
                for subentity in &value.subentities {
                    self.writer.write_i32(90, *subentity)?;
                }
                Ok(())
            }
            Data::GeoMapImage(value) => {
                self.write_class_object_header(object, "AcDbGeomapImage")?;
                self.writer.write_i32(90, value.class_version)?;
                self.writer.write_point3d(10, value.origin)?;
                self.writer.write_point2d(13, value.image_size)?;
                self.writer.write_i16(70, value.display_properties)?;
                self.writer.write_bool(280, value.clipping_enabled)?;
                self.writer.write_byte(281, value.brightness)?;
                self.writer.write_byte(282, value.contrast)?;
                self.writer.write_byte(283, value.fade)?;
                Ok(())
            }
            Data::DetailViewStyle(value) => {
                self.write_class_object_header(object, "AcDbModelDocViewStyle")?;
                self.writer.write_i16(70, value.base.class_version)?;
                self.writer.write_string(3, &value.base.description)?;
                self.writer
                    .write_bool(290, value.base.modified_for_recompute)?;
                if self.dxf_version >= DxfVersion::AC1032 {
                    self.writer
                        .write_string(300, value.base.display_name_or_description())?;
                    self.writer.write_i32(90, value.base.flags)?;
                }
                self.writer.write_subclass("AcDbDetailViewStyle")?;
                self.writer.write_i16(70, value.class_version)?;
                self.writer.write_i16(71, 0)?;
                self.writer.write_i32(90, value.flags)?;
                self.writer.write_i16(71, 1)?;
                self.writer.write_handle(340, value.identifier_style)?;
                self.writer.write_color(62, value.identifier_color)?;
                self.writer.write_double(40, value.identifier_height)?;
                self.writer.write_handle(340, value.arrow_symbol)?;
                self.writer.write_color(62, value.arrow_symbol_color)?;
                self.writer.write_double(40, value.arrow_symbol_size)?;
                self.writer
                    .write_string(300, &value.identifier_excluded_characters)?;
                self.writer.write_double(40, value.identifier_offset)?;
                self.writer.write_byte(280, value.identifier_placement)?;
                self.writer.write_i16(71, 2)?;
                self.writer.write_handle(340, value.boundary_linetype)?;
                self.writer.write_i32(90, value.boundary_lineweight)?;
                self.writer.write_color(62, value.boundary_color)?;
                self.writer.write_i16(71, 3)?;
                self.writer.write_handle(340, value.view_label_text_style)?;
                self.writer.write_color(62, value.view_label_text_color)?;
                self.writer.write_double(40, value.view_label_text_height)?;
                self.writer.write_i32(90, value.view_label_attachment)?;
                self.writer.write_double(40, value.view_label_offset)?;
                self.writer.write_i32(90, value.view_label_alignment)?;
                self.writer.write_string(300, &value.view_label_pattern)?;
                self.writer.write_i16(71, 4)?;
                self.writer.write_handle(340, value.connection_linetype)?;
                self.writer.write_i32(90, value.connection_lineweight)?;
                self.writer.write_color(62, value.connection_color)?;
                self.writer.write_handle(340, value.border_linetype)?;
                self.writer.write_i32(90, value.border_lineweight)?;
                self.writer.write_color(62, value.border_color)?;
                self.writer.write_byte(280, value.model_edge)?;
                Ok(())
            }
            Data::SectionViewStyle(value) => {
                self.write_class_object_header(object, "AcDbModelDocViewStyle")?;
                self.writer.write_i16(70, value.base.class_version)?;
                self.writer.write_string(3, &value.base.description)?;
                self.writer
                    .write_bool(290, value.base.modified_for_recompute)?;
                if self.dxf_version >= DxfVersion::AC1032 {
                    self.writer
                        .write_string(300, value.base.display_name_or_description())?;
                    self.writer.write_i32(90, value.base.flags)?;
                }
                self.writer.write_subclass("AcDbSectionViewStyle")?;
                self.writer.write_i16(70, value.class_version)?;
                self.writer.write_i16(71, 0)?;
                self.writer.write_i32(90, value.flags)?;
                self.writer.write_i16(71, 1)?;
                self.writer.write_handle(340, value.identifier_style)?;
                self.writer.write_color(62, value.identifier_color)?;
                self.writer.write_double(40, value.identifier_height)?;
                self.writer.write_handle(340, value.arrow_start_symbol)?;
                self.writer.write_handle(340, value.arrow_end_symbol)?;
                self.writer.write_color(62, value.arrow_symbol_color)?;
                self.writer.write_double(40, value.arrow_symbol_size)?;
                self.writer
                    .write_string(300, &value.identifier_excluded_characters)?;
                self.writer
                    .write_double(40, value.arrow_symbol_extension_length)?;
                self.writer.write_i32(90, value.identifier_position)?;
                self.writer.write_double(40, value.identifier_offset)?;
                self.writer.write_i32(90, value.arrow_position)?;
                self.writer.write_i16(71, 2)?;
                self.writer.write_handle(340, value.plane_linetype)?;
                self.writer.write_i32(90, value.plane_lineweight)?;
                self.writer.write_color(62, value.plane_color)?;
                self.writer.write_handle(340, value.bend_linetype)?;
                self.writer.write_i32(90, value.bend_lineweight)?;
                self.writer.write_color(62, value.bend_color)?;
                self.writer.write_double(40, value.bend_line_length)?;
                self.writer.write_double(40, value.end_line_overshoot)?;
                self.writer.write_double(40, value.end_line_length)?;
                self.writer.write_i16(71, 3)?;
                self.writer.write_handle(340, value.view_label_text_style)?;
                self.writer.write_color(62, value.view_label_text_color)?;
                self.writer.write_double(40, value.view_label_text_height)?;
                self.writer.write_i32(90, value.view_label_attachment)?;
                self.writer.write_double(40, value.view_label_offset)?;
                self.writer.write_i32(90, value.view_label_alignment)?;
                self.writer.write_string(300, &value.view_label_pattern)?;
                self.writer.write_i16(71, 4)?;
                self.writer.write_color(62, value.hatch_color)?;
                self.writer.write_color(62, value.hatch_background_color)?;
                self.writer.write_string(300, &value.hatch_pattern)?;
                self.writer.write_double(40, value.hatch_scale)?;
                self.writer.write_i32(90, value.hatch_transparency)?;
                self.writer.write_bool(290, value.reserved_flags[0])?;
                self.writer.write_bool(290, value.reserved_flags[1])?;
                self.writer.write_i32(90, value.hatch_angles.len() as i32)?;
                for angle in &value.hatch_angles {
                    self.writer.write_double(40, *angle)?;
                }
                Ok(())
            }
            Data::AcMeCommandHistory(_) => {
                self.write_class_object_header(object, "AcMeCommandHistory")
            }
            Data::AcMeScope(_) => self.write_class_object_header(object, "AcMeScope"),
            Data::AcMeStateManager(_) => self.write_class_object_header(object, "AcMeStateMgr"),
            Data::CsacDocumentOptions(_) => self.write_class_object_header(object, ""),
            Data::ViewRepSourceManager(value) => {
                self.write_class_object_header(object, "AcDbViewRepSourceMgr")?;
                self.writer.write_bool(290, value.has_source)?;
                self.writer.write_handle(350, value.source)?;
                self.writer.write_i32(90, value.status)?;
                Ok(())
            }
            Data::ViewRepStandard(value) => {
                self.write_class_object_header(object, "AcDbViewRepStandard")?;
                for item in value.values {
                    self.writer.write_i32(90, item)?;
                }
                Ok(())
            }
            Data::ViewRepOrientationDefinition => {
                self.write_class_object_header(object, "AcDbViewRepOrientationDef")
            }
            Data::ViewRepOrientation(value) => {
                self.write_class_object_header(object, "AcDbViewRepOrientation")?;
                self.writer.write_point3d(10, value.camera)?;
                self.writer.write_point3d(10, value.target)?;
                self.writer.write_point3d(210, value.normal)?;
                Ok(())
            }
            Data::ViewRepSectionDefinition(value) => {
                self.write_class_object_header(object, "AcDbViewRepCutDefinition")?;
                self.writer.write_subclass("AcDbViewRepSectionDefinition")?;
                self.writer.write_i32(90, value.version)?;
                self.writer.write_double(40, value.section_depth)?;
                self.writer.write_i32(90, value.flags[0])?;
                self.writer.write_i32(90, value.flags[1])?;
                Ok(())
            }
            Data::ViewRepModelSpaceViewSelectionSet(value) => {
                self.write_class_object_header(object, "AcDbViewRepModelSpaceViewSelSet")?;
                self.writer.write_i32(90, value.version)?;
                self.writer.write_i32(90, value.entities.len() as i32)?;
                for handle in &value.entities {
                    self.writer.write_handle(330, *handle)?;
                }
                Ok(())
            }
        }
    }

    /// Find the root named-objects dictionary by scanning for a Dictionary
    /// with owner == NULL.  Prefers the one with the most entries.
    fn find_root_dict_handle(objects: &std::collections::HashMap<Handle, ObjectType>) -> Handle {
        let mut best = Handle::NULL;
        let mut best_count = 0usize;
        for (handle, obj) in objects {
            if let ObjectType::Dictionary(dict) = obj {
                if dict.owner.is_null() {
                    if dict.entries.len() > best_count
                        || (dict.entries.len() == best_count && handle.value() > best.value())
                    {
                        best = *handle;
                        best_count = dict.entries.len();
                    }
                }
            }
        }
        best
    }

    fn write_dictionary(&mut self, dict: &Dictionary, document: &CadDocument) -> Result<()> {
        let objects = &document.objects;
        self.writer.write_string(0, "DICTIONARY")?;
        self.writer.write_handle(5, dict.handle)?;
        let valid = |handle: &Handle| {
            !handle.is_null()
                && (self.valid_handles.is_empty() || self.valid_handles.contains(handle))
        };
        // A DWG read keeps the reactors aside when the record has none.
        let reactors: Vec<Handle> = if dict.reactors.is_empty() {
            document.reactors_by_handle.get(&dict.handle).cloned().unwrap_or_default()
        } else {
            dict.reactors.clone()
        };
        let reactors: Vec<Handle> = reactors.into_iter().filter(valid).collect();
        if !reactors.is_empty() {
            self.writer.write_string(102, "{ACAD_REACTORS")?;
            for reactor in reactors {
                self.writer.write_handle(330, reactor)?;
            }
            self.writer.write_string(102, "}")?;
        }
        if let Some(xdictionary) = dict
            .xdictionary_handle
            .or_else(|| document.extension_dictionary_handle(dict.handle))
            .filter(valid)
        {
            self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
            self.writer.write_handle(360, xdictionary)?;
            self.writer.write_string(102, "}")?;
        }
        let dict_owner = if dict.owner == Handle::NULL
            || self.valid_handles.is_empty()
            || self.valid_handles.contains(&dict.owner)
            || self.root_dict_handle == Handle::NULL
        {
            dict.owner
        } else {
            self.root_dict_handle
        };
        self.writer.write_handle(330, dict_owner)?;
        self.writer.write_subclass("AcDbDictionary")?;
        self.writer
            .write_byte(280, if dict.hard_owner { 1 } else { 0 })?;
        self.writer.write_byte(281, dict.duplicate_cloning as u8)?;

        for (key, handle) in &dict.entries {
            // Skip entries pointing to objects that don't exist in the document,
            // OR that exist only as an Unknown DWG object with no DXF form (and
            // are therefore filtered out of valid_handles and never written).
            // Writing dangling references causes CAD programs to report audit
            // errors.
            if !(objects.contains_key(handle) || self.carriers.objects.contains_key(handle))
                || (!self.valid_handles.is_empty() && !self.valid_handles.contains(handle))
            {
                continue;
            }
            self.writer.write_string(3, key)?;
            // These dictionary keys are hard-owner entries by definition even
            // when the dictionary-wide hard-owner flag is clear. AutoCAD,
            // ODA and LibreDWG all emit code 360 for them. ACAD_LAYOUT and
            // ACAD_PLOTSTYLENAME own their sub-dictionaries; ACAD_FIELD owns
            // the drawing's FIELDLIST. Writing them as soft pointers (350)
            // makes CAD applications treat the targets as erasable orphans
            // and reject the file on save.
            let forced_hard_owner =
                dict.is_entry_hard_owner(key) || Dictionary::is_canonical_hard_owner_key(key);
            self.writer.write_handle(
                if dict.hard_owner || forced_hard_owner {
                    360
                } else {
                    350
                },
                *handle,
            )?;
        }

        Ok(())
    }

    fn write_layout(&mut self, layout: &Layout) -> Result<()> {
        self.writer.write_string(0, "LAYOUT")?;
        self.writer.write_handle(5, layout.handle)?;

        // Extension dictionary
        if let Some(xdict) = layout.xdictionary_handle {
            if xdict != Handle::NULL
                && (self.valid_handles.is_empty() || self.valid_handles.contains(&xdict))
            {
                self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
                self.writer.write_handle(360, xdict)?;
                self.writer.write_string(102, "}")?;
            }
        }

        // Reactors (filter out references to non-existent objects)
        if !layout.reactors.is_empty() {
            let valid_reactors: Vec<Handle> = if self.valid_handles.is_empty() {
                layout.reactors.clone()
            } else {
                layout
                    .reactors
                    .iter()
                    .copied()
                    .filter(|r| self.valid_handles.contains(r))
                    .collect()
            };
            if !valid_reactors.is_empty() {
                self.writer.write_string(102, "{ACAD_REACTORS")?;
                for r in &valid_reactors {
                    self.writer.write_handle(330, *r)?;
                }
                self.writer.write_string(102, "}")?;
            }
        }

        self.writer.write_handle(330, layout.owner)?;
        self.writer.write_subclass("AcDbPlotSettings")?;

        // Write plot settings: use preserved raw codes if available,
        // otherwise write minimal defaults.
        if let Some(ref codes) = layout.raw_plot_settings_codes {
            for (code, value) in codes {
                self.writer.write_string(*code, value)?;
            }
        } else {
            self.writer.write_string(1, &layout.plot_page_name)?;
            self.writer.write_string(2, &layout.plot_printer_name)?;
            self.writer.write_string(4, &layout.paper_size)?;
            // The DWG of R13..R2000 stores the plot view name (empty or not); later ones do not.
            if !layout.plot_view_name.is_empty() || self.dwg_source.is_some_and(|v| v <= DxfVersion::AC1015) {
                self.writer.write_string(6, &layout.plot_view_name)?;
            }
            self.writer.write_string(7, &layout.plot_style_sheet)?;
            self.writer.write_double(40, layout.plot_margin_left)?;
            self.writer.write_double(41, layout.plot_margin_bottom)?;
            self.writer.write_double(42, layout.plot_margin_right)?;
            self.writer.write_double(43, layout.plot_margin_top)?;
            self.writer.write_double(44, layout.paper_width)?;
            self.writer.write_double(45, layout.paper_height)?;
            self.writer.write_double(46, layout.plot_origin_x)?;
            self.writer.write_double(47, layout.plot_origin_y)?;
            self.writer.write_double(48, layout.plot_window_min_x)?;
            self.writer.write_double(49, layout.plot_window_min_y)?;
            self.writer.write_double(140, layout.plot_window_max_x)?;
            self.writer.write_double(141, layout.plot_window_max_y)?;
            self.writer.write_double(142, layout.plot_scale_numerator)?;
            self.writer
                .write_double(143, layout.plot_scale_denominator)?;
            self.writer.write_double(147, layout.plot_scale_factor)?;
            self.writer.write_double(148, layout.paper_image_origin_x)?;
            self.writer.write_double(149, layout.paper_image_origin_y)?;
            let mut plot_flags = layout.plot_flags.to_bits();
            if layout.name == "Model" {
                plot_flags |= 1024;
            }
            self.writer.write_i16(70, plot_flags as i16)?;
            self.writer.write_i16(72, layout.plot_paper_units)?;
            self.writer.write_i16(73, layout.plot_rotation)?;
            self.writer.write_i16(74, layout.plot_type)?;
            self.writer.write_i16(75, layout.plot_scale_type)?;
            self.writer.write_i16(76, layout.shade_plot_mode)?;
            self.writer.write_i16(77, layout.shade_plot_resolution)?;
            // 0 is the unset value of a layout read from a DWG that has no such field (valid DPI is 100 and up).
            if layout.shade_plot_dpi != 0 {
                self.writer.write_i16(78, layout.shade_plot_dpi)?;
            }
            if !layout.visual_style_handle.is_null() {
                self.writer.write_handle(333, layout.visual_style_handle)?;
            }
        }

        self.writer.write_subclass("AcDbLayout")?;
        self.writer.write_string(1, &layout.name)?;
        self.writer.write_i16(70, layout.flags)?;
        self.writer.write_i16(71, layout.tab_order)?;
        self.writer.write_double(10, layout.min_limits.0)?;
        self.writer.write_double(20, layout.min_limits.1)?;
        self.writer.write_double(11, layout.max_limits.0)?;
        self.writer.write_double(21, layout.max_limits.1)?;
        self.writer.write_double(12, layout.insertion_base.0)?;
        self.writer.write_double(22, layout.insertion_base.1)?;
        self.writer.write_double(32, layout.insertion_base.2)?;
        self.writer.write_double(14, layout.min_extents.0)?;
        self.writer.write_double(24, layout.min_extents.1)?;
        self.writer.write_double(34, layout.min_extents.2)?;
        self.writer.write_double(15, layout.max_extents.0)?;
        self.writer.write_double(25, layout.max_extents.1)?;
        self.writer.write_double(35, layout.max_extents.2)?;
        self.writer.write_double(146, layout.elevation)?;
        self.writer.write_double(13, layout.ucs_origin.0)?;
        self.writer.write_double(23, layout.ucs_origin.1)?;
        self.writer.write_double(33, layout.ucs_origin.2)?;
        self.writer.write_double(16, layout.ucs_x_axis.0)?;
        self.writer.write_double(26, layout.ucs_x_axis.1)?;
        self.writer.write_double(36, layout.ucs_x_axis.2)?;
        self.writer.write_double(17, layout.ucs_y_axis.0)?;
        self.writer.write_double(27, layout.ucs_y_axis.1)?;
        self.writer.write_double(37, layout.ucs_y_axis.2)?;
        self.writer.write_i16(76, layout.ucs_ortho_type)?;
        self.writer.write_handle(330, layout.block_record)?;
        if layout.viewport != Handle::NULL {
            self.writer.write_handle(331, layout.viewport)?;
        }
        if !layout.named_ucs.is_null() {
            self.writer.write_handle(345, layout.named_ucs)?;
        }
        if !layout.base_ucs.is_null() {
            self.writer.write_handle(346, layout.base_ucs)?;
        }

        Ok(())
    }

    fn write_xrecord(&mut self, xrecord: &XRecord, document: &CadDocument) -> Result<()> {
        use crate::objects::XRecordValue;

        self.writer.write_string(0, "XRECORD")?;
        self.writer.write_handle(5, xrecord.handle)?;
        if !xrecord.reactors.is_empty() {
            self.writer.write_string(102, "{ACAD_REACTORS")?;
            for reactor in &xrecord.reactors {
                self.writer.write_handle(330, *reactor)?;
            }
            self.writer.write_string(102, "}")?;
        }
        if let Some(xdictionary) = xrecord.xdictionary_handle {
            self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
            self.writer.write_handle(360, xdictionary)?;
            self.writer.write_string(102, "}")?;
        }
        self.writer.write_handle(330, xrecord.owner)?;
        self.writer.write_subclass("AcDbXrecord")?;
        self.writer
            .write_byte(280, xrecord.cloning_flags.to_code() as u8)?;

        let advanced_material_entries = match document.objects.get(&xrecord.owner) {
            Some(ObjectType::Dictionary(dictionary))
                if dictionary.entries.iter().any(|(name, handle)| {
                    name.eq_ignore_ascii_case("ADVMATERIAL") && *handle == xrecord.handle
                }) =>
            {
                document.objects.values().find_map(|object| match object {
                    ObjectType::Material(material)
                        if material.xdictionary_handle == Some(xrecord.owner)
                            && material.has_advanced_data() =>
                    {
                        let values = vec![
                            XRecordEntry::double(460, material.color_bleed_scale * 100.0),
                            XRecordEntry::double(461, material.indirect_bump_scale * 100.0),
                            XRecordEntry::double(462, material.reflectance_scale * 100.0),
                            XRecordEntry::double(463, material.transmittance_scale * 100.0),
                            XRecordEntry::bool(290, material.two_sided_material),
                            XRecordEntry::int16(270, material.luminance_mode),
                            XRecordEntry::double(464, material.luminance),
                            XRecordEntry::bool(293, material.is_anonymous),
                            XRecordEntry::int16(272, material.global_illumination),
                            XRecordEntry::int16(273, material.final_gather),
                        ];
                        let mut merged = xrecord.entries.clone();
                        for value in values {
                            if let Some(existing) =
                                merged.iter_mut().find(|entry| entry.code == value.code)
                            {
                                existing.value = value.value;
                            } else {
                                merged.push(value);
                            }
                        }
                        Some(merged)
                    }
                    _ => None,
                })
            }
            _ => None,
        };
        let entries = advanced_material_entries
            .as_deref()
            .unwrap_or(&xrecord.entries);

        // Write each entry's group code and value
        for (index, entry) in entries.iter().enumerate() {
            match &entry.value {
                XRecordValue::String(s) => {
                    // The spelling the source used (`\U+2205` rather than the decoded character) while it still
                    // decodes to the entry; caret escapes are re-derived from the decoded text by the stream writer
                    // (a binary string holds line breaks and carets as they are).
                    let spelled = if advanced_material_entries.is_none() {
                        xrecord.source_value(index)
                    } else {
                        None
                    };
                    match spelled {
                        Some(spelled) => self
                            .writer
                            .write_xrecord_string(entry.code, &source_spelling_characters(spelled, s))?,
                        None => self.writer.write_xrecord_string(entry.code, s)?,
                    }
                }
                XRecordValue::Double(d) => {
                    self.writer.write_double(entry.code, *d)?;
                }
                XRecordValue::Int16(i) => {
                    self.writer.write_i16(entry.code, *i)?;
                }
                XRecordValue::Int32(i) => {
                    self.writer.write_i32(entry.code, *i)?;
                }
                XRecordValue::Int64(i) => {
                    self.writer.write_i64(entry.code, *i)?;
                }
                XRecordValue::Byte(b) => {
                    self.writer.write_byte(entry.code, *b)?;
                }
                XRecordValue::Bool(b) => {
                    self.writer.write_bool(entry.code, *b)?;
                }
                XRecordValue::Handle(h) => {
                    self.writer.write_handle(entry.code, *h)?;
                }
                XRecordValue::Point3D(x, y, z) => {
                    self.writer.write_double(entry.code, *x)?;
                    self.writer.write_double(entry.code + 10, *y)?;
                    self.writer.write_double(entry.code + 20, *z)?;
                }
                XRecordValue::Chunk(data) => {
                    self.writer.write_binary(entry.code, data)?;
                }
            }
        }

        Ok(())
    }

    fn write_group(&mut self, group: &Group) -> Result<()> {
        self.writer.write_string(0, "GROUP")?;
        self.writer.write_handle(5, group.handle)?;
        self.write_pending_attachments()?;
        self.writer.write_handle(330, group.owner)?;
        self.writer.write_subclass("AcDbGroup")?;

        // Group description (code 300)
        self.writer.write_string(300, &group.description)?;

        // Unnamed flag (code 70) - 1 if unnamed, 0 if named
        self.writer
            .write_i16(70, if group.is_unnamed() { 1 } else { 0 })?;

        // Selectable flag (code 71)
        self.writer
            .write_i16(71, if group.selectable { 1 } else { 0 })?;

        // Entity handles (code 340)
        for entity_handle in group.iter() {
            self.writer.write_handle(340, *entity_handle)?;
        }

        Ok(())
    }

    fn write_mlinestyle(&mut self, style: &MLineStyle) -> Result<()> {
        self.writer.write_string(0, "MLINESTYLE")?;
        self.writer.write_handle(5, style.handle)?;
        self.write_pending_attachments()?;
        self.writer.write_handle(330, style.owner)?;
        self.writer.write_subclass("AcDbMlineStyle")?;

        // Style name (code 2)
        self.writer.write_string(2, &style.name)?;

        // Flags (code 70)
        self.writer.write_i16(70, style.flags.to_bits() as i16)?;

        // Description (code 3)
        self.writer.write_string(3, &style.description)?;

        // Fill color (code 62)
        let fill_color_index = match style.fill_color {
            Color::ByLayer => 256,
            Color::None => 257,
            Color::ByBlock => 0,
            Color::Index(i) => i as i16,
            Color::Rgb { .. } => 256, // Fall back to ByLayer for RGB color
        };
        self.writer.write_i16(62, fill_color_index)?;

        // Start angle (code 51) — DXF expects degrees
        self.writer
            .write_double(51, style.start_angle.to_dxf_degrees())?;

        // End angle (code 52) — DXF expects degrees
        self.writer.write_double(52, style.end_angle.to_dxf_degrees())?;

        // Number of elements (code 71)
        self.writer.write_i16(71, style.element_count() as i16)?;

        // Write each element
        for element in style.iter() {
            // Element offset (code 49)
            self.writer.write_double(49, element.offset)?;

            // Element color (code 62)
            let elem_color_index = match element.color {
                Color::ByLayer => 256,
                Color::None => 257,
                Color::ByBlock => 0,
                Color::Index(i) => i as i16,
                Color::Rgb { .. } => 256,
            };
            self.writer.write_i16(62, elem_color_index)?;

            // Element linetype (code 6)
            self.writer.write_string(6, &element.linetype)?;
        }

        Ok(())
    }

    fn write_image_definition(&mut self, imagedef: &ImageDefinition) -> Result<()> {
        self.writer.write_string(0, "IMAGEDEF")?;
        self.writer.write_handle(5, imagedef.handle)?;
        self.write_pending_attachments()?;
        self.writer.write_handle(330, imagedef.owner)?;
        self.writer.write_subclass("AcDbRasterImageDef")?;

        // Class version (code 90)
        self.writer.write_i32(90, imagedef.class_version)?;

        // File name (code 1)
        self.writer.write_string(1, &imagedef.file_name)?;

        // Image size in pixels (codes 10, 20)
        self.writer
            .write_double(10, imagedef.size_in_pixels.0 as f64)?;
        self.writer
            .write_double(20, imagedef.size_in_pixels.1 as f64)?;

        // Default pixel size (codes 11, 21)
        self.writer.write_double(11, imagedef.pixel_size.0)?;
        self.writer.write_double(21, imagedef.pixel_size.1)?;

        // Is loaded (code 280)
        self.writer
            .write_byte(280, if imagedef.is_loaded { 1 } else { 0 })?;

        // Resolution units (code 281)
        self.writer
            .write_byte(281, imagedef.resolution_unit.to_code() as u8)?;

        Ok(())
    }

    /// Write a PDF/DWF/DGN underlay definition object.
    fn write_underlay_definition(
        &mut self,
        def: &crate::objects::UnderlayDefinition,
        document: &CadDocument,
    ) -> Result<()> {
        self.writer.write_string(0, def.entity_name())?;
        self.writer.write_handle(5, def.handle)?;
        // A document read from DWG keeps the reactor list on the side map (the owning dictionary is one).
        let reactors = document
            .reactors_by_handle
            .get(&def.handle)
            .filter(|handles| !handles.is_empty())
            .unwrap_or(&def.reactors);
        if !reactors.is_empty() {
            self.writer.write_string(102, "{ACAD_REACTORS")?;
            for reactor in reactors {
                if !self.valid_handles.is_empty() && !self.valid_handles.contains(reactor) {
                    continue;
                }
                self.writer.write_handle(330, *reactor)?;
            }
            self.writer.write_string(102, "}")?;
        }
        self.writer.write_handle(330, def.owner_handle)?;
        self.writer.write_subclass("AcDbUnderlayDefinition")?;
        self.writer.write_string(1, &def.file_path)?;
        self.writer.write_string(2, &def.page_name)?;
        if def.unloaded {
            self.writer.write_string(1001, "ACAD")?;
            self.writer.write_string(1000, "NOLOAD")?;
        }
        Ok(())
    }

    fn write_plot_settings(&mut self, settings: &PlotSettings) -> Result<()> {
        self.writer.write_string(0, "PLOTSETTINGS")?;
        self.writer.write_handle(5, settings.handle)?;
        if !settings.reactors.is_empty() {
            self.writer.write_string(102, "{ACAD_REACTORS")?;
            for reactor in &settings.reactors {
                self.writer.write_handle(330, *reactor)?;
            }
            self.writer.write_string(102, "}")?;
        }
        if let Some(xdictionary) = settings.xdictionary_handle {
            self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
            self.writer.write_handle(360, xdictionary)?;
            self.writer.write_string(102, "}")?;
        }
        self.writer.write_handle(330, settings.owner)?;
        self.writer.write_subclass("AcDbPlotSettings")?;

        // Page setup name (code 1)
        self.writer.write_string(1, &settings.page_name)?;

        // Printer/plotter name (code 2)
        self.writer.write_string(2, &settings.printer_name)?;

        // Paper size (code 4)
        self.writer.write_string(4, &settings.paper_size)?;

        // Plot view name (code 6)
        self.writer.write_string(6, &settings.plot_view_name)?;

        // Style sheet (code 7)
        self.writer.write_string(7, &settings.current_style_sheet)?;

        // Unprintable margins (codes 40-43)
        self.writer.write_double(40, settings.margins.left)?;
        self.writer.write_double(41, settings.margins.bottom)?;
        self.writer.write_double(42, settings.margins.right)?;
        self.writer.write_double(43, settings.margins.top)?;

        // Paper size (codes 44, 45)
        self.writer.write_double(44, settings.paper_width)?;
        self.writer.write_double(45, settings.paper_height)?;

        // Plot origin (codes 46, 47)
        self.writer.write_double(46, settings.origin_x)?;
        self.writer.write_double(47, settings.origin_y)?;

        // Plot window (codes 48, 49, 140, 141)
        self.writer
            .write_double(48, settings.plot_window.lower_left_x)?;
        self.writer
            .write_double(49, settings.plot_window.lower_left_y)?;
        self.writer
            .write_double(140, settings.plot_window.upper_right_x)?;
        self.writer
            .write_double(141, settings.plot_window.upper_right_y)?;

        // Custom scale (codes 142, 143)
        self.writer.write_double(142, settings.scale_numerator)?;
        self.writer.write_double(143, settings.scale_denominator)?;

        self.writer
            .write_double(147, settings.standard_scale_factor)?;
        self.writer
            .write_double(148, settings.paper_image_origin_x)?;
        self.writer
            .write_double(149, settings.paper_image_origin_y)?;

        // Flags (code 70)
        self.writer.write_i16(70, settings.flags.to_bits() as i16)?;

        // Paper units (code 72)
        self.writer.write_i16(72, settings.paper_units.to_code())?;

        // Rotation (code 73)
        self.writer.write_i16(73, settings.rotation.to_code())?;

        // Plot type (code 74)
        self.writer.write_i16(74, settings.plot_type.to_code())?;

        // Standard scale type (code 75)
        self.writer.write_i16(75, settings.scale_type.to_code())?;

        // Shade plot mode (code 76)
        self.writer
            .write_i16(76, settings.shade_plot_mode.to_code())?;

        // Shade plot resolution level (code 77)
        self.writer
            .write_i16(77, settings.shade_plot_resolution.to_code())?;

        // Shade plot custom DPI (code 78)
        self.writer.write_i16(78, settings.shade_plot_dpi)?;

        if !settings.visual_style_handle.is_null() {
            self.writer
                .write_handle(333, settings.visual_style_handle)?;
        }

        Ok(())
    }

    /// Write MultiLeaderStyle object
    fn write_multileader_style(&mut self, style: &MultiLeaderStyle) -> Result<()> {
        self.writer.write_string(0, "MLEADERSTYLE")?;
        self.writer.write_handle(5, style.handle)?;
        self.write_pending_attachments()?;
        self.writer.write_handle(330, style.owner_handle)?;
        self.writer.write_subclass("AcDbMLeaderStyle")?;

        // A source that left a group out keeps it out (the reference reader then uses its own default) unless
        // the value was changed from the native default.
        let defaults = MultiLeaderStyle::new("");
        let from_dxf = style.dxf_present_codes.is_some();
        let keep = |code: i32, at_default: bool| {
            !from_dxf || style.dxf_code_present(code) || !at_default
        };
        // A DWG before R2010 has no text bottom/top attachment fields (groups 272, 273): they stay at the
        // defaults a DXF without the groups gets, and are left out unless changed.
        let dwg_lacks_attachments = self.dwg_source.is_some_and(|version| version < DxfVersion::AC1024);
        let keep_attachment = |code: i32, at_default: bool| keep(code, at_default) && !(dwg_lacks_attachments && at_default);

        // Content type
        self.writer.write_i16(170, style.content_type as i16)?;

        // Draw mleader order type
        self.writer
            .write_i16(171, style.multileader_draw_order as i16)?;

        // Draw leader order type
        self.writer.write_i16(172, style.leader_draw_order as i16)?;

        // Max leader points
        self.writer.write_i32(90, style.max_leader_points)?;

        // First segment angle constraint
        self.writer.write_double(40, style.first_segment_angle)?;

        // Second segment angle constraint
        self.writer.write_double(41, style.second_segment_angle)?;

        // Leader line type
        if keep(173, style.path_type == defaults.path_type) {
            self.writer.write_i16(173, style.path_type as i16)?;
        }

        // Leader line color
        self.write_color_i32(91, style.line_color)?;

        // Leader line type handle (use ByLayer as fallback)
        {
            let h = style
                .line_type_handle
                .filter(|h| *h != Handle::NULL)
                .unwrap_or(self.bylayer_linetype_handle);
            if h != Handle::NULL {
                self.writer.write_handle(340, h)?;
            }
        }

        // Leader line weight
        self.writer
            .write_i32(92, style.line_weight.value() as i32)?;

        // Enable landing
        self.writer.write_bool(290, style.enable_landing)?;

        // Landing gap
        self.writer.write_double(42, style.landing_gap)?;

        // Enable dogleg
        self.writer.write_bool(291, style.enable_dogleg)?;

        // Dogleg length
        self.writer.write_double(43, style.landing_distance)?;

        // The style name is stored in its owner dictionary; code 3 is the description.
        self.writer.write_string(3, &style.description)?;

        // Arrow head block handle
        if let Some(h) = style.arrowhead_handle {
            self.writer.write_handle(341, h)?;
        }

        // Arrow head size
        self.writer.write_double(44, style.arrowhead_size)?;

        // Default mtext contents
        if keep(300, style.default_text == defaults.default_text) {
            self.writer.write_string(300, &style.default_text)?;
        }

        // Text style handle
        if let Some(h) = style.text_style_handle {
            self.writer.write_handle(342, h)?;
        }

        // Text left attachment type
        if keep(174, style.text_left_attachment == defaults.text_left_attachment) {
            self.writer
            .write_i16(174, style.text_left_attachment as i16)?;
        }

        // Text angle type
        self.writer.write_i16(175, style.text_angle_type as i16)?;

        // Text alignment type
        self.writer.write_i16(176, style.text_alignment as i16)?;

        // Text right attachment type
        if keep(178, style.text_right_attachment == defaults.text_right_attachment) {
            self.writer
            .write_i16(178, style.text_right_attachment as i16)?;
        }

        // Text color
        self.write_color_i32(93, style.text_color)?;

        // Text height
        self.writer.write_double(45, style.text_height)?;

        // Enable frame text
        self.writer.write_bool(292, style.text_frame)?;

        // Text always left justify
        self.writer.write_bool(297, style.text_always_left)?;

        // Align space
        if keep(46, style.align_space == defaults.align_space) {
            self.writer.write_double(46, style.align_space)?;
        }

        // Block content handle
        if let Some(h) = style.block_content_handle {
            self.writer.write_handle(343, h)?;
        }

        // Block content color
        self.write_color_i32(94, style.block_content_color)?;

        // Block content scale (x, y, z)
        self.writer.write_double(47, style.block_content_scale_x)?;
        self.writer.write_double(49, style.block_content_scale_y)?;
        self.writer.write_double(140, style.block_content_scale_z)?;

        // Enable block content scale
        self.writer.write_bool(293, style.enable_block_scale)?;

        // Block content rotation
        self.writer
            .write_double(141, style.block_content_rotation)?;

        // Enable block content rotation
        self.writer.write_bool(294, style.enable_block_rotation)?;

        // Block content connection type
        self.writer
            .write_i16(177, style.block_content_connection as i16)?;

        // Scale factor
        self.writer.write_double(142, style.scale_factor)?;

        // Property changed flag
        self.writer.write_bool(295, style.property_changed)?;

        // Is annotative
        self.writer.write_bool(296, style.is_annotative)?;

        // Break gap size
        self.writer.write_double(143, style.break_gap_size)?;

        if self.dxf_version >= DxfVersion::AC1024 {
            self.writer
                .write_i16(271, style.text_attachment_direction as i16)?;
            if keep_attachment(272, style.text_bottom_attachment == defaults.text_bottom_attachment) {
            self.writer
                .write_i16(272, style.text_bottom_attachment as i16)?;
        }
            if keep_attachment(273, style.text_top_attachment == defaults.text_top_attachment) {
            self.writer
                .write_i16(273, style.text_top_attachment as i16)?;
        }
        }
        if self.dxf_version >= DxfVersion::AC1027 {
            self.writer.write_bool(298, style.unknown_flag_298)?;
        }

        Ok(())
    }

    /// Write TableStyle object
    fn write_table_style(&mut self, style: &TableStyle) -> Result<()> {
        self.writer.write_string(0, "TABLESTYLE")?;
        if let Some(raw_dxf_codes) = &style.raw_dxf_codes {
            let has_flags = raw_dxf_codes.iter().any(|(code, _)| *code == 71);
            let has_title_suppressed = raw_dxf_codes
                .iter()
                .skip_while(|(code, _)| *code != 3)
                .any(|(code, _)| *code == 280);
            let has_header_suppressed = raw_dxf_codes.iter().any(|(code, _)| *code == 281);
            let mut saw_name = false;
            for (code, value) in raw_dxf_codes {
                match *code {
                    3 => {
                        self.writer.write_string(*code, value)?;
                        if !saw_name {
                            if !has_flags {
                                self.writer.write_i16(71, style.effective_flags().bits())?;
                            }
                            if !has_title_suppressed {
                                self.writer.write_byte(280, (!style.has_title_row()) as u8)?;
                            }
                            if !has_header_suppressed {
                                self.writer.write_byte(281, (!style.has_header_row()) as u8)?;
                            }
                        }
                        saw_name = true;
                    }
                    71 => self.writer.write_i16(71, style.effective_flags().bits())?,
                    280 if saw_name => {
                        self.writer.write_byte(280, (!style.has_title_row()) as u8)?;
                    }
                    281 => self.writer.write_byte(281, (!style.has_header_row()) as u8)?,
                    _ => self.writer.write_string(*code, value)?,
                }
            }
            return Ok(());
        }
        self.writer.write_handle(5, style.handle)?;
        self.write_pending_attachments()?;
        self.writer.write_handle(330, style.owner_handle)?;
        self.writer.write_subclass("AcDbTableStyle")?;

        self.writer.write_string(3, &style.name)?;

        // Flow direction
        self.writer.write_i16(70, style.flow_direction as i16)?;

        // Flags
        self.writer.write_i16(71, style.effective_flags().bits())?;

        // Horizontal margin
        self.writer.write_double(40, style.horizontal_margin)?;

        // Vertical margin
        self.writer.write_double(41, style.vertical_margin)?;

        // Title suppressed
        self.writer.write_byte(280, (!style.has_title_row()) as u8)?;

        // Header suppressed
        self.writer.write_byte(281, (!style.has_header_row()) as u8)?;

        // Write cell style info for data row
        self.write_table_cell_style(&style.data_row_style)?;
        self.write_table_cell_style(&style.title_row_style)?;
        self.write_table_cell_style(&style.header_row_style)?;
        self.write_annotative_xdata(style.annotative)?;

        Ok(())
    }

    /// Helper to write table cell style
    fn write_table_cell_style(&mut self, style: &crate::objects::RowCellStyle) -> Result<()> {
        self.writer.write_string(7, &style.text_style_name)?;
        self.writer.write_double(140, style.text_height)?;
        self.writer.write_i16(170, style.alignment as i16)?;
        self.write_color_i16(62, style.text_color)?;
        self.write_color_i16(63, style.fill_color)?;
        self.writer.write_byte(283, style.fill_enabled as u8)?;
        self.writer.write_i32(90, style.data_type)?;
        self.writer.write_i32(91, style.unit_type)?;
        self.writer.write_string(1, &style.format_string)?;
        for (index, border) in [
            &style.top_border,
            &style.horizontal_inside_border,
            &style.bottom_border,
            &style.left_border,
            &style.vertical_inside_border,
            &style.right_border,
        ]
        .into_iter()
        .enumerate()
        {
            self.writer
                .write_i16(274 + index as i32, border.line_weight.as_i16())?;
            self.writer
                .write_byte(284 + index as i32, (!border.is_invisible) as u8)?;
            self.write_color_i16(64 + index as i32, border.color)?;
            if let Some(rgb) = border.color.to_true_color_value() {
                self.writer.write_i32(422 + index as i32, rgb)?;
            }
        }
        if let Some(rgb) = style.text_color.to_true_color_value() {
            self.writer.write_i32(420, rgb)?;
        }
        if let Some(rgb) = style.fill_color.to_true_color_value() {
            self.writer.write_i32(421, rgb)?;
        }
        Ok(())
    }

    /// Write Scale object
    fn write_scale(&mut self, scale: &Scale) -> Result<()> {
        self.writer.write_string(0, "SCALE")?;
        self.writer.write_handle(5, scale.handle)?;
        self.write_pending_attachments()?;
        self.writer.write_handle(330, scale.owner_handle)?;
        self.writer.write_subclass("AcDbScale")?;

        // Flag word (always 0 in AutoCAD output); AutoCAD requires it before 300.
        self.writer.write_i16(70, 0)?;

        // Scale name
        self.writer.write_string(300, &scale.name)?;

        // Paper units
        self.writer.write_double(140, scale.paper_units)?;

        // Drawing units
        self.writer.write_double(141, scale.drawing_units)?;

        // Is unit scale
        self.writer.write_bool(290, scale.is_unit_scale)?;

        Ok(())
    }

    /// Write an annotative per-object context leaf (`AcDb*ObjectContextData`)
    /// as DXF group codes: the shared `AcDbObjectContextData` /
    /// `AcDbAnnotScaleObjectContextData` base then the type-specific payload.
    fn write_object_context_data(&mut self, ctx: &ObjectContextData) -> Result<()> {
        self.writer.write_string(0, ctx.class_name())?;
        self.writer.write_handle(5, ctx.handle)?;
        if !ctx.reactors.is_empty() {
            self.writer.write_string(102, "{ACAD_REACTORS")?;
            for r in &ctx.reactors {
                self.writer.write_handle(330, *r)?;
            }
            self.writer.write_string(102, "}")?;
        }
        self.writer.write_handle(330, ctx.owner_handle)?;

        self.writer.write_subclass("AcDbObjectContextData")?;
        self.writer.write_i16(70, ctx.class_version)?;
        self.writer.write_bool(290, ctx.is_default)?;

        self.writer
            .write_subclass("AcDbAnnotScaleObjectContextData")?;
        self.writer.write_handle(340, ctx.scale)?;

        match &ctx.kind {
            ObjectContextKind::AnnotScale => {}
            ObjectContextKind::BlkRef {
                rotation,
                insertion,
                scale_factor,
            } => {
                self.writer.write_subclass("AcDbBlkRefObjectContextData")?;
                self.writer.write_double(50, *rotation)?;
                self.writer.write_double(10, insertion.x)?;
                self.writer.write_double(20, insertion.y)?;
                self.writer.write_double(30, insertion.z)?;
                self.writer.write_double(41, scale_factor.x)?;
                self.writer.write_double(42, scale_factor.y)?;
                self.writer.write_double(43, scale_factor.z)?;
            }
            ObjectContextKind::Text {
                horizontal_mode,
                rotation,
                insertion,
                alignment,
            } => {
                self.writer.write_subclass("AcDbTextObjectContextData")?;
                self.writer.write_i16(70, *horizontal_mode)?;
                self.writer.write_double(50, *rotation)?;
                self.writer.write_double(10, insertion.x)?;
                self.writer.write_double(20, insertion.y)?;
                self.writer.write_double(11, alignment.x)?;
                self.writer.write_double(21, alignment.y)?;
            }
            ObjectContextKind::MText(m) => {
                // AutoCAD writes these fields straight after the
                // AcDbAnnotScaleObjectContextData groups, without an
                // AcDbMTextObjectContextData marker (it rejects the object
                // with one: "Missing DXF group code: 1"), and with the x-axis
                // direction in 10 and the insertion point in 11.
                self.writer.write_i32(70, m.attachment)?;
                self.writer.write_double(10, m.x_axis_dir.x)?;
                self.writer.write_double(20, m.x_axis_dir.y)?;
                self.writer.write_double(30, m.x_axis_dir.z)?;
                self.writer.write_double(11, m.insertion.x)?;
                self.writer.write_double(21, m.insertion.y)?;
                self.writer.write_double(31, m.insertion.z)?;
                self.writer.write_double(40, m.rect_width)?;
                self.writer.write_double(41, m.rect_height)?;
                self.writer.write_double(42, m.extents_width)?;
                self.writer.write_double(43, m.extents_height)?;
                self.writer.write_i32(71, m.column_type)?;
                if m.column_type != 0 {
                    if let Some(c) = &m.columns {
                        self.writer.write_i32(72, c.num_heights)?;
                        self.writer.write_double(44, c.width)?;
                        self.writer.write_double(45, c.gutter)?;
                        self.writer.write_bool(73, c.auto_height)?;
                        self.writer.write_bool(74, c.flow_reversed)?;
                        if !c.auto_height && m.column_type == 2 {
                            for h in &c.heights {
                                self.writer.write_double(46, *h)?;
                            }
                        }
                    }
                }
            }
            ObjectContextKind::Dim(d) => {
                self.writer
                    .write_subclass("AcDbDimensionObjectContextData")?;
                self.writer.write_handle(2, d.block)?;
                self.writer.write_bool(293, d.b293)?;
                self.writer.write_double(10, d.def_pt.x)?;
                self.writer.write_double(20, d.def_pt.y)?;
                self.writer.write_double(30, 0.0)?;
                self.writer.write_bool(294, d.is_def_textloc)?;
                self.writer.write_double(140, d.text_rotation)?;
                self.writer.write_bool(298, d.dimtofl)?;
                self.writer.write_bool(291, d.dimosxd)?;
                self.writer.write_bool(70, d.dimatfit)?;
                self.writer.write_bool(292, d.dimtix)?;
                self.writer.write_bool(71, d.dimtmove)?;
                self.writer.write_i16(280, d.override_code as i16)?;
                self.writer.write_bool(295, d.has_arrow2)?;
                self.writer.write_bool(296, d.flip_arrow2)?;
                self.writer.write_bool(297, d.flip_arrow1)?;
                self.writer.write_subclass(d.subtype.subclass_marker())?;
                let mut wp = |code: i32, p: &crate::types::Vector3| -> Result<()> {
                    self.writer.write_double(code, p.x)?;
                    self.writer.write_double(code + 10, p.y)?;
                    self.writer.write_double(code + 20, p.z)?;
                    Ok(())
                };
                match &d.subtype {
                    DimSubtype::Aligned { dimline_pt } => wp(11, dimline_pt)?,
                    DimSubtype::Angular { arc_pt } => wp(11, arc_pt)?,
                    DimSubtype::Diametric {
                        first_arc_pt,
                        def_pt,
                    } => {
                        wp(11, first_arc_pt)?;
                        wp(12, def_pt)?;
                    }
                    DimSubtype::Radial { first_arc_pt } => wp(11, first_arc_pt)?,
                    DimSubtype::RadialLarge {
                        ovr_center,
                        jog_point,
                    } => {
                        wp(12, ovr_center)?;
                        wp(13, jog_point)?;
                    }
                    DimSubtype::Ordinate {
                        feature_location_pt,
                        leader_endpt,
                    } => {
                        wp(11, feature_location_pt)?;
                        wp(12, leader_endpt)?;
                    }
                }
            }
            ObjectContextKind::MLeader(context) => {
                self.writer.write_subclass("AcDbMLeaderObjectContextData")?;
                self.write_mleader_context_data(context)?;
            }
            ObjectContextKind::MTextAttribute(value) => {
                self.writer.write_i16(70, value.horizontal_mode)?;
                self.writer.write_double(50, value.rotation.to_dxf_degrees())?;
                self.writer.write_point2d(10, value.insertion)?;
                self.writer.write_point2d(11, value.alignment)?;
                let has_context = value.enable_context && value.context.is_some();
                self.writer.write_bool(290, has_context)?;
                if let Some(context) = value.context.as_ref().filter(|_| has_context) {
                    self.writer.write_string(101, "Embedded Object")?;
                    self.writer.write_subclass("AcDbObjectContextData")?;
                    self.writer.write_i16(70, context.class_version)?;
                    self.writer.write_bool(290, context.is_default)?;
                    self.writer
                        .write_subclass("AcDbAnnotScaleObjectContextData")?;
                    self.writer.write_handle(340, context.scale)?;
                    self.writer.write_i32(70, context.mtext.attachment)?;
                    self.writer.write_point3d(10, context.mtext.x_axis_dir)?;
                    self.writer.write_point3d(11, context.mtext.insertion)?;
                    self.writer.write_double(40, context.mtext.rect_width)?;
                    self.writer.write_double(41, context.mtext.rect_height)?;
                    self.writer.write_double(42, context.mtext.extents_width)?;
                    self.writer.write_double(43, context.mtext.extents_height)?;
                    self.writer.write_i32(71, context.mtext.column_type)?;
                    if context.mtext.column_type != 0 {
                        if let Some(columns) = &context.mtext.columns {
                            self.writer.write_i32(72, columns.num_heights)?;
                            self.writer.write_double(44, columns.width)?;
                            self.writer.write_double(45, columns.gutter)?;
                            self.writer.write_bool(73, columns.auto_height)?;
                            self.writer.write_bool(74, columns.flow_reversed)?;
                            if !columns.auto_height && context.mtext.column_type == 2 {
                                for height in &columns.heights {
                                    self.writer.write_double(46, *height)?;
                                }
                            }
                        }
                    }
                }
            }
            ObjectContextKind::Leader(value) => {
                self.writer.write_subclass("AcDbLeaderObjectContextData")?;
                self.writer.write_i32(70, value.points.len() as i32)?;
                for point in &value.points {
                    self.writer.write_point3d(10, *point)?;
                }
                self.writer.write_point3d(11, value.x_direction)?;
                self.writer.write_bool(290, value.annotation_enabled)?;
                self.writer.write_point3d(12, value.insertion_offset)?;
                self.writer.write_point3d(13, value.endpoint_projection)?;
            }
            ObjectContextKind::Fcf {
                location,
                horizontal_direction,
            } => {
                self.writer.write_subclass("AcDbFcfObjectContextData")?;
                self.writer.write_point3d(10, *location)?;
                self.writer.write_point3d(11, *horizontal_direction)?;
            }
            ObjectContextKind::HatchScale(value) => {
                self.write_hatch_scale_context_data(value)?;
            }
            ObjectContextKind::HatchView(value) => {
                self.write_hatch_scale_context_data(&value.hatch)?;
                self.writer.write_subclass("AcDbHatchViewContextData")?;
                self.writer.write_handle(330, value.view)?;
                self.writer.write_point3d(10, value.view_normal)?;
                self.writer
                    .write_double(51, value.view_rotation.to_dxf_degrees())?;
                self.writer.write_bool(290, value.evaluate_hatch)?;
            }
            ObjectContextKind::Opaque => {}
        }

        Ok(())
    }

    fn write_hatch_scale_context_data(
        &mut self,
        value: &crate::objects::HatchScaleContext,
    ) -> Result<()> {
        self.writer.write_subclass("AcDbHatchObjectContextData")?;
        self.writer
            .write_i16(78, value.pattern_lines.len() as i16)?;
        for line in &value.pattern_lines {
            self.writer.write_double(53, line.angle.to_dxf_degrees())?;
            self.writer.write_double(43, line.base_point.x)?;
            self.writer.write_double(44, line.base_point.y)?;
            self.writer.write_double(45, line.offset.x)?;
            self.writer.write_double(46, line.offset.y)?;
            self.writer.write_i16(79, line.dash_lengths.len() as i16)?;
            for dash in &line.dash_lengths {
                self.writer.write_double(49, *dash)?;
            }
        }
        self.writer.write_double(40, value.pattern_scale)?;
        self.writer.write_point3d(10, value.pattern_base)?;
        self.writer.write_i32(90, value.loops.len() as i32)?;
        for loop_data in &value.loops {
            self.writer.write_i32(90, loop_data.loop_type)?;
            self.writer.write_bool(290, loop_data.supports_context)?;
            if !loop_data.supports_context {
                let edges = loop_data
                    .boundary
                    .as_ref()
                    .map(|boundary| boundary.edges.as_slice())
                    .unwrap_or_default();
                if loop_data.loop_type & 2 == 0 {
                    self.writer.write_i32(93, edges.len() as i32)?;
                }
                if let Some(edge @ BoundaryEdge::Polyline(_)) =
                    edges.first().filter(|_| loop_data.loop_type & 2 != 0)
                {
                    self.write_hatch_edge(edge)?;
                } else if loop_data.loop_type & 2 == 0 {
                    for edge in edges {
                        self.write_hatch_edge(edge)?;
                    }
                } else {
                    self.writer.write_i16(72, 0)?;
                    self.writer.write_i16(73, 0)?;
                    self.writer.write_i32(93, 0)?;
                }
            }
        }
        Ok(())
    }

    /// Write SortEntitiesTable object
    fn write_sort_entities_table(&mut self, table: &SortEntitiesTable) -> Result<()> {
        self.writer.write_string(0, "SORTENTSTABLE")?;
        if table.raw_dxf_version == Some(self.dxf_version) {
            if let Some(raw_dxf_codes) = &table.raw_dxf_codes {
                for (code, value) in raw_dxf_codes {
                    self.writer.write_string(*code, value)?;
                }
                return Ok(());
            }
        }
        self.writer.write_handle(5, table.handle)?;
        self.write_pending_attachments()?;
        self.writer.write_handle(330, table.owner_handle)?;
        self.writer.write_subclass("AcDbSortentsTable")?;

        // Block owner handle
        self.writer.write_handle(330, table.block_owner_handle)?;

        // A sort handle is a sort key (AutoCAD itself often uses the entity's own handle), so an entry keeps the
        // one it has. Only a missing or repeated key gets a new handle, allocated in key order so the relative
        // draw order is kept.
        let entries: Vec<_> = table.entries().collect();
        let mut sorted_indices: Vec<usize> = (0..entries.len()).collect();
        sorted_indices.sort_by_key(|&i| entries[i].sort_handle.value());

        let mut new_handles = vec![Handle::NULL; entries.len()];
        let mut used = HashSet::new();
        for &idx in &sorted_indices {
            let key = entries[idx].sort_handle;
            new_handles[idx] = if !key.is_null() && used.insert(key) {
                key
            } else {
                self.allocate_handle()
            };
        }

        // Write entries in original order with new unique handles
        for (i, entry) in entries.iter().enumerate() {
            self.writer.write_handle(331, entry.entity_handle)?;
            self.writer.write_handle(5, new_handles[i])?;
        }

        Ok(())
    }

    /// Write DictionaryVariable object
    fn write_dictionary_variable(&mut self, var: &DictionaryVariable) -> Result<()> {
        self.writer.write_string(0, "DICTIONARYVAR")?;
        self.writer.write_handle(5, var.handle)?;

        // Reactor group: owner dictionary is a reactor
        if var.owner_handle != Handle::NULL {
            self.writer.write_string(102, "{ACAD_REACTORS")?;
            self.writer.write_handle(330, var.owner_handle)?;
            self.writer.write_string(102, "}")?;
        }

        self.writer.write_handle(330, var.owner_handle)?;
        self.writer.write_subclass("DictionaryVariables")?;

        // Schema number
        self.writer.write_byte(280, var.schema_number as u8)?;

        // Value
        self.writer.write_string(1, &var.value)?;

        Ok(())
    }

    fn visual_style_core_properties(obj: &VisualStyle) -> Vec<VisualStyleProperty> {
        obj.core_properties()
    }

    fn visual_style_extended_properties(obj: &VisualStyle) -> Vec<VisualStyleProperty> {
        obj.extended_properties()
    }

    fn visual_style_legacy_properties(obj: &VisualStyle) -> Vec<VisualStyleProperty> {
        obj.legacy_properties()
    }

    /// Write a VISUALSTYLE object
    fn write_visualstyle(&mut self, obj: &VisualStyle) -> Result<()> {
        self.writer.write_string(0, "VISUALSTYLE")?;
        self.writer.write_handle(5, obj.handle)?;
        if !obj.reactors.is_empty() {
            self.writer.write_string(102, "{ACAD_REACTORS")?;
            for reactor in &obj.reactors {
                self.writer.write_handle(330, *reactor)?;
            }
            self.writer.write_string(102, "}")?;
        }
        if let Some(xdictionary) = obj.xdictionary_handle {
            self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
            self.writer.write_handle(360, xdictionary)?;
            self.writer.write_string(102, "}")?;
        }
        self.writer.write_handle(330, obj.owner)?;
        self.writer.write_subclass("AcDbVisualStyle")?;
        self.writer.write_string(2, &obj.description)?;
        self.writer.write_i16(70, obj.style_type)?;
        // The R2013 layout holds every property; the older layouts hold only the core ones. A style that carries
        // more than the core properties is written in the R2013 layout whatever the file version, so none is lost.
        let extended = self.dxf_version >= DxfVersion::AC1027
            || obj.properties.len() > Self::visual_style_core_properties(obj).len();
        if extended {
            let mut properties = Self::visual_style_core_properties(obj);
            properties.extend(Self::visual_style_extended_properties(obj));
            self.writer.write_i16(177, obj.extended_lighting_model)?;
            self.writer.write_bool(291, obj.internal_use_only)?;
            self.writer
                .write_i16(70, properties.len().min(i16::MAX as usize) as i16)?;
            for property in properties.iter().take(i16::MAX as usize) {
                self.write_visual_style_dxf_property(property, None, true)?;
            }
        } else if self.dxf_version >= DxfVersion::AC1024 {
            let properties = Self::visual_style_core_properties(obj);
            self.writer.write_i16(177, obj.extended_lighting_model)?;
            self.writer.write_bool(291, obj.internal_use_only)?;
            let codes = [
                71, 72, 73, 90, 40, 41, 63, 74, 91, 64, 65, 75, 175, 42, 92, 66, 43, 76, 77, 78,
                67, 79, 170, 171, 290, 93, 44, 173,
            ];
            for (property, code) in properties.iter().zip(codes) {
                self.write_visual_style_dxf_property(property, Some(code), true)?;
            }
        } else {
            self.writer.write_i16(71, obj.face_lighting_model)?;
            self.writer.write_i16(72, obj.face_lighting_quality)?;
            self.writer.write_i16(73, obj.face_color_mode)?;
            self.writer.write_i32(90, obj.face_modifier)?;
            self.writer.write_i32(74, obj.edge_model)?;
            self.writer.write_i32(91, obj.edge_style)?;
            let properties = Self::visual_style_legacy_properties(obj);
            let codes = [
                40, 41, 63, 64, 65, 75, 42, 92, 66, 43, 76, 77, 78, 67, 79, 170, 171, 290, 174,
                175, 93, 44, 173, 45,
            ];
            for (property, code) in properties.iter().zip(codes) {
                self.write_visual_style_dxf_property(property, Some(code), false)?;
            }
            self.writer.write_bool(291, obj.internal_use_only)?;
        }
        Ok(())
    }

    fn write_visual_style_dxf_property(
        &mut self,
        property: &VisualStyleProperty,
        code: Option<i32>,
        write_enabled: bool,
    ) -> Result<()> {
        match &property.value {
            VisualStylePropertyValue::Short(value) => {
                let code = code.unwrap_or(90);
                if (60..=79).contains(&code)
                    || (170..=179).contains(&code)
                    || (270..=289).contains(&code)
                {
                    self.writer.write_i16(code, *value)?;
                } else {
                    self.writer.write_i32(code, *value as i32)?;
                }
            }
            VisualStylePropertyValue::Long(value) => {
                let code = code.unwrap_or(90);
                if (60..=79).contains(&code)
                    || (170..=179).contains(&code)
                    || (270..=289).contains(&code)
                {
                    self.writer.write_i16(code, *value as i16)?;
                } else {
                    self.writer.write_i32(code, *value)?;
                }
            }
            VisualStylePropertyValue::Double(value) => {
                self.writer.write_double(code.unwrap_or(40), *value)?;
            }
            VisualStylePropertyValue::Bool(value) => {
                self.writer.write_bool(code.unwrap_or(290), *value)?;
            }
            VisualStylePropertyValue::Color(value) => {
                self.writer.write_color(code.unwrap_or(62), value.clone())?;
                if let Some(true_color) = value.to_true_color_value() {
                    self.writer.write_i32(420, true_color)?;
                }
            }
            VisualStylePropertyValue::Text(value) => {
                self.writer.write_string(code.unwrap_or(1), value)?;
            }
        }
        if write_enabled {
            self.writer.write_i16(176, property.enabled)?;
        }
        Ok(())
    }

    /// Write a MATERIAL object
    fn write_material(&mut self, obj: &Material) -> Result<()> {
        self.writer.write_string(0, "MATERIAL")?;
        self.writer.write_handle(5, obj.handle)?;
        if !obj.reactors.is_empty() {
            self.writer.write_string(102, "{ACAD_REACTORS")?;
            for reactor in &obj.reactors {
                self.writer.write_handle(330, *reactor)?;
            }
            self.writer.write_string(102, "}")?;
        }
        if let Some(xdictionary) = obj.xdictionary_handle {
            self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
            self.writer.write_handle(360, xdictionary)?;
            self.writer.write_string(102, "}")?;
        }
        self.writer.write_handle(330, obj.owner)?;
        self.writer.write_subclass("AcDbMaterial")?;
        self.writer.write_string(1, &obj.name)?;
        // Built-in materials (ByLayer / ByBlock / Global) of a document that did not come from DXF are written
        // in the minimal form BricsCAD itself exports (their full default field set is rejected by BricsCAD's
        // audit, issue #51); a value that differs from the default is still written. A material read from DXF
        // writes the groups its source record had, plus those whose value differs from the native default.
        let builtin = matches!(
            obj.name.to_ascii_uppercase().as_str(),
            "BYLAYER" | "BYBLOCK" | "GLOBAL"
        );
        let gate = MaterialGate { source: obj.dxf_source.as_deref(), minimal: builtin };
        let defaults = Material::new();
        // Translucence, self-illumination, reflectivity, illumination model, channel flags and mode are R2007+
        // fields of a DWG material: from an older DWG they hold the native default, which the file never stated,
        // so only a value that was changed is written.
        let r2007_fields = !self.dwg_source.is_some_and(|version| version < DxfVersion::AC1021);
        let stated_r2007 = |code: usize, differs: bool| {
            if r2007_fields {
                gate.writes(code, differs)
            } else {
                differs
            }
        };
        if !obj.description.is_empty() {
            self.writer.write_string(2, &obj.description)?;
        }
        self.write_material_dxf_color(&obj.ambient_color, &defaults.ambient_color, 70, 40, 90, Some(&gate))?;
        self.write_material_dxf_color(&obj.diffuse_color, &defaults.diffuse_color, 71, 41, 91, Some(&gate))?;
        self.write_material_dxf_map(&obj.diffuse_map, &gate, 42, 72, 3, 73, 74, 75, 43)?;
        self.write_material_dxf_color(&obj.specular_color, &defaults.specular_color, 76, 45, 92, Some(&gate))?;
        if gate.writes(44, obj.specular_gloss_factor != defaults.specular_gloss_factor) {
            self.writer.write_double(44, obj.specular_gloss_factor)?;
        }
        self.write_material_dxf_map(&obj.specular_map, &gate, 46, 77, 4, 78, 79, 170, 47)?;
        self.write_material_dxf_map(&obj.reflection_map, &gate, 48, 171, 6, 172, 173, 174, 49)?;
        if gate.writes(140, obj.opacity_percent != defaults.opacity_percent) {
            self.writer.write_double(140, obj.opacity_percent)?;
        }
        self.write_material_dxf_map(&obj.opacity_map, &gate, 141, 175, 7, 176, 177, 178, 142)?;
        self.write_material_dxf_map(&obj.bump_map, &gate, 143, 179, 8, 270, 271, 272, 144)?;
        if gate.writes(145, obj.refraction_index != defaults.refraction_index) {
            self.writer.write_double(145, obj.refraction_index)?;
        }
        self.write_material_dxf_map(&obj.refraction_map, &gate, 146, 273, 9, 274, 275, 276, 147)?;
        if stated_r2007(148, obj.translucence != defaults.translucence) {
            self.writer.write_double(148, obj.translucence)?;
        }
        // Group 90 is the self-illumination integer unless it is the first one of an RGB ambient colour. It is not
        // written at the default: ACadSharp maps every group 90 to the ambient colour, the last one winning.
        let source_has_illumination = obj.dxf_source.as_deref().is_some_and(|source| {
            source.contains(90) && obj.ambient_color.flag != 1
        });
        if source_has_illumination || obj.self_illumination != defaults.self_illumination {
            self.writer
                .write_i32(90, obj.self_illumination.round() as i32)?;
        }
        if stated_r2007(149, obj.self_illumination != defaults.self_illumination) {
            self.writer.write_double(149, obj.self_illumination)?;
        }
        if stated_r2007(468, obj.reflectivity != defaults.reflectivity) {
            self.writer.write_double(468, obj.reflectivity)?;
        }
        if stated_r2007(93, obj.illumination_model != defaults.illumination_model) {
            self.writer.write_i32(93, obj.illumination_model)?;
        }
        // Built-in materials always carry the channel flags; the others when the source or an edit has them.
        if builtin && obj.dxf_source.is_none() && r2007_fields
            || stated_r2007(94, obj.channel_flags != defaults.channel_flags)
        {
            self.writer.write_i32(94, obj.channel_flags)?;
        }
        if stated_r2007(282, obj.mode != defaults.mode) {
            self.writer.write_i16(282, obj.mode as i16)?;
        }
        // Advanced properties (460-465, 290, 293, 270-273 and the normal map)
        // are not part of the MATERIAL body: the reference application rejects
        // them here ("Unexpected DXF group code: 460") and stores them in the
        // ADVMATERIAL XRECORD of the material's extension dictionary, which
        // `write_xrecord` refreshes from the model.
        Ok(())
    }

    fn write_material_dxf_color(
        &mut self,
        value: &MaterialColor,
        default: &MaterialColor,
        flag_code: i32,
        factor_code: i32,
        rgb_code: i32,
        gate: Option<&MaterialGate>,
    ) -> Result<()> {
        let writes = |code: i32, differs: bool| gate.is_none_or(|gate| gate.writes(code as usize, differs));
        if writes(flag_code, value.flag != default.flag) {
            self.writer.write_i16(flag_code, value.flag as i16)?;
        }
        if writes(factor_code, value.factor != default.factor) {
            self.writer.write_double(factor_code, value.factor)?;
        }
        if value.flag == 1 && (value.rgb.is_some() || gate.is_none_or(|gate| gate.source.is_none())) {
            self.writer
                .write_i32(rgb_code, value.rgb.unwrap_or_default())?;
        }
        Ok(())
    }

    fn write_material_dxf_texture(&mut self, value: &MaterialTexture) -> Result<()> {
        self.writer.write_i16(277, value.mode)?;
        match value.mode {
            0 => {
                self.write_material_dxf_color(&value.color1, &MaterialColor::default(), 278, 460, 95, None)?;
                self.write_material_dxf_color(&value.color2, &MaterialColor::default(), 279, 461, 96, None)?;
            }
            1 => {
                self.write_material_dxf_color(&value.color1, &MaterialColor::default(), 280, 465, 97, None)?;
                self.write_material_dxf_color(&value.color2, &MaterialColor::default(), 281, 466, 98, None)?;
            }
            2 => match &value.procedural {
                Some(MaterialProceduralValue::Bool(item)) => {
                    self.writer.write_bool(291, *item)?;
                }
                Some(MaterialProceduralValue::Integer(item)) => {
                    self.writer.write_i16(271, *item)?;
                }
                Some(MaterialProceduralValue::Real(item)) => {
                    self.writer.write_double(469, *item)?;
                }
                Some(MaterialProceduralValue::Color(item)) => {
                    self.writer.write_color(62, item.clone())?;
                    if let Some(true_color) = item.to_true_color_value() {
                        self.writer.write_i32(420, true_color)?;
                    }
                }
                Some(MaterialProceduralValue::Text(item)) => {
                    self.writer.write_string(301, item)?;
                }
                Some(MaterialProceduralValue::Table(items)) => {
                    for (name, texture) in items.iter().take(i16::MAX as usize) {
                        self.writer.write_string(300, name)?;
                        self.write_material_dxf_texture(texture)?;
                    }
                    self.writer.write_bool(292, value.table_end)?;
                }
                None => {}
            },
            _ => {}
        }
        Ok(())
    }

    fn write_material_dxf_map(
        &mut self,
        value: &MaterialMap,
        gate: &MaterialGate,
        blend_code: i32,
        source_code: i32,
        file_code: i32,
        projection_code: i32,
        tiling_code: i32,
        auto_transform_code: i32,
        matrix_code: i32,
    ) -> Result<()> {
        let default = MaterialMap::default();
        // Field order follows the DXF specification: blend factor, file
        // name, source, projection, tiling, auto-transform, matrix. The
        // file name is only written when set - BricsCAD's audit rejects
        // materials with an empty map file name (issue #51).
        if gate.writes(blend_code as usize, value.blend_factor != default.blend_factor) {
            self.writer.write_double(blend_code, value.blend_factor)?;
        }
        if !value.file_name.is_empty() {
            self.writer.write_string(file_code, &value.file_name)?;
        }
        if gate.writes(source_code as usize, value.source != default.source) {
            self.writer.write_i16(source_code, value.source as i16)?;
        }
        if gate.writes(projection_code as usize, value.projection != default.projection) {
            self.writer
                .write_i16(projection_code, value.projection as i16)?;
        }
        if gate.writes(tiling_code as usize, value.tiling != default.tiling) {
            self.writer.write_i16(tiling_code, value.tiling as i16)?;
        }
        if gate.writes(auto_transform_code as usize, value.auto_transform != default.auto_transform) {
            self.writer
                .write_i16(auto_transform_code, value.auto_transform as i16)?;
        }
        if gate.writes(matrix_code as usize, value.transform != default.transform) {
            for item in value.transform {
                self.writer.write_double(matrix_code, item)?;
            }
        }
        if value.source == 2 {
            if let Some(texture) = &value.texture {
                self.write_material_dxf_texture(texture)?;
            } else {
                self.write_material_dxf_texture(&MaterialTexture::default())?;
            }
        }
        Ok(())
    }

    /// Write an IMAGEDEF_REACTOR object
    fn write_imagedef_reactor(&mut self, obj: &ImageDefinitionReactor) -> Result<()> {
        self.writer.write_string(0, "IMAGEDEF_REACTOR")?;
        self.writer.write_handle(5, obj.handle)?;
        self.write_pending_attachments()?;
        self.writer.write_handle(330, obj.owner)?;
        self.writer.write_subclass("AcDbRasterImageDefReactor")?;
        self.writer.write_i32(90, 2)?; // class version
        self.writer.write_handle(330, obj.image_handle)?;
        Ok(())
    }

    fn write_geodata(&mut self, obj: &GeoData) -> Result<()> {
        self.writer.write_string(0, "GEODATA")?;
        self.writer.write_handle(5, obj.handle)?;
        if !obj.reactors.is_empty() {
            self.writer.write_string(102, "{ACAD_REACTORS")?;
            for reactor in &obj.reactors {
                self.writer.write_handle(330, *reactor)?;
            }
            self.writer.write_string(102, "}")?;
        }
        if let Some(xdictionary) = obj.xdictionary_handle {
            self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
            self.writer.write_handle(360, xdictionary)?;
            self.writer.write_string(102, "}")?;
        }
        self.writer.write_handle(330, obj.owner)?;
        self.writer.write_subclass("AcDbGeoData")?;
        self.writer.write_i32(90, obj.version)?;
        self.writer.write_handle(330, obj.host_block)?;
        self.writer.write_i16(70, obj.coordinate_type)?;
        if obj.version == 1 {
            self.writer.write_double(40, obj.reference_point.y)?;
            self.writer.write_double(41, obj.reference_point.x)?;
            self.writer.write_double(42, obj.reference_point.z)?;
            self.writer.write_i32(91, obj.horizontal_units)?;
            self.writer.write_point3d(10, obj.design_point)?;
            self.writer
                .write_point3d(11, obj.obsolete_observation_point)?;
            let north_angle =
                std::f64::consts::FRAC_PI_2 - obj.north_direction.y.atan2(obj.north_direction.x);
            self.writer.write_double(52, north_angle.to_dxf_degrees())?;
            self.writer.write_double(43, obj.obsolete_scale_vector.x)?;
            self.writer.write_double(44, obj.obsolete_scale_vector.y)?;
            self.writer.write_double(45, obj.obsolete_scale_vector.z)?;
            self.writer.write_double(46, obj.horizontal_unit_scale)?;
        } else {
            self.writer.write_point3d(10, obj.design_point)?;
            self.writer.write_point3d(11, obj.reference_point)?;
            self.writer.write_point2d(12, obj.north_direction)?;
            self.writer.write_double(40, obj.horizontal_unit_scale)?;
            self.writer.write_double(41, obj.vertical_unit_scale)?;
            self.writer.write_i32(91, obj.horizontal_units)?;
            self.writer.write_i32(92, obj.vertical_units)?;
        }
        self.writer.write_point3d(210, obj.up_direction)?;
        self.writer.write_i32(95, obj.scale_estimation_method)?;
        self.writer.write_double(141, obj.user_scale_factor)?;
        self.writer.write_bool(294, obj.sea_level_correction)?;
        self.writer.write_double(142, obj.sea_level_elevation)?;
        self.writer
            .write_double(143, obj.coordinate_projection_radius)?;
        self.writer
            .write_xrecord_string(301, &obj.coordinate_system_definition)?;
        if obj.version == 1 {
            self.writer
                .write_string(303, &obj.coordinate_system_datum)?;
            self.writer.write_string(304, &obj.coordinate_system_wkt)?;
        }
        self.writer.write_string(302, &obj.geo_rss_tag)?;
        self.writer.write_string(305, &obj.observation_from_tag)?;
        self.writer.write_string(306, &obj.observation_to_tag)?;
        self.writer
            .write_string(307, &obj.observation_coverage_tag)?;
        self.writer.write_i32(93, obj.mesh_points.len() as i32)?;
        for point in &obj.mesh_points {
            let source_code = if obj.version == 1 { 12 } else { 13 };
            self.writer.write_point2d(source_code, point.source)?;
            self.writer
                .write_point2d(source_code + 1, point.destination)?;
        }
        self.writer.write_i32(96, obj.mesh_faces.len() as i32)?;
        for face in &obj.mesh_faces {
            self.writer.write_i32(97, face.first)?;
            self.writer.write_i32(98, face.second)?;
            self.writer.write_i32(99, face.third)?;
        }
        if obj.version == 1 {
            self.writer.write_string(3, "CIVIL3D_DATA_BEGIN")?;
            self.writer.write_bool(292, obj.civil_obsolete_flag)?;
            self.writer.write_point2d(14, obj.civil_reference_point1)?;
            self.writer.write_point2d(15, obj.civil_reference_point2)?;
            self.writer.write_i32(93, obj.civil_unknown1)?;
            self.writer.write_i32(94, obj.civil_unknown2)?;
            self.writer.write_bool(293, obj.civil_unknown_flag1)?;
            self.writer.write_point2d(16, obj.civil_zero_point1)?;
            self.writer.write_point2d(17, obj.civil_zero_point2)?;
            self.writer
                .write_double(54, obj.civil_north_angle_degrees)?;
            self.writer
                .write_double(140, obj.civil_north_angle_radians)?;
            self.writer.write_i32(95, obj.scale_estimation_method)?;
            self.writer.write_double(141, obj.user_scale_factor)?;
            self.writer.write_bool(294, obj.sea_level_correction)?;
            self.writer.write_double(142, obj.sea_level_elevation)?;
            self.writer
                .write_double(143, obj.coordinate_projection_radius)?;
            self.writer.write_string(4, "CIVIL3D_DATA_END")?;
        }
        Ok(())
    }

    fn write_data_object_header(&mut self, obj: &DataObject, subclass: &str) -> Result<()> {
        self.writer.write_string(0, obj.dxf_name())?;
        self.writer.write_handle(5, obj.handle)?;
        if !obj.reactors.is_empty() {
            self.writer.write_string(102, "{ACAD_REACTORS")?;
            for reactor in &obj.reactors {
                self.writer.write_handle(330, *reactor)?;
            }
            self.writer.write_string(102, "}")?;
        }
        if let Some(xdictionary) = obj.xdictionary_handle {
            self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
            self.writer.write_handle(360, xdictionary)?;
            self.writer.write_string(102, "}")?;
        }
        self.writer.write_handle(330, obj.owner)?;
        if !subclass.is_empty() {
            self.writer.write_subclass(subclass)?;
        }
        Ok(())
    }

    fn write_data_object_dxf(&mut self, obj: &DataObject) -> Result<()> {
        match &obj.data {
            DataObjectData::BreakData(value) => {
                self.write_data_object_header(obj, "AcDbBreakData")?;
                self.writer.write_i16(70, value.version)?;
                self.writer.write_handle(330, value.dimension_reference)?;
                self.writer
                    .write_i32(90, value.point_references.len() as i32)?;
                for reference in &value.point_references {
                    self.writer.write_subclass("AcDbBreakPointRef")?;
                    self.writer.write_i32(71, reference.reference_type)?;
                    self.writer.write_i16(72, reference.flags)?;
                    self.writer.write_i32(91, reference.identifier)?;
                    self.writer.write_point3d(10, reference.first_point)?;
                    self.writer.write_point3d(11, reference.second_point)?;
                }
            }
            DataObjectData::BreakPointRef => {
                self.write_data_object_header(obj, "AcDbBreakPointRef")?;
            }
            DataObjectData::CellStyleMap(value) => {
                self.write_data_object_header(obj, "AcDbCellStyleMap")?;
                self.writer.write_i32(90, value.cells.len() as i32)?;
                for cell in &value.cells {
                    self.write_named_table_cell_style_dxf(cell)?;
                }
            }
            DataObjectData::AcDsRecord => {
                self.write_data_object_header(obj, "AcDsRecord")?;
            }
            DataObjectData::AcDsSchema => {
                self.write_data_object_header(obj, "AcDsSchema")?;
            }
            DataObjectData::Dummy => {
                self.write_data_object_header(obj, "")?;
            }
            DataObjectData::IdBuffer(value) => {
                self.write_data_object_header(obj, "AcDbIdBuffer")?;
                for handle in &value.object_ids {
                    self.writer.write_handle(330, *handle)?;
                }
            }
            DataObjectData::Index(value) => {
                self.write_data_object_header(obj, "AcDbIndex")?;
                let timestamp = value.last_updated_julian_day as f64
                    + value.last_updated_milliseconds as f64 / 86_400_000.0;
                self.writer.write_double(40, timestamp)?;
            }
            DataObjectData::LayerIndex(value) => {
                self.write_data_object_header(obj, "AcDbIndex")?;
                let timestamp = value.last_updated_julian_day as f64
                    + value.last_updated_milliseconds as f64 / 86_400_000.0;
                self.writer.write_double(40, timestamp)?;
                self.writer.write_subclass("AcDbLayerIndex")?;
                self.writer.write_i32(90, 0)?;
                for entry in &value.entries {
                    self.writer.write_string(8, &entry.name)?;
                    self.writer.write_handle(360, entry.id_buffer)?;
                    self.writer.write_i32(90, entry.layer_count)?;
                }
            }
            DataObjectData::PartialViewingFilter(_) => {
                self.write_data_object_header(obj, "AcDbFilter")?;
            }
            DataObjectData::LongTransaction => {
                self.write_data_object_header(obj, "AcDbLongTransaction")?;
            }
            DataObjectData::ObjectPointer => {
                self.write_data_object_header(obj, "CAseDLPNTableRecord")?;
            }
            DataObjectData::TableGeometry(value) => {
                self.write_data_object_header(obj, "AcDbTableGeometry")?;
                self.writer.write_i32(90, value.rows)?;
                self.writer.write_i32(91, value.columns)?;
                self.writer.write_i32(92, value.cells.len() as i32)?;
                for cell in &value.cells {
                    self.writer.write_i32(93, cell.geometry_data_flag)?;
                    self.writer.write_double(40, cell.width_with_gap)?;
                    self.writer.write_double(41, cell.height_with_gap)?;
                    self.writer.write_handle(330, cell.table_geometry)?;
                    self.writer.write_i32(94, cell.geometry.len() as i32)?;
                    for geometry in &cell.geometry {
                        self.writer
                            .write_point3d(10, geometry.distance_to_top_left)?;
                        self.writer.write_point3d(11, geometry.distance_to_center)?;
                        self.writer.write_double(43, geometry.width)?;
                        self.writer.write_double(44, geometry.height)?;
                        self.writer.write_double(45, geometry.outer_width)?;
                        self.writer.write_double(46, geometry.outer_height)?;
                        self.writer.write_i32(95, geometry.flags)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn write_named_table_cell_style_dxf(
        &mut self,
        value: &crate::objects::NamedTableCellStyle,
    ) -> Result<()> {
        self.writer.write_string(300, "CELLSTYLE")?;
        self.write_table_cell_style_data_dxf(&value.cell_style)?;
        self.writer.write_string(1, "CELLSTYLE_BEGIN")?;
        self.writer.write_i32(90, value.id)?;
        self.writer.write_i32(91, value.style_type)?;
        self.writer.write_string(300, &value.name)?;
        self.writer.write_string(309, "CELLSTYLE_END")?;
        Ok(())
    }

    fn write_table_cell_style_data_dxf(
        &mut self,
        value: &crate::objects::TableCellStyleData,
    ) -> Result<()> {
        self.writer.write_string(1, "TABLEFORMAT_BEGIN")?;
        self.writer.write_i32(90, value.style_type)?;
        self.writer.write_i16(170, value.data_flags)?;
        if value.data_flags != 0 {
            self.writer.write_i32(91, value.property_override_flags)?;
            self.writer.write_i32(92, value.merge_flags)?;
            self.writer.write_color(62, value.background_color)?;
            if let Some(rgb) = value.background_color.to_true_color_value() {
                self.writer.write_i32(420, rgb)?;
            }
            self.writer.write_i32(93, value.content_layout)?;
            let format = &value.content_format;
            self.writer.write_string(300, "CONTENTFORMAT")?;
            self.writer.write_string(1, "CONTENTFORMAT_BEGIN")?;
            self.writer.write_i32(90, format.property_override_flags)?;
            self.writer.write_i32(91, format.property_flags)?;
            self.writer.write_i32(92, format.value_data_type)?;
            self.writer.write_i32(93, format.value_unit_type)?;
            self.writer.write_string(300, &format.value_format_string)?;
            self.writer.write_double(40, format.rotation)?;
            self.writer.write_double(140, format.block_scale)?;
            self.writer.write_i32(94, format.cell_alignment)?;
            self.writer.write_color(62, format.content_color)?;
            if let Some(rgb) = format.content_color.to_true_color_value() {
                self.writer.write_i32(420, rgb)?;
            }
            self.writer.write_handle(340, format.text_style)?;
            self.writer.write_double(144, format.text_height)?;
            self.writer.write_string(309, "CONTENTFORMAT_END")?;
            self.writer.write_i16(171, value.margin_override_flags)?;
            if value.margin_override_flags != 0 {
                self.writer.write_string(301, "MARGIN")?;
                self.writer.write_string(1, "CELLMARGIN_BEGIN")?;
                self.writer.write_double(40, value.vertical_margin)?;
                self.writer.write_double(40, value.horizontal_margin)?;
                self.writer.write_double(40, value.bottom_margin)?;
                self.writer.write_double(40, value.right_margin)?;
                self.writer.write_double(40, value.horizontal_spacing)?;
                self.writer.write_double(40, value.vertical_spacing)?;
                self.writer.write_string(309, "CELLMARGIN_END")?;
            }
            self.writer
                .write_i32(94, value.borders.len().min(6) as i32)?;
            for grid in value.borders.iter().take(6) {
                if grid.index_mask != 0 {
                    self.writer.write_i32(95, grid.index_mask)?;
                    self.writer.write_string(302, "GRIDFORMAT")?;
                    self.writer.write_string(1, "GRIDFORMAT_BEGIN")?;
                    self.writer
                        .write_i32(90, grid.border.property_flags.bits())?;
                    self.writer.write_i32(91, grid.border.border_type as i32)?;
                    self.writer.write_color(62, grid.border.color)?;
                    if let Some(rgb) = grid.border.color.to_true_color_value() {
                        self.writer.write_i32(420, rgb)?;
                    }
                    self.writer
                        .write_i32(92, grid.border.line_weight.as_i16() as i32)?;
                    self.writer.write_handle(340, grid.line_type)?;
                    self.writer
                        .write_i32(93, (!grid.border.is_invisible) as i32)?;
                    self.writer
                        .write_double(40, grid.border.double_line_spacing)?;
                }
                self.writer.write_string(309, "GRIDFORMAT_END")?;
            }
        }
        self.writer.write_string(309, "TABLEFORMAT_END")?;
        Ok(())
    }

    /// Write a SPATIAL_FILTER object (block reference / XCLIP clip boundary).
    ///
    /// Inverse of [`read_spatial_filter`]. The two 4×3 transforms are emitted
    /// as 12 code-40 doubles each, in column-major order, after the front/back
    /// clip flags and distances.
    fn write_spatial_filter(&mut self, obj: &SpatialFilter, document: &CadDocument) -> Result<()> {
        self.writer.write_string(0, "SPATIAL_FILTER")?;
        self.writer.write_handle(5, obj.handle)?;
        self.write_table_entry_reactors(obj.handle, document)?;
        // An inverted clip keeps its drawn boundary in the filter's extension
        // dictionary; without the link that dictionary is left unreferenced.
        self.write_table_entry_xdictionary(obj.handle, document)?;
        self.writer.write_handle(330, obj.owner)?;
        self.writer.write_subclass("AcDbFilter")?;
        self.writer.write_subclass("AcDbSpatialFilter")?;
        self.writer
            .write_i16(70, obj.boundary_points.len() as i16)?;
        for p in &obj.boundary_points {
            self.writer.write_point2d(10, *p)?;
        }
        self.writer.write_point3d(210, obj.normal)?;
        self.writer.write_point3d(11, obj.origin)?;
        self.writer.write_i16(71, obj.display_enabled as i16)?;
        self.writer.write_i16(72, obj.front_clip.is_some() as i16)?;
        if let Some(d) = obj.front_clip {
            self.writer.write_double(40, d)?;
        }
        self.writer.write_i16(73, obj.back_clip.is_some() as i16)?;
        if let Some(d) = obj.back_clip {
            self.writer.write_double(41, d)?;
        }
        self.write_matrix_4x3(&obj.inverse_block_transform)?;
        self.write_matrix_4x3(&obj.clip_bound_transform)?;
        Ok(())
    }

    /// Write a `SPATIAL_FILTER` transform as 12 code-40 doubles the way
    /// AutoCAD does: one matrix row per group of four, translation last
    /// (the bottom row is implied). See `matrix_from_dxf_4x3` in the reader.
    fn write_matrix_4x3(&mut self, m: &crate::types::Matrix4) -> Result<()> {
        for row in 0..3 {
            for col in 0..4 {
                self.writer.write_double(40, m.m[row][col])?;
            }
        }
        Ok(())
    }

    /// Write a RASTERVARIABLES object
    fn write_raster_variables(&mut self, obj: &RasterVariables) -> Result<()> {
        self.writer.write_string(0, "RASTERVARIABLES")?;
        self.writer.write_handle(5, obj.handle)?;
        self.write_pending_attachments()?;
        self.writer.write_handle(330, obj.owner)?;
        self.writer.write_subclass("AcDbRasterVariables")?;
        self.writer.write_i32(90, obj.class_version)?;
        self.writer.write_i16(70, obj.display_image_frame)?;
        self.writer.write_i16(71, obj.image_quality)?;
        self.writer.write_i16(72, obj.units)?;
        Ok(())
    }

    /// Write a DBCOLOR object
    fn write_bookcolor(&mut self, obj: &BookColor) -> Result<()> {
        self.writer.write_string(0, "DBCOLOR")?;
        self.writer.write_handle(5, obj.handle)?;
        self.write_pending_attachments()?;
        self.writer.write_handle(330, obj.owner)?;
        self.writer.write_subclass("AcDbColor")?;
        // A source record without a color keeps none (the reader then applies its own default color).
        let source_without_color =
            obj.dxf_color_present == Some(false) && obj.color == crate::types::Color::ByLayer;
        if !source_without_color {
            self.writer.write_i16(62, obj.color.approximate_index())?;
            if let Some(true_color) = obj.color.to_true_color_value() {
                self.writer.write_i32(420, true_color)?;
            }
        }
        if let Some(name) =
            crate::io::dxf::join_color_book_name(Some(&obj.book_name), Some(&obj.color_name))
        {
            self.writer.write_string(430, &name)?;
        }
        Ok(())
    }

    /// Write an ACDBDICTIONARYWDFLT object
    fn write_dict_with_default(
        &mut self,
        obj: &DictionaryWithDefault,
        objects: &std::collections::HashMap<Handle, ObjectType>,
    ) -> Result<()> {
        self.writer.write_string(0, "ACDBDICTIONARYWDFLT")?;
        self.writer.write_handle(5, obj.handle)?;
        self.write_pending_attachments()?;
        self.writer.write_handle(330, obj.owner)?;
        self.writer.write_subclass("AcDbDictionary")?;
        // BricsCAD's own DXF export does not write the hard-owner flag (280)
        // for ACDBDICTIONARYWDFLT records; emitting it makes BricsCAD's
        // audit reject the plot style references (issue #51).
        self.writer.write_i16(281, obj.duplicate_cloning)?;
        for (key, handle) in &obj.entries {
            if !objects.contains_key(handle) {
                continue;
            }
            self.writer.write_string(3, key)?;
            self.writer.write_handle(350, *handle)?;
        }
        self.writer.write_subclass("AcDbDictionaryWithDefault")?;
        self.writer.write_handle(340, obj.default_handle)?;
        Ok(())
    }

    /// Write a WIPEOUTVARIABLES object
    fn write_wipeout_variables(&mut self, obj: &WipeoutVariables) -> Result<()> {
        self.writer.write_string(0, "WIPEOUTVARIABLES")?;
        self.writer.write_handle(5, obj.handle)?;
        self.write_pending_attachments()?;
        self.writer.write_handle(330, obj.owner)?;
        self.writer.write_subclass("AcDbWipeoutVariables")?;
        self.writer.write_i16(70, obj.display_frame)?;
        Ok(())
    }

    fn write_block_visibility_parameter(&mut self, obj: &BlockVisibilityParameter) -> Result<()> {
        self.writer.write_string(0, "BLOCKVISIBILITYPARAMETER")?;
        self.writer.write_handle(5, obj.handle)?;
        self.writer.write_handle(330, obj.owner)?;
        // Same AcDbEvalExpr layout as every other parameter: a -9999 "no
        // value" sentinel emits nothing after 99 (a "70 -9999" pair makes
        // AutoCAD reject the object), otherwise an empty expression string,
        // the value type, then the value.
        self.write_dynamic_eval_dxf(&crate::objects::BlockEvalExpression {
            parent_id: crate::objects::BlockEvalExpression::NO_PARENT,
            major: obj.eval_major,
            minor: obj.eval_minor,
            value_code: obj.eval_value_code,
            value: obj.eval_value.clone(),
            node_id: obj.eval_node_id,
        })?;
        self.writer.write_subclass("AcDbBlockElement")?;
        self.writer.write_string(300, &obj.element_name)?;
        self.writer.write_i32(98, obj.element_major)?;
        self.writer.write_i32(99, obj.element_minor)?;
        self.writer.write_i32(1071, obj.element_eed_1071)?;
        self.writer.write_subclass("AcDbBlockParameter")?;
        self.writer.write_bool(280, obj.show_properties)?;
        self.writer.write_bool(281, obj.chain_actions)?;
        self.writer.write_subclass("AcDbBlock1PtParameter")?;
        self.writer.write_double(1010, obj.def_point.x)?;
        self.writer.write_double(1020, obj.def_point.y)?;
        self.writer.write_double(1030, obj.def_point.z)?;
        self.writer.write_i32(93, obj.property_info_count)?;
        for (index, property) in obj.property_info.iter().enumerate() {
            let count_code = 170 + index as i32;
            let value_code = 91 + index as i32;
            let name_code = 301 + index as i32;
            self.writer
                .write_i32(count_code, property.connections.len() as i32)?;
            for connection in &property.connections {
                self.writer.write_i32(value_code, connection.code)?;
                self.writer.write_string(name_code, &connection.name)?;
            }
        }
        self.writer.write_subclass("AcDbBlockVisibilityParameter")?;
        self.writer.write_bool(281, obj.is_initialized)?;
        self.writer.write_string(301, &obj.name)?;
        self.writer.write_string(302, &obj.description)?;
        self.writer.write_bool(91, obj.unknown_bool)?;
        self.writer.write_i32(93, obj.all_blocks.len() as i32)?;
        for handle in &obj.all_blocks {
            self.writer.write_handle(331, *handle)?;
        }
        self.writer.write_i32(92, obj.states.len() as i32)?;
        for state in &obj.states {
            self.writer.write_string(303, &state.name)?;
            self.writer
                .write_i32(94, state.visible_blocks.len() as i32)?;
            for handle in &state.visible_blocks {
                self.writer.write_handle(332, *handle)?;
            }
            self.writer
                .write_i32(95, state.visible_params.len() as i32)?;
            for handle in &state.visible_params {
                self.writer.write_handle(333, *handle)?;
            }
        }
        Ok(())
    }

    /// Write a minimal stub object (handle + owner only)
    fn write_stub_handle_only(
        &mut self,
        type_name: &str,
        handle: Handle,
        owner: Handle,
    ) -> Result<()> {
        self.writer.write_string(0, type_name)?;
        self.writer.write_handle(5, handle)?;
        // Match BricsCAD's own export: the placeholder carries a reactor
        // group pointing at its owner and no AcDbPlaceHolder subclass
        // marker (issue #51 BricsCAD audit).
        self.writer.write_string(102, "{ACAD_REACTORS")?;
        self.writer.write_handle(330, owner)?;
        self.writer.write_string(102, "}")?;
        self.writer.write_handle(330, owner)?;
        if self.dxf_version < DxfVersion::AC1015 && type_name == "ACDBPLACEHOLDER" {
            self.writer.write_subclass("AcDbPlaceHolder")?;
        }
        Ok(())
    }

    /// Write an unknown object, preserving raw group codes if available.
    fn write_unknown_object(
        &mut self,
        type_name: &str,
        handle: Handle,
        owner: Handle,
        raw_dxf_codes: Option<&[(i32, String)]>,
    ) -> Result<()> {
        if let Some(codes) = raw_dxf_codes {
            self.writer.write_string(0, type_name)?;
            self.writer.write_handle(5, handle)?;
            self.writer.write_handle(330, owner)?;
            for (code, value) in codes {
                self.writer.write_string(*code, value)?;
            }
        }
        // No raw data — skip this object (nothing to write)
        Ok(())
    }

    /// Write the CMC long used by MULTILEADER and MULTILEADERSTYLE.
    fn write_color_i32(&mut self, code: i32, color: Color) -> Result<()> {
        let value = match color {
            Color::ByLayer => 0xC000_0000_u32,
            Color::ByBlock => 0xC100_0000_u32,
            Color::Rgb { r, g, b } => {
                0xC200_0000_u32 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
            }
            Color::Index(index) => 0xC300_0000_u32 | index as u32,
            Color::None => 0xC800_0000_u32,
        };
        self.writer.write_i32(code, value as i32)?;
        Ok(())
    }

    /// Helper to write color as i16 (index format)
    fn write_color_i16(&mut self, code: i32, color: Color) -> Result<()> {
        match color {
            Color::ByLayer => self.writer.write_i16(code, 256)?,
            Color::None => self.writer.write_i16(code, 257)?,
            Color::ByBlock => self.writer.write_i16(code, 0)?,
            Color::Index(i) => self.writer.write_i16(code, i as i16)?,
            Color::Rgb { .. } => self.writer.write_i16(code, 7)?, // Default to white/black
        }
        Ok(())
    }

    fn write_object_xdata(
        &mut self,
        document: &CadDocument,
        handle: Handle,
        object: &ObjectType,
    ) -> Result<()> {
        if crate::io::dxf::object_has_own_dxf_xdata(object) {
            return Ok(());
        }
        match document.object_xdata.get(&handle) {
            Some(xdata) => self.write_xdata(xdata),
            None => Ok(()),
        }
    }

    fn write_table_annotative_xdata(&mut self, document: &CadDocument, handle: Handle, annotative: bool) -> Result<()> {
        self.write_table_annotative_xdata_with(document, handle, annotative, Vec::new())
    }

    fn write_table_annotative_xdata_with(&mut self, document: &CadDocument, handle: Handle, annotative: bool, mut generated: Vec<crate::xdata::ExtendedDataRecord>) -> Result<()> {
        let has_source = document.object_xdata.get(&handle).is_some_and(|data| data.get_record("AcadAnnotative").is_some());
        if annotative || has_source {
            let mut record = crate::xdata::ExtendedDataRecord::new("AcadAnnotative");
            record.values = vec![
                XDataValue::String("AnnotativeData".to_string()),
                XDataValue::ControlString("{".to_string()),
                XDataValue::Integer16(1), XDataValue::Integer16(i16::from(annotative)),
                XDataValue::ControlString("}".to_string()),
            ];
            generated.push(record);
        }
        self.write_table_xdata(document, handle, &generated)
    }

    fn write_table_xdata(&mut self, document: &CadDocument, handle: Handle, generated: &[crate::xdata::ExtendedDataRecord]) -> Result<()> {
        let mut data = document.object_xdata.get(&handle).cloned().unwrap_or_default();
        for replacement in generated {
            data.merge_modeled_record(replacement);
        }
        self.write_xdata(&data)
    }

    /// Write extended data (XDATA)
    pub(crate) fn write_xdata(&mut self, xdata: &ExtendedData) -> Result<()> {
        if xdata.is_empty() {
            return Ok(());
        }

        for record in xdata.records() {
            self.writer.write_string(1001, &record.application_name)?;

            for value in &record.values {
                match value {
                    XDataValue::String(s) => {
                        self.writer.write_string(1000, s)?;
                    }
                    XDataValue::ControlString(s) => {
                        self.writer.write_string(1002, s)?;
                    }
                    XDataValue::LayerName(s) => {
                        self.writer.write_string(1003, s)?;
                    }
                    XDataValue::BinaryData(data) => {
                        self.writer.write_binary(1004, data)?;
                    }
                    XDataValue::Handle(h) => {
                        self.writer.write_handle(1005, *h)?;
                    }
                    XDataValue::Point3D(p) => {
                        self.writer.write_double(1010, p.x)?;
                        self.writer.write_double(1020, p.y)?;
                        self.writer.write_double(1030, p.z)?;
                    }
                    XDataValue::Position3D(p) => {
                        self.writer.write_double(1011, p.x)?;
                        self.writer.write_double(1021, p.y)?;
                        self.writer.write_double(1031, p.z)?;
                    }
                    XDataValue::Displacement3D(p) => {
                        self.writer.write_double(1012, p.x)?;
                        self.writer.write_double(1022, p.y)?;
                        self.writer.write_double(1032, p.z)?;
                    }
                    XDataValue::Direction3D(p) => {
                        self.writer.write_double(1013, p.x)?;
                        self.writer.write_double(1023, p.y)?;
                        self.writer.write_double(1033, p.z)?;
                    }
                    XDataValue::Real(r) => {
                        self.writer.write_double(1040, *r)?;
                    }
                    XDataValue::Distance(d) => {
                        self.writer.write_double(1041, *d)?;
                    }
                    XDataValue::ScaleFactor(s) => {
                        self.writer.write_double(1042, *s)?;
                    }
                    XDataValue::Integer16(i) => {
                        self.writer.write_i16(1070, *i)?;
                    }
                    XDataValue::Integer32(i) => {
                        self.writer.write_i32(1071, *i)?;
                    }
                }
            }
        }

        Ok(())
    }

    fn write_mleader_line_breaks(
        &mut self,
        line: &crate::entities::multileader::LeaderLine,
    ) -> Result<()> {
        if line.break_infos.is_empty() {
            if !line.break_points.is_empty() {
                self.writer.write_i32(90, line.segment_index)?;
                for points in &line.break_points {
                    self.writer.write_point3d(11, points.start_point)?;
                    self.writer.write_point3d(12, points.end_point)?;
                }
            }
        } else {
            for info in &line.break_infos {
                self.writer.write_i32(90, info.segment_index)?;
                for points in &info.break_points {
                    self.writer.write_point3d(11, points.start_point)?;
                    self.writer.write_point3d(12, points.end_point)?;
                }
            }
        }
        Ok(())
    }

    fn write_mleader_context_data(
        &mut self,
        ctx: &crate::entities::multileader::MultiLeaderAnnotContext,
    ) -> Result<()> {
        self.writer.write_string(300, "CONTEXT_DATA{")?;
        self.writer.write_double(40, ctx.scale_factor)?;
        self.writer.write_point3d(10, ctx.content_base_point)?;
        self.writer.write_double(41, ctx.text_height)?;
        self.writer.write_double(140, ctx.arrowhead_size)?;
        self.writer.write_double(145, ctx.landing_gap)?;
        self.writer
            .write_i16(174, ctx.text_left_attachment as i16)?;
        self.writer
            .write_i16(175, ctx.text_right_attachment as i16)?;
        self.writer.write_i16(176, ctx.text_alignment as i16)?;
        self.writer
            .write_i16(177, ctx.block_connection_type as i16)?;
        self.writer.write_bool(290, ctx.has_text_contents)?;

        if ctx.has_text_contents {
            self.writer.write_string(304, &ctx.text_string)?;
            self.writer.write_point3d(11, ctx.text_normal)?;
            if let Some(handle) = ctx.text_style_handle {
                self.writer.write_handle(340, handle)?;
            }
            self.writer.write_point3d(12, ctx.text_location)?;
            self.writer.write_point3d(13, ctx.text_direction)?;
            self.writer.write_double(42, ctx.text_rotation)?;
            self.writer.write_double(43, ctx.text_width)?;
            self.writer.write_double(44, ctx.text_boundary_height)?;
            self.writer.write_double(45, ctx.line_spacing_factor)?;
            self.writer.write_i16(170, ctx.line_spacing_style as i16)?;
            self.write_color_i32(90, ctx.text_color)?;
            self.writer
                .write_i16(171, ctx.text_attachment_point as i16)?;
            self.writer.write_i16(172, ctx.text_flow_direction as i16)?;
            self.write_color_i32(91, ctx.background_fill_color)?;
            self.writer.write_double(141, ctx.background_scale_factor)?;
            self.writer.write_i32(92, ctx.background_transparency)?;
            self.writer.write_bool(291, ctx.background_fill_enabled)?;
            self.writer.write_bool(292, ctx.background_mask_fill_on)?;
            self.writer.write_i16(173, ctx.column_type)?;
            self.writer.write_bool(293, ctx.text_height_automatic)?;
            self.writer.write_double(142, ctx.column_width)?;
            self.writer.write_double(143, ctx.column_gutter)?;
            self.writer.write_bool(294, ctx.column_flow_reversed)?;
            for size in &ctx.column_sizes {
                self.writer.write_double(144, *size)?;
            }
            // Group 295 is R2013+, but the reader accepts it in any version (the default is true): an earlier
            // file that stated false keeps it.
            if self.dxf_version >= DxfVersion::AC1027 || !ctx.word_break {
                self.writer.write_bool(295, ctx.word_break)?;
            }
        }

        self.writer.write_bool(296, ctx.has_block_contents)?;
        if ctx.has_block_contents {
            if let Some(handle) = ctx.block_content_handle {
                self.writer.write_handle(341, handle)?;
            }
            self.writer.write_point3d(14, ctx.block_content_normal)?;
            self.writer.write_point3d(15, ctx.block_content_location)?;
            self.writer.write_point3d(16, ctx.block_content_scale)?;
            self.writer.write_double(46, ctx.block_rotation)?;
            self.write_color_i32(93, ctx.block_content_color)?;
            for value in ctx.transform_matrix {
                self.writer.write_double(47, value)?;
            }
        }

        self.writer.write_point3d(110, ctx.base_point)?;
        self.writer.write_point3d(111, ctx.base_direction)?;
        self.writer.write_point3d(112, ctx.base_vertical)?;
        self.writer.write_bool(297, ctx.normal_reversed)?;

        for root in &ctx.leader_roots {
            self.writer.write_string(302, "LEADER{")?;
            self.writer.write_bool(290, root.content_valid)?;
            self.writer.write_bool(291, root.unknown)?;
            self.writer.write_point3d(10, root.connection_point)?;
            self.writer.write_point3d(11, root.direction)?;
            self.writer.write_i32(90, root.leader_index)?;
            for points in &root.break_points {
                self.writer.write_point3d(12, points.start_point)?;
                self.writer.write_point3d(13, points.end_point)?;
            }
            self.writer.write_double(40, root.landing_distance)?;
            for line in &root.lines {
                self.writer.write_string(304, "LEADER_LINE{")?;
                for point in &line.points {
                    self.writer.write_point3d(10, *point)?;
                }
                self.write_mleader_line_breaks(line)?;
                self.writer.write_i32(91, line.index)?;
                self.writer.write_i16(170, line.path_type as i16)?;
                self.write_color_i32(92, line.line_color)?;
                if let Some(handle) = line.line_type_handle {
                    self.writer.write_handle(340, handle)?;
                }
                self.writer.write_i16(171, line.line_weight.value())?;
                self.writer.write_double(40, line.arrowhead_size)?;
                if let Some(handle) = line.arrowhead_handle {
                    self.writer.write_handle(341, handle)?;
                }
                self.writer
                    .write_i32(93, line.override_flags.bits() as i32)?;
                self.writer.write_string(305, "}")?;
            }
            if self.dxf_version >= DxfVersion::AC1024 {
                self.writer
                    .write_i16(271, root.text_attachment_direction as i16)?;
            }
            self.writer.write_string(303, "}")?;
        }

        if self.dxf_version >= DxfVersion::AC1024 {
            self.writer
                .write_i16(272, ctx.text_bottom_attachment as i16)?;
            self.writer.write_i16(273, ctx.text_top_attachment as i16)?;
        }
        self.writer.write_string(301, "}")?;
        Ok(())
    }

    /// Write MULTILEADER entity
    fn write_multileader(
        &mut self,
        mleader: &crate::entities::MultiLeader,
        owner: Handle,
    ) -> Result<()> {
        self.writer.write_entity_type("MULTILEADER")?;
        self.write_common_entity_data(&mleader.common, owner)?;
        self.writer.write_subclass("AcDbMLeader")?;

        if self.dxf_version >= DxfVersion::AC1024 {
            self.writer.write_i16(270, mleader.dwg_version)?;
        }

        // Context data - write the annotation context
        let ctx = &mleader.context;
        self.writer.write_string(300, "CONTEXT_DATA{")?;

        // Scale and position
        self.writer.write_double(40, ctx.scale_factor)?;
        self.writer.write_point3d(10, ctx.content_base_point)?;
        self.writer.write_double(41, ctx.text_height)?;
        self.writer.write_double(140, ctx.arrowhead_size)?;
        self.writer.write_double(145, ctx.landing_gap)?;

        // Text/block attachment types
        self.writer
            .write_i16(174, ctx.text_left_attachment as i16)?;
        self.writer
            .write_i16(175, ctx.text_right_attachment as i16)?;
        self.writer.write_i16(176, ctx.text_alignment as i16)?;
        self.writer
            .write_i16(177, ctx.block_connection_type as i16)?;

        // Has text contents
        self.writer.write_bool(290, ctx.has_text_contents)?;

        // Text content fields (conditional)
        if ctx.has_text_contents {
            self.writer.write_string(304, &ctx.text_string)?;
            self.writer.write_point3d(11, ctx.text_normal)?;
            if let Some(h) = ctx.text_style_handle {
                self.writer.write_handle(340, h)?;
            }
            self.writer.write_point3d(12, ctx.text_location)?;
            self.writer.write_point3d(13, ctx.text_direction)?;
            self.writer.write_double(42, ctx.text_rotation)?;
            self.writer.write_double(43, ctx.text_width)?;
            self.writer.write_double(44, ctx.text_boundary_height)?;
            self.writer.write_double(45, ctx.line_spacing_factor)?;
            self.writer.write_i16(170, ctx.line_spacing_style as i16)?;
            self.write_color_i32(90, ctx.text_color)?;
            self.writer
                .write_i16(171, ctx.text_attachment_point as i16)?;
            self.writer.write_i16(172, ctx.text_flow_direction as i16)?;
            self.write_color_i32(91, ctx.background_fill_color)?;
            self.writer.write_double(141, ctx.background_scale_factor)?;
            self.writer.write_i32(92, ctx.background_transparency)?;
            self.writer.write_bool(291, ctx.background_fill_enabled)?;
            self.writer.write_bool(292, ctx.background_mask_fill_on)?;
            self.writer.write_i16(173, ctx.column_type)?;
            self.writer.write_bool(293, ctx.text_height_automatic)?;
            self.writer.write_double(142, ctx.column_width)?;
            self.writer.write_double(143, ctx.column_gutter)?;
            self.writer.write_bool(294, ctx.column_flow_reversed)?;
            for size in &ctx.column_sizes {
                self.writer.write_double(144, *size)?;
            }
            // Group 295 is R2013+, but the reader accepts it in any version (the default is true): an earlier
            // file that stated false keeps it.
            if self.dxf_version >= DxfVersion::AC1027 || !ctx.word_break {
                self.writer.write_bool(295, ctx.word_break)?;
            }
        }

        // Has block contents
        self.writer.write_bool(296, ctx.has_block_contents)?;

        if ctx.has_block_contents {
            if let Some(h) = ctx.block_content_handle {
                self.writer.write_handle(341, h)?;
            }
            self.writer.write_point3d(14, ctx.block_content_normal)?;
            self.writer.write_point3d(15, ctx.block_content_location)?;
            self.writer.write_point3d(16, ctx.block_content_scale)?;
            self.writer.write_double(46, ctx.block_rotation)?;
            self.write_color_i32(93, ctx.block_content_color)?;
            for value in ctx.transform_matrix {
                self.writer.write_double(47, value)?;
            }
        }

        // Transformation base
        self.writer.write_point3d(110, ctx.base_point)?;
        self.writer.write_point3d(111, ctx.base_direction)?;
        self.writer.write_point3d(112, ctx.base_vertical)?;
        self.writer.write_bool(297, ctx.normal_reversed)?;

        // Leader roots
        for root in &ctx.leader_roots {
            self.writer.write_string(302, "LEADER{")?;
            self.writer.write_bool(290, root.content_valid)?;
            self.writer.write_bool(291, root.unknown)?;
            self.writer.write_point3d(10, root.connection_point)?;
            self.writer.write_point3d(11, root.direction)?;
            self.writer.write_i32(90, root.leader_index)?;
            for bp in &root.break_points {
                self.writer.write_point3d(12, bp.start_point)?;
                self.writer.write_point3d(13, bp.end_point)?;
            }
            self.writer.write_double(40, root.landing_distance)?;

            // Leader lines
            for line in &root.lines {
                self.writer.write_string(304, "LEADER_LINE{")?;

                // Vertex points
                for pt in &line.points {
                    self.writer.write_point3d(10, *pt)?;
                }

                self.write_mleader_line_breaks(line)?;

                // Per-line properties
                self.writer.write_i32(91, line.index)?;
                self.writer.write_i16(170, line.path_type as i16)?;
                self.write_color_i32(92, line.line_color)?;
                self.writer
                    .write_handle(340, line.line_type_handle.unwrap_or(Handle::NULL))?;
                self.writer.write_i16(171, line.line_weight.value())?;
                self.writer.write_double(40, line.arrowhead_size)?;
                self.writer
                    .write_handle(341, line.arrowhead_handle.unwrap_or(Handle::NULL))?;
                self.writer
                    .write_i32(93, line.override_flags.bits() as i32)?;
                self.writer.write_string(305, "}")?;
            }

            if self.dxf_version >= DxfVersion::AC1024 {
                self.writer
                    .write_i16(271, root.text_attachment_direction as i16)?;
            }
            self.writer.write_string(303, "}")?;
        }

        // Post-leader attachments
        if self.dxf_version >= DxfVersion::AC1024 {
            self.writer
                .write_i16(272, ctx.text_bottom_attachment as i16)?;
            self.writer.write_i16(273, ctx.text_top_attachment as i16)?;
        }

        self.writer.write_string(301, "}")?; // End CONTEXT_DATA

        // Main properties — the code map mirrors AutoCAD's own DXF output
        // (verified against an R2018 sample): 170 is the leader PATH type and
        // 172 the content type; 342 the arrow block, 343 the text style, 344
        // the block content. The old map wrote the content type at 170, which
        // AutoCAD reads as the path type (an MText leader became "spline").

        // Style handle
        if let Some(h) = mleader.style_handle {
            self.writer.write_handle(340, h)?;
        }

        // Property override flags
        self.writer
            .write_i32(90, mleader.property_override_flags.bits() as i32)?;

        // Leader path type
        self.writer.write_i16(170, mleader.path_type as i16)?;

        // Leader line color
        self.write_color_i32(91, mleader.line_color)?;

        // Leader line type handle (code 341; ByLayer if null, ByBlock is kept as the source states it)
        {
            let h = mleader
                .line_type_handle
                .filter(|h| *h != Handle::NULL)
                .unwrap_or(self.bylayer_linetype_handle);
            if h != Handle::NULL {
                self.writer.write_handle(341, h)?;
            }
        }

        // Leader line weight
        self.writer.write_i16(171, mleader.line_weight.value())?;

        // Enable landing
        self.writer.write_bool(290, mleader.enable_landing)?;

        // Enable dogleg
        self.writer.write_bool(291, mleader.enable_dogleg)?;

        // Dogleg length
        self.writer.write_double(41, mleader.dogleg_length)?;

        // Arrowhead block handle
        if let Some(h) = mleader.arrowhead_handle {
            if h != Handle::NULL {
                self.writer.write_handle(342, h)?;
            }
        }

        // Arrowhead size
        self.writer.write_double(42, mleader.arrowhead_size)?;

        // Content type
        self.writer.write_i16(172, mleader.content_type as i16)?;

        // Text style handle
        if let Some(h) = mleader.text_style_handle {
            self.writer.write_handle(343, h)?;
        }

        // Text left / right attachment types
        self.writer
            .write_i16(173, mleader.text_left_attachment as i16)?;
        self.writer
            .write_i32(95, mleader.text_right_attachment as i32)?;

        // Text angle type
        self.writer.write_i16(174, mleader.text_angle_type as i16)?;

        // Text alignment type
        self.writer.write_i16(175, mleader.text_alignment as i16)?;

        // Text color
        self.write_color_i32(92, mleader.text_color)?;

        // Text frame
        self.writer.write_bool(292, mleader.text_frame)?;

        // Block content handle
        if let Some(h) = mleader.block_content_handle {
            self.writer.write_handle(344, h)?;
        }

        // Block content color
        self.write_color_i32(93, mleader.block_content_color)?;

        // Block content scale
        self.writer.write_point3d(10, mleader.block_scale)?;

        // Block content rotation
        self.writer.write_double(43, mleader.block_rotation)?;

        // Overall scale
        self.writer.write_double(45, mleader.scale_factor)?;

        // Block content connection type
        self.writer
            .write_i16(176, mleader.block_connection_type as i16)?;

        // Enable annotation scale
        self.writer
            .write_bool(293, mleader.enable_annotation_scale)?;

        for override_value in &mleader.arrowhead_overrides {
            self.writer.write_i32(94, override_value.index)?;
            self.writer
                .write_handle(345, override_value.arrowhead_handle.unwrap_or(Handle::NULL))?;
        }

        for attribute in &mleader.block_attributes {
            self.writer.write_handle(
                330,
                attribute
                    .attribute_definition_handle
                    .unwrap_or(Handle::NULL),
            )?;
            self.writer.write_i16(177, attribute.index)?;
            self.writer.write_double(44, attribute.width)?;
            self.writer.write_string(302, &attribute.text)?;
        }

        self.writer
            .write_bool(294, mleader.text_direction_negative)?;
        self.writer.write_i16(178, mleader.text_align_in_ipe)?;
        self.writer
            .write_i16(179, mleader.text_attachment_point as i16)?;
        if self.dxf_version >= DxfVersion::AC1024 {
            self.writer
                .write_i16(271, mleader.text_attachment_direction as i16)?;

            // Text bottom / top attachment types; a source that left them out keeps them out unless changed.
            let defaults = MultiLeader::new();
            let source = mleader.dxf_source.as_deref();
            if source.map_or(true, |s| s.contains(272))
                || mleader.text_bottom_attachment != defaults.text_bottom_attachment
            {
                self.writer
                    .write_i16(272, mleader.text_bottom_attachment as i16)?;
            }
            if source.map_or(true, |s| s.contains(273))
                || mleader.text_top_attachment != defaults.text_top_attachment
            {
                self.writer
                    .write_i16(273, mleader.text_top_attachment as i16)?;
            }
        }

        // Extend leader to text
        if self.dxf_version >= DxfVersion::AC1027 {
            self.writer.write_bool(295, mleader.extend_leader_to_text)?;
        }

        Ok(())
    }

    /// Write MLINE entity
    fn write_mline(&mut self, mline: &crate::entities::MLine, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("MLINE")?;
        self.write_common_entity_data(&mline.common, owner)?;
        self.writer.write_subclass("AcDbMline")?;

        // Style name
        self.writer.write_string(2, &mline.style_name)?;

        // Style handle — always write (CAD requires non-null reference)
        let style_h = mline.style_handle.unwrap_or(Handle::NULL);
        self.writer.write_handle(340, style_h)?;

        // Scale factor
        self.writer.write_double(40, mline.scale_factor)?;

        // Justification
        self.writer.write_i16(70, mline.justification as i16)?;

        // Flags: HAS_VERTICES is derived from the serialized vertex list.
        self.writer.write_i16(71, mline.serialized_flags().bits())?;

        // Number of vertices
        self.writer.write_i16(72, mline.vertices.len() as i16)?;

        // Number of style elements
        self.writer
            .write_i16(73, mline.style_element_count as i16)?;

        // Start point
        self.writer.write_point3d(10, mline.start_point)?;

        // Normal
        self.writer.write_point3d(210, mline.normal)?;

        // Vertices
        for vertex in &mline.vertices {
            // Position
            self.writer.write_point3d(11, vertex.position)?;

            // Direction
            self.writer.write_point3d(12, vertex.direction)?;

            // Miter
            self.writer.write_point3d(13, vertex.miter)?;

            // Segments for each element
            for segment in &vertex.segments {
                // Number of parameters
                self.writer.write_i16(74, segment.parameters.len() as i16)?;

                // Parameters
                for param in &segment.parameters {
                    self.writer.write_double(41, *param)?;
                }

                // Number of area fill parameters
                self.writer
                    .write_i16(75, segment.area_fill_parameters.len() as i16)?;

                // Area fill parameters
                for param in &segment.area_fill_parameters {
                    self.writer.write_double(42, *param)?;
                }
            }
        }

        Ok(())
    }

    /// Write MESH entity
    fn write_mesh(&mut self, mesh: &crate::entities::Mesh, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("MESH")?;
        self.write_common_entity_data(&mesh.common, owner)?;
        self.writer.write_subclass("AcDbSubDMesh")?;

        // Version
        self.writer.write_i16(71, mesh.version)?;

        // Blend crease
        self.writer
            .write_i16(72, if mesh.blend_crease { 1 } else { 0 })?;

        // Subdivision level
        self.writer.write_i32(91, mesh.subdivision_level)?;

        // Vertex count
        self.writer.write_i32(92, mesh.vertices.len() as i32)?;

        // Vertices
        for v in &mesh.vertices {
            self.writer.write_point3d(10, *v)?;
        }

        // Face count (face list size = count of all indices + face size prefixes)
        let face_list_size: i32 = mesh.faces.iter().map(|f| 1 + f.vertices.len() as i32).sum();
        self.writer.write_i32(93, face_list_size)?;

        // Face data: each face is: vertex_count, v0, v1, v2, ...
        for face in &mesh.faces {
            self.writer.write_i32(90, face.vertices.len() as i32)?;
            for vi in &face.vertices {
                self.writer.write_i32(90, *vi as i32)?;
            }
        }

        // Edge count
        self.writer.write_i32(94, mesh.edges.len() as i32)?;

        // Edges: start_index, end_index pairs
        for edge in &mesh.edges {
            self.writer.write_i32(90, edge.start as i32)?;
            self.writer.write_i32(90, edge.end as i32)?;
        }

        // Edge crease count and one value per edge
        self.writer.write_i32(95, mesh.edges.len() as i32)?;
        for edge in &mesh.edges {
            self.writer.write_double(140, edge.crease_value())?;
        }

        // Sub-entities with overridden properties
        self.writer.write_i32(90, 0)?;

        Ok(())
    }

    /// Write IMAGE (RasterImage) entity
    fn write_raster_image(
        &mut self,
        image: &crate::entities::RasterImage,
        owner: Handle,
    ) -> Result<()> {
        self.writer.write_entity_type("IMAGE")?;
        self.write_common_entity_data(&image.common, owner)?;
        self.writer.write_subclass("AcDbRasterImage")?;

        // Class version
        self.writer.write_i32(90, image.class_version)?;

        // Insertion point
        self.writer.write_point3d(10, image.insertion_point)?;

        // U vector (size of single pixel in world)
        self.writer.write_point3d(11, image.u_vector)?;

        // V vector
        self.writer.write_point3d(12, image.v_vector)?;

        // Image size in pixels
        self.writer.write_double(13, image.size.x)?;
        self.writer.write_double(23, image.size.y)?;

        // Image definition handle
        if let Some(h) = image.definition_handle {
            self.writer.write_handle(340, h)?;
        }

        // Display properties
        self.writer.write_i16(70, image.flags.bits())?;

        // Clipping boundary on
        self.writer
            .write_byte(280, if image.clipping_enabled { 1 } else { 0 })?;

        // Brightness
        self.writer.write_byte(281, image.brightness)?;

        // Contrast
        self.writer.write_byte(282, image.contrast)?;

        // Fade
        self.writer.write_byte(283, image.fade)?;

        // Image definition reactor handle
        if let Some(h) = image.definition_reactor_handle {
            self.writer.write_handle(360, h)?;
        }

        // Clipping boundary type
        self.writer
            .write_i16(71, image.clip_boundary.clip_type as i16)?;

        // Number of clip boundary vertices
        self.writer
            .write_i32(91, image.clip_boundary.vertices.len() as i32)?;

        // Clip boundary vertices
        for v in &image.clip_boundary.vertices {
            self.writer.write_double(14, v.x)?;
            self.writer.write_double(24, v.y)?;
        }

        Ok(())
    }

    /// Write 3DSOLID entity
    fn write_solid3d(&mut self, solid: &Solid3D, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("3DSOLID")?;
        self.write_common_entity_data(&solid.common, owner)?;
        self.writer.write_subclass("AcDbModelerGeometry")?;

        if self.needs_sab() {
            // AC1027+: SAB binary format stored in ACDSDATA section
            self.writer.write_bool(290, solid.acis_data.acis_empty_bit)?;
            // The entity's unique identifier (group 2, R2013+); the zero GUID when it has none.
            self.writer.write_string(2, modeler_uid(&solid.uid))?;

            // Convert SAT to SAB and queue for ACDSDATA section
            self.queue_sab_data(&solid.acis_data, solid.common.handle);
        } else {
            // Pre-AC1027: SAT cipher text inline.
            // Always write version=1 here: the DXF SAT path always outputs
            // SAT text (code-1 groups), even when the source was SAB binary.
            // Writing version=2 would be inconsistent with the SAT output.
            self.writer.write_i16(70, 1)?;
            self.write_acis_data(&solid.acis_data)?;
        }

        if self.dxf_version >= DxfVersion::AC1021 {
            self.writer.write_subclass("AcDb3dSolid")?;
            let h = solid.history_handle.unwrap_or(Handle::NULL);
            self.writer.write_handle(350, h)?;
        }

        Ok(())
    }

    /// Write REGION entity
    fn write_region(&mut self, region: &Region, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("REGION")?;
        self.write_common_entity_data(&region.common, owner)?;
        self.writer.write_subclass("AcDbModelerGeometry")?;

        if self.needs_sab() {
            self.writer.write_bool(290, region.acis_data.acis_empty_bit)?;
            // The entity's unique identifier (group 2, R2013+); the zero GUID when it has none.
            self.writer.write_string(2, modeler_uid(&region.uid))?;
            self.queue_sab_data(&region.acis_data, region.common.handle);
        } else {
            self.writer.write_i16(70, 1)?;
            self.write_acis_data(&region.acis_data)?;
        }

        Ok(())
    }

    /// Write BODY entity
    fn write_body(&mut self, body: &Body, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("BODY")?;
        self.write_common_entity_data(&body.common, owner)?;
        self.writer.write_subclass("AcDbModelerGeometry")?;

        if self.needs_sab() {
            self.writer.write_bool(290, body.acis_data.acis_empty_bit)?;
            // The entity's unique identifier (group 2, R2013+); the zero GUID when it has none.
            self.writer.write_string(2, modeler_uid(&body.uid))?;
            self.queue_sab_data(&body.acis_data, body.common.handle);
        } else {
            self.writer.write_i16(70, 1)?;
            self.write_acis_data(&body.acis_data)?;
        }

        Ok(())
    }

    fn write_light(&mut self, light: &Light, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("LIGHT")?;
        self.write_common_entity_data(&light.common, owner)?;
        self.writer.write_subclass("AcDbLight")?;
        self.writer.write_i32(90, light.class_version)?;
        self.writer.write_string(1, &light.name)?;
        self.writer.write_i16(70, light.light_type as i16)?;
        self.writer.write_bool(290, light.status)?;
        self.writer.write_color(63, light.light_color)?;
        if let Color::Rgb { r, g, b } = light.light_color {
            let rgb = ((r as i32) << 16) | ((g as i32) << 8) | b as i32;
            self.writer.write_i32(421, rgb)?;
        }
        self.writer.write_bool(291, light.plot_glyph)?;
        self.writer.write_double(40, light.intensity)?;
        self.writer.write_point3d(10, light.position)?;
        self.writer.write_point3d(11, light.target)?;
        self.writer.write_i16(72, light.attenuation_type as i16)?;
        self.writer.write_bool(292, light.use_attenuation_limits)?;
        self.writer
            .write_double(41, light.attenuation_start_limit)?;
        self.writer.write_double(42, light.attenuation_end_limit)?;
        self.writer.write_double(50, light.hotspot_angle)?;
        self.writer.write_double(51, light.falloff_angle)?;
        self.writer.write_bool(293, light.cast_shadows)?;
        self.writer.write_i16(73, light.shadow_type as i16)?;
        self.writer.write_i32(91, light.shadow_map_size as i32)?;
        self.writer
            .write_i16(280, light.shadow_map_softness as i16)?;
        if light.photometric_mode {
            self.writer
                .write_bool(295, light.photometric_data.is_some())?;
            if let Some(data) = &light.photometric_data {
                self.writer.write_bool(290, data.has_web_file)?;
                self.writer.write_string(300, &data.web_file)?;
                self.writer.write_i16(70, data.physical_intensity_method)?;
                self.writer.write_double(40, data.physical_intensity)?;
                self.writer.write_double(41, data.illuminance_distance)?;
                self.writer.write_i16(71, data.lamp_color_type)?;
                self.writer.write_double(42, data.lamp_color_temperature)?;
                self.writer.write_i16(72, data.lamp_color_preset)?;
                self.writer.write_point3d(43, data.web_rotation)?;
                self.writer.write_i16(73, data.extended_light_shape)?;
                self.writer.write_double(46, data.extended_light_length)?;
                self.writer.write_double(47, data.extended_light_width)?;
                self.writer.write_double(48, data.extended_light_radius)?;
                self.writer.write_i16(74, data.web_file_type)?;
                self.writer.write_i16(75, data.web_symmetry)?;
                self.writer.write_i16(76, data.has_target_grip)?;
                self.writer.write_double(49, data.web_flux)?;
                for (index, angle) in data.web_angles.iter().enumerate() {
                    self.writer.write_double(50 + index as i32, *angle)?;
                }
                self.writer.write_i16(77, data.glyph_display_type)?;
            }
        }
        Ok(())
    }

    fn write_surface(&mut self, surface: &Surface, owner: Handle) -> Result<()> {
        self.writer.write_entity_type(surface.kind.dxf_name())?;
        self.write_common_entity_data(&surface.common, owner)?;
        self.writer.write_subclass("AcDbModelerGeometry")?;
        if self.needs_sab() {
            self.writer.write_bool(290, false)?;
            self.writer
                .write_string(2, "{00000000-0000-0000-0000-000000000000}")?;
            self.queue_sab_data(&surface.acis_data, surface.common.handle);
        } else {
            self.writer.write_i16(70, 1)?;
            self.write_acis_data(&surface.acis_data)?;
        }
        self.writer.write_subclass("AcDbSurface")?;
        self.writer.write_i16(71, surface.u_isolines)?;
        self.writer.write_i16(72, surface.v_isolines)?;

        match &surface.surface_data {
            SurfaceData::Generic => {}
            SurfaceData::Plane { .. } => {}
            SurfaceData::Extruded {
                sweep_entity,
                options,
                sweep_vector,
                sweep_transform,
            } => {
                self.writer.write_subclass("AcDbExtrudedSurface")?;
                if let Some(entity) = sweep_entity {
                    self.write_surface_profile(90, entity, false)?;
                } else {
                    self.writer.write_i32(90, 0)?;
                    self.writer.write_i32(90, 0)?;
                }
                self.writer.write_point3d(10, *sweep_vector)?;
                for value in &crate::entities::surface::transpose_matrix(*sweep_transform) {
                    self.writer.write_double(40, *value)?;
                }
                self.write_surface_sweep_options_dxf(options)?;
            }
            SurfaceData::Lofted {
                loft_transform,
                cross_section_entities,
                guide_entities,
                path_entity,
                plane_normal_lofting_type,
                start_draft_angle,
                end_draft_angle,
                start_draft_magnitude,
                end_draft_magnitude,
                arc_length_parameterization,
                no_twist,
                align_direction,
                simple_surfaces,
                closed_surfaces,
                solid,
                ruled_surface,
                virtual_guide,
                cross_sections,
                guide_curves,
                path_curve,
            } => {
                self.writer.write_subclass("AcDbLoftedSurface")?;
                for value in &crate::entities::surface::transpose_matrix(*loft_transform) {
                    self.writer.write_double(40, *value)?;
                }
                let resolve_inputs = |embedded: &[EmbeddedEntity], handles: &[Handle]| {
                    if embedded.is_empty() {
                        handles
                            .iter()
                            .map(|handle| self.loft_input_entities.get(handle).cloned())
                            .collect::<Option<Vec<_>>>()
                            .unwrap_or_default()
                    } else {
                        embedded.to_vec()
                    }
                };
                let section_entities = resolve_inputs(cross_section_entities, cross_sections);
                let guide_entities = resolve_inputs(guide_entities, guide_curves);
                let path_entity = path_entity.clone().or_else(|| {
                    path_curve.and_then(|handle| self.loft_input_entities.get(&handle).cloned())
                });
                for entity in &section_entities {
                    self.write_surface_profile(90, entity, true)?;
                }
                for entity in &guide_entities {
                    self.write_surface_profile(91, entity, true)?;
                }
                if let Some(entity) = &path_entity {
                    self.write_surface_profile(92, entity, true)?;
                }
                self.writer
                    .write_i16(70, *plane_normal_lofting_type as i16)?;
                self.writer.write_double(41, *start_draft_angle)?;
                self.writer.write_double(42, *end_draft_angle)?;
                self.writer.write_double(43, *start_draft_magnitude)?;
                self.writer.write_double(44, *end_draft_magnitude)?;
                self.writer.write_bool(290, *arc_length_parameterization)?;
                self.writer.write_bool(291, *no_twist)?;
                self.writer.write_bool(292, *align_direction)?;
                self.writer.write_bool(293, *simple_surfaces)?;
                self.writer.write_bool(294, *closed_surfaces)?;
                self.writer.write_bool(295, *solid)?;
                self.writer.write_bool(296, *ruled_surface)?;
                self.writer.write_bool(297, *virtual_guide)?;
            }
            SurfaceData::Revolved {
                revolve_entity,
                class_version,
                entity_id,
                axis_point,
                axis_vector,
                revolve_angle,
                start_angle,
                entity_transform,
                draft_angle,
                draft_start_distance,
                draft_end_distance,
                twist_angle,
                solid,
                close_to_axis,
            } => {
                self.writer.write_subclass("AcDbRevolvedSurface")?;
                if let Some(entity) = revolve_entity {
                    self.write_surface_profile(90, entity, false)?;
                } else {
                    self.writer.write_i32(90, *class_version)?;
                    self.writer.write_i32(90, *entity_id)?;
                }
                self.writer.write_point3d(10, *axis_point)?;
                self.writer.write_point3d(11, *axis_vector)?;
                self.writer.write_double(40, *revolve_angle)?;
                self.writer.write_double(41, *start_angle)?;
                for value in &crate::entities::surface::transpose_matrix(*entity_transform) {
                    self.writer.write_double(42, *value)?;
                }
                self.writer.write_double(43, *draft_angle)?;
                self.writer.write_double(44, *draft_start_distance)?;
                self.writer.write_double(45, *draft_end_distance)?;
                self.writer.write_double(46, *twist_angle)?;
                self.writer.write_bool(290, *solid)?;
                self.writer.write_bool(291, *close_to_axis)?;
            }
            SurfaceData::Swept {
                class_version: _,
                sweep_entity,
                path_entity,
                sweep_transform,
                path_transform,
                options,
            } => {
                self.writer.write_subclass("AcDbSweptSurface")?;
                // The reference application starts with the profile type;
                // the class version is not written to DXF.
                if let Some(entity) = sweep_entity {
                    self.write_surface_profile(90, entity, false)?;
                } else {
                    self.writer.write_i32(90, 0)?;
                    self.writer.write_i32(90, 0)?;
                }
                if let Some(entity) = path_entity {
                    self.write_surface_profile(91, entity, false)?;
                } else {
                    self.writer.write_i32(91, 0)?;
                    self.writer.write_i32(90, 0)?;
                }
                for value in &crate::entities::surface::transpose_matrix(*sweep_transform) {
                    self.writer.write_double(40, *value)?;
                }
                for value in &crate::entities::surface::transpose_matrix(*path_transform) {
                    self.writer.write_double(41, *value)?;
                }
                self.write_surface_sweep_options_dxf(options)?;
            }
            SurfaceData::Nurb {
                short_170,
                cv_hull_display,
                u_vector1,
                v_vector1,
                u_vector2,
                v_vector2,
            } => {
                self.writer.write_subclass("AcDbNurbSurface")?;
                self.writer.write_i16(170, *short_170)?;
                self.writer.write_bool(290, *cv_hull_display)?;
                self.writer.write_point3d(10, *u_vector1)?;
                self.writer.write_point3d(11, *v_vector1)?;
                self.writer.write_point3d(12, *u_vector2)?;
                self.writer.write_point3d(13, *v_vector2)?;
            }
        }
        Ok(())
    }

    fn write_surface_sweep_options_dxf(&mut self, options: &SurfaceSweepOptions) -> Result<()> {
        self.writer.write_double(42, options.draft_angle)?;
        self.writer.write_double(43, options.draft_start_distance)?;
        self.writer.write_double(44, options.draft_end_distance)?;
        self.writer.write_double(45, options.twist_angle)?;
        self.writer.write_double(48, options.scale_factor)?;
        self.writer.write_double(49, options.align_angle)?;
        for value in &crate::entities::surface::transpose_matrix(options.sweep_entity_transform) {
            self.writer.write_double(46, *value)?;
        }
        for value in &crate::entities::surface::transpose_matrix(options.path_entity_transform) {
            self.writer.write_double(47, *value)?;
        }
        self.writer.write_bool(290, options.is_solid)?;
        self.writer.write_i16(70, options.sweep_alignment_flags)?;
        self.writer.write_i16(71, options.path_flags)?;
        self.writer.write_bool(292, options.align_start)?;
        self.writer.write_bool(293, options.bank)?;
        self.writer.write_bool(294, options.base_point_set)?;
        self.writer
            .write_bool(295, options.sweep_entity_transform_computed)?;
        self.writer
            .write_bool(296, options.path_entity_transform_computed)?;
        self.writer.write_point3d(11, options.reference_vector)?;
        Ok(())
    }

    /// Write ACIS data (shared by Solid3D, Region, Body)
    ///
    /// SAT text is split by newlines; each line becomes a separate DXF
    /// group-code entry using group code 1. Lines longer than 2049 bytes
    /// are subdivided into 2049-byte sub-chunks: the first
    /// sub-chunk uses group code 1 and continuation sub-chunks use
    /// group code 3.
    ///
    /// When only SAB binary data is present (no SAT text), attempts to
    /// convert via `SabReader` before falling back to an empty entry.
    fn write_surface_profile(
        &mut self,
        type_group: i32,
        entity: &EmbeddedEntity,
        exact_bits: bool,
    ) -> Result<()> {
        if let EmbeddedEntity::Body {
            type_code,
            acis_data,
        } = entity
        {
            self.writer.write_i32(type_group, *type_code)?;
            self.writer.write_i16(70, 1)?;
            return self.write_acis_data(acis_data);
        }
        let dwg_version = crate::io::dwg::DwgVersion::from_dxf_version(self.dxf_version)
            .unwrap_or(crate::io::dwg::DwgVersion::AC24);
        let encoded = crate::io::dwg::embedded_entity::encode_embedded_entity(
            entity,
            dwg_version,
            self.dxf_version,
        );
        let bit_length = if exact_bits {
            encoded.bit_length
        } else {
            encoded.bytes.len() * 8
        };
        self.writer.write_i32(type_group, encoded.type_code)?;
        self.writer.write_i32(90, bit_length as i32)?;
        for chunk in encoded.bytes.chunks(127) {
            self.writer.write_binary(310, chunk)?;
        }
        Ok(())
    }

    fn write_acis_data(&mut self, acis: &AcisData) -> Result<()> {
        let converted;
        let data: &str = if acis.sat_data.is_empty() && !acis.sab_data.is_empty() {
            // SAB binary only — convert via SabReader.
            match crate::entities::acis::SabReader::read(&acis.sab_data) {
                Ok(doc) => {
                    converted = doc.to_sat_string();
                    &converted
                }
                Err(_) => "",
            }
        } else if !acis.sat_data.is_empty() {
            // Keep the original token stream. Re-serializing expands compact
            // ACIS booleans and makes legacy DXF modelers reject freeform
            // spline and p-curve payloads.
            &acis.sat_data
        } else {
            &acis.sat_data
        };

        if data.is_empty() {
            self.writer.write_string(1, "")?;
            return Ok(());
        }

        // Append the terminator — internal sat_data never contains it.
        let mut full = AcisData::strip_sat_terminator(data);
        full.push_str("End-of-ACIS-data\n");
        // SAT text from a binary body has one line per record; the reference
        // application only reads lines of up to 255 characters, so long
        // records are wrapped between tokens as ACIS itself writes them.
        if full.lines().any(|line| line.len() > 255) {
            full = full
                .lines()
                .flat_map(wrap_sat_line)
                .fold(String::new(), |mut text, line| {
                    text.push_str(line);
                    text.push('\n');
                    text
                });
        }

        // Version 1: apply the DXF character cipher to SAT text.
        // SAB-converted data is always treated as Version1 for DXF output.
        let use_version1_cipher = acis.version == AcisVersion::Version1
            || (acis.sat_data.is_empty() && !acis.sab_data.is_empty());
        let encoded = if use_version1_cipher {
            if self.writer.is_binary() {
                AcisData::encode_sat_binary(&full)
            } else {
                AcisData::encode_sat(&full)
            }
        } else {
            full
        };

        let mut any_written = false;
        for line in encoded.lines() {
            if line.len() <= 2049 {
                // Whole line fits in one chunk → group code 1
                self.writer.write_string(1, line)?;
            } else {
                // Split into 2049-byte sub-chunks without breaking UTF-8:
                // the first sub-chunk → gc 1, continuations → gc 3
                let mut remaining = line;
                let mut code = 1;
                while !remaining.is_empty() {
                    let mut end = remaining.len().min(2049);
                    while !remaining.is_char_boundary(end) {
                        end -= 1;
                    }
                    let (chunk, rest) = remaining.split_at(end);
                    self.writer.write_string(code, chunk)?;
                    code = 3;
                    remaining = rest;
                }
            }
            any_written = true;
        }

        if !any_written {
            self.writer.write_string(1, "")?;
        }

        Ok(())
    }

    fn write_table_custom_data_dxf(&mut self, values: &[table::TableCustomData]) -> Result<()> {
        self.writer.write_string(301, "CUSTOMDATA")?;
        self.writer.write_string(1, "DATAMAP_BEGIN")?;
        self.writer.write_i32(90, values.len() as i32)?;
        for value in values {
            self.writer.write_string(300, &value.name)?;
            self.writer.write_string(301, "DATAMAP_VALUE")?;
            self.write_field_cell_value_dxf(&value.value)?;
            self.writer.write_string(304, "ACVALUE_END")?;
        }
        self.writer.write_string(309, "DATAMAP_END")?;
        Ok(())
    }

    fn write_table_content_format_dxf(&mut self, content: &table::CellContent) -> Result<()> {
        self.writer.write_string(300, "CONTENTFORMAT")?;
        self.writer.write_string(1, "CONTENTFORMAT_BEGIN")?;
        self.writer.write_i32(90, content.format_override_flags)?;
        self.writer.write_i32(91, content.format_property_flags)?;
        self.writer.write_i32(92, content.format_value_data_type)?;
        self.writer.write_i32(93, content.format_value_unit_type)?;
        self.writer.write_string(300, &content.value_format)?;
        self.writer.write_double(40, content.rotation)?;
        self.writer.write_double(140, content.scale)?;
        self.writer.write_i32(94, content.alignment)?;
        self.writer.write_color(62, content.color)?;
        if let Some(handle) = content.text_style_handle {
            self.writer.write_handle(340, handle)?;
        } else if !content.text_style_name.is_empty() {
            self.writer.write_string(7, &content.text_style_name)?;
        } else {
            self.writer.write_handle(340, Handle::NULL)?;
        }
        self.writer.write_double(144, content.text_height)?;
        self.writer.write_string(309, "CONTENTFORMAT_END")?;
        Ok(())
    }

    fn write_table_style_content_format_dxf(&mut self, style: &table::CellStyle) -> Result<()> {
        self.writer.write_string(300, "CONTENTFORMAT")?;
        self.writer.write_string(1, "CONTENTFORMAT_BEGIN")?;
        self.writer
            .write_i32(90, style.content_format_override_flags)?;
        self.writer.write_i32(91, style.content_property_flags)?;
        self.writer.write_i32(92, style.value_data_type)?;
        self.writer.write_i32(93, style.value_unit_type)?;
        self.writer.write_string(300, &style.value_format)?;
        self.writer.write_double(40, style.rotation)?;
        self.writer.write_double(140, style.scale)?;
        self.writer.write_i32(94, style.alignment)?;
        self.writer.write_color(62, style.content_color)?;
        if let Some(handle) = style.text_style_handle {
            self.writer.write_handle(340, handle)?;
        } else if !style.text_style_name.is_empty() {
            self.writer.write_string(7, &style.text_style_name)?;
        } else {
            self.writer.write_handle(340, Handle::NULL)?;
        }
        self.writer.write_double(144, style.text_height)?;
        self.writer.write_string(309, "CONTENTFORMAT_END")?;
        Ok(())
    }

    fn write_table_grid_format_dxf(&mut self, edge: u32, border: &table::CellBorder) -> Result<()> {
        self.writer.write_i32(95, edge as i32)?;
        self.writer.write_string(302, "GRIDFORMAT")?;
        self.writer.write_string(1, "GRIDFORMAT_BEGIN")?;
        self.writer
            .write_i32(90, border.override_flags.bits() as i32)?;
        self.writer.write_i32(91, border.border_type as i32)?;
        self.writer.write_color(62, border.color)?;
        self.writer
            .write_i32(92, border.line_weight.value() as i32)?;
        self.writer
            .write_handle(340, border.line_type_handle.unwrap_or(Handle::NULL))?;
        self.writer.write_bool(93, border.invisible)?;
        self.writer.write_double(40, border.double_spacing)?;
        self.writer.write_string(309, "GRIDFORMAT_END")?;
        Ok(())
    }

    fn write_table_cell_style_override_dxf(
        &mut self,
        style: Option<&table::CellStyle>,
        fallback_type: table::CellStyleType,
    ) -> Result<()> {
        self.writer.write_string(1, "TABLEFORMAT_BEGIN")?;
        let Some(style) = style else {
            self.writer.write_i32(90, fallback_type as i32)?;
            self.writer.write_i16(170, 0)?;
            self.writer.write_string(309, "TABLEFORMAT_END")?;
            return Ok(());
        };

        self.writer.write_i32(90, style.style_type as i32)?;
        self.writer.write_i16(170, 1)?;
        self.writer.write_i32(91, style.override_flags)?;
        self.writer
            .write_i32(92, style.property_flags.bits() as i32)?;
        self.writer.write_color(62, style.background_color)?;
        self.writer
            .write_i32(93, style.layout_flags.bits() as i32)?;
        self.write_table_style_content_format_dxf(style)?;
        self.writer.write_i16(171, style.margin_override_flags)?;
        if style.margin_override_flags != 0 {
            self.writer.write_string(301, "MARGIN")?;
            self.writer.write_string(1, "CELLMARGIN_BEGIN")?;
            for value in [
                style.margin_top,
                style.margin_left,
                style.margin_bottom,
                style.margin_right,
                style.horizontal_spacing,
                style.vertical_spacing,
            ] {
                self.writer.write_double(40, value)?;
            }
            self.writer.write_string(309, "CELLMARGIN_END")?;
        }

        let mut borders: Vec<(u32, &table::CellBorder)> = Vec::new();
        for (edge, border) in [
            (table::CellEdgeFlags::TOP, &style.top_border),
            (table::CellEdgeFlags::RIGHT, &style.right_border),
            (table::CellEdgeFlags::BOTTOM, &style.bottom_border),
            (table::CellEdgeFlags::LEFT, &style.left_border),
        ] {
            if style.applied_border_edges.contains(edge) {
                borders.push((edge.bits(), border));
            }
        }
        borders.extend(
            style
                .additional_borders
                .iter()
                .map(|(edge, border)| (*edge, border)),
        );
        self.writer.write_i32(94, borders.len() as i32)?;
        for (edge, border) in borders {
            self.write_table_grid_format_dxf(edge, border)?;
        }
        self.writer.write_string(309, "TABLEFORMAT_END")?;
        Ok(())
    }

    fn table_content_has_format(content: &table::CellContent) -> bool {
        content.format_override_flags != 0
            || content.format_property_flags != 0
            || content.format_value_data_type != 0
            || content.format_value_unit_type != 0
            || !content.value_format.is_empty()
            || content.rotation != 0.0
            || content.scale != 1.0
            || content.alignment != 0
            || content.color != Color::ByBlock
            || content.text_style_handle.is_some()
            || !content.text_style_name.is_empty()
            || content.text_height != 0.18
    }

    fn write_table_modern_content_dxf(&mut self, content: &table::CellContent) -> Result<()> {
        self.writer.write_string(302, "CONTENT")?;
        self.writer.write_string(1, "CELLCONTENT_BEGIN")?;
        self.writer.write_i32(90, content.content_type as i32)?;
        match content.content_type {
            TableCellContentType::Value => {
                self.writer.write_string(300, "VALUE")?;
                self.write_field_cell_value_dxf(&content.value)?;
                self.writer.write_string(304, "ACVALUE_END")?;
            }
            TableCellContentType::Field => {
                self.writer
                    .write_handle(340, content.field_handle.unwrap_or(Handle::NULL))?;
            }
            TableCellContentType::Block => {
                self.writer
                    .write_handle(340, content.block_handle.unwrap_or(Handle::NULL))?;
            }
            TableCellContentType::Unknown => {}
        }
        self.writer.write_i32(91, content.attributes.len() as i32)?;
        for attribute in &content.attributes {
            self.writer.write_handle(330, attribute.definition_handle)?;
            self.writer.write_string(301, &attribute.value)?;
            self.writer.write_i32(92, attribute.index)?;
        }
        self.writer.write_string(309, "CELLCONTENT_END")?;
        self.writer.write_string(1, "FORMATTEDCELLCONTENT_BEGIN")?;
        let has_format = Self::table_content_has_format(content);
        self.writer.write_i16(170, has_format as i16)?;
        if has_format {
            self.write_table_content_format_dxf(content)?;
        }
        self.writer.write_string(309, "FORMATTEDCELLCONTENT_END")?;
        Ok(())
    }

    fn write_table_modern_cell_dxf(&mut self, cell: &table::TableCell) -> Result<()> {
        self.writer.write_string(300, "CELL")?;
        self.writer.write_string(1, "LINKEDTABLEDATACELL_BEGIN")?;
        self.writer.write_i32(90, cell.state.bits() as i32)?;
        self.writer.write_string(300, &cell.tooltip)?;
        self.writer.write_i32(91, cell.custom_data)?;
        self.write_table_custom_data_dxf(&cell.custom_data_items)?;
        self.writer.write_i32(92, cell.has_linked_data as i32)?;
        if cell.has_linked_data {
            self.writer
                .write_handle(340, cell.data_link_handle.unwrap_or(Handle::NULL))?;
            self.writer.write_i32(93, cell.data_link_rows)?;
            self.writer.write_i32(94, cell.data_link_columns)?;
            self.writer.write_i32(96, cell.data_link_unknown)?;
        }
        self.writer.write_i32(95, cell.contents.len() as i32)?;
        for content in &cell.contents {
            self.write_table_modern_content_dxf(content)?;
        }
        self.writer.write_string(309, "LINKEDTABLEDATACELL_END")?;

        self.writer
            .write_string(1, "FORMATTEDTABLEDATACELL_BEGIN")?;
        self.writer.write_string(300, "CELLTABLEFORMAT")?;
        self.write_table_cell_style_override_dxf(cell.style.as_ref(), table::CellStyleType::Cell)?;
        self.writer
            .write_string(309, "FORMATTEDTABLEDATACELL_END")?;

        self.writer.write_string(1, "TABLECELL_BEGIN")?;
        self.writer.write_i32(90, cell.style_id)?;
        let geometries: Vec<_> = if !cell.geometries.is_empty() {
            cell.geometries.iter().collect()
        } else if let Some(geometry) = cell.geometry.as_ref() {
            vec![geometry]
        } else {
            cell.contents
                .iter()
                .filter_map(|content| content.geometry.as_ref())
                .collect()
        };
        let has_geometry = !geometries.is_empty()
            || cell.geometry_handle.is_some()
            || cell.geometry_data_flag != 0
            || cell.geometry_width_with_gap != 0.0
            || cell.geometry_height_with_gap != 0.0;
        self.writer.write_i32(91, has_geometry as i32)?;
        if has_geometry {
            self.writer.write_i32(91, cell.geometry_data_flag)?;
            self.writer.write_double(40, cell.geometry_width_with_gap)?;
            self.writer
                .write_double(41, cell.geometry_height_with_gap)?;
            self.writer
                .write_handle(330, cell.geometry_handle.unwrap_or(Handle::NULL))?;
            self.writer.write_i32(92, geometries.len() as i32)?;
            for geometry in geometries {
                self.writer
                    .write_point3d(10, geometry.distance_to_top_left)?;
                self.writer.write_point3d(11, geometry.distance_to_center)?;
                self.writer.write_double(43, geometry.width)?;
                self.writer.write_double(44, geometry.height)?;
                self.writer.write_double(45, geometry.outer_width)?;
                self.writer.write_double(46, geometry.outer_height)?;
                self.writer.write_i32(95, geometry.flags)?;
            }
        }
        self.writer.write_string(309, "TABLECELL_END")?;
        Ok(())
    }

    fn write_table_content_object_dxf(&mut self, table: &table::Table) -> Result<()> {
        self.writer.write_string(0, "TABLECONTENT")?;
        self.writer.write_handle(5, table.common.handle)?;
        if !table.common.reactors.is_empty() {
            self.writer.write_string(102, "{ACAD_REACTORS")?;
            for reactor in &table.common.reactors {
                self.writer.write_handle(330, *reactor)?;
            }
            self.writer.write_string(102, "}")?;
        }
        if let Some(xdictionary) = table.common.xdictionary_handle {
            self.writer.write_string(102, "{ACAD_XDICTIONARY")?;
            self.writer.write_handle(360, xdictionary)?;
            self.writer.write_string(102, "}")?;
        }
        self.writer.write_handle(330, table.common.owner_handle)?;
        self.writer.write_subclass("AcDbLinkedData")?;
        self.writer.write_string(1, &table.name)?;
        self.writer.write_string(300, &table.description)?;
        self.writer.write_subclass("AcDbLinkedTableData")?;
        self.writer.write_i32(90, table.columns.len() as i32)?;
        for column in &table.columns {
            self.writer.write_string(300, "COLUMN")?;
            self.writer.write_string(1, "LINKEDTABLEDATACOLUMN_BEGIN")?;
            self.writer.write_string(300, &column.name)?;
            self.writer.write_i32(91, column.custom_data)?;
            self.write_table_custom_data_dxf(&column.custom_data_items)?;
            self.writer.write_string(309, "LINKEDTABLEDATACOLUMN_END")?;
            self.writer
                .write_string(1, "FORMATTEDTABLEDATACOLUMN_BEGIN")?;
            self.writer.write_string(300, "COLUMNTABLEFORMAT")?;
            self.write_table_cell_style_override_dxf(
                column.style.as_ref(),
                table::CellStyleType::Column,
            )?;
            self.writer
                .write_string(309, "FORMATTEDTABLEDATACOLUMN_END")?;
            self.writer.write_string(1, "TABLECOLUMN_BEGIN")?;
            self.writer.write_i32(90, column.style_id)?;
            self.writer.write_double(40, column.width)?;
            self.writer.write_string(309, "TABLECOLUMN_END")?;
        }
        self.writer.write_i32(91, table.rows.len() as i32)?;
        for row in &table.rows {
            self.writer.write_string(301, "ROW")?;
            self.writer.write_string(1, "LINKEDTABLEDATAROW_BEGIN")?;
            self.writer.write_i32(90, row.cells.len() as i32)?;
            for cell in &row.cells {
                self.write_table_modern_cell_dxf(cell)?;
            }
            self.writer.write_i32(91, row.custom_data)?;
            self.write_table_custom_data_dxf(&row.custom_data_items)?;
            self.writer.write_string(309, "LINKEDTABLEDATAROW_END")?;
            self.writer.write_string(1, "FORMATTEDTABLEDATAROW_BEGIN")?;
            self.writer.write_string(300, "ROWTABLEFORMAT")?;
            self.write_table_cell_style_override_dxf(
                row.style.as_ref(),
                table::CellStyleType::Row,
            )?;
            self.writer.write_string(309, "FORMATTEDTABLEDATAROW_END")?;
            self.writer.write_string(1, "TABLEROW_BEGIN")?;
            self.writer.write_i32(90, row.style_id)?;
            self.writer.write_double(40, row.height)?;
            self.writer.write_string(309, "TABLEROW_END")?;
        }
        self.writer
            .write_i32(92, table.field_handles.len() as i32)?;
        // The table owns its cell fields (hard owner, as the reference writes them).
        for field in &table.field_handles {
            self.writer.write_handle(360, *field)?;
        }
        self.writer.write_subclass("AcDbFormattedTableData")?;
        self.writer.write_string(300, "TABLEFORMAT")?;
        self.write_table_cell_style_override_dxf(
            table.base_style.as_ref(),
            table::CellStyleType::FormattedTableData,
        )?;
        self.writer
            .write_i32(90, table.merged_ranges.len() as i32)?;
        for range in &table.merged_ranges {
            self.writer.write_i32(91, range.top_row as i32)?;
            self.writer.write_i32(92, range.left_col as i32)?;
            self.writer.write_i32(93, range.bottom_row as i32)?;
            self.writer.write_i32(94, range.right_col as i32)?;
        }
        self.writer.write_subclass("AcDbTableContent")?;
        self.writer
            .write_handle(340, table.table_style_handle.unwrap_or(Handle::NULL))?;
        Ok(())
    }

    fn legacy_table_override_flags(table: &table::Table) -> (i32, i32, i32, i32) {
        (
            table
                .legacy_style_override
                .as_ref()
                .map(|value| value.flags)
                .unwrap_or(table.override_flag as i32),
            table
                .legacy_border_colors
                .as_ref()
                .map(|value| value.flags)
                .unwrap_or(table.override_border_color as i32),
            table
                .legacy_border_line_weights
                .as_ref()
                .map(|value| value.flags)
                .unwrap_or(table.override_border_line_weight as i32),
            table
                .legacy_border_visibility
                .as_ref()
                .map(|value| value.flags)
                .unwrap_or(table.override_border_visibility as i32),
        )
    }

    fn write_legacy_table_override_values_dxf(
        &mut self,
        table: &table::Table,
        flags: (i32, i32, i32, i32),
    ) -> Result<()> {
        let default_style = table::LegacyTableStyleOverride::default();
        let style = table
            .legacy_style_override
            .as_ref()
            .unwrap_or(&default_style);
        let style_flags = flags.0;
        if style_flags & 0x0001 != 0 {
            self.writer
                .write_bool(280, style.title_suppressed.unwrap_or(false))?;
        }
        if style_flags & 0x0002 != 0 {
            self.writer
                .write_bool(281, style.header_suppressed.unwrap_or(false))?;
        }
        if style_flags & 0x0004 != 0 {
            self.writer
                .write_i16(70, style.flow_direction.unwrap_or(0))?;
        }
        if style_flags & 0x0008 != 0 {
            self.writer
                .write_double(40, style.horizontal_cell_margin.unwrap_or(0.0))?;
        }
        if style_flags & 0x0010 != 0 {
            self.writer
                .write_double(41, style.vertical_cell_margin.unwrap_or(0.0))?;
        }

        let mut index = 0usize;
        for bit in [0x0020, 0x0040, 0x0080] {
            if style_flags & bit != 0 {
                self.writer.write_color(
                    64,
                    style
                        .row_colors
                        .get(index)
                        .cloned()
                        .unwrap_or(Color::ByBlock),
                )?;
                index += 1;
            }
        }
        index = 0;
        for bit in [0x0100, 0x0200, 0x0400] {
            if style_flags & bit != 0 {
                self.writer.write_bool(
                    283,
                    style.row_fill_none.get(index).copied().unwrap_or(false),
                )?;
                index += 1;
            }
        }
        index = 0;
        for bit in [0x0800, 0x1000, 0x2000] {
            if style_flags & bit != 0 {
                self.writer.write_color(
                    63,
                    style
                        .row_fill_colors
                        .get(index)
                        .cloned()
                        .unwrap_or(Color::ByBlock),
                )?;
                index += 1;
            }
        }
        index = 0;
        for bit in [0x4000, 0x8000, 0x10000] {
            if style_flags & bit != 0 {
                self.writer
                    .write_i16(170, style.row_alignments.get(index).copied().unwrap_or(0))?;
                index += 1;
            }
        }
        index = 0;
        for bit in [0x20000, 0x40000, 0x80000] {
            if style_flags & bit != 0 {
                let name = style
                    .text_style_names
                    .get(index)
                    .cloned()
                    .or_else(|| {
                        style
                            .text_style_handles
                            .get(index)
                            .and_then(|handle| self.text_style_names.get(handle).cloned())
                    })
                    .unwrap_or_else(|| "Standard".to_string());
                self.writer.write_string(7, &name)?;
                index += 1;
            }
        }
        index = 0;
        for bit in [0x100000, 0x200000, 0x400000] {
            if style_flags & bit != 0 {
                self.writer
                    .write_double(140, style.row_heights.get(index).copied().unwrap_or(0.0))?;
                index += 1;
            }
        }

        let color_codes = [64, 65, 66, 63, 68, 69];
        let default_colors = table::LegacyBorderOverrides::<Color>::default();
        let colors = table
            .legacy_border_colors
            .as_ref()
            .unwrap_or(&default_colors);
        index = 0;
        for bit in 0..18 {
            if flags.1 & (1 << bit) != 0 {
                self.writer.write_color(
                    color_codes[bit % 6],
                    colors.values.get(index).cloned().unwrap_or(Color::ByBlock),
                )?;
                index += 1;
            }
        }

        let default_weights = table::LegacyBorderOverrides::<LineWeight>::default();
        let weights = table
            .legacy_border_line_weights
            .as_ref()
            .unwrap_or(&default_weights);
        index = 0;
        for bit in 0..18 {
            if flags.2 & (1 << bit) != 0 {
                self.writer.write_i16(
                    274 + (bit % 6) as i32,
                    weights
                        .values
                        .get(index)
                        .copied()
                        .unwrap_or(LineWeight::ByLayer)
                        .value(),
                )?;
                index += 1;
            }
        }

        let default_visibility = table::LegacyBorderOverrides::<bool>::default();
        let visibility = table
            .legacy_border_visibility
            .as_ref()
            .unwrap_or(&default_visibility);
        index = 0;
        for bit in 0..18 {
            if flags.3 & (1 << bit) != 0 {
                self.writer.write_bool(
                    284 + (bit % 6) as i32,
                    visibility.values.get(index).copied().unwrap_or(true),
                )?;
                index += 1;
            }
        }
        Ok(())
    }

    /// Write ACAD_TABLE entity
    fn write_acad_table(&mut self, table: &table::Table, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("ACAD_TABLE")?;
        self.write_common_entity_data(&table.common, owner)?;
        self.writer.write_subclass("AcDbBlockReference")?;

        let block_name = if !table.block_name.is_empty() {
            Some(table.block_name.as_str())
        } else {
            table
                .block_record_handle
                .and_then(|handle| self.block_record_names.get(&handle).map(String::as_str))
        };
        if let Some(name) = block_name {
            self.writer.write_string(2, name)?;
        }

        // Insertion point
        self.writer.write_point3d(10, table.insertion_point)?;
        self.writer.write_point3d(210, table.normal)?;

        self.writer.write_subclass("AcDbTable")?;

        // Table style handle
        if let Some(h) = table.table_style_handle {
            self.writer.write_handle(342, h)?;
        }
        if let Some(h) = table.block_record_handle {
            self.writer.write_handle(343, h)?;
        }

        // Data version
        self.writer.write_byte(280, table.data_version as u8)?;

        // Horizontal direction
        self.writer.write_point3d(11, table.horizontal_direction)?;

        self.writer.write_i32(90, table.value_flags)?;

        // Number of rows
        self.writer.write_i32(91, table.rows.len() as i32)?;

        // Number of columns
        self.writer.write_i32(92, table.columns.len() as i32)?;

        let override_flags = Self::legacy_table_override_flags(table);
        self.writer.write_i32(93, override_flags.0)?;
        self.writer.write_i32(94, override_flags.1)?;
        self.writer.write_i32(95, override_flags.2)?;
        self.writer.write_i32(96, override_flags.3)?;
        self.write_legacy_table_override_values_dxf(table, override_flags)?;

        // Row heights
        for row in &table.rows {
            self.writer.write_double(141, row.height)?;
        }

        // Column widths
        for col in &table.columns {
            self.writer.write_double(142, col.width)?;
        }

        // Write cells
        let merged_ranges = table.canonical_merged_ranges();
        for (row_index, row) in table.rows.iter().enumerate() {
            for (column_index, cell) in row.cells.iter().enumerate() {
                let merged_range = merged_ranges
                    .iter()
                    .find(|range| range.contains(row_index, column_index));
                let anchor_range = merged_range.filter(|range| {
                    range.top_row == row_index && range.left_col == column_index
                });
                // A table the DXF reader completed from its TABLECONTENT object states its cell overrides as the
                // legacy entity did: the object's modern styles do not map back to the legacy groups.
                let legacy = table
                    .dxf_legacy
                    .0
                    .as_ref()
                    .and_then(|legacy| legacy.rows.get(row_index))
                    .and_then(|legacy| legacy.cells.get(column_index));
                match legacy {
                    Some(legacy) => {
                        let mut view = cell.clone();
                        view.style = legacy.style.clone();
                        view.source_style_type = legacy.source_style_type;
                        // The items too, while the cell still shows what the legacy entity showed (the legacy
                        // block cell has a block pointer and a value of its own; the completed cell has the
                        // pointer only).
                        let shown = |cell: &TableCell| {
                            cell.contents
                                .first()
                                .map(|content| (content.content_type, content.value.display().to_string()))
                        };
                        if shown(cell) == shown(legacy) {
                            view.contents = legacy.contents.clone();
                        }
                        self.write_table_cell(&view, anchor_range, merged_range.is_some())?;
                    }
                    None => self.write_table_cell(cell, anchor_range, merged_range.is_some())?,
                }
            }
        }

        Ok(())
    }

    /// Write table cell data
    fn write_table_cell(
        &mut self,
        cell: &TableCell,
        merged_range: Option<&table::CellRange>,
        merged: bool,
    ) -> Result<()> {
        // Cell type
        self.writer.write_i16(171, cell.cell_type as i16)?;

        // Legacy cell-edge flags. State is stored separately in the R2007+
        // extended cell flags (group 92).
        self.writer.write_i16(172, cell.edge_flags as i16)?;

        self.writer
            .write_i16(173, if merged_range.is_some() {
                0
            } else if merged {
                1
            } else {
                cell.merged as i16
            })?;
        self.writer.write_i16(174, cell.auto_fit as i16)?;
        let merge_width = merged_range.map_or(cell.merge_width, |range| range.col_count() as i32);
        let merge_height = merged_range.map_or(cell.merge_height, |range| range.row_count() as i32);
        self.writer.write_i16(175, merge_width as i16)?;
        self.writer.write_i16(176, merge_height as i16)?;
        // Group 91 (177 before R2007) is the override word of the cell's legacy style: the bits its groups imply
        // and the ones no group states.
        let flag = cell
            .style
            .as_ref()
            .filter(|style| style.legacy_override_bits)
            .map_or(cell.flag, |style| style.override_flags);
        if self.dxf_version >= DxfVersion::AC1021 {
            self.writer.write_i32(91, flag)?;
            self.writer.write_i32(92, cell.state.bits() as i32)?;
        } else {
            self.writer.write_i16(177, flag as i16)?;
        }

        // Virtual edge flag
        self.writer.write_i16(178, cell.virtual_edge)?;

        // Rotation
        self.writer.write_double(145, cell.rotation)?;
        if cell.cell_type == crate::entities::table::CellType::Block {
            self.writer.write_double(144, cell.block_scale)?;
        }

        // ACAD_TABLE stores one effective content item per legacy cell.
        if let Some(content) = cell.contents.first() {
            if cell.cell_type == crate::entities::table::CellType::Text {
                if let Some(handle) = content.field_handle {
                    self.writer.write_handle(344, handle)?;
                } else if self.dxf_version < DxfVersion::AC1021
                    || content.content_type != TableCellContentType::Value
                {
                    let text = if !content.value.text.is_empty() {
                        content.value.text.as_str()
                    } else {
                        content.value.formatted_value.as_str()
                    };
                    let mut remaining = text;
                    while remaining.len() > 250 {
                        let mut split = 250;
                        while split > 0 && !remaining.is_char_boundary(split) {
                            split -= 1;
                        }
                        let (chunk, rest) = remaining.split_at(split);
                        self.writer.write_string(2, chunk)?;
                        remaining = rest;
                    }
                    self.writer.write_string(1, remaining)?;
                }
            } else if let Some(handle) = content.block_handle {
                self.writer.write_handle(340, handle)?;
            }

            // A block cell states the value of its own after the block pointer (the second item).
            let value_item = if content.content_type == TableCellContentType::Value {
                Some(content)
            } else {
                cell.contents
                    .get(1)
                    .filter(|item| item.content_type == TableCellContentType::Value)
                    .filter(|_| cell.cell_type == crate::entities::table::CellType::Block)
            };
            if let Some(item) = value_item.filter(|_| self.dxf_version >= DxfVersion::AC1021) {
                self.writer.write_string(301, "CELL_VALUE")?;
                self.write_field_cell_value_dxf(&item.value)?;
                self.writer.write_string(304, "ACVALUE_END")?;
            }

            if !content.attributes.is_empty() {
                self.writer
                    .write_i16(179, content.attributes.len() as i16)?;
                for attribute in &content.attributes {
                    self.writer.write_handle(331, attribute.definition_handle)?;
                    self.writer.write_string(300, &attribute.value)?;
                }
            }
        }

        // A style read from a binary R2010+ file states what it overrides in the property-flag layout, not in the
        // legacy bits below, and a cell that overrides nothing has no style groups.
        let modern_style = cell
            .source_style_type
            .is_some()
            .then_some(cell.style.as_ref())
            .flatten()
            .filter(|s| !s.legacy_override_bits);
        // Cell style: only the overridden properties when the cell says which.
        if let Some(style) = modern_style {
            self.write_modern_cell_style_overrides(style)?;
        } else if let Some(style) = cell.style.as_ref().filter(|s| s.override_flags != 0) {
            let f = style.override_flags;
            if f & 0x01 != 0 {
                self.writer.write_i16(170, style.alignment as i16)?;
            }
            if f & 0x02 != 0 {
                self.writer.write_bool(283, !style.fill_enabled)?;
            }
            if f & 0x04 != 0 {
                self.writer.write_color(63, style.background_color)?;
            }
            if f & 0x08 != 0 {
                self.writer.write_color(64, style.content_color)?;
            }
            if f & 0x10 != 0 && !style.text_style_name.is_empty() {
                self.writer.write_string(7, &style.text_style_name)?;
            }
            if f & 0x20 != 0 {
                self.writer.write_double(140, style.text_height)?;
            }
            for (color_bit, lw_bit, (color_code, weight_code, visibility_code), border) in [
                (0x40, 0x400, (69, 279, 289), &style.top_border),
                (0x80, 0x800, (65, 275, 285), &style.right_border),
                (0x100, 0x1000, (66, 276, 286), &style.bottom_border),
                (0x200, 0x2000, (68, 278, 288), &style.left_border),
            ] {
                if f & color_bit != 0 {
                    self.writer.write_color(color_code, border.color)?;
                }
                if f & lw_bit != 0 {
                    self.writer.write_i16(weight_code, border.line_weight.value())?;
                    self.writer.write_bool(visibility_code, !border.invisible)?;
                }
            }
        } else if let Some(ref style) = cell.style {
            if !style.text_style_name.is_empty() {
                self.writer.write_string(7, &style.text_style_name)?;
            }
            self.writer.write_double(140, style.text_height)?;
            self.writer.write_i16(170, style.alignment as i16)?;
            self.writer.write_color(64, style.content_color)?;
            self.writer.write_color(63, style.background_color)?;
            self.writer.write_bool(283, !style.fill_enabled)?;

            for (color_code, weight_code, visibility_code, border) in [
                (69, 279, 289, &style.top_border),
                (65, 275, 285, &style.right_border),
                (66, 276, 286, &style.bottom_border),
                (68, 278, 288, &style.left_border),
            ] {
                self.writer.write_color(color_code, border.color)?;
                self.writer
                    .write_i16(weight_code, border.line_weight.value())?;
                self.writer.write_bool(visibility_code, !border.invisible)?;
            }
        }

        Ok(())
    }

    /// The legacy cell-style groups for a style read from a binary R2010+ file: only the properties it overrides.
    fn write_modern_cell_style_overrides(&mut self, style: &crate::entities::CellStyle) -> Result<()> {
        use crate::entities::table::{BorderPropertyFlags as B, CellEdgeFlags, CellStylePropertyFlags as P};
        let p = style.overridden();
        if p.contains(P::ALIGNMENT) {
            self.writer.write_i16(170, style.alignment as i16)?;
        }
        if p.contains(P::BACKGROUND_COLOR) {
            self.writer.write_bool(283, !style.fill_enabled)?;
            if style.fill_enabled {
                self.writer.write_color(63, style.background_color)?;
            }
        }
        if p.contains(P::CONTENT_COLOR) {
            self.writer.write_color(64, style.content_color)?;
        }
        if p.contains(P::TEXT_STYLE) {
            let name = style
                .text_style_handle
                .and_then(|handle| self.text_style_names.get(&handle).cloned())
                .unwrap_or_else(|| style.text_style_name.clone());
            if !name.is_empty() {
                self.writer.write_string(7, &name)?;
            }
        }
        if p.contains(P::TEXT_HEIGHT) {
            self.writer.write_double(140, style.text_height)?;
        }
        for (edge, (color_code, weight_code, visibility_code), border) in [
            (CellEdgeFlags::TOP, (69, 279, 289), &style.top_border),
            (CellEdgeFlags::RIGHT, (65, 275, 285), &style.right_border),
            (CellEdgeFlags::BOTTOM, (66, 276, 286), &style.bottom_border),
            (CellEdgeFlags::LEFT, (68, 278, 288), &style.left_border),
        ] {
            if !style.applied_border_edges.contains(edge) {
                continue;
            }
            if border.override_flags.contains(B::COLOR) {
                self.writer.write_color(color_code, border.color)?;
            }
            if border.override_flags.intersects(B::LINE_WEIGHT | B::INVISIBILITY) {
                self.writer.write_i16(weight_code, border.line_weight.value())?;
                self.writer.write_bool(visibility_code, !border.invisible)?;
            }
        }
        Ok(())
    }

    /// Write a Tolerance entity
    fn write_tolerance(&mut self, tolerance: &Tolerance, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("TOLERANCE")?;
        self.write_common_entity_data(&tolerance.common, owner)?;
        self.writer.write_subclass("AcDbFcf")?;

        // Dimension style name
        self.writer
            .write_string(3, &tolerance.dimension_style_name)?;

        // Insertion point
        self.writer.write_double(10, tolerance.insertion_point.x)?;
        self.writer.write_double(20, tolerance.insertion_point.y)?;
        self.writer.write_double(30, tolerance.insertion_point.z)?;

        // Normal vector
        self.writer.write_double(210, tolerance.normal.x)?;
        self.writer.write_double(220, tolerance.normal.y)?;
        self.writer.write_double(230, tolerance.normal.z)?;

        // Direction vector: a source that omitted it keeps it omitted (the reference reader then uses a zero
        // vector rather than the native +X default).
        if tolerance.dxf_source.as_ref().map_or(true, |source| source.has_direction)
            || tolerance.direction != Vector3::UNIT_X
        {
            self.writer.write_double(11, tolerance.direction.x)?;
            self.writer.write_double(21, tolerance.direction.y)?;
            self.writer.write_double(31, tolerance.direction.z)?;
        }

        // Tolerance text: a line break is the caret escape ^J, not the MTEXT paragraph mark. The text DXF source
        // spelling is kept while it still decodes to the text; binary DXF holds the characters themselves.
        let raw = tolerance
            .dxf_source
            .as_ref()
            .and_then(|source| source.raw_text.as_deref())
            .filter(|raw| {
                raw.replace("^J", "\n")
                    .replace("^M", "\r")
                    .replace("^I", "\t")
                    .replace("^ ", "^")
                    == tolerance.text
            });
        match raw {
            Some(raw) if self.writer.writes_source_spelling() => {
                self.writer.write_source_spelling(1, raw)?
            }
            _ => self.writer.write_xrecord_string(1, &tolerance.text)?,
        }

        Ok(())
    }

    /// Write a PolyfaceMesh entity
    fn write_polyface_mesh(&mut self, mesh: &PolyfaceMesh, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("POLYLINE")?;
        self.write_common_entity_data(&mesh.common, owner)?;
        self.writer.write_subclass("AcDbPolyFaceMesh")?;

        // Entities follow flag (VERTEX records follow)
        self.writer.write_i16(66, 1)?;

        // Dummy point with elevation (reference pattern)
        self.writer.write_double(10, 0.0)?;
        self.writer.write_double(20, 0.0)?;
        self.writer.write_double(30, mesh.elevation)?;

        // Polyline flags (64 = polyface mesh) - MUST be before 71/72
        self.writer.write_i16(70, mesh.flags.bits())?;

        // Vertex count - MUST come before smooth surface type
        self.writer.write_i16(71, mesh.vertex_count() as i16)?;
        // Face count - MUST come before smooth surface type
        self.writer.write_i16(72, mesh.face_count() as i16)?;

        // XDATA precedes the child VERTEX/SEQEND records.
        self.write_xdata(&mesh.common.extended_data)?;

        // Write vertices with proper subclass markers
        for vertex in mesh.vertices.iter() {
            let mut common = vertex.common.clone();
            let vertex_handle = if common.handle.is_null() {
                Self::inherit_compound_child_defaults(&mut common, &mesh.common);
                self.allocate_handle()
            } else {
                common.handle
            };
            common.handle = vertex_handle;
            common.owner_handle = mesh.common.handle;
            self.writer.write_entity_type("VERTEX")?;
            self.write_common_entity_data(&common, mesh.common.handle)?;
            self.writer.write_subclass("AcDbVertex")?;
            self.writer.write_subclass("AcDbPolyFaceMeshVertex")?;

            self.writer.write_double(10, vertex.location.x)?;
            self.writer.write_double(20, vertex.location.y)?;
            self.writer.write_double(30, vertex.location.z)?;

            let flags = vertex.flags
                | PolyfaceVertexFlags::POLYGON_MESH
                | PolyfaceVertexFlags::POLYFACE_MESH;
            self.writer.write_i16(70, flags.bits())?;
            self.write_xdata(&common.extended_data)?;
        }

        // Write faces with proper subclass markers
        for face in mesh.faces.iter() {
            let mut common = face.common.clone();
            let face_handle = if common.handle.is_null() {
                Self::inherit_compound_child_defaults(&mut common, &mesh.common);
                self.allocate_handle()
            } else {
                common.handle
            };
            common.handle = face_handle;
            common.owner_handle = mesh.common.handle;
            self.writer.write_entity_type("VERTEX")?;
            self.write_common_entity_data(&common, mesh.common.handle)?;
            if let Some(c) = face.color {
                if let Some(tc) = c.to_true_color_value() {
                    self.writer.write_i32(420, tc)?;
                } else if let Some(idx) = c.index() {
                    self.writer.write_i16(62, idx as i16)?;
                }
            }
            self.writer.write_subclass("AcDbFaceRecord")?;

            // Dummy position
            self.writer.write_double(10, 0.0)?;
            self.writer.write_double(20, 0.0)?;
            self.writer.write_double(30, 0.0)?;

            let flags = face.flags | PolyfaceVertexFlags::POLYFACE_MESH;
            self.writer.write_i16(70, flags.bits())?; // Face record flag

            // Vertex indices (preserve sign for edge visibility)
            self.writer.write_i16(71, face.index1)?;
            self.writer.write_i16(72, face.index2)?;
            self.writer.write_i16(73, face.index3)?;
            if face.index4 != 0 {
                self.writer.write_i16(74, face.index4)?;
            }
            self.write_xdata(&common.extended_data)?;
        }

        // Write SEQEND with AcDbEntity subclass. A retained SEQEND wins;
        // seqend_handle remains the compatibility fallback for callers that
        // populated only the old scalar field.
        let seqend_handle = mesh
            .seqend
            .as_ref()
            .map(|seqend| seqend.common.handle)
            .filter(|handle| !handle.is_null())
            .or_else(|| mesh.seqend_handle.filter(|handle| !handle.is_null()))
            .unwrap_or_else(|| self.allocate_handle());
        self.write_owned_seqend(
            mesh.seqend.as_deref(),
            &mesh.common,
            seqend_handle,
            mesh.common.handle,
        )?;

        Ok(())
    }

    /// Write a Wipeout entity
    fn write_wipeout(&mut self, wipeout: &Wipeout, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("WIPEOUT")?;
        self.write_common_entity_data(&wipeout.common, owner)?;
        self.writer.write_subclass("AcDbWipeout")?;

        // Class version
        self.writer.write_i32(90, wipeout.class_version)?;

        // Insertion point
        self.writer.write_double(10, wipeout.insertion_point.x)?;
        self.writer.write_double(20, wipeout.insertion_point.y)?;
        self.writer.write_double(30, wipeout.insertion_point.z)?;

        // U-vector
        self.writer.write_double(11, wipeout.u_vector.x)?;
        self.writer.write_double(21, wipeout.u_vector.y)?;
        self.writer.write_double(31, wipeout.u_vector.z)?;

        // V-vector
        self.writer.write_double(12, wipeout.v_vector.x)?;
        self.writer.write_double(22, wipeout.v_vector.y)?;
        self.writer.write_double(32, wipeout.v_vector.z)?;

        // Size
        self.writer.write_double(13, wipeout.size.x)?;
        self.writer.write_double(23, wipeout.size.y)?;

        if let Some(handle) = wipeout.definition_handle {
            self.writer.write_handle(340, handle)?;
        }

        // Display flags
        self.writer.write_i16(70, wipeout.flags.bits())?;

        // Clipping
        self.writer
            .write_byte(280, if wipeout.clipping_enabled { 1 } else { 0 })?;
        self.writer.write_byte(281, wipeout.brightness)?;
        self.writer.write_byte(282, wipeout.contrast)?;
        self.writer.write_byte(283, wipeout.fade)?;

        if let Some(handle) = wipeout.definition_reactor_handle {
            self.writer.write_handle(360, handle)?;
        }

        if self.dxf_version >= DxfVersion::AC1024 {
            self.writer.write_bool(
                290,
                wipeout.clip_mode == crate::entities::WipeoutClipMode::Inside,
            )?;
        }

        // Clip boundary type
        self.writer.write_i16(71, wipeout.clip_type as i16)?;

        // Clip boundary count
        self.writer
            .write_i32(91, wipeout.clip_boundary_vertices.len() as i32)?;

        // Clip boundary vertices
        for v in &wipeout.clip_boundary_vertices {
            self.writer.write_double(14, v.x)?;
            self.writer.write_double(24, v.y)?;
        }

        Ok(())
    }

    /// Write a Shape entity
    fn write_shape(&mut self, shape: &Shape, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("SHAPE")?;
        self.write_common_entity_data(&shape.common, owner)?;
        self.writer.write_subclass("AcDbShape")?;

        // Thickness
        if shape.thickness.abs() > 1e-10 {
            self.writer.write_double(39, shape.thickness)?;
        }

        // Insertion point
        self.writer.write_double(10, shape.insertion_point.x)?;
        self.writer.write_double(20, shape.insertion_point.y)?;
        self.writer.write_double(30, shape.insertion_point.z)?;

        // Size
        self.writer.write_double(40, shape.size)?;

        // Shape name
        self.writer.write_string(2, &shape.shape_name)?;

        // Rotation
        if shape.rotation.abs() > 1e-10 {
            self.writer.write_double(50, shape.rotation.to_dxf_degrees())?;
        }

        // Relative X scale
        if (shape.relative_x_scale - 1.0).abs() > 1e-10 {
            self.writer.write_double(41, shape.relative_x_scale)?;
        }

        // Oblique angle
        if shape.oblique_angle.abs() > 1e-10 {
            self.writer
                .write_double(51, shape.oblique_angle.to_dxf_degrees())?;
        }

        // Normal
        if shape.has_custom_normal() {
            self.writer.write_double(210, shape.normal.x)?;
            self.writer.write_double(220, shape.normal.y)?;
            self.writer.write_double(230, shape.normal.z)?;
        }

        Ok(())
    }

    /// Write an Underlay entity (PDF, DWF, or DGN)
    fn write_underlay(&mut self, underlay: &Underlay, owner: Handle) -> Result<()> {
        self.writer.write_entity_type(underlay.entity_name())?;
        self.write_common_entity_data(&underlay.common, owner)?;
        self.writer.write_subclass("AcDbUnderlayReference")?;

        // Definition handle
        self.writer.write_handle(340, underlay.definition_handle)?;

        // Insertion point
        self.writer.write_double(10, underlay.insertion_point.x)?;
        self.writer.write_double(20, underlay.insertion_point.y)?;
        self.writer.write_double(30, underlay.insertion_point.z)?;

        // Scale factors
        self.writer.write_double(41, underlay.x_scale)?;
        self.writer.write_double(42, underlay.y_scale)?;
        self.writer.write_double(43, underlay.z_scale)?;

        // Rotation
        self.writer
            .write_double(50, underlay.rotation.to_dxf_degrees())?;

        // Normal
        self.writer.write_double(210, underlay.normal.x)?;
        self.writer.write_double(220, underlay.normal.y)?;
        self.writer.write_double(230, underlay.normal.z)?;

        // Flags
        self.writer.write_byte(280, underlay.display_flags().bits())?;

        // Contrast
        self.writer.write_byte(281, underlay.contrast)?;

        // Fade
        self.writer.write_byte(282, underlay.fade)?;

        // Clip boundary vertices
        for v in &underlay.clip_boundary_vertices {
            self.writer.write_double(11, v.x)?;
            self.writer.write_double(21, v.y)?;
        }

        Ok(())
    }

    /// Write SEQEND entity (end-of-sequence marker)
    fn write_seqend(&mut self, seqend: &Seqend, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("SEQEND")?;
        self.write_common_entity_data(&seqend.common, owner)?;
        Ok(())
    }

    /// Apply the inherited layer/color defaults used by newly-created inline
    /// children while retaining any explicit child override.
    fn inherit_compound_child_defaults(child: &mut EntityCommon, parent: &EntityCommon) {
        if child.layer == "0" {
            child.layer = parent.layer.clone();
        }
        if child.color == Color::ByLayer {
            child.color = parent.color;
        }
    }

    /// Write a compound entity's owned SEQEND, retaining the source common
    /// fields when the reader attached one. Newly-created compounds inherit
    /// the parent common fields as they did before child identity was stored.
    fn write_owned_seqend(
        &mut self,
        seqend: Option<&Seqend>,
        fallback: &EntityCommon,
        handle: Handle,
        owner: Handle,
    ) -> Result<()> {
        let mut common = if let Some(seqend) = seqend {
            seqend.common.clone()
        } else {
            // A synthesized SEQEND has historically carried only the parent
            // layer/color and the standard common defaults. Do not copy the
            // parent's XDATA, reactors, or other entity-specific overrides.
            let mut common = EntityCommon::new();
            common.layer = fallback.layer.clone();
            common.color = fallback.color;
            common
        };
        common.handle = handle;
        common.owner_handle = owner;
        self.writer.write_entity_type("SEQEND")?;
        self.write_common_entity_data(&common, owner)?;
        self.write_xdata(&common.extended_data)?;
        Ok(())
    }

    /// Write OLE2FRAME entity
    fn write_ole2frame(&mut self, ole: &Ole2Frame, owner: Handle) -> Result<()> {
        self.writer.write_entity_type("OLE2FRAME")?;
        self.write_common_entity_data(&ole.common, owner)?;
        self.writer.write_subclass("AcDbOle2Frame")?;
        self.writer.write_i16(70, ole.version)?;
        if !ole.source_application.is_empty() {
            self.writer.write_string(3, &ole.source_application)?;
        }
        self.writer.write_double(10, ole.upper_left_corner.x)?;
        self.writer.write_double(20, ole.upper_left_corner.y)?;
        self.writer.write_double(30, ole.upper_left_corner.z)?;
        self.writer.write_double(11, ole.lower_right_corner.x)?;
        self.writer.write_double(21, ole.lower_right_corner.y)?;
        self.writer.write_double(31, ole.lower_right_corner.z)?;
        self.writer.write_i16(71, ole.ole_object_type as i16)?;
        self.writer
            .write_i16(72, if ole.is_paper_space { 1 } else { 0 })?;
        self.writer.write_i16(73, ole.lock_aspect as i16)?;
        let data = ole.encoded_payload();
        if !data.is_empty() {
            self.writer.write_i32(90, data.len() as i32)?;
            // Write binary data in 127-byte hex chunks (code 310)
            for chunk in data.chunks(127) {
                let hex: String = chunk.iter().map(|b| format!("{:02X}", b)).collect();
                self.writer.write_string(310, &hex)?;
            }
        }
        self.writer.write_string(1, "OLE")?;
        Ok(())
    }

    /// Write PolygonMesh entity (POLYLINE with flag bit 16)
    fn write_polygon_mesh(&mut self, mesh: &PolygonMeshEntity, owner: Handle) -> Result<()> {
        use crate::entities::polygon_mesh::PolygonMeshFlags;

        self.writer.write_entity_type("POLYLINE")?;
        self.write_common_entity_data(&mesh.common, owner)?;
        self.writer.write_subclass("AcDbPolygonMesh")?;

        // Entities follow flag (VERTEX records follow)
        self.writer.write_i16(66, 1)?;

        // Dummy origin point (required by DXF spec for POLYLINE entity)
        self.writer.write_double(10, 0.0)?;
        self.writer.write_double(20, 0.0)?;
        self.writer.write_double(30, 0.0)?;

        // Ensure PolygonMesh flag (16) is always set
        let flags = mesh.flags | PolygonMeshFlags::POLYGON_MESH;
        self.writer.write_i16(70, flags.bits())?;
        self.writer.write_i16(71, mesh.m_vertex_count)?;
        self.writer.write_i16(72, mesh.n_vertex_count)?;
        self.writer.write_i16(73, mesh.m_smooth_density)?;
        self.writer.write_i16(74, mesh.n_smooth_density)?;
        self.writer.write_i16(75, mesh.smooth_type as i16)?;
        if mesh.normal != Vector3::UNIT_Z {
            self.writer.write_double(210, mesh.normal.x)?;
            self.writer.write_double(220, mesh.normal.y)?;
            self.writer.write_double(230, mesh.normal.z)?;
        }

        // XDATA precedes the child VERTEX/SEQEND records.
        self.write_xdata(&mesh.common.extended_data)?;

        // VERTEX and SEQEND are owned by the mesh entity
        let mesh_handle = mesh.common.handle;

        // Write vertices with proper subclass markers
        for vertex in &mesh.vertices {
            let mut common = vertex.common.clone();
            let vertex_handle = if common.handle.is_null() {
                Self::inherit_compound_child_defaults(&mut common, &mesh.common);
                self.allocate_handle()
            } else {
                common.handle
            };
            common.handle = vertex_handle;
            common.owner_handle = mesh_handle;
            self.writer.write_entity_type("VERTEX")?;
            self.write_common_entity_data(&common, mesh_handle)?;
            self.writer.write_subclass("AcDbVertex")?;
            self.writer.write_subclass("AcDbPolygonMeshVertex")?;
            self.writer.write_point3d(10, vertex.location)?;
            if vertex.flags != 0 {
                self.writer.write_i16(70, vertex.flags)?;
            }
            self.write_xdata(&common.extended_data)?;
        }

        // Write SEQEND
        let seqend_handle = mesh
            .seqend
            .as_ref()
            .map(|seqend| seqend.common.handle)
            .filter(|handle| !handle.is_null())
            .unwrap_or_else(|| self.allocate_handle());
        self.write_owned_seqend(
            mesh.seqend.as_deref(),
            &mesh.common,
            seqend_handle,
            mesh_handle,
        )?;

        Ok(())
    }

    /// Convert ACIS data to SAB binary and queue for ACDSDATA section.
    fn queue_sab_data(&mut self, acis: &AcisData, entity_handle: Handle) {
        if acis.is_binary && !acis.sab_data.is_empty() {
            // Already have SAB binary data, use it directly
            self.sab_entries
                .push((entity_handle, acis.sab_for_version(self.dxf_version).into_owned()));
        } else if !acis.sat_data.is_empty() {
            // Convert SAT text to SAB binary via SatDocument.
            // Strip non-geometry entities (attributes, refinement, etc.)
            // which cause ACIS "NOT THAT KIND OF CLASS" errors in SAB.
            if let Ok(mut sat_doc) = crate::entities::acis::SatDocument::parse(&acis.sat_data) {
                sat_doc.strip_for_sab();
                let sab = crate::entities::acis::SabWriter::write(&sat_doc);
                self.sab_entries.push((entity_handle, sab));
            }
        }
    }

    /// Write the ACDSDATA section (AC1027+ only, for SAB binary ACIS data).
    ///
    /// This section stores ACIS SAB binary data for 3DSOLID, REGION, and BODY
    /// entities when the DXF version is AC1027 (R2013) or later.
    pub fn write_acdsdata(&mut self) -> Result<()> {
        if self.sab_entries.is_empty() {
            return Ok(());
        }

        self.writer.write_section_start("ACDSDATA")?;

        // Section-level header
        self.writer.write_i16(70, 2)?;
        self.writer.write_i16(71, 2)?;

        // Schema 0: AcDb_Thumbnail_Schema (standard boilerplate)
        self.write_acds_thumbnail_schema()?;

        // Schema 1: AcDb3DSolid_ASM_Data (for SAB data)
        self.write_acds_asm_schema()?;

        // Schemas 2-5: Standard infrastructure schemas
        self.write_acds_infrastructure_schemas()?;

        // ACDSRECORD entries (one per entity with SAB data)
        // Take entries from self to avoid borrow issues
        let entries = std::mem::take(&mut self.sab_entries);
        for (entity_handle, sab_data) in &entries {
            self.write_acds_record(*entity_handle, sab_data)?;
        }
        self.sab_entries = entries;

        self.writer.write_section_end()?;
        Ok(())
    }

    /// Write the THUMBNAILIMAGE section (R2000 and later): group 90 holds the image size in bytes, the
    /// image follows in 310 chunks of at most 127 bytes. The image is the document's preview as stored
    /// (a DIB without BITMAPFILEHEADER for a bitmap, as AutoCAD writes it).
    pub fn write_thumbnail(&mut self, document: &CadDocument) -> Result<()> {
        let Some(preview) = document.preview.as_ref().filter(|preview| !preview.data.is_empty()) else {
            return Ok(());
        };
        if self.dxf_version < DxfVersion::AC1015 {
            return Ok(());
        }
        self.writer.write_section_start("THUMBNAILIMAGE")?;
        self.writer.write_i32(90, preview.data.len() as i32)?;
        for chunk in preview.data.chunks(127) {
            self.writer.write_binary(310, chunk)?;
        }
        self.writer.write_section_end()
    }

    fn write_acds_thumbnail_schema(&mut self) -> Result<()> {
        self.writer.write_string(0, "ACDSSCHEMA")?;
        self.writer.write_i32(90, 0)?;
        self.writer.write_string(1, "AcDb_Thumbnail_Schema")?;
        self.writer.write_string(2, "AcDbDs::ID")?;
        self.writer.write_byte(280, 10)?;
        self.writer.write_i32(91, 8)?;
        self.writer.write_string(2, "Thumbnail_Data")?;
        self.writer.write_byte(280, 15)?;
        self.writer.write_i32(91, 0)?;

        // Schema records
        self.write_acds_schema_records(0)?;
        Ok(())
    }

    fn write_acds_asm_schema(&mut self) -> Result<()> {
        self.writer.write_string(0, "ACDSSCHEMA")?;
        self.writer.write_i32(90, 1)?;
        self.writer.write_string(1, "AcDb3DSolid_ASM_Data")?;
        self.writer.write_string(2, "AcDbDs::ID")?;
        self.writer.write_byte(280, 10)?;
        self.writer.write_i32(91, 8)?;
        self.writer.write_string(2, "ASM_Data")?;
        self.writer.write_byte(280, 15)?;
        self.writer.write_i32(91, 0)?;

        // Schema records
        self.write_acds_schema_records(1)?;
        Ok(())
    }

    fn write_acds_schema_records(&mut self, schema_id: i32) -> Result<()> {
        // TreatedAsObjectData record
        self.writer.write_string(101, "ACDSRECORD")?;
        self.writer.write_i32(95, schema_id)?;
        self.writer.write_i32(90, 2)?;
        self.writer.write_string(2, "AcDbDs::TreatedAsObjectData")?;
        self.writer.write_byte(280, 1)?;
        self.writer.write_bool(291, true)?;

        // Legacy record
        self.writer.write_string(101, "ACDSRECORD")?;
        self.writer.write_i32(95, schema_id)?;
        self.writer.write_i32(90, 3)?;
        self.writer.write_string(2, "AcDbDs::Legacy")?;
        self.writer.write_byte(280, 1)?;
        self.writer.write_bool(291, true)?;

        // Indexable record
        self.writer.write_string(101, "ACDSRECORD")?;
        self.writer.write_string(1, "AcDbDs::ID")?;
        self.writer.write_i32(90, 4)?;
        self.writer.write_string(2, "AcDs:Indexable")?;
        self.writer.write_byte(280, 1)?;
        self.writer.write_bool(291, true)?;

        // HandleAttribute record
        self.writer.write_string(101, "ACDSRECORD")?;
        self.writer.write_string(1, "AcDbDs::ID")?;
        self.writer.write_i32(90, 5)?;
        self.writer.write_string(2, "AcDbDs::HandleAttribute")?;
        self.writer.write_byte(280, 7)?;
        self.writer.write_i16(282, 1)?;

        Ok(())
    }

    fn write_acds_infrastructure_schemas(&mut self) -> Result<()> {
        // Schema 2: TreatedAsObjectDataSchema
        self.writer.write_string(0, "ACDSSCHEMA")?;
        self.writer.write_i32(90, 2)?;
        self.writer
            .write_string(1, "AcDbDs::TreatedAsObjectDataSchema")?;
        self.writer.write_string(2, "AcDbDs::TreatedAsObjectData")?;
        self.writer.write_byte(280, 1)?;
        self.writer.write_i32(91, 0)?;

        // Schema 3: LegacySchema
        self.writer.write_string(0, "ACDSSCHEMA")?;
        self.writer.write_i32(90, 3)?;
        self.writer.write_string(1, "AcDbDs::LegacySchema")?;
        self.writer.write_string(2, "AcDbDs::Legacy")?;
        self.writer.write_byte(280, 1)?;
        self.writer.write_i32(91, 0)?;

        // Schema 4: IndexedPropertySchema
        self.writer.write_string(0, "ACDSSCHEMA")?;
        self.writer.write_i32(90, 4)?;
        self.writer
            .write_string(1, "AcDbDs::IndexedPropertySchema")?;
        self.writer.write_string(2, "AcDs:Indexable")?;
        self.writer.write_byte(280, 1)?;
        self.writer.write_i32(91, 0)?;

        // Schema 5: HandleAttributeSchema
        self.writer.write_string(0, "ACDSSCHEMA")?;
        self.writer.write_i32(90, 5)?;
        self.writer
            .write_string(1, "AcDbDs::HandleAttributeSchema")?;
        self.writer.write_string(2, "AcDbDs::HandleAttribute")?;
        self.writer.write_byte(280, 7)?;
        self.writer.write_i32(91, 1)?;
        self.writer.write_i16(284, 1)?;

        Ok(())
    }

    fn write_acds_record(&mut self, entity_handle: Handle, sab_data: &[u8]) -> Result<()> {
        self.writer.write_string(0, "ACDSRECORD")?;

        // Schema reference (1 = AcDb3DSolid_ASM_Data)
        self.writer.write_i32(90, 1)?;

        // Entity handle reference
        self.writer.write_string(2, "AcDbDs::ID")?;
        self.writer.write_byte(280, 10)?;
        self.writer.write_handle(320, entity_handle)?;

        // ASM_Data field with SAB binary
        self.writer.write_string(2, "ASM_Data")?;
        self.writer.write_byte(280, 15)?;

        // Total byte count
        self.writer.write_i32(94, sab_data.len() as i32)?;

        // Write SAB data in 127-byte chunks as gc=310
        for chunk in sab_data.chunks(127) {
            self.writer.write_binary(310, chunk)?;
        }

        Ok(())
    }
}

/// Helper to extract invisible edge bits
fn get_invisible_edge_bits(flags: &InvisibleEdgeFlags) -> u8 {
    let mut bits = 0u8;
    if flags.is_first_invisible() {
        bits |= 1;
    }
    if flags.is_second_invisible() {
        bits |= 2;
    }
    if flags.is_third_invisible() {
        bits |= 4;
    }
    if flags.is_fourth_invisible() {
        bits |= 8;
    }
    bits
}

/// Helper to extract boundary path flag bits
fn get_boundary_path_bits(flags: &BoundaryPathFlags) -> u32 {
    flags.bits()
}

fn wrap_sat_line(line: &str) -> Vec<&str> {
    const LIMIT: usize = 240;
    let bytes = line.as_bytes();
    let mut pieces = Vec::new();
    let (mut start, mut split, mut index) = (0, None, 0);
    while index < bytes.len() {
        let token_start = index == 0 || bytes[index - 1] == b' ';
        if bytes[index] == b'@' && token_start {
            // Skip the count, the separating space and the counted text.
            let digits = bytes[index + 1..].iter().take_while(|b| b.is_ascii_digit()).count();
            let count: usize = line[index + 1..index + 1 + digits].parse().unwrap_or(0);
            index = (index + 2 + digits + count).min(bytes.len());
            continue;
        }
        if bytes[index] == b' ' {
            split = Some(index);
        }
        if index - start >= LIMIT {
            if let Some(at) = split.filter(|&at| at > start) {
                pieces.push(&line[start..at]);
                start = at;
                split = None;
            }
        }
        index += 1;
    }
    pieces.push(&line[start..]);
    pieces
}

/// The group 2 identifier of a modeler entity: its own GUID, or the zero GUID AutoCAD writes for none.
fn modeler_uid(uid: &str) -> &str {
    if uid.is_empty() {
        "{00000000-0000-0000-0000-000000000000}"
    } else {
        uid
    }
}
