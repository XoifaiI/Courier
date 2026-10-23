//! Bundles the editor extension when it has been packaged, so `courier vscode` can install it.

use std::path::Path;

fn main() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editors/vscode");
    let vsix = folder.join("courier.vsix");
    println!("cargo::rerun-if-changed={}", folder.display());
    println!("cargo::rustc-check-cfg=cfg(vsix)");
    if vsix.is_file() {
        println!("cargo::rerun-if-changed={}", vsix.display());
        println!("cargo::rustc-cfg=vsix");
        println!("cargo::rustc-env=COURIER_VSIX={}", vsix.display());
    }
}
