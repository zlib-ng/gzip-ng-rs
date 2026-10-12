// build.rs -- builds the zlib-ng in the submodule and links it in
// For conditions of distribution and use, see LICENSE.md

use std::env;
use std::path::Path;

fn main() {
    let source = Path::new("zlib-ng");
    let target = env::var("TARGET").unwrap_or_default();

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=zlib-ng");
    assert!(
        source.join("CMakeLists.txt").exists(),
        "the zlib-ng submodule is not checked out, run `git submodule update --init`"
    );

    // A static zlib-ng with the native zng_ API, and none of its tests.
    let mut config = cmake::Config::new(source);
    config
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("ZLIB_COMPAT", "OFF")
        .define("BUILD_TESTING", "OFF")
        .define("CMAKE_INSTALL_LIBDIR", "lib");
    if target == "i686-pc-windows-msvc" {
        config.define("CMAKE_GENERATOR_PLATFORM", "Win32");
    }
    let install = config.build();

    // zlib-ng names its static library for the toolchain, and for a debug build under MSVC.
    let name = if !target.contains("msvc") {
        "z-ng"
    } else if config.get_profile() == "Debug" {
        "zlibstatic-ngd"
    } else {
        "zlibstatic-ng"
    };
    println!("cargo:rustc-link-search=native={}", install.join("lib").display());
    println!("cargo:rustc-link-lib=static={name}");
}
