//! On Windows, embed the icon and version details in badterm.exe, so
//! Explorer, the taskbar and file properties show them.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon/badterm.ico");
    #[cfg(windows)]
    windows_resources();
}

#[cfg(windows)]
fn windows_resources() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // Version numbers come from Cargo.toml.
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icon/badterm.ico");
    res.set("ProductName", "BadTerm");
    res.set("FileDescription", "BadTerm BBS client");
    res.set("LegalCopyright", "Copyright (C) 2026 Bad Cluster. GPL-3.0-or-later.");
    res.set("CompanyName", "Bad Cluster");
    if let Err(e) = res.compile() {
        panic!("embedding Windows resources: {e}");
    }
}
