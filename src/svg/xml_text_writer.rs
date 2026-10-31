// SPDX-License-Identifier: MPL-2.0

//! The subset of .NET's `System.Xml.XmlTextWriter` that ACadSharp's `SvgXmlWriter` derives from.
//!
//! `SvgWriter` hands the .NET writer no encoding, so the document is UTF-8 without a byte order mark
//! and the declaration is `<?xml version="1.0"?>`. Indentation is two spaces, attributes use double
//! quotes, and the line break is LF (the C# reference runs on macOS).

use super::error::SvgError;
use std::io::Write;

/// `System.Xml.Formatting`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Formatting {
    None,
    Indented,
}

struct Element {
    name: String,
    start_tag_open: bool,
    has_text: bool,
    has_node: bool,
}

/// A streaming XML writer with the behaviour of `XmlTextWriter` for the calls the SVG writer makes.
pub struct XmlTextWriter<W: Write> {
    out: W,
    pub formatting: Formatting,
    elements: Vec<Element>,
    document_started: bool,
    document_ended: bool,
    closed: bool,
    root_written: bool,
    attribute_open: bool,
    text_depth: usize,
}

impl<W: Write> XmlTextWriter<W> {
    pub fn new(out: W) -> Self {
        XmlTextWriter {
            out,
            formatting: Formatting::None,
            elements: Vec::new(),
            document_started: false,
            document_ended: false,
            closed: false,
            root_written: false,
            attribute_open: false,
            text_depth: 0,
        }
    }

    /// Gives the underlying stream back (after `close`).
    pub fn into_inner(self) -> W {
        self.out
    }

    pub fn write_start_document(&mut self) -> Result<(), SvgError> {
        if self.document_started {
            return Err(SvgError::invalid_operation("WriteStartDocument has already been called."));
        }
        self.write_raw("<?xml version=\"1.0\"?>")?;
        self.document_started = true;
        Ok(())
    }

    pub fn write_end_document(&mut self) -> Result<(), SvgError> {
        if self.document_ended {
            return Ok(());
        }
        if !self.root_written {
            return Err(SvgError::argument("The document must contain a root element."));
        }
        while !self.elements.is_empty() {
            self.write_end_element()?;
        }
        self.document_ended = true;
        Ok(())
    }

    pub fn write_start_element(&mut self, local_name: &str) -> Result<(), SvgError> {
        self.ensure_usable()?;
        if self.attribute_open {
            self.write_end_attribute()?;
        }
        if local_name.is_empty() {
            return Err(SvgError::argument("The element name cannot be empty."));
        }
        if let Some(index) = self.elements.len().checked_sub(1) {
            self.close_start_tag(index)?;
            if self.can_indent() && !self.elements[index].has_text {
                self.write_raw("\n")?;
                self.write_indent(self.elements.len())?;
            }
            self.elements[index].has_node = true;
        } else if self.formatting == Formatting::Indented && self.document_started {
            self.write_raw("\n")?;
        }
        if self.elements.is_empty() {
            self.root_written = true;
        }
        self.write_raw("<")?;
        self.write_raw(local_name)?;
        self.elements.push(Element {
            name: local_name.to_owned(),
            start_tag_open: true,
            has_text: false,
            has_node: false,
        });
        Ok(())
    }

    pub fn write_end_element(&mut self) -> Result<(), SvgError> {
        self.ensure_usable()?;
        if self.attribute_open {
            self.write_end_attribute()?;
        }
        let Some(element) = self.elements.pop() else {
            return Err(SvgError::invalid_operation("The element stack is empty."));
        };
        if element.has_text {
            self.text_depth -= 1;
        }
        if element.start_tag_open {
            return self.write_raw(" />");
        }
        if self.can_indent() && element.has_node && !element.has_text {
            self.write_raw("\n")?;
            self.write_indent(self.elements.len())?;
        }
        self.write_raw("</")?;
        self.write_raw(&element.name)?;
        self.write_raw(">")
    }

    pub fn write_start_attribute(&mut self, local_name: &str) -> Result<(), SvgError> {
        self.ensure_usable()?;
        if local_name.is_empty() {
            return Err(SvgError::argument("The attribute name cannot be empty."));
        }
        if self.attribute_open {
            self.write_end_attribute()?;
        }
        match self.elements.last() {
            Some(e) if e.start_tag_open => {}
            _ => {
                return Err(SvgError::invalid_operation(
                    "Attributes can only be written inside an element start tag.",
                ))
            }
        }
        self.write_raw(" ")?;
        self.write_raw(local_name)?;
        self.write_raw("=\"")?;
        self.attribute_open = true;
        Ok(())
    }

    pub fn write_end_attribute(&mut self) -> Result<(), SvgError> {
        if !self.attribute_open {
            return Err(SvgError::invalid_operation("There is no open attribute."));
        }
        self.write_raw("\"")?;
        self.attribute_open = false;
        Ok(())
    }

    pub fn write_attribute_string(&mut self, local_name: &str, value: &str) -> Result<(), SvgError> {
        self.write_start_attribute(local_name)?;
        self.write_string(value)?;
        self.write_end_attribute()
    }

    /// `WriteValue(string)`.
    pub fn write_value_str(&mut self, value: &str) -> Result<(), SvgError> {
        self.write_string(value)
    }

    /// `WriteValue(double)`: `XmlConvert.ToString(double)`.
    pub fn write_value_f64(&mut self, value: f64) -> Result<(), SvgError> {
        let text = if value.is_nan() {
            "NaN".to_owned()
        } else if value.is_infinite() {
            if value < 0.0 { "-INF" } else { "INF" }.to_owned()
        } else {
            super::converter::double_to_string(value)
        };
        self.write_string(&text)
    }

    pub fn write_string(&mut self, value: &str) -> Result<(), SvgError> {
        self.ensure_usable()?;
        if value.is_empty() {
            return Ok(());
        }
        if self.attribute_open {
            let escaped = escape(value, true);
            return self.write_raw(&escaped);
        }
        let Some(index) = self.elements.len().checked_sub(1) else {
            return Err(SvgError::invalid_operation("Text cannot be written outside an element."));
        };
        self.close_start_tag(index)?;
        if !self.elements[index].has_text {
            self.elements[index].has_text = true;
            self.text_depth += 1;
        }
        self.elements[index].has_node = true;
        let escaped = escape(value, false);
        self.write_raw(&escaped)
    }

    pub fn write_comment(&mut self, text: &str) -> Result<(), SvgError> {
        self.ensure_usable()?;
        if text.contains("--") || text.ends_with('-') {
            return Err(SvgError::argument(
                "An XML comment cannot contain '--', and '-' cannot be the last character.",
            ));
        }
        if let Some(index) = self.elements.len().checked_sub(1) {
            self.close_start_tag(index)?;
            if self.can_indent() && !self.elements[index].has_text {
                self.write_raw("\n")?;
                self.write_indent(self.elements.len())?;
            }
            self.elements[index].has_node = true;
        } else if self.formatting == Formatting::Indented && self.document_started {
            self.write_raw("\n")?;
        }
        self.write_raw("<!--")?;
        self.write_raw(text)?;
        self.write_raw("-->")
    }

    pub fn flush(&mut self) -> Result<(), SvgError> {
        self.out.flush()?;
        Ok(())
    }

    /// `Close()`: ends open nodes, flushes and marks the writer closed. (The stream stays owned by the caller.)
    pub fn close(&mut self) -> Result<(), SvgError> {
        if self.closed {
            return Ok(());
        }
        if self.attribute_open {
            self.write_end_attribute()?;
        }
        if !self.elements.is_empty() {
            self.write_end_document()?;
        }
        self.flush()?;
        self.closed = true;
        Ok(())
    }

    fn ensure_usable(&self) -> Result<(), SvgError> {
        if self.closed {
            return Err(SvgError::invalid_operation("The writer is closed."));
        }
        if self.document_ended {
            return Err(SvgError::invalid_operation("The document has already been ended."));
        }
        Ok(())
    }

    fn write_raw(&mut self, value: &str) -> Result<(), SvgError> {
        self.out.write_all(value.as_bytes())?;
        Ok(())
    }

    fn close_start_tag(&mut self, index: usize) -> Result<(), SvgError> {
        if self.elements[index].start_tag_open {
            self.write_raw(">")?;
            self.elements[index].start_tag_open = false;
        }
        Ok(())
    }

    /// Once an element holds text, its whole subtree stays inline.
    fn can_indent(&self) -> bool {
        self.formatting == Formatting::Indented && self.text_depth == 0
    }

    fn write_indent(&mut self, depth: usize) -> Result<(), SvgError> {
        for _ in 0..depth {
            self.write_raw("  ")?;
        }
        Ok(())
    }
}

/// `XmlTextEncoder` escaping for text content (`attribute` false) and attribute values (true).
fn escape(value: &str, attribute: bool) -> String {
    let mut result = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' if attribute => result.push_str("&quot;"),
            // In an attribute value a line break is written as a character reference; a tab stays (.NET 10:
            // `WriteAttributeString("t", "a\tb\nc")` writes `t="a<TAB>b&#xA;c"`).
            '\n' | '\r' if attribute => result.push_str(&format!("&#x{:X};", ch as u32)),
            '\t' | '\n' | '\r' => result.push(ch),
            c if (c as u32) < 0x20 => result.push_str(&format!("&#x{:X};", c as u32)),
            '\u{FFFE}' | '\u{FFFF}' => result.push_str(&format!("&#x{:X};", ch as u32)),
            c => result.push(c),
        }
    }
    result
}
