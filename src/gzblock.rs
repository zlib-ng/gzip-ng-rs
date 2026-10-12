// gzblock.rs -- gzip members made of independent deflate blocks, written and read in parallel
// For conditions of distribution and use, see LICENSE.md

//! One ordinary gzip member whose deflate stream is cut into independent blocks. Each block ends
//! with two empty stored blocks, the nine bytes 00 00 FF FF 00 00 00 FF FF, the same shape pigz
//! --independent writes, so a block inflates on its own and a boundary is hard to fake. Nothing in
//! the header says any of this, the reader finds the boundaries by looking for them, and a pair is
//! what tells it a boundary from a flush inside a block or a chance pattern in stored data. Any
//! deflate stream built the same way decodes here, pigz --independent output included, and
//! streams with single full flush markers are read by scanning for those.
//!
//! A pool of workers runs deflate or inflate over a ring of slots, filled in order and drained in
//! order, so output order is slot order and memory is bounded by the ring. Errors are reported
//! with a zlib return code and a message.

use std::error;
use std::fmt;
use std::io;

pub use crate::reader::Reader;
pub use crate::writer::Writer;

/// Largest block size accepted, from a caller or from a file's header. Bounds what a member can
/// make the reader allocate, two slots of input and output at this size stay within the ring
/// budget.
pub const MAX_BLOCK: u32 = 256 << 20;

/// What went wrong, with zlib's own return codes as the values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Code {
    /// Reading or writing failed.
    Errno = -1,
    /// The stream was misused or deflate failed.
    Stream = -2,
    /// The data is not valid.
    Data = -3,
    /// Memory or threads could not be had.
    Mem = -4,
    /// The data ended early.
    Buf = -5,
}

/// An error from the reader or the writer. Once one has failed, every later call fails with the
/// same code and message.
#[derive(Debug)]
pub struct Error {
    code: Code,
    msg: String,
    source: Option<io::Error>,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub(crate) fn new(code: Code, msg: impl Into<String>) -> Self {
        Self {
            code,
            msg: msg.into(),
            source: None,
        }
    }

    pub(crate) fn with_source(code: Code, msg: impl Into<String>, source: io::Error) -> Self {
        Self {
            code,
            msg: msg.into(),
            source: Some(source),
        }
    }

    pub fn code(&self) -> Code {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.msg
    }

    /// The read or write error behind an Errno, where the failure first surfaced.
    pub fn io_error(&self) -> Option<&io::Error> {
        self.source.as_ref()
    }

    /// The error an io error from the reader or the writer carries, None for any other.
    pub fn from_io(err: &io::Error) -> Option<&Self> {
        err.get_ref()?.downcast_ref()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        self.source.as_ref().map(|source| source as _)
    }
}

/// The error rides along as the payload, so a caller holding the io error can get it back.
impl From<Error> for io::Error {
    fn from(err: Error) -> Self {
        let kind = match (err.code, &err.source) {
            (Code::Errno, Some(source)) => source.kind(),
            (Code::Data, _) => io::ErrorKind::InvalidData,
            (Code::Buf, _) => io::ErrorKind::UnexpectedEof,
            (Code::Mem, _) => io::ErrorKind::OutOfMemory,
            _ => io::ErrorKind::Other,
        };
        io::Error::new(kind, err)
    }
}
