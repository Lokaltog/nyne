use std::env;
use std::path::PathBuf;

use flate2::read::GzDecoder;
use tar::Archive;

/// Pinned commit from <https://github.com/alexmozaidze/tree-sitter-fennel>.
const REPO: &str = "alexmozaidze/tree-sitter-fennel";
const REV: &str = "3f0f6b24d599e92460b969aabc4f4c5a914d15a0";

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let grammar_dir = out_dir.join("grammar");

    if !grammar_dir.join("src/parser.c").exists() {
        fetch_grammar(&grammar_dir);
    }

    let src_dir = grammar_dir.join("src");

    let mut build = cc::Build::new();
    build.include(&src_dir).warnings(false).file(src_dir.join("parser.c"));

    // Include the scanner if the grammar has one.
    let scanner_c = src_dir.join("scanner.c");
    if scanner_c.exists() {
        build.file(scanner_c);
    }

    build.compile("tree_sitter_fennel");
}

fn fetch_grammar(dest: &PathBuf) {
    let url = format!("https://github.com/{REPO}/archive/{REV}.tar.gz");

    let response = ureq::get(&url).call().expect("failed to download fennel grammar");
    let reader = response.into_body().into_reader();
    let decoder = GzDecoder::new(reader);
    let mut archive = Archive::new(decoder);

    // The tarball extracts to `tree-sitter-fennel-<rev>/`. We need its `src/`
    // directory. Extract everything into a temp dir, then move `src/` to dest.
    let extract_dir = dest.with_extension("extract");
    std::fs::create_dir_all(&extract_dir).unwrap();
    archive.unpack(&extract_dir).unwrap();

    // Find the extracted directory (there's exactly one top-level entry).
    let top_level = std::fs::read_dir(&extract_dir).unwrap().next().unwrap().unwrap().path();

    // Move the entire extracted directory to dest and clean up.
    if dest.exists() {
        std::fs::remove_dir_all(dest).unwrap();
    }
    std::fs::rename(&top_level, dest).unwrap();
    let _ = std::fs::remove_dir_all(&extract_dir);
}
