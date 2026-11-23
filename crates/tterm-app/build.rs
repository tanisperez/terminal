//! Embeds the Windows resources (the executable icon, `assets/tterm.rc`) into `tterm.exe`, as ted
//! does. The resources follow the target, not the host, so a cross build from Linux
//! (`cargo xwin`, which provides `llvm-rc`) gets them too.

use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/tterm.rc");
        println!("cargo:rerun-if-changed=assets/tterm.ico");
        if let Err(error) =
            embed_resource::compile("assets/tterm.rc", embed_resource::NONE).manifest_optional()
        {
            panic!("failed to embed the Windows resources: {error}");
        }
    }
}
