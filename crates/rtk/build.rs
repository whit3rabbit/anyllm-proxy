//! Embeds every `filters/*.json` at compile time so the proxy binary ships the
//! built-in RTK filter catalog with no runtime file IO. Regenerates when the
//! filters directory changes. Emits `FILTER_JSONS: &[&str]`.

use std::{env, fs, path::Path};

fn main() {
    println!("cargo:rerun-if-changed=filters");
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let dir = Path::new(&manifest).join("filters");

    let mut names: Vec<String> = fs::read_dir(&dir)
        .expect("filters dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".json"))
        .collect();
    names.sort();

    let mut out = String::from("pub static FILTER_JSONS: &[&str] = &[\n");
    for name in &names {
        out.push_str(&format!(
            "    include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/filters/{name}\")),\n"
        ));
    }
    out.push_str("];\n");

    let out_dir = env::var("OUT_DIR").unwrap();
    fs::write(Path::new(&out_dir).join("filters_generated.rs"), out).unwrap();
}
