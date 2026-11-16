//! Embeds the BoogieBox server icons and version details in the Windows executables. The tray
//! launcher (`src/bin/boogiebox.rs`) shows icon 1 while the server runs and icon 2 (grey) while it
//! starts or stops.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let icon = "icons/boogiebox-server.ico";
    let stopped = "icons/boogiebox-server-stopped.ico";
    println!("cargo:rerun-if-changed={icon}");
    println!("cargo:rerun-if-changed={stopped}");
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon(icon)
        .set_icon_with_id(stopped, "2")
        .set("ProductName", "BoogieBox")
        .set("FileDescription", "BoogieBox Server");
    if let Err(error) = resource.compile() {
        // No resource compiler (rc.exe) on this machine: build without the icon; the tray
        // launcher falls back to the generic application icon.
        println!("cargo:warning=Windows icon resource not embedded: {error}");
    }
}
