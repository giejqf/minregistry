//! Points the embedded UI (`src/ui.rs`) at `web/dist`, or at a placeholder
//! page when the frontend has not been built, so backend-only builds work.

use std::{env, fs, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let dist = manifest_dir.join("..").join("web").join("dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    println!("cargo:rerun-if-changed={}", dist.join("index.html").display());
    println!("cargo:rerun-if-changed=migrations");

    let folder = if dist.join("index.html").is_file() {
        dist
    } else {
        let placeholder = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("web-placeholder");
        fs::create_dir_all(&placeholder).expect("create placeholder dir");
        fs::write(
            placeholder.join("index.html"),
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>MinRegistry</title></head>\
             <body><p>The MinRegistry web UI was not built into this binary. \
             Run <code>pnpm --dir web build</code> and rebuild the server.</p></body></html>\n",
        )
        .expect("write placeholder index.html");
        placeholder
    };
    let folder = folder.canonicalize().expect("canonicalize UI folder");
    println!("cargo:rustc-env=MINREGISTRY_WEB_DIST={}", folder.display());
}
