//! DXF file reader

mod binary_reader;
mod section_reader;
mod stream_reader;
mod text_reader;

pub use binary_reader::DxfBinaryReader;
pub use stream_reader::DxfStreamReader;
pub use text_reader::DxfTextReader;
use stream_reader::XDataRecorder;

use section_reader::SectionReader;

use crate::document::CadDocument;
use crate::entities::solid3d::AcisVersion;
use crate::entities::EntityType;
use crate::error::{DxfError, Result};
use crate::io::read::{push_read_diagnostic, ReadDiagnostic, ReadStage, SourceFormat};
use crate::tables::TableEntry;
use crate::types::Handle;
use std::collections::HashMap;
use std::cell::{Cell, RefCell};
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceSection {
    None,
    Header,
    Tables,
    Blocks,
    Entities,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SourceRecordKind {
    None,
    Seqend,
    Attrib,
    EntitiesViewport,
}

/// Observes explicit identity values while the source stream is decoded.
///
/// Native table constructors assign handles to several R12 records that do
/// not carry one in the file.  Looking at the completed document therefore
/// cannot distinguish a source handle from a generated default.  Keep this
/// observation at the stream boundary instead, before any native defaults are
/// materialized.
struct SourceHandleTracker {
    inner: Box<dyn DxfStreamReader>,
    max: Rc<Cell<u64>>,
    entity_owners: Rc<RefCell<HashMap<u64, u64>>>,
    skip_handseed: bool,
    section: SourceSection,
    section_pending: bool,
    record_kind: SourceRecordKind,
    record_handle: Option<u64>,
    record_owner: Option<u64>,
    record_identity_seen: bool,
    reactor_depth: usize,
}

impl SourceHandleTracker {
    fn new(
        inner: Box<dyn DxfStreamReader>,
        max: Rc<Cell<u64>>,
        entity_owners: Rc<RefCell<HashMap<u64, u64>>>,
    ) -> Self {
        Self {
            inner,
            max,
            entity_owners,
            skip_handseed: false,
            section: SourceSection::None,
            section_pending: false,
            record_kind: SourceRecordKind::None,
            record_handle: None,
            record_owner: None,
            record_identity_seen: false,
            reactor_depth: 0,
        }
    }

    fn flush_record(&mut self) {
        if self.record_kind != SourceRecordKind::None {
            if let Some(handle) = self.record_handle {
                self.entity_owners
                    .borrow_mut()
                    .insert(handle, self.record_owner.unwrap_or(0));
            }
        }
    }
}

impl DxfStreamReader for SourceHandleTracker {
    fn read_pair(&mut self) -> Result<Option<stream_reader::DxfCodePair>> {
        let Some(pair) = self.inner.read_pair()? else {
            self.flush_record();
            self.record_kind = SourceRecordKind::None;
            return Ok(None);
        };

        if pair.code == 0 {
            self.flush_record();
            let value = pair.value_string.trim();
            self.record_kind = if value.eq_ignore_ascii_case("SEQEND") {
                SourceRecordKind::Seqend
            } else if value.eq_ignore_ascii_case("ATTRIB") {
                SourceRecordKind::Attrib
            } else if value.eq_ignore_ascii_case("VIEWPORT")
                && self.section == SourceSection::Entities
            {
                SourceRecordKind::EntitiesViewport
            } else {
                SourceRecordKind::None
            };
            self.record_handle = None;
            self.record_owner = None;
            self.record_identity_seen = false;
            self.reactor_depth = 0;
            if value.eq_ignore_ascii_case("SECTION") {
                self.section_pending = true;
            } else if value.eq_ignore_ascii_case("ENDSEC") {
                self.section = SourceSection::None;
                self.section_pending = false;
            } else {
                self.section_pending = false;
            }
        } else if pair.code == 2 && self.section_pending {
            let value = pair.value_string.trim();
            self.section = if value.eq_ignore_ascii_case("HEADER") {
                SourceSection::Header
            } else if value.eq_ignore_ascii_case("TABLES") {
                SourceSection::Tables
            } else if value.eq_ignore_ascii_case("BLOCKS") {
                SourceSection::Blocks
            } else if value.eq_ignore_ascii_case("ENTITIES") {
                SourceSection::Entities
            } else {
                SourceSection::Other
            };
            self.section_pending = false;
        }
        let skip = self.skip_handseed;
        self.skip_handseed = if pair.code == 9 {
            pair.value_string.trim() == "$HANDSEED"
        } else {
            false
        };
        if matches!(pair.code, 5 | 105) && !self.record_identity_seen {
            self.record_identity_seen = true;
            if !skip {
                if let Ok(handle) = u64::from_str_radix(pair.value_string.trim(), 16) {
                    self.max.set(self.max.get().max(handle));
                    if pair.code == 5
                        && self.record_kind != SourceRecordKind::None
                        && self.reactor_depth == 0
                    {
                        self.record_handle = Some(handle);
                    }
                }
            }
        }
        if pair.code == 102 {
            let marker = pair.value_string.trim();
            if marker.starts_with('{') {
                self.reactor_depth = self.reactor_depth.saturating_add(1);
            } else if marker == "}" {
                self.reactor_depth = self.reactor_depth.saturating_sub(1);
            }
        } else if pair.code == 330
            && self.record_kind != SourceRecordKind::None
            && self.reactor_depth == 0
            && self.record_owner.is_none()
        {
            self.record_owner = pair
                .as_handle()
                .or_else(|| u64::from_str_radix(pair.value_string.trim(), 16).ok());
        }
        Ok(Some(pair))
    }

    fn peek_code(&mut self) -> Result<Option<i32>> {
        self.inner.peek_code()
    }

    fn push_back(&mut self, pair: stream_reader::DxfCodePair) {
        self.inner.push_back(pair);
    }

    fn reset(&mut self) -> Result<()> {
        self.entity_owners.borrow_mut().clear();
        self.max.set(0);
        self.skip_handseed = false;
        self.section = SourceSection::None;
        self.section_pending = false;
        self.record_kind = SourceRecordKind::None;
        self.record_handle = None;
        self.record_owner = None;
        self.record_identity_seen = false;
        self.reactor_depth = 0;
        self.inner.reset()
    }

    fn set_encoding(&mut self, encoding: crate::io::dxf::code_page::LegacyEncoding) {
        self.inner.set_encoding(encoding);
    }

    fn diagnostic_context(&self) -> stream_reader::DxfStreamContext {
        self.inner.diagnostic_context()
    }

    fn record_xdata(&mut self, on: bool) {
        self.inner.record_xdata(on);
    }

    fn take_recorded_xdata(
        &mut self,
    ) -> (Option<String>, Vec<stream_reader::DxfCodePair>) {
        self.inner.take_recorded_xdata()
    }

    fn take_recorded_owner(&mut self) -> Option<Handle> {
        self.inner.take_recorded_owner()
    }
}

/// Configuration for the DXF reader.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DxfReaderConfiguration {
    /// When `true`, parse errors within individual entities/objects/sections
    /// are caught and reported as notifications instead of aborting the read.
    ///
    /// Default: `false` (strict mode — errors propagate).
    pub failsafe: bool,

    /// Default encoding to use for non-UTF8 strings if the DXF file does not
    /// specify it via $DWGCODEPAGE.
    ///
    /// Only applies to DXF versions prior to AC1021 (AutoCAD 2007).
    pub default_encoding: Option<String>,
}

impl Default for DxfReaderConfiguration {
    fn default() -> Self {
        Self {
            failsafe: false,
            default_encoding: None,
        }
    }
}

/// DXF file reader
pub struct DxfReader {
    reader: Box<dyn DxfStreamReader>,
    source_handle_max: Rc<Cell<u64>>,
    source_entity_owners: Rc<RefCell<HashMap<u64, u64>>>,
    config: DxfReaderConfiguration,
    /// Estimated entity count based on stream size (used for pre-allocation).
    estimated_entities: usize,
    source_path: Option<String>,
}

impl DxfReader {
    /// Create a new DXF reader from any reader
    pub fn from_reader<R: Read + Seek + 'static>(reader: R) -> Result<Self> {
        let mut buf_reader = BufReader::with_capacity(64 * 1024, reader);

        // Estimate entity count from stream size (~300 bytes per entity on average)
        let stream_size = buf_reader.seek(std::io::SeekFrom::End(0)).unwrap_or(0);
        buf_reader.seek(std::io::SeekFrom::Start(0))?;
        let estimated_entities = (stream_size as usize / 300).max(16);

        // Detect if binary
        let is_binary = Self::is_binary(&mut buf_reader)?;

        // Create appropriate reader
        let reader: Box<dyn DxfStreamReader> = if is_binary {
            Box::new(XDataRecorder::new(Box::new(DxfBinaryReader::new(buf_reader)?)))
        } else {
            // Seek back to start for text DXF files
            buf_reader.seek(std::io::SeekFrom::Start(0))?;
            Box::new(XDataRecorder::new(Box::new(DxfTextReader::new(buf_reader)?)))
        };
        let source_handle_max = Rc::new(Cell::new(0));
        let source_entity_owners = Rc::new(RefCell::new(HashMap::new()));
        let reader = Box::new(SourceHandleTracker::new(
            reader,
            source_handle_max.clone(),
            source_entity_owners.clone(),
        ));

        Ok(Self {
            reader,
            source_handle_max,
            source_entity_owners,
            config: DxfReaderConfiguration::default(),
            estimated_entities,
            source_path: None,
        })
    }

    /// Create a new DXF reader from a file path
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let source_path = Some(path.as_ref().to_string_lossy().into_owned());
        let file = File::open(path)?;
        let mut buf_reader = BufReader::with_capacity(64 * 1024, file);

        // Estimate entity count from stream size (~300 bytes per entity on average)
        let stream_size = buf_reader.seek(std::io::SeekFrom::End(0)).unwrap_or(0);
        buf_reader.seek(std::io::SeekFrom::Start(0))?;
        let estimated_entities = (stream_size as usize / 300).max(16);

        // Detect if binary
        let is_binary = Self::is_binary(&mut buf_reader)?;

        // Create appropriate reader
        let reader: Box<dyn DxfStreamReader> = if is_binary {
            Box::new(XDataRecorder::new(Box::new(DxfBinaryReader::new(buf_reader)?)))
        } else {
            // Seek back to start for text DXF files
            buf_reader.seek(std::io::SeekFrom::Start(0))?;
            Box::new(XDataRecorder::new(Box::new(DxfTextReader::new(buf_reader)?)))
        };
        let source_handle_max = Rc::new(Cell::new(0));
        let source_entity_owners = Rc::new(RefCell::new(HashMap::new()));
        let reader = Box::new(SourceHandleTracker::new(
            reader,
            source_handle_max.clone(),
            source_entity_owners.clone(),
        ));

        Ok(Self {
            reader,
            source_handle_max,
            source_entity_owners,
            config: DxfReaderConfiguration::default(),
            estimated_entities,
            source_path,
        })
    }

    /// Check if a stream contains binary DXF data
    fn is_binary<R: Read + Seek>(reader: &mut R) -> Result<bool> {
        const SENTINEL: &[u8] = b"AutoCAD Binary DXF";
        let mut buffer = vec![0u8; SENTINEL.len()];

        // Try to read the sentinel bytes
        let bytes_read = reader.read(&mut buffer)?;

        // Always seek back to start after checking
        reader.seek(std::io::SeekFrom::Start(0))?;

        // If file is too small or doesn't match, it's not binary
        if bytes_read < SENTINEL.len() {
            return Ok(false);
        }

        Ok(buffer == SENTINEL)
    }

    /// Set the reader configuration.
    pub fn with_configuration(mut self, config: DxfReaderConfiguration) -> Self {
        self.config = config;
        self
    }

    fn read_diagnostic(
        &self,
        code: impl Into<String>,
        stage: ReadStage,
        message: impl Into<String>,
    ) -> ReadDiagnostic {
        let context = self.reader.diagnostic_context();
        let mut diagnostic = ReadDiagnostic::new(code, stage, message);
        diagnostic.source_offset = context.source_offset;
        diagnostic.source_offset_basis = Some("file-byte".to_string());
        diagnostic.source_line = context.source_line;
        diagnostic.record_handle = context.record_handle;
        diagnostic.record_type = context.record_type;
        diagnostic
    }

    /// Read a DXF file and return a CadDocument
    pub fn read(self) -> Result<CadDocument> {
        self.read_with_stats().map(|outcome| outcome.document)
    }

    /// Recover as much usable drawing data as possible and apply conservative
    /// document repairs. Recovery always enables failsafe parsing.
    pub fn recover(mut self) -> Result<crate::io::recover::RecoverOutcome> {
        self.config.failsafe = true;
        crate::io::recover::RecoverOutcome::from_read_outcome(self.read_with_stats()?)
    }

    /// Read a DXF file and return the document with source/decode statistics.
    pub fn read_with_stats(mut self) -> Result<crate::io::read::ReadOutcome> {
        // Set default encoding if provided
        if let Some(ref encoding_name) = self.config.default_encoding {
            if let Some(enc) = crate::io::dxf::code_page::encoding_from_code_page(encoding_name) {
                self.reader.set_encoding(enc);
            }
        }

        // Create document with pre-allocated entity storage
        let mut document = CadDocument::new();
        // Mark this as reader provenance even when the source omits APPID;
        // constructed documents leave these fields unset.
        document.source_app_id_order = Some(Vec::new());
        document.legacy_dxf_tables = Some(Vec::new());
        document.legacy_dxf_blocks = Some(Vec::new());
        document.source_explicit_handle_max = None;
        document.classes.begin_source();
        document.entities.reserve(self.estimated_entities);
        document.entity_index.reserve(self.estimated_entities);

        // Snapshot the handles initialize_defaults() handed to its well-known
        // table entries. The file may reuse one of those handles for its own
        // records while lacking the default entry itself; re-handling that
        // case after parsing prevents duplicate handles in written output
        // (issue #51 comment by Apicqq).
        let default_entry_handles = snapshot_default_entry_handles(&document);

        // Read all sections
        let failsafe = self.config.failsafe;
        let mut source_sections = 0usize;
        let mut source_records = 0usize;
        let mut decoded_source_records = 0usize;
        let mut skipped_source_records = 0usize;
        let mut record_stream_read = false;
        let mut stream_completed = false;
        let mut diagnostics = Vec::new();

        loop {
            let pair = match self.reader.read_pair() {
                Ok(Some(pair)) => pair,
                Ok(None) if failsafe => {
                    let message = "Drawing stream ended before the EOF marker".to_string();
                    document.notifications.notify(
                        crate::notification::NotificationType::Error,
                        message.clone(),
                    );
                    push_read_diagnostic(
                        &mut diagnostics,
                        self.read_diagnostic(
                            "stream-ended-early",
                            ReadStage::RecordStream,
                            message,
                        ),
                    );
                    break;
                }
                Ok(None) => {
                    return Err(DxfError::Parse(
                        "drawing stream ended before the EOF marker".to_string(),
                    ));
                }
                Err(error) if failsafe => {
                    let message = format!("Error reading drawing stream: {}", error);
                    document.notifications.notify(
                        crate::notification::NotificationType::Error,
                        message.clone(),
                    );
                    push_read_diagnostic(
                        &mut diagnostics,
                        self.read_diagnostic(
                            "stream-read-failed",
                            ReadStage::RecordStream,
                            message,
                        ),
                    );
                    break;
                }
                Err(error) => return Err(error),
            };
            if pair.code == 0 && pair.value_string == "SECTION" {
                // Read section name
                let section_pair = match self.reader.read_pair() {
                    Ok(Some(section_pair)) => Some(section_pair),
                    Ok(None) if failsafe => {
                        let message = "Drawing stream ended before the section name".to_string();
                        document.notifications.notify(
                            crate::notification::NotificationType::Error,
                            message.clone(),
                        );
                        push_read_diagnostic(
                            &mut diagnostics,
                            self.read_diagnostic(
                                "section-name-missing",
                                ReadStage::Section,
                                message,
                            ),
                        );
                        break;
                    }
                    Ok(None) => {
                        return Err(DxfError::Parse(
                            "drawing stream ended before the section name".to_string(),
                        ));
                    }
                    Err(error) if failsafe => {
                        let message = format!("Error reading section name: {}", error);
                        document.notifications.notify(
                            crate::notification::NotificationType::Error,
                            message.clone(),
                        );
                        push_read_diagnostic(
                            &mut diagnostics,
                            self.read_diagnostic(
                                "section-name-read-failed",
                                ReadStage::Section,
                                message,
                            ),
                        );
                        break;
                    }
                    Err(error) => return Err(error),
                };
                if let Some(section_pair) = section_pair {
                    if section_pair.code == 2 {
                        let section_name = section_pair.value_string.clone();
                        let carries_records = matches!(
                            section_name.as_str(),
                            "TABLES" | "BLOCKS" | "ENTITIES" | "OBJECTS" | "ACDSDATA"
                        );
                        let decoded_before = decoded_source_records;
                        let result = match section_name.as_str() {
                            "HEADER" => self.read_header_section(&mut document),
                            "CLASSES" => self.read_classes_section(&mut document),
                            "TABLES" => self.read_tables_section(
                                &mut document,
                                &mut decoded_source_records,
                                &mut skipped_source_records,
                                &mut diagnostics,
                            ),
                            "BLOCKS" => self.read_blocks_section(
                                &mut document,
                                &mut decoded_source_records,
                                &mut skipped_source_records,
                                &mut diagnostics,
                            ),
                            "ENTITIES" => self.read_entities_section(
                                &mut document,
                                &mut decoded_source_records,
                                &mut skipped_source_records,
                                &mut diagnostics,
                            ),
                            "OBJECTS" => self.read_objects_section(
                                &mut document,
                                &mut decoded_source_records,
                                &mut skipped_source_records,
                                &mut diagnostics,
                            ),
                            "ACDSDATA" => self.read_acdsdata_section(&mut document),
                            "THUMBNAILIMAGE" => self.read_thumbnail_section(&mut document),
                            _ => {
                                // Skip unknown section
                                self.skip_section()
                            }
                        };

                        if carries_records {
                            source_sections = source_sections.saturating_add(1);
                            record_stream_read = true;
                            let decoded = decoded_source_records.saturating_sub(decoded_before);
                            source_records = source_records.saturating_add(decoded);
                        }

                        // In failsafe mode, catch errors and continue
                        if let Err(e) = result {
                            if failsafe {
                                let message =
                                    format!("Error reading {} section: {}", section_name, e);
                                document.notifications.notify(
                                    crate::notification::NotificationType::Error,
                                    message.clone(),
                                );
                                let mut diagnostic = self.read_diagnostic(
                                    "section-read-failed",
                                    ReadStage::Section,
                                    message,
                                );
                                diagnostic.section = Some(section_name.clone());
                                push_read_diagnostic(&mut diagnostics, diagnostic);
                                skipped_source_records = skipped_source_records.saturating_add(1);
                                // Try to skip to the end of the section
                                let _ = self.skip_section();
                            } else {
                                return Err(e);
                            }
                        }
                    }
                }
            } else if pair.code == 0 && pair.value_string == "EOF" {
                stream_completed = true;
                break;
            }
        }

        // Post-read resolution: advance the allocator past every file-sourced
        // identity before re-handling surviving defaults, then assign owners
        // and repair cross-references.
        document.source_explicit_handle_max = (self.source_handle_max.get() != 0)
            .then_some(self.source_handle_max.get());
        document.source_entity_owners = Some(
            self.source_entity_owners
                .borrow()
                .iter()
                .map(|(&handle, &owner)| (Handle::new(handle), Handle::new(owner)))
                .collect(),
        );
        if document.version != crate::types::DxfVersion::AC1009 {
            // These structures exist only to replay the R12 source template;
            // modern readers already expose complete native table metadata.
            document.legacy_dxf_tables = None;
            document.legacy_dxf_blocks = None;
            document.source_entity_owners = None;
        }
        document.synchronize_handle_allocator();
        rehandle_colliding_default_entries(&mut document, &default_entry_handles);
        document.resolve_references();
        // Must follow `resolve_references`: handle-less legacy table records
        // (R12 STYLE entries) only receive their handles there, so running this
        // earlier would resolve every reference to a null handle.
        rewire_stale_text_style_references(&mut document);
        drop_decoded_typefaces(&mut document);
        name_viewport_override_layers(&mut document);
        name_dynamic_block_classes(&mut document);
        normalize_by_block_material(&mut document);
        // The current annotation scale is the CANNOSCALE entry of the variable dictionary, in a DXF as in a DWG.
        crate::io::dwg::dwg_document_builder::DwgDocumentBuilder::reflect_annotation_scale(&mut document);
        resolve_table_style_text_styles(&mut document);
        rewire_dangling_layout_block_records(&mut document);
        document.ensure_model_layout();
        if document.version == crate::types::DxfVersion::AC1009 {
            let layers: HashMap<_, _> = document.layers.iter().map(|layer| (layer.name.to_uppercase(), layer.handle)).collect();
            for entity in document.entities_mut_keep_raw() {
                if let EntityType::Viewport(viewport) = entity {
                    crate::io::dwg::legacy_viewport::restore_r12(viewport, |name| layers.get(&name.to_uppercase()).copied());
                }
            }
        } else if document.version <= crate::types::DxfVersion::AC1014 {
            // The view parameters of an R13/R14 viewport are in its MVIEW extended data, as in the DWG.
            let layers: HashMap<_, _> = document.layers.iter().map(|layer| (layer.name.to_uppercase(), layer.handle)).collect();
            for entity in document.entities_mut_keep_raw() {
                match entity {
                    EntityType::Viewport(viewport) => {
                        crate::io::dwg::legacy_viewport::restore(viewport, |name| layers.get(&name.to_uppercase()).copied());
                    }
                    // The by-block colour of an R13/R14 leader is group 77, as its DWG record states it.
                    EntityType::Leader(leader) => {
                        leader.byblock_color = leader.override_color.index().map_or(0, |index| index as i16);
                    }
                    _ => {}
                }
            }
        }

        // Pre-R2004 (R2000/R14) down-saved gradient hatches keep their gradient
        // in the ACAD round-trip metadata (GradientColor1/2ACI EED + an
        // ACAD_XREC_ROUNDTRIP XRecord) rather than a native gradient block, so
        // they read back as flat solid fills. Rebuild them — gated to pre-R2004
        // so a native R2004+ gradient (read directly) always wins over any
        // stale round-trip EED left by an earlier edit.
        if document.version < crate::types::DxfVersion::AC1018 {
            crate::io::dwg::dwg_reader::recover_roundtrip_gradients(&mut document);
            // Pre-R2004 also stores an MTEXT background fill as round-trip EED
            // rather than the native codes; rebuild it (dimension text fills).
            crate::io::dwg::dwg_reader::recover_mtext_bg_roundtrip(&mut document);
        }

        // The legacy viewport-entity table (VX_CONTROL) exists in R13-R2000 files only. A DWG of a later
        // version has no such control object (its header handle is null); the document default would
        // reserve a handle for a table the file does not have.
        if document.version >= crate::types::DxfVersion::AC1018 && document.vx_table.is_empty() {
            document.vx_table.set_handle(Handle::NULL);
        }

        source_records = source_records.saturating_add(skipped_source_records);
        let stats = crate::io::read::ReadStats::from_document(
            &document,
            SourceFormat::Dxf,
            source_sections,
            source_records,
            decoded_source_records,
            skipped_source_records,
            record_stream_read,
            failsafe,
            stream_completed,
            diagnostics,
        );
        document.source_path = self.source_path.take();
        crate::objects::restore_visual_style_roundtrip(&mut document);
        crate::objects::restore_table_style_roundtrip(&mut document);
        crate::entities::table::restore_table_roundtrip(&mut document);
        Ok(crate::io::read::ReadOutcome::new(document, stats))
    }

    /// Read the HEADER section
    fn read_header_section(&mut self, document: &mut CadDocument) -> Result<()> {
        let mut section_reader = SectionReader::new(&mut self.reader);
        section_reader.read_header(document)
    }

    /// Read the CLASSES section
    fn read_classes_section(&mut self, document: &mut CadDocument) -> Result<()> {
        let mut section_reader = SectionReader::new(&mut self.reader);
        section_reader.read_classes(document)
    }

    /// Read the TABLES section
    fn read_tables_section(
        &mut self,
        document: &mut CadDocument,
        decoded_records: &mut usize,
        skipped_records: &mut usize,
        diagnostics: &mut Vec<ReadDiagnostic>,
    ) -> Result<()> {
        let mut section_reader =
            SectionReader::new(&mut self.reader).with_failsafe(self.config.failsafe);
        let result = section_reader.read_tables(document);
        *decoded_records = decoded_records.saturating_add(section_reader.decoded_records());
        Self::collect_section_recovery(
            &mut section_reader,
            document,
            "TABLES",
            skipped_records,
            diagnostics,
        );
        result
    }

    /// Read the BLOCKS section
    fn read_blocks_section(
        &mut self,
        document: &mut CadDocument,
        decoded_records: &mut usize,
        skipped_records: &mut usize,
        diagnostics: &mut Vec<ReadDiagnostic>,
    ) -> Result<()> {
        let mut section_reader =
            SectionReader::new(&mut self.reader).with_failsafe(self.config.failsafe);
        let result = section_reader.read_blocks(document);
        *decoded_records = decoded_records.saturating_add(section_reader.decoded_records());
        Self::collect_section_recovery(
            &mut section_reader,
            document,
            "BLOCKS",
            skipped_records,
            diagnostics,
        );
        result
    }

    /// Read the ENTITIES section
    fn read_entities_section(
        &mut self,
        document: &mut CadDocument,
        decoded_records: &mut usize,
        skipped_records: &mut usize,
        diagnostics: &mut Vec<ReadDiagnostic>,
    ) -> Result<()> {
        let mut section_reader =
            SectionReader::new(&mut self.reader).with_failsafe(self.config.failsafe);
        let result = section_reader.read_entities(document);
        *decoded_records = decoded_records.saturating_add(section_reader.decoded_records());
        Self::collect_section_recovery(
            &mut section_reader,
            document,
            "ENTITIES",
            skipped_records,
            diagnostics,
        );
        result
    }

    /// Read the OBJECTS section
    fn read_objects_section(
        &mut self,
        document: &mut CadDocument,
        decoded_records: &mut usize,
        skipped_records: &mut usize,
        diagnostics: &mut Vec<ReadDiagnostic>,
    ) -> Result<()> {
        let mut section_reader =
            SectionReader::new(&mut self.reader).with_failsafe(self.config.failsafe);
        let result = section_reader.read_objects(document);
        *decoded_records = decoded_records.saturating_add(section_reader.decoded_records());
        Self::collect_section_recovery(
            &mut section_reader,
            document,
            "OBJECTS",
            skipped_records,
            diagnostics,
        );
        result
    }

    fn collect_section_recovery(
        section_reader: &mut SectionReader<'_>,
        document: &mut CadDocument,
        section: &str,
        skipped_records: &mut usize,
        diagnostics: &mut Vec<ReadDiagnostic>,
    ) {
        *skipped_records = skipped_records.saturating_add(section_reader.skipped_records());
        for recovery in section_reader.take_recovery_errors() {
            let message = format!("Error reading {} record: {}", section, recovery.message);
            document.notifications.notify(
                crate::notification::NotificationType::Error,
                message.clone(),
            );
            let mut diagnostic = ReadDiagnostic::new(
                "record-read-failed",
                ReadStage::RecordStream,
                message,
            );
            diagnostic.section = Some(section.to_string());
            diagnostic.source_offset = recovery.context.source_offset;
            diagnostic.source_offset_basis = Some("file-byte".to_string());
            diagnostic.source_line = recovery.context.source_line;
            diagnostic.record_handle = recovery.context.record_handle;
            diagnostic.record_type = recovery.context.record_type;
            push_read_diagnostic(diagnostics, diagnostic);
        }
    }

    /// Read the ACDSDATA section (the AcDb data store).
    ///
    /// From R2013 (AC1027) on, a 3D solid / region / body / surface no longer
    /// carries its ACIS geometry inline in the entity (the `AcDbModelerGeometry`
    /// block is empty); the binary SAB blob lives here instead, in an
    /// `ACDSRECORD` whose `ASM_Data` property is bound to the owning entity by a
    /// 320 soft-pointer handle. The DWG reader gets this from the merged AcDs
    /// stream; the DXF reader used to skip the whole section, leaving every
    /// modeler entity geometry-less. Parse the records, then attach each SAB
    /// blob to its entity so the same downstream SAB → mesh path runs.
    fn read_acdsdata_section(&mut self, document: &mut CadDocument) -> Result<()> {
        // (entity handle, SAB bytes) pairs collected from ASM_Data records.
        let mut blobs: Vec<(u64, Vec<u8>)> = Vec::new();

        // Per-record accumulator, flushed on each record/schema boundary (0-code).
        let mut cur_handle: Option<u64> = None;
        let mut is_asm = false;
        let mut chunks: Vec<u8> = Vec::new();

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                // Record boundary — flush a complete ASM_Data record.
                if is_asm && !chunks.is_empty() {
                    if let Some(h) = cur_handle {
                        blobs.push((h, std::mem::take(&mut chunks)));
                    }
                }
                cur_handle = None;
                is_asm = false;
                chunks.clear();
                if pair.value_string == "ENDSEC" {
                    break;
                }
                continue;
            }
            match pair.code {
                // AcDbDs::ID soft-pointer to the owning entity.
                320 => {
                    if let Ok(h) = u64::from_str_radix(pair.value_string.trim(), 16) {
                        cur_handle = Some(h);
                    }
                }
                // Property name — the binary payload that follows is ACIS only
                // for the ASM_Data property (Thumbnail_Data etc. are skipped).
                2 => is_asm = pair.value_string == "ASM_Data",
                // Binary chunk (hex-encoded); only kept once inside ASM_Data.
                310 if is_asm => {
                    let hex = pair.value_string.trim().as_bytes();
                    let mut i = 0;
                    while i + 1 < hex.len() {
                        let hi = (hex[i] as char).to_digit(16);
                        let lo = (hex[i + 1] as char).to_digit(16);
                        if let (Some(hi), Some(lo)) = (hi, lo) {
                            chunks.push((hi * 16 + lo) as u8);
                        }
                        i += 2;
                    }
                }
                _ => {}
            }
        }

        // Attach each SAB blob to its modeler entity.
        for (handle, sab) in blobs {
            let Some(entity) = document.get_entity_mut(Handle::new(handle)) else {
                continue;
            };
            match entity {
                EntityType::Solid3D(e) => {
                    e.common.has_ds_data = true;
                    e.acis_data.sab_data = sab;
                    e.acis_data.is_binary = true;
                    e.acis_data.version = AcisVersion::Version2;
                    e.point_of_reference = e
                        .acis_data
                        .geometry_centre()
                        .or_else(|| e.acis_data.placement_origin())
                        .unwrap_or(e.point_of_reference);
                }
                EntityType::Region(e) => {
                    e.common.has_ds_data = true;
                    e.acis_data.sab_data = sab;
                    e.acis_data.is_binary = true;
                    e.acis_data.version = AcisVersion::Version2;
                    e.point_of_reference = e
                        .acis_data
                        .geometry_centre()
                        .or_else(|| e.acis_data.placement_origin())
                        .unwrap_or(e.point_of_reference);
                }
                EntityType::Body(e) => {
                    e.common.has_ds_data = true;
                    e.acis_data.sab_data = sab;
                    e.acis_data.is_binary = true;
                    e.acis_data.version = AcisVersion::Version2;
                    e.point_of_reference = e
                        .acis_data
                        .geometry_centre()
                        .or_else(|| e.acis_data.placement_origin())
                        .unwrap_or(e.point_of_reference);
                }
                EntityType::Surface(e) => {
                    e.common.has_ds_data = true;
                    e.acis_data.sab_data = sab;
                    e.acis_data.is_binary = true;
                    e.acis_data.version = AcisVersion::Version2;
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Read the THUMBNAILIMAGE section: group 90 holds the image size in bytes, the image follows in
    /// 310 chunks. AutoCAD writes the Windows DIB (a BITMAPINFOHEADER, palette and pixels, without the
    /// 14-byte BITMAPFILEHEADER) of the drawing's preview; the format is told from the bytes, so a PNG
    /// or a metafile (as a DWG can carry) is kept as that.
    fn read_thumbnail_section(&mut self, document: &mut CadDocument) -> Result<()> {
        use crate::document::{Preview, PreviewFormat};
        let mut size: Option<usize> = None;
        let mut data: Vec<u8> = Vec::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                if pair.value_string == "ENDSEC" {
                    break;
                }
                continue;
            }
            match pair.code {
                90 => size = pair.as_int().and_then(|value| usize::try_from(value).ok()),
                310 => {
                    let hex = pair.value_string.trim().as_bytes();
                    for digits in hex.chunks_exact(2) {
                        if let (Some(high), Some(low)) = (
                            (digits[0] as char).to_digit(16),
                            (digits[1] as char).to_digit(16),
                        ) {
                            data.push((high * 16 + low) as u8);
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(size) = size {
            data.truncate(size);
        }
        let header_size = |bytes: &[u8]| {
            bytes
                .get(..4)
                .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
        };
        let (format, skip) = if data.starts_with(&[0x89, b'P', b'N', b'G']) {
            (Some(PreviewFormat::Png), 0)
        } else if data.starts_with(&[0xD7, 0xCD, 0xC6, 0x9A])
            || data.starts_with(&[0x01, 0x00, 0x09, 0x00])
            || data.starts_with(&[0x02, 0x00, 0x09, 0x00])
        {
            (Some(PreviewFormat::Wmf), 0)
        } else if data.starts_with(b"BM") && data.len() > 14 {
            // A BITMAPFILEHEADER in front of the DIB: the model keeps the DIB.
            (Some(PreviewFormat::Bmp), 14)
        } else if matches!(header_size(&data), Some(12 | 40 | 52 | 56 | 64 | 108 | 124)) {
            (Some(PreviewFormat::Bmp), 0)
        } else {
            (None, 0)
        };
        match format {
            Some(format) if data.len() > skip => {
                document.preview = Some(Preview {
                    format,
                    data: data.split_off(skip),
                });
            }
            _ if !data.is_empty() => document.notifications.notify(
                crate::notification::NotificationType::Warning,
                "THUMBNAILIMAGE section skipped: image format not recognised",
            ),
            _ => {}
        }
        Ok(())
    }

    /// Skip the current section
    fn skip_section(&mut self) -> Result<()> {
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 && pair.value_string == "ENDSEC" {
                break;
            }
        }
        Ok(())
    }
}

/// Snapshot the handles `initialize_defaults()` handed to its well-known
/// table entries: (table, entry name) → handle.
fn snapshot_default_entry_handles(document: &CadDocument) -> HashMap<(&'static str, String), u64> {
    let mut map: HashMap<(&'static str, String), u64> = HashMap::new();
    macro_rules! snapshot {
        ($tag:literal, $table:expr) => {
            for entry in $table.iter() {
                let handle = entry.handle().value();
                if handle != 0 {
                    map.insert(($tag, entry.name().to_string()), handle);
                }
            }
        };
    }
    snapshot!("layers", document.layers);
    snapshot!("line_types", document.line_types);
    snapshot!("text_styles", document.text_styles);
    snapshot!("dim_styles", document.dim_styles);
    snapshot!("app_ids", document.app_ids);
    snapshot!("views", document.views);
    snapshot!("vports", document.vports);
    snapshot!("ucss", document.ucss);
    snapshot!("vx_table", document.vx_table);
    snapshot!("block_records", document.block_records);
    map
}

/// Re-handle surviving `initialize_defaults()` entries whose handles the
/// file reused for its own records.
///
/// `CadDocument::new()` creates well-known entries (Standard text style and
/// dimstyle, ByLayer/ByBlock linetypes, ...) before the file is parsed. When
/// the file lacks such an entry but uses its handle for one of its own
/// records, the writer would emit two records with the same handle — a hard
/// integrity error CAD applications reject. Nothing inside the file
/// references a default the file does not contain, so the surviving default
/// is moved to a fresh handle and header references still pointing at it
/// follow (issue #51 comment by Apicqq).
fn rehandle_colliding_default_entries(
    document: &mut CadDocument,
    defaults: &HashMap<(&'static str, String), u64>,
) {
    if defaults.is_empty() {
        return;
    }

    // Count how often every handle occurs across everything that is
    // written: table entries, block-record placeholder handles, entities
    // and objects.
    let mut usage: HashMap<u64, usize> = HashMap::new();
    macro_rules! count {
        ($handle:expr) => {{
            let handle: u64 = $handle;
            if handle != 0 {
                *usage.entry(handle).or_insert(0) += 1;
            }
        }};
    }
    macro_rules! count_table {
        ($table:expr) => {
            for entry in $table.iter() {
                count!(entry.handle().value());
            }
        };
    }
    count_table!(document.layers);
    count_table!(document.line_types);
    count_table!(document.text_styles);
    count_table!(document.dim_styles);
    count_table!(document.app_ids);
    count_table!(document.views);
    count_table!(document.vports);
    count_table!(document.ucss);
    count_table!(document.vx_table);
    count_table!(document.block_records);
    for br in document.block_records.iter() {
        count!(br.block_entity_handle.value());
        count!(br.block_end_handle.value());
    }
    for entity in document.entities() {
        count!(entity.common().handle.value());
    }
    for handle in document.objects.keys() {
        count!(handle.value());
    }

    let collides = |tag: &'static str, name: &str, handle: u64| -> bool {
        handle != 0
            && defaults.get(&(tag, name.to_string())) == Some(&handle)
            && usage.get(&handle).copied().unwrap_or(0) > 1
    };

    // Collect (tag, name, old handle) candidates first, then apply, in a
    // deterministic table order.
    let mut moves: Vec<(&'static str, String, u64)> = Vec::new();
    macro_rules! collect {
        ($tag:literal, $table:expr) => {
            for entry in $table.iter() {
                if collides($tag, entry.name(), entry.handle().value()) {
                    moves.push(($tag, entry.name().to_string(), entry.handle().value()));
                }
            }
        };
    }
    collect!("layers", document.layers);
    collect!("line_types", document.line_types);
    collect!("text_styles", document.text_styles);
    collect!("dim_styles", document.dim_styles);
    collect!("app_ids", document.app_ids);
    collect!("views", document.views);
    collect!("vports", document.vports);
    collect!("ucss", document.ucss);
    collect!("vx_table", document.vx_table);
    collect!("block_records", document.block_records);

    for (tag, name, old) in moves {
        let new = document.allocate_handle();
        macro_rules! apply {
            ($t:literal, $table:expr) => {
                if tag == $t {
                    if let Some(entry) = $table.get_mut(&name) {
                        entry.set_handle(new);
                    }
                }
            };
        }
        apply!("layers", document.layers);
        apply!("line_types", document.line_types);
        apply!("text_styles", document.text_styles);
        apply!("dim_styles", document.dim_styles);
        apply!("app_ids", document.app_ids);
        apply!("views", document.views);
        apply!("vports", document.vports);
        apply!("ucss", document.ucss);
        apply!("vx_table", document.vx_table);
        apply!("block_records", document.block_records);

        // Header references that still point at the moved default follow,
        // but only those selecting an entry of the moved one's table: a
        // default APPID sharing the file's `*Model_Space` handle must not
        // drag the model-space header reference along (#64).
        let header = &mut document.header;
        macro_rules! follow {
            ($field:ident, $t:literal) => {
                if tag == $t && header.$field.value() == old {
                    header.$field = new;
                }
            };
        }
        follow!(current_layer_handle, "layers");
        follow!(continuous_linetype_handle, "line_types");
        follow!(bylayer_linetype_handle, "line_types");
        follow!(current_linetype_handle, "line_types");
        follow!(byblock_linetype_handle, "line_types");
        follow!(current_text_style_handle, "text_styles");
        follow!(dim_text_style_handle, "text_styles");
        follow!(current_dimstyle_handle, "dim_styles");
        follow!(model_space_block_handle, "block_records");
        follow!(paper_space_block_handle, "block_records");
        follow!(dim_linetype_handle, "line_types");
        follow!(dim_linetype1_handle, "line_types");
        follow!(dim_linetype2_handle, "line_types");

        // Default entries cross-referencing the moved one (the Standard
        // dimstyle points at the Standard text style).
        if tag == "text_styles" {
            for ds in document.dim_styles.iter_mut() {
                if ds.dimtxsty_handle.value() == old {
                    ds.dimtxsty_handle = new;
                }
            }
        }
    }
}

fn rewire_dangling_layout_block_records(document: &mut CadDocument) {
    use crate::objects::ObjectType;

    let records: HashMap<u64, String> = document
        .block_records
        .iter()
        .map(|record| (record.handle.value(), record.name.to_ascii_lowercase()))
        .collect();
    let mut dangling = Vec::new();
    let mut owned = std::collections::HashSet::new();
    for (handle, object) in &document.objects {
        if let ObjectType::Layout(layout) = object {
            if records.contains_key(&layout.block_record.value()) {
                owned.insert(layout.block_record.value());
            } else {
                dangling.push((layout.tab_order, *handle, layout.name.eq_ignore_ascii_case("Model")));
            }
        }
    }
    if dangling.is_empty() {
        return;
    }
    dangling.sort();

    // `*Paper_Space` first, then `*Paper_Space0`, `*Paper_Space1`, ...
    let mut free_paper: Vec<(Option<u64>, u64)> = records
        .iter()
        .filter(|(handle, _)| !owned.contains(*handle))
        .filter_map(|(handle, name)| {
            let suffix = name.strip_prefix("*paper_space")?;
            let order = if suffix.is_empty() { None } else { Some(suffix.parse().ok()?) };
            Some((order, *handle))
        })
        .collect();
    free_paper.sort();
    let mut free_paper = free_paper.into_iter().map(|(_, handle)| handle);
    let model = document.header.model_space_block_handle.value();

    for (_, layout_handle, is_model) in dangling {
        let target = if is_model && records.contains_key(&model) && !owned.contains(&model) {
            model
        } else if is_model {
            continue;
        } else {
            let Some(handle) = free_paper.next() else {
                continue;
            };
            handle
        };
        owned.insert(target);
        if let Some(ObjectType::Layout(layout)) = document.objects.get_mut(&layout_handle) {
            layout.block_record = Handle::new(target);
        }
        if let Some(record) = document
            .block_records
            .iter_mut()
            .find(|record| record.handle.value() == target)
        {
            record.layout = layout_handle;
        }
    }
}

/// An entity whose group 347 names the built-in `ByBlock` material has the material flag "ByBlock" (what a DWG
/// stores in two bits), not a handle of its own: the DXF writer states the flag as that handle, so the same
/// entity reads back the same from DXF and from DWG.
fn normalize_by_block_material(document: &mut CadDocument) {
    if document.version < crate::types::DxfVersion::AC1021 {
        return;
    }
    let Some(by_block) = document.objects.iter().find_map(|(handle, object)| match object {
        crate::objects::ObjectType::Material(material) if material.name.eq_ignore_ascii_case("ByBlock") => Some(*handle),
        _ => None,
    }) else {
        return;
    };
    let names_it = |entity: &EntityType| {
        let common = entity.common();
        common.material_flags == 0b11 && common.material_handle == Some(by_block)
    };
    if !document.entities().any(names_it) {
        return;
    }
    for entity in document.entities_mut_keep_raw() {
        if names_it(entity) {
            let common = entity.common_mut();
            common.material_flags = 1;
            common.material_handle = None;
        }
    }
}

/// Re-point text-style handles on default records that no longer resolve to a
/// text style.
///
/// A dynamic-block object names its C++ class as the file's CLASSES section does (the DWG reader takes it
/// from there); the DXF subclass marker of some objects differs (`AcDbBlockRotationAction` for the class
/// `AcDbBlockRotateAction`).
fn name_dynamic_block_classes(document: &mut CadDocument) {
    use crate::objects::ObjectType;
    let renamed: Vec<(Handle, String)> = document
        .objects
        .iter()
        .filter_map(|(handle, object)| match object {
            ObjectType::DynamicBlock(block) => document
                .classes
                .source_classes()
                .and_then(|classes| {
                    classes
                        .iter()
                        .find(|class| class.dxf_name.eq_ignore_ascii_case(&block.dxf_name))
                })
                .filter(|class| {
                    !class.cpp_class_name.is_empty() && class.cpp_class_name != block.cpp_class_name
                })
                .map(|class| (*handle, class.cpp_class_name.clone())),
            _ => None,
        })
        .collect();
    for (handle, name) in renamed {
        if let Some(ObjectType::DynamicBlock(block)) = document.objects.get_mut(&handle) {
            block.cpp_class_name = name;
        }
    }
}

/// AutoCAD's hidden per-viewport layer-override records (`<base layer> @ <viewport handle>`) are named by
/// their base layer in an entity of a DWG, which keeps the record's handle. A DXF entity names the record
/// itself (group 8); read it the same way (base layer and the record's handle) so that one drawing has one
/// model, and the writer writes the record's name back.
fn name_viewport_override_layers(document: &mut CadDocument) {
    use crate::io::dwg::dwg_document_builder::viewport_override_base_layer;
    let overrides: std::collections::HashMap<String, (String, Handle)> = document
        .layers
        .iter()
        .filter(|layer| !layer.handle.is_null())
        .filter_map(|layer| {
            viewport_override_base_layer(&layer.name)
                .map(|base| (layer.name.clone(), (base.to_string(), layer.handle)))
        })
        .collect();
    if overrides.is_empty() {
        return;
    }
    for entity in document.entities_mut_keep_raw() {
        let common = entity.common_mut();
        if let Some((base, handle)) = overrides.get(&common.layer) {
            common.layer = base.clone();
            common.layer_handle = Some(*handle);
        }
    }
}

/// A STYLE's TrueType typeface is XDATA (`ACAD`: face name, flags) in DXF. The reader decodes it into the
/// style's `true_type_font` fields; as the DWG reader does, drop the record from the retained XDATA so one
/// datum has one place (the writer writes the fields back), and remember where the source listed it.
fn drop_decoded_typefaces(document: &mut CadDocument) {
    use crate::xdata::XDataValue;
    let handles: Vec<Handle> = document
        .text_styles
        .iter()
        .filter(|style| !style.true_type_font.is_empty())
        .map(|style| style.handle)
        .collect();
    for handle in handles {
        let Some(xdata) = document.object_xdata.get_mut(&handle) else {
            continue;
        };
        let Some(index) = xdata
            .records()
            .iter()
            .position(|record| record.application_name == "ACAD")
        else {
            continue;
        };
        // Only the exact shape the typeface fields were decoded from: a string and a 32-bit flags word.
        let decoded = matches!(
            xdata.records()[index].values.as_slice(),
            [XDataValue::String(_), XDataValue::Integer32(_)]
        );
        if !decoded {
            continue;
        }
        xdata.remove_record("ACAD");
        if xdata.is_empty() {
            document.object_xdata.remove(&handle);
        }
        if let Some(style) = document.text_styles.iter_mut().find(|style| style.handle == handle) {
            style.true_type_font_eed_position = Some(index);
        }
    }
}

/// A TABLESTYLE row names its text style (group 7) and DXF has no handle for it; the DWG reader keeps the
/// handle. Resolve the name to the style's handle so one drawing has one model.
fn resolve_table_style_text_styles(document: &mut CadDocument) {
    use crate::objects::ObjectType;
    let handles: Vec<Handle> = document
        .objects
        .iter()
        .filter(|(_, object)| matches!(object, ObjectType::TableStyle(_)))
        .map(|(handle, _)| *handle)
        .collect();
    for handle in handles {
        let Some(ObjectType::TableStyle(style)) = document.objects.get(&handle) else {
            continue;
        };
        let wanted: Vec<Option<Handle>> = [&style.data_row_style, &style.header_row_style, &style.title_row_style]
            .iter()
            .map(|row| {
                (row.text_style_handle.is_none() && !row.text_style_name.is_empty())
                    .then(|| document.text_styles.get(&row.text_style_name).map(|text| text.handle))
                    .flatten()
            })
            .collect();
        if wanted.iter().all(Option::is_none) {
            continue;
        }
        if let Some(ObjectType::TableStyle(style)) = document.objects.get_mut(&handle) {
            for (row, found) in [&mut style.data_row_style, &mut style.header_row_style, &mut style.title_row_style]
                .into_iter()
                .zip(wanted)
            {
                if found.is_some() {
                    row.text_style_handle = found;
                }
            }
        }
    }
}

/// The default Standard DIMSTYLE and Standard MLEADERSTYLE both wire their
/// text-style reference to the default Standard TEXT STYLE handle. When the
/// input file replaces that text style with its own record at a different
/// handle, the surviving default is left pointing at a numeric handle the file
/// has since given to an unrelated record - e.g. the ByBlock linetype - and the
/// writer emits a `340`/`342` that consumers resolve to the wrong table
/// (issues #64 and #68). Re-point such stale handles at the file's text style
/// of the same name.
fn rewire_stale_text_style_references(document: &mut CadDocument) {
    let text_style_handles: std::collections::HashSet<u64> = document
        .text_styles
        .iter()
        .map(|style| style.handle.value())
        .collect();
    let is_stale =
        |handle: Handle| !handle.is_null() && !text_style_handles.contains(&handle.value());

    // The default wiring is Standard -> Standard, so a stale reference is
    // re-pointed at the text style carrying the same name as the record that
    // holds it. When no such text style exists the handle is left alone rather
    // than guessed at.
    let resolve = |document: &CadDocument, name: &str| -> Option<Handle> {
        document.text_styles.get(name).map(|s| s.handle)
    };

    // ── DIMSTYLE (group 340) ──
    let mut stale_names: Vec<String> = Vec::new();
    for style in document.dim_styles.iter() {
        if is_stale(style.dimtxsty_handle) {
            stale_names.push(style.name().to_string());
        }
    }
    for name in stale_names {
        if let Some(new) = resolve(document, &name) {
            if let Some(style) = document.dim_styles.get_mut(&name) {
                style.dimtxsty_handle = new;
            }
        }
    }

    // ── MLEADERSTYLE (group 342) ──
    let mut stale_styles: Vec<(Handle, String)> = Vec::new();
    for (handle, object) in document.objects.iter() {
        if let crate::objects::ObjectType::MultiLeaderStyle(style) = object {
            if style.text_style_handle.is_some_and(is_stale) {
                stale_styles.push((*handle, style.name.clone()));
            }
        }
    }
    for (handle, name) in stale_styles {
        let Some(new) = resolve(document, &name) else {
            continue;
        };
        if let Some(crate::objects::ObjectType::MultiLeaderStyle(style)) =
            document.objects.get_mut(&handle)
        {
            style.text_style_handle = Some(new);
        }
    }
}

#[cfg(test)]
mod r5a4_tests {
    use super::*;
    use crate::classes::DxfClass;
    use crate::objects::{DynamicBlockObject, ObjectType};

    #[test]
    fn a_dynamic_block_object_takes_the_cpp_name_of_the_files_class() {
        let mut document = CadDocument::new();
        document
            .classes
            .record_source(DxfClass::new("BLOCKROTATEACTION", "AcDbBlockRotateAction"));
        let object = DynamicBlockObject::new("BLOCKROTATEACTION", "AcDbBlockRotationAction");
        let handle = Handle::new(0x500);
        document.objects.insert(handle, ObjectType::DynamicBlock(object));
        name_dynamic_block_classes(&mut document);
        let Some(ObjectType::DynamicBlock(object)) = document.objects.get(&handle) else {
            panic!("object")
        };
        assert_eq!(object.cpp_class_name, "AcDbBlockRotateAction");
    }
}
