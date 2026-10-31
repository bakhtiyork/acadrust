// SPDX-License-Identifier: MPL-2.0

//! Errors of the SVG writer.
//!
//! ACadSharp's writer throws .NET exceptions; the port returns them as values. Each error knows the
//! short name of the C# exception it stands for, which the `acad-dump write-svg` tool prints.

use std::fmt;

/// An error raised while writing an SVG.
#[derive(Debug)]
pub enum SvgError {
    /// `System.NotImplementedException` (e.g. an entity whose bounding box ACadSharp does not implement).
    NotImplemented(String),
    /// `System.ComponentModel.InvalidEnumArgumentException` (units the pixel conversion does not support).
    InvalidEnumArgument(String),
    /// `System.ArgumentOutOfRangeException`.
    ArgumentOutOfRange(String),
    /// `System.ArgumentException`.
    Argument(String),
    /// `System.NullReferenceException`.
    NullReference(String),
    /// `System.InvalidOperationException`.
    InvalidOperation(String),
    /// `System.IndexOutOfRangeException`.
    IndexOutOfRange(String),
    /// `System.IO.IOException`: the output stream failed.
    Io(std::io::Error),
}

impl SvgError {
    pub(crate) fn not_implemented(message: impl Into<String>) -> Self {
        SvgError::NotImplemented(message.into())
    }
    pub(crate) fn invalid_enum(message: impl Into<String>) -> Self {
        SvgError::InvalidEnumArgument(message.into())
    }
    pub(crate) fn out_of_range(message: impl Into<String>) -> Self {
        SvgError::ArgumentOutOfRange(message.into())
    }
    pub(crate) fn argument(message: impl Into<String>) -> Self {
        SvgError::Argument(message.into())
    }
    pub(crate) fn null_reference(message: impl Into<String>) -> Self {
        SvgError::NullReference(message.into())
    }
    pub(crate) fn invalid_operation(message: impl Into<String>) -> Self {
        SvgError::InvalidOperation(message.into())
    }

    /// The short name of the C# exception type this error stands for.
    pub fn csharp_exception(&self) -> &'static str {
        match self {
            SvgError::NotImplemented(_) => "NotImplementedException",
            SvgError::InvalidEnumArgument(_) => "InvalidEnumArgumentException",
            SvgError::ArgumentOutOfRange(_) => "ArgumentOutOfRangeException",
            SvgError::Argument(_) => "ArgumentException",
            SvgError::NullReference(_) => "NullReferenceException",
            SvgError::InvalidOperation(_) => "InvalidOperationException",
            SvgError::IndexOutOfRange(_) => "IndexOutOfRangeException",
            SvgError::Io(_) => "IOException",
        }
    }
}

impl fmt::Display for SvgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SvgError::NotImplemented(m)
            | SvgError::InvalidEnumArgument(m)
            | SvgError::ArgumentOutOfRange(m)
            | SvgError::Argument(m)
            | SvgError::NullReference(m)
            | SvgError::IndexOutOfRange(m)
            | SvgError::InvalidOperation(m) => write!(f, "{}: {m}", self.csharp_exception()),
            SvgError::Io(e) => write!(f, "IOException: {e}"),
        }
    }
}

impl std::error::Error for SvgError {}

impl From<std::io::Error> for SvgError {
    fn from(e: std::io::Error) -> Self {
        SvgError::Io(e)
    }
}
