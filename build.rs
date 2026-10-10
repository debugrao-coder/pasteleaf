fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    let root = std::env::var("CARGO_MANIFEST_DIR").expect("Cargo sets the project directory");
    println!("cargo:rustc-link-arg-bin=pasteleaf=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bin=pasteleaf=/MANIFESTINPUT:{root}/app.manifest");
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=assets/pasteleaf.ico");
    let out = std::env::var("OUT_DIR").expect("Cargo sets OUT_DIR");
    let kits = std::path::Path::new("C:/Program Files (x86)/Windows Kits/10/bin");
    let mut candidates: Vec<_> = std::fs::read_dir(kits)
        .expect("Windows SDK required")
        .filter_map(Result::ok)
        .map(|e| e.path().join("x64/rc.exe"))
        .filter(|p| p.is_file())
        .collect();
    candidates.sort();
    let rc = candidates
        .last()
        .expect("Windows SDK resource compiler required");
    let res = format!("{out}/app.res");
    let status = std::process::Command::new(rc)
        .args(["/nologo", "/fo", &res, "app.rc"])
        .status()
        .expect("run resource compiler");
    assert!(status.success(), "resource compilation failed");
    println!("cargo:rustc-link-arg-bin=pasteleaf={res}");
}
