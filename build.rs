//! Guarantee `console/dist/` exists so `rust-embed` (which embeds that folder
//! into the `cryohub` binary) compiles from a git checkout that has never run
//! `npm run build`. An empty folder embeds nothing; the hub then answers pages
//! with its "built without the console" setup page.
fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let dist = std::path::Path::new(&manifest).join("console").join("dist");
    std::fs::create_dir_all(&dist).expect("create console/dist");
    println!("cargo:rerun-if-changed=console/dist");
}
