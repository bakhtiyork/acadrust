//! The group-code view of a DXF R12 (AC1009) file.
//!
//! The regular [`SectionWriter`](super::SectionWriter) writes the R13 and later object model: subclass markers
//! (100), reactor and extension dictionary blocks (102 with 330 and 360), owner pointers (330) and the DIMSTYLE
//! handle (105). R12 has none of these groups, so for R12 output the stream states every other group the writer
//! produces and drops these; the structure of the file (sections, symbol-table and block handles, the entity forms)
//! is decided by the section writer itself, which knows the version.

use super::stream_writer::DxfStreamWriter;
use crate::error::Result;
use crate::types::Handle;

/// Group codes that do not exist in an R12 file.
fn absent_in_r12(code: i32) -> bool {
    matches!(code, 100 | 102 | 105 | 330 | 360)
}

/// A stream writer that leaves out the groups an R12 file does not have.
pub(super) struct R12Stream<'a, W: DxfStreamWriter> {
    inner: &'a mut W,
}

impl<'a, W: DxfStreamWriter> R12Stream<'a, W> {
    pub(super) fn new(inner: &'a mut W) -> Self {
        Self { inner }
    }
}

impl<W: DxfStreamWriter> DxfStreamWriter for R12Stream<'_, W> {
    fn is_binary(&self) -> bool {
        self.inner.is_binary()
    }

    fn write_string(&mut self, code: i32, value: &str) -> Result<()> {
        if absent_in_r12(code) {
            return Ok(());
        }
        self.inner.write_string(code, value)
    }

    fn write_xrecord_string(&mut self, code: i32, value: &str) -> Result<()> {
        if absent_in_r12(code) {
            return Ok(());
        }
        self.inner.write_xrecord_string(code, value)
    }

    fn writes_source_spelling(&self) -> bool {
        self.inner.writes_source_spelling()
    }

    fn write_source_spelling(&mut self, code: i32, text: &str) -> Result<()> {
        if absent_in_r12(code) {
            return Ok(());
        }
        self.inner.write_source_spelling(code, text)
    }

    fn write_byte(&mut self, code: i32, value: u8) -> Result<()> {
        if absent_in_r12(code) {
            return Ok(());
        }
        self.inner.write_byte(code, value)
    }

    fn write_i16(&mut self, code: i32, value: i16) -> Result<()> {
        if absent_in_r12(code) {
            return Ok(());
        }
        self.inner.write_i16(code, value)
    }

    fn write_i32(&mut self, code: i32, value: i32) -> Result<()> {
        if absent_in_r12(code) {
            return Ok(());
        }
        self.inner.write_i32(code, value)
    }

    fn write_i64(&mut self, code: i32, value: i64) -> Result<()> {
        if absent_in_r12(code) {
            return Ok(());
        }
        self.inner.write_i64(code, value)
    }

    fn write_double(&mut self, code: i32, value: f64) -> Result<()> {
        if absent_in_r12(code) {
            return Ok(());
        }
        self.inner.write_double(code, value)
    }

    fn write_bool(&mut self, code: i32, value: bool) -> Result<()> {
        if absent_in_r12(code) {
            return Ok(());
        }
        self.inner.write_bool(code, value)
    }

    fn write_handle(&mut self, code: i32, handle: Handle) -> Result<()> {
        if absent_in_r12(code) {
            return Ok(());
        }
        self.inner.write_handle(code, handle)
    }

    fn write_binary(&mut self, code: i32, data: &[u8]) -> Result<()> {
        if absent_in_r12(code) {
            return Ok(());
        }
        self.inner.write_binary(code, data)
    }

    fn flush(&mut self) -> Result<()> {
        self.inner.flush()
    }
}
