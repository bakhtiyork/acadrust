//! DXF writer module

mod binary_writer;
mod r12;
mod section_writer;
mod stream_writer;
mod text_writer;
#[cfg(test)]
mod r5a_tests;
#[cfg(test)]
mod r5a2_tests;
#[cfg(test)]
mod r5a3_tests;

pub use binary_writer::DxfBinaryWriter;
pub use section_writer::SectionWriter;
pub use stream_writer::{value_type_for_code, DxfStreamWriter, DxfStreamWriterExt};
pub use text_writer::DxfTextWriter;

/// Angles are radians in the model and degrees in DXF. A plain `to_degrees` followed by the reader's `to_radians`
/// can come back one unit in the last place away; this picks the degree value (the plain one first, then its
/// neighbours) that reads back as exactly the stored radians, when one exists.
pub(crate) trait ToDxfDegrees {
    fn to_dxf_degrees(self) -> f64;
}

impl ToDxfDegrees for f64 {
    fn to_dxf_degrees(self) -> f64 {
        let plain = self.to_degrees();
        if !plain.is_finite() || plain.to_radians() == self {
            return plain;
        }
        let (mut up, mut down) = (plain, plain);
        for _ in 0..4 {
            up = up.next_up();
            down = down.next_down();
            if up.to_radians() == self {
                return up;
            }
            if down.to_radians() == self {
                return down;
            }
        }
        plain
    }
}

use crate::document::CadDocument;
use crate::entities::EntityType;
use crate::error::Result;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// DXF file writer
pub struct DxfWriter<'a> {
    document: &'a CadDocument,
    /// Whether to write binary DXF format
    pub binary: bool,
    /// Explicit target version; `None` writes the document's own version.
    version: Option<crate::types::DxfVersion>,
}

impl<'a> DxfWriter<'a> {
    /// Create a new DXF writer for ASCII output
    pub fn new(document: &'a CadDocument) -> Self {
        Self {
            document,
            binary: false,
            version: None,
        }
    }

    /// Create a new DXF writer for binary output
    pub fn new_binary(document: &'a CadDocument) -> Self {
        Self {
            document,
            binary: true,
            version: None,
        }
    }

    /// Write the document as `version` instead of its own version.
    ///
    /// Requesting [`DxfVersion::AC1009`](crate::types::DxfVersion::AC1009)
    /// produces a genuine R12 file (no handles, no CLASSES/OBJECTS sections,
    /// newer entities converted to R12 entities; ASCII only). A document that
    /// merely *carries* `AC1009` without this call keeps being written as R13,
    /// because the R13 structure preserves everything the document holds.
    pub fn with_version(mut self, version: crate::types::DxfVersion) -> Self {
        self.version = Some(version);
        self
    }

    /// Set whether to write binary format
    pub fn set_binary(&mut self, binary: bool) {
        self.binary = binary;
    }

    /// Write to a file
    pub fn write_to_file<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        self.write_to_writer(writer)
    }

    /// Write to any writer
    pub fn write_to_writer<W: Write>(&self, writer: W) -> Result<()> {
        if self.version == Some(crate::types::DxfVersion::AC1009) {
            return self.write_r12(writer);
        }
        // BricsCAD's binary DXF importer rejects the R2000/R2004 headers even
        // when their records are otherwise valid.  The same records are
        // accepted as R2010 binary DXF, whose strings are UTF-8.  Keep this
        // compatibility promotion local to binary output; ASCII and DWG
        // continue to preserve the requested legacy version and code page.
        let output_version = self.output_version();
        let encoding = if output_version < crate::types::DxfVersion::AC1021 {
            crate::io::dxf::code_page::encoding_from_code_page(&self.document.header.code_page)
        } else {
            None
        };
        if self.binary {
            let mut stream_writer = DxfBinaryWriter::new(writer)?;
            if let Some(encoding) = encoding {
                stream_writer.set_encoding(encoding);
            }
            self.write_dxf(&mut stream_writer, output_version)?;
            stream_writer.flush()?;
        } else {
            let mut stream_writer = DxfTextWriter::new(writer);
            if let Some(encoding) = encoding {
                stream_writer.set_encoding(encoding);
            }
            self.write_dxf(&mut stream_writer, output_version)?;
            stream_writer.flush()?;
        }
        Ok(())
    }

    /// Write to a byte vector (useful for testing)
    pub fn write_to_vec(&self) -> Result<Vec<u8>> {
        // Pre-allocate based on entity count: ~512 bytes per entity is a reasonable estimate
        let entity_count = self.document.entities().count();
        let estimated = (entity_count + 64) * 512;
        let mut buffer = Vec::with_capacity(estimated);
        self.write_to_writer(&mut buffer)?;
        Ok(buffer)
    }

    /// Write DXF content to a stream writer
    fn write_dxf<W: DxfStreamWriter>(
        &self,
        writer: &mut W,
        output_version: crate::types::DxfVersion,
    ) -> Result<()> {
        let mut prepared = crate::io::loft_parameters::prepared(self.document);
        // DXF and DWG share the same ownership graph. Repair table-backed
        // entities before either writer snapshots handles, otherwise an
        // ACAD_TABLE can emit a null/nonexistent block-record pointer.
        crate::io::dwg::dwg_writer::prepare_database_references(&mut prepared);
        // An in-place table rename leaves the entry keyed under its old name,
        // so entity layer/linetype lookups miss and reference a name the
        // written table no longer defines (issue #80).
        crate::io::dwg::dwg_writer::prepare_table_keys(&mut prepared);
        // Table entries added after the document was built or read
        // (`Layer::new` + `layers.add`, `AppId::new` + `app_ids.add`) still
        // carry NULL handles. Written as they are, each becomes a `5 / 0`
        // record, a duplicate of the others, and each also claims the root
        // dictionary (owned by NULL) as its extension dictionary. Assign them
        // on the output copy, as the DWG writer does.
        if prepared.has_null_table_entries() {
            prepared.to_mut().assign_table_entry_handles();
        }
        crate::io::dwg::dwg_writer::prepare_active_viewport_elevation(&mut prepared);
        // The modern cell styles of a table style have no place in the DXF TABLESTYLE record: they travel
        // in a CELLSTYLEMAP object of its extension dictionary (an R2010+ DWG keeps them in the record). The
        // output states those objects besides the document's own; the document is not copied for them.
        let carriers = crate::objects::plan_table_style_carriers(prepared.as_ref(), output_version);
        self.write_prepared_dxf(writer, prepared.as_ref(), output_version, carriers)
    }

    fn write_prepared_dxf<W: DxfStreamWriter>(
        &self,
        writer: &mut W,
        document: &CadDocument,
        output_version: crate::types::DxfVersion,
        carriers: Option<crate::objects::TableStyleCarriers>,
    ) -> Result<()> {
        let carrier_max_handle = carriers.as_ref().map_or(0, |carriers| carriers.max_handle());
        // The handles the carriers took from the allocator are used handles.
        let handle_start = compute_max_handle(document)
            .max(carriers.as_ref().and_then(|carriers| carriers.next_handle).unwrap_or(0))
            .max(carrier_max_handle + 1);
        // Count against the version actually emitted, not the one the document
        // was loaded as, so the handle budget matches what the writer produces.
        let extra_handles = count_extra_handles(
            document,
            SectionWriter::<W>::writable_version(output_version),
        );
        // A document read from a file keeps the seed its source stated unless a handle at or above it is
        // written: the native seed advances while the reader creates default objects, and writing it back would
        // grow it with every round trip.
        let handle_seed = match document.header.source_handle_seed {
            Some(source) if extra_handles == 0 => source.max(max_written_handle(document).max(carrier_max_handle) + 1),
            _ => handle_start + extra_handles + 1,
        };
        let mut section_writer = SectionWriter::new(writer, handle_start, handle_seed);
        section_writer.set_version(output_version);
        if let Some(carriers) = carriers {
            section_writer.set_carriers(carriers);
        }
        section_writer.set_dwg_source(document.dwg_source_version);
        section_writer.set_layer_names(document);
        section_writer.build_valid_handles(document);

        // Write all sections
        section_writer.write_header(document)?;
        section_writer.write_classes(document)?;
        section_writer.write_tables(document)?;
        section_writer.write_blocks(document)?;
        section_writer.write_entities(document)?;
        section_writer.write_objects(document)?;
        section_writer.write_acdsdata()?;
        section_writer.write_thumbnail(document)?;

        // Write EOF
        writer.write_string(0, "EOF")?;

        Ok(())
    }

    fn output_version(&self) -> crate::types::DxfVersion {
        let version = self.version.unwrap_or(self.document.version);
        if self.binary && version < crate::types::DxfVersion::AC1021 {
            crate::types::DxfVersion::AC1024
        } else {
            version
        }
    }

    /// Genuine R12 output (see [`DxfWriter::with_version`]).
    fn write_r12<W: Write>(&self, writer: W) -> Result<()> {
        if self.binary {
            // R12 binary DXF uses one-byte group codes, which the binary writer does not emit
            return Err(crate::error::DxfError::UnsupportedVersion(
                "binary DXF R12 (AC1009); write R12 as ASCII DXF".into(),
            ));
        }
        let mut stream_writer = DxfTextWriter::new(writer);
        if let Some(encoding) = crate::io::dxf::code_page::encoding_from_code_page(&self.document.header.code_page) {
            stream_writer.set_encoding(encoding);
        }
        r12::write_r12(&mut stream_writer, self.document)?;
        stream_writer.flush()?;
        Ok(())
    }

    /// Get a reference to the document
    pub fn document(&self) -> &CadDocument {
        &self.document
    }
}

fn count_extra_handles(document: &CadDocument, version: crate::types::DxfVersion) -> u64 {
    use crate::objects::ObjectType;

    let mut count = 0u64;

    for entity in document.entities() {
        match entity {
            EntityType::Polyline(polyline) => {
                // Vertices + SEQEND all use allocate_handle()
                count += polyline.vertices.len() as u64 + 1;
            }
            EntityType::Polyline2D(polyline) => {
                // Plain 2D polylines down-save as LWPOLYLINE for R2000+,
                // which emits no child records (see SectionWriter).
                if version >= crate::types::DxfVersion::AC1015
                    && section_writer::polyline2d_downsaves_to_lwpolyline(polyline)
                {
                    continue;
                }
                count += polyline
                    .vertices
                    .iter()
                    .filter(|vertex| vertex.common.handle.is_null())
                    .count() as u64;
                if polyline.seqend.as_ref().is_none_or(|seqend| seqend.common.handle.is_null()) {
                    count += 1;
                }
            }
            EntityType::Polyline3D(polyline) => {
                for vertex in &polyline.vertices {
                    if vertex.common.handle.is_null() {
                        count += 1;
                    }
                }
                if polyline.seqend.as_ref().is_none_or(|seqend| seqend.common.handle.is_null()) {
                    count += 1;
                }
            }
            EntityType::PolyfaceMesh(mesh) => {
                for vertex in &mesh.vertices {
                    if vertex.common.handle.is_null() {
                        count += 1;
                    }
                }
                for face in &mesh.faces {
                    if face.common.handle.is_null() {
                        count += 1;
                    }
                }
                let has_seqend_handle = mesh.seqend.as_ref().is_some_and(|seqend| !seqend.common.handle.is_null())
                    || mesh.seqend_handle.is_some_and(|handle| !handle.is_null());
                if !has_seqend_handle {
                    count += 1;
                }
            }
            EntityType::PolygonMesh(mesh) => {
                for vertex in &mesh.vertices {
                    if vertex.common.handle.is_null() {
                        count += 1;
                    }
                }
                if mesh.seqend.as_ref().is_none_or(|seqend| seqend.common.handle.is_null()) {
                    count += 1;
                }
            }
            EntityType::Insert(insert) => {
                let has_sequence = insert.has_attributes()
                    || insert.seqend.is_some()
                    || insert.seqend_handle.is_some();
                if has_sequence {
                    count += insert
                        .attributes
                        .iter()
                        .filter(|attribute| attribute.common.handle.is_null())
                        .count() as u64;
                    let has_seqend_handle = insert.seqend.as_ref().is_some_and(|seqend| !seqend.common.handle.is_null())
                        || insert.seqend_handle.is_some_and(|handle| !handle.is_null());
                    if !has_seqend_handle {
                        count += 1;
                    }
                }
            }
            _ => {}
        }
    }

    // Count SortEntitiesTable sort entries (allocate_handle used for each)
    for obj in document.objects.values() {
        if let ObjectType::SortEntitiesTable(table) = obj {
            // A table kept in the source records of this very version is written verbatim, allocating none.
            let verbatim = table.raw_dxf_version == Some(version) && table.raw_dxf_codes.is_some();
            if !verbatim {
                count += section_writer::sort_entries_needing_handles(table) as u64;
            }
        }
    }

    // Null layer handles are allocated while writing the LAYER table.
    count += document
        .layers
        .iter()
        .filter(|layer| layer.handle.is_null())
        .count() as u64;

    count
}

/// The highest handle among the records the writer emits (0 when there are none).
fn max_written_handle(document: &CadDocument) -> u64 {
    let mut max = 0u64;
    for entity in document.entities() {
        max = max.max(entity.common().handle.value());
        entity.for_each_child_handle(|handle| max = max.max(handle.value()));
    }
    max = document.objects.keys().fold(max, |max, handle| max.max(handle.value()));
    for br in document.block_records.iter() {
        max = max.max(br.handle.value()).max(br.block_entity_handle.value()).max(br.block_end_handle.value());
        for eh in &br.entity_handles {
            max = max.max(eh.value());
        }
    }
    for r in document.layers.iter() {
        max = max.max(r.handle.value());
    }
    for r in document.line_types.iter() {
        max = max.max(r.handle.value());
    }
    for r in document.text_styles.iter() {
        max = max.max(r.handle.value());
    }
    for r in document.dim_styles.iter() {
        max = max.max(r.handle.value());
    }
    let unused = section_writer::unused_default_app_ids(document);
    for r in document.app_ids.iter() {
        if !unused.contains(&r.name.to_ascii_uppercase()) {
            max = max.max(r.handle.value());
        }
    }
    for r in document.views.iter() {
        max = max.max(r.handle.value());
    }
    for r in document.vports.iter() {
        max = max.max(r.handle.value());
    }
    for r in document.ucss.iter() {
        max = max.max(r.handle.value());
    }
    max
}

/// Compute the true maximum handle across all document objects.
/// Returns max_handle + 1 (the first safe handle to allocate).
/// This is needed because DWG-loaded documents may have next_handle
/// below some object handles (DWG reader doesn't call resolve_references).
fn compute_max_handle(document: &CadDocument) -> u64 {
    let mut max = document.next_handle();

    for entity in document.entities() {
        let h = entity.common().handle.value();
        if h >= max {
            max = h + 1;
        }
        entity.for_each_child_handle(|handle| {
            max = max.max(handle.value().saturating_add(1));
        });
    }
    for (handle, _) in &document.objects {
        let h = handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for br in document.block_records.iter() {
        let h = br.handle.value();
        if h >= max {
            max = h + 1;
        }
        for eh in &br.entity_handles {
            let h = eh.value();
            if h >= max {
                max = h + 1;
            }
        }
        // BLOCK/ENDBLK markers are excluded from document.entities(), so scan
        // their handles here explicitly to avoid re-issuing them.
        let h = br.block_entity_handle.value();
        if h >= max {
            max = h + 1;
        }
        let h = br.block_end_handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for r in document.layers.iter() {
        let h = r.handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for r in document.line_types.iter() {
        let h = r.handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for r in document.text_styles.iter() {
        let h = r.handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for r in document.dim_styles.iter() {
        let h = r.handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for r in document.app_ids.iter() {
        let h = r.handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for r in document.views.iter() {
        let h = r.handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for r in document.vports.iter() {
        let h = r.handle.value();
        if h >= max {
            max = h + 1;
        }
    }
    for r in document.ucss.iter() {
        let h = r.handle.value();
        if h >= max {
            max = h + 1;
        }
    }

    max
}

/// Convenience function to write a document to a file
pub fn write_dxf<P: AsRef<Path>>(document: &CadDocument, path: P) -> Result<()> {
    let writer = DxfWriter::new(document);
    writer.write_to_file(path)
}

/// Convenience function to write a document to a binary DXF file
pub fn write_binary_dxf<P: AsRef<Path>>(document: &CadDocument, path: P) -> Result<()> {
    let writer = DxfWriter::new_binary(document);
    writer.write_to_file(path)
}
