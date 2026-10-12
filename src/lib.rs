// lib.rs -- the gzip-ng library, the block engine and the stream functions over it
// For conditions of distribution and use, see LICENSE.md

mod buf;
mod codec;
mod compress;
mod cutter;
mod decompress;
pub mod format;
pub mod gzblock;
mod pipeline;
mod pool;
mod reader;
mod rolling;
#[doc(hidden)]
pub mod scanner;
#[doc(hidden)]
pub mod sys;
mod writer;
mod zng;

pub use compress::{Compress, DEFAULT_BLOCK, compress_stream};
pub use decompress::decompress_stream;
pub use zng::Strategy;

/// Version of gzip-ng.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The compression level when nothing says otherwise.
pub const DEFAULT_LEVEL: i32 = 7;

/// The level that leaves the choice to zlib-ng.
pub const DEFAULT_COMPRESSION: i32 = -1;

/// Read and write in this much at a time.
pub(crate) const IO_CHUNK: usize = 256 * 1024;

/// Bytes a stream function read and wrote.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub total_in: u64,
    pub total_out: u64,
}

/// Version of gzip-ng.
pub fn version() -> &'static str {
    VERSION
}

/// Version of the zlib-ng it was built against.
pub fn zlibng_version() -> &'static str {
    zng::version()
}
