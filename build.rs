// build.rs -- fetches zlib-ng, builds it, and links it in
// For conditions of distribution and use, see LICENSE.md

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const ZLIBNG_URL: &str = "https://github.com/zlib-ng/zlib-ng.git";

fn git(dir: &Path, args: &[&str]) -> bool {
    let status = Command::new("git").current_dir(dir).args(args).status();
    status.is_ok_and(|status| status.success())
}

fn main() {
    let source = PathBuf::from(env::var("OUT_DIR").unwrap()).join("zlib-ng");
    let tag = env::var("ZLIBNG_TAG").unwrap_or_else(|_| "develop".into());
    let target = env::var("TARGET").unwrap_or_default();

    // A path that never exists, so every build looks for a newer zlib-ng.
    println!("cargo:rerun-if-changed=zlib-ng-head");

    // The head of the branch. A build with no network keeps the source it has.
    if !source.join(".git").exists() {
        fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "--quiet"]);
        git(&source, &["remote", "add", "origin", ZLIBNG_URL]);
    }
    let fetched = git(&source, &["fetch", "--quiet", "--depth", "1", "origin", &tag])
        && git(&source, &["checkout", "--quiet", "--force", "FETCH_HEAD"]);
    if !fetched {
        println!("cargo:warning=could not fetch zlib-ng {tag}, building the source already here");
    }
    assert!(
        source.join("CMakeLists.txt").exists(),
        "could not fetch zlib-ng {tag}, the first build needs git and a network"
    );

    // A static zlib-ng with the native zng_ API, and none of its tests.
    let mut config = cmake::Config::new(&source);
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
