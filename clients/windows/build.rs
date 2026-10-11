use std::{env, path::PathBuf, process::Command};
fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let icon = root
        .join("../../assets/brand/pasteleaf-app-icon-v2.ico")
        .canonicalize()
        .expect("PasteLeaf icon");
    let manifest = root.join("app.manifest");
    println!("cargo:rerun-if-changed={}", icon.display());
    println!("cargo:rerun-if-changed={}", manifest.display());
    let escaped = |p: &std::path::Path| p.to_string_lossy().replace('\\', "\\\\");
    let rc = out.join("pasteleaf.rc");
    std::fs::write(
        &rc,
        format!(
            "1 ICON \"{}\"\n1 24 \"{}\"\n",
            escaped(&icon),
            escaped(&manifest)
        ),
    )
    .unwrap();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_env == "gnu" {
        let compiler = env::var("WINDRES").unwrap_or_else(|_| {
            if cfg!(windows) {
                "windres".into()
            } else {
                "x86_64-w64-mingw32-windres".into()
            }
        });
        let object = out.join("pasteleaf-res.o");
        let status = Command::new(compiler)
            .args(["-i"])
            .arg(&rc)
            .args(["-o"])
            .arg(&object)
            .args(["-O", "coff"])
            .status()
            .expect("Install the MinGW windres resource compiler or set WINDRES");
        assert!(status.success(), "Windows resource compilation failed");
        println!("cargo:rustc-link-arg={}", object.display());
    } else {
        let resource = out.join("pasteleaf.res");
        let status = Command::new("rc.exe")
            .arg("/nologo")
            .arg(format!("/fo{}", resource.display()))
            .arg(&rc)
            .status()
            .expect("Run from a Visual Studio Developer Shell with rc.exe");
        assert!(status.success(), "Windows resource compilation failed");
        println!("cargo:rustc-link-arg={}", resource.display());
    }
}
