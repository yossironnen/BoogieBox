//! Embeds the BoogieBox icon and version details in the Windows executables; the tray
//! launcher (`src/bin/boogiebox.rs`) loads the same icon (resource id 1) for its tray icon.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let icon = "../../../desktop/src-tauri/icons/icon.ico";
    println!("cargo:rerun-if-changed={icon}");
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(icon)
        .set("ProductName", "BoogieBox")
        .set("FileDescription", "BoogieBox Server");
    if let Err(error) = resource.compile() {
        // No resource compiler (rc.exe) on this machine: build without the icon; the tray
        // launcher falls back to the generic application icon.
        println!("cargo:warning=Windows icon resource not embedded: {error}");
    }
}
