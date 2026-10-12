// gzng.rs -- the version strings and the zlib-ng linked behind them
// For conditions of distribution and use, see LICENSE.md

mod common;

use common::{gzip, whole_inflate};

#[test]
fn version_reported() {
    assert_eq!(gzng::version(), gzng::VERSION);
    assert_eq!(env!("CARGO_PKG_VERSION"), gzng::VERSION);
    assert!(!gzng::zlibng_version().is_empty());
}

#[test]
fn zlibng_roundtrip() {
    let text = vec![b'a'; 100_000];
    let packed = gzip(&text, None);

    assert!(packed.len() < text.len());
    assert_eq!(text, whole_inflate(&packed, text.len()));
}
