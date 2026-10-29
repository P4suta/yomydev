use std::env;
use std::path::PathBuf;

fn main() -> Result<(), env::VarError> {
    println!("cargo::rerun-if-changed=Info.plist");
    if env::var("CARGO_CFG_TARGET_OS")? == "macos" {
        let plist = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join("Info.plist");
        println!(
            "cargo::rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__info_plist,{}",
            plist.display()
        );
    }
    Ok(())
}
