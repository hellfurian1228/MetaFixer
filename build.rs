fn main() {
    println!("cargo:rerun-if-changed=windows.manifest");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os != "windows" {
        return;
    }
    let mut res = winres::WindowsResource::new();
    res.set_manifest_file("windows.manifest");
    res.set("ProductName", "Meta Fixer");
    res.set("FileDescription", "Meta Fixer");
    res.set("LegalCopyright", "Meta Fixer");
    if cfg!(unix) {
        res.set_windres_path("x86_64-w64-mingw32-windres");
        res.set_ar_path("x86_64-w64-mingw32-ar");
    }
    if let Err(err) = res.compile() {
        println!("cargo:warning=winres failed: {err}");
    }
}
