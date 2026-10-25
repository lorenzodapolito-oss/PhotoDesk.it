//! Windows only: embed the app icon and version info into `photodesk.exe`.
//! On every other target this does nothing; a missing resource compiler is only a warning.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/photodesk.ico");
    #[cfg(windows)]
    embed();
}

#[cfg(windows)]
fn embed() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/photodesk.ico")
        .set("ProductName", "PhotoDesk")
        .set("FileDescription", "PhotoDesk image browser")
        .set("OriginalFilename", "photodesk.exe")
        .set("InternalName", "photodesk");
    if let Err(e) = res.compile() {
        println!("cargo:warning=photodesk.exe built without icon/version resources: {e}");
    }
}
