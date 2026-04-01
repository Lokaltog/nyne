//! Generates `use nyne_<plugin> as _;` linkage for every plugin crate
//! dependency so their `linkme` distributed-slice entries are discovered
//! at link time without a hand-maintained import list in `main.rs`.
#![allow(clippy::unwrap_used)]

use std::fmt::Write as _;
use std::path::Path;
use std::{env, fs};

fn main() {
    println!("cargo::rerun-if-changed=Cargo.toml");

    let manifest = fs::read_to_string(Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("Cargo.toml")).unwrap();

    let mut in_deps = false;
    let mut plugins = Vec::new();

    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_deps = trimmed == "[dependencies]";
            continue;
        }
        if !in_deps {
            continue;
        }
        // Match lines like `nyne-cache.workspace = true`.
        let Some(name) = trimmed.split('.').next() else {
            continue;
        };
        if name.starts_with("nyne-") {
            plugins.push(name.to_owned());
        }
    }

    plugins.sort();

    let mut out = String::new();
    for name in &plugins {
        let ident = name.replace('-', "_");
        writeln!(out, "use {ident} as _;").unwrap();
    }

    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("plugin_linkage.rs"), out).unwrap();
}
