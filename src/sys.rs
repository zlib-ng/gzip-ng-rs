// sys.rs -- the declarations of zlib-ng's native API that the engine calls
// For conditions of distribution and use, see LICENSE.md

#![allow(non_camel_case_types, non_snake_case)]

use std::ffi::{c_char, c_int, c_uint, c_ulong, c_void};

// The zlib-ng declared here is the one libz-ng-sys builds and links.
use libz_ng_sys as _;

pub type alloc_func = Option<unsafe extern "C" fn(opaque: *mut c_void, items: c_uint, size: c_uint) -> *mut c_void>;
pub type free_func = Option<unsafe extern "C" fn(opaque: *mut c_void, address: *mut c_void)>;

/// Not visible by applications.
#[repr(C)]
pub struct internal_state {
    _private: [u8; 0],
}

/// A stream of zeros is a valid one to hand to init, which fills in the allocator.
#[repr(C)]
pub struct zng_stream {
    pub next_in: *const u8,
    pub avail_in: u32,
    pub total_in: usize,
    pub next_out: *mut u8,
    pub avail_out: u32,
    pub total_out: usize,
    pub msg: *const c_char,
    pub state: *mut internal_state,
    pub zalloc: alloc_func,
    pub zfree: free_func,
    pub opaque: *mut c_void,
    pub data_type: c_int,
    pub adler: u32,
    pub reserved: c_ulong,
}

#[repr(C)]
pub struct zng_gz_header {
    pub text: i32,
    pub time: c_ulong,
    pub xflags: i32,
    pub os: i32,
    pub extra: *mut u8,
    pub extra_len: u32,
    pub extra_max: u32,
    pub name: *mut u8,
    pub name_max: u32,
    pub comment: *mut u8,
    pub comm_max: u32,
    pub hcrc: i32,
    pub done: i32,
}

pub const Z_NO_FLUSH: i32 = 0;
pub const Z_SYNC_FLUSH: i32 = 2;
pub const Z_FULL_FLUSH: i32 = 3;
pub const Z_FINISH: i32 = 4;
pub const Z_BLOCK: i32 = 5;

pub const Z_OK: i32 = 0;
pub const Z_STREAM_END: i32 = 1;
pub const Z_BUF_ERROR: i32 = -5;

pub const Z_DEFAULT_STRATEGY: i32 = 0;
pub const Z_DEFLATED: i32 = 8;

unsafe extern "C" {
    pub fn zlibng_version() -> *const c_char;

    pub fn zng_deflateInit2(
        strm: *mut zng_stream,
        level: i32,
        method: i32,
        windowBits: i32,
        memLevel: i32,
        strategy: i32,
    ) -> i32;
    pub fn zng_deflate(strm: *mut zng_stream, flush: i32) -> i32;
    pub fn zng_deflateEnd(strm: *mut zng_stream) -> i32;
    pub fn zng_deflateReset(strm: *mut zng_stream) -> i32;
    pub fn zng_deflateParams(strm: *mut zng_stream, level: i32, strategy: i32) -> i32;
    pub fn zng_deflateBound(strm: *mut zng_stream, sourceLen: c_ulong) -> c_ulong;
    pub fn zng_deflateSetHeader(strm: *mut zng_stream, head: *mut zng_gz_header) -> i32;

    pub fn zng_inflateInit2(strm: *mut zng_stream, windowBits: i32) -> i32;
    pub fn zng_inflate(strm: *mut zng_stream, flush: i32) -> i32;
    pub fn zng_inflateEnd(strm: *mut zng_stream) -> i32;
    pub fn zng_inflateReset(strm: *mut zng_stream) -> i32;
    pub fn zng_inflateReset2(strm: *mut zng_stream, windowBits: i32) -> i32;

    pub fn zng_crc32_z(crc: u32, buf: *const u8, len: usize) -> u32;
}
