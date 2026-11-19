// Embeds assets/app.ico as icon resource #1 into both binaries. gpui's Windows
// platform loads resource #1 via LoadImageW for the window class, which gives
// the settings window its title-bar and taskbar icon for free. rc.exe lives in
// the portable Windows SDK on this machine (same reason gpui needs
// GPUI_FXC_PATH), so it is located explicitly instead of via the registry.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let ico = manifest.join("assets").join("app.ico");
    if !ico.is_file() {
        println!("cargo:warning=assets/app.ico missing; skipping icon resource");
        return;
    }
    println!("cargo:rerun-if-changed={}", ico.display());

    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    // rc.exe resolves quoted icon paths relative to the .rc file, so stage the
    // ico next to it in OUT_DIR
    let staged = out.join("app.ico");
    fs::copy(&ico, &staged).unwrap();
    let rc_file = out.join("app.rc");
    fs::write(&rc_file, "1 ICON \"app.ico\"\r\n").unwrap();
    let res_file = out.join("app.res");

    let rc = env::var_os("CUA_RC_PATH")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| {
            let p = PathBuf::from(r"D:\Windows Kits\10\bin\10.0.22621.0\x64\rc.exe");
            p.is_file().then_some(p)
        });
    let Some(rc) = rc else {
        panic!("rc.exe not found (set CUA_RC_PATH to the Windows SDK x64 rc.exe)");
    };
    let status = Command::new(rc)
        .args(["/nologo", "/fo"])
        .arg(&res_file)
        .arg(&rc_file)
        .status()
        .expect("failed to run rc.exe");
    if !status.success() {
        panic!("rc.exe failed to compile the icon resource");
    }
    println!("cargo:rustc-link-arg-bins={}", res_file.display());
}
