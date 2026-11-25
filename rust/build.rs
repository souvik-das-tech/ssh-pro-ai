use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=../sshpro.ico");
    println!("cargo:rerun-if-changed=../sshpro/web");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // The exe icon (resource 1) is also the window and taskbar icon.
    let mut res = winresource::WindowsResource::new();
    res.set_icon_with_id("../sshpro.ico", "1");
    res.set("ProductName", "SSH Pro AI");
    res.set("FileDescription", "SSH Pro AI");
    if let Err(e) = res.compile() {
        println!("cargo:warning=icon resource not embedded: {e}");
    }
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") {
        copy_webview2_loader();
    }
}

/// With the GNU toolchain wry links WebView2Loader.dll dynamically (MSVC links it statically), so
/// the DLL must sit next to sshpro.exe. webview2-com-sys has already copied it into its build output.
fn copy_webview2_loader() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    // OUT_DIR = target/<profile>/build/sshpro-<hash>/out, or build/sshpro/<hash>/out (newer cargo)
    let Some(build) = out.ancestors().find(|p| p.file_name().is_some_and(|n| n == "build")).map(Path::to_path_buf) else {
        return;
    };
    let profile = build.parent().unwrap().to_path_buf();
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86") => "x86",
        _ => "x64",
    };
    let dirs = |p: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()
    };
    let mut outs: Vec<PathBuf> = Vec::new();
    for dir in dirs(&build) {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with("webview2-com-sys-") {
            outs.push(dir.join("out")); // build/webview2-com-sys-<hash>/out
        } else if name == "webview2-com-sys" {
            outs.extend(dirs(&dir).into_iter().map(|h| h.join("out"))); // build/webview2-com-sys/<hash>/out
        }
    }
    let newest = outs
        .into_iter()
        .map(|o| o.join(arch).join("WebView2Loader.dll"))
        .filter(|p| p.is_file())
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    match newest {
        Some(dll) => {
            if let Err(e) = std::fs::copy(&dll, profile.join("WebView2Loader.dll")) {
                println!("cargo:warning=could not copy WebView2Loader.dll: {e}");
            }
        }
        None => println!("cargo:warning=WebView2Loader.dll not found; copy it next to sshpro.exe"),
    }
}
