use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // Only compile Swift bridge when apple-music feature is enabled
    if env::var("CARGO_FEATURE_APPLE_MUSIC").is_err() {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let bridge_dir = PathBuf::from("swift-bridge");

    // Find the Swift resource directory for linking the Swift runtime
    let swift_res = Command::new("swiftc")
        .args(["--print-target-info"])
        .output()
        .ok()
        .and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout);
            // Parse JSON to find runtimeResourcePath
            text.lines()
                .find(|l| l.contains("\"runtimeResourcePath\""))
                .and_then(|l| {
                    let parts: Vec<&str> = l.split('"').collect();
                    parts.get(3).map(|s| PathBuf::from(s))
                })
        });

    // Collect all Swift sources in the bridge directory
    let swift_sources: Vec<_> = std::fs::read_dir(&bridge_dir)
        .expect("swift-bridge/ directory not found")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |ext| ext == "swift"))
        .collect();

    // Compile Swift to a static library
    let lib_path = out_dir.join("libmusic_bridge.a");
    let mut cmd = Command::new("swiftc");
    cmd.args(["-emit-library", "-static"])
        .arg("-o")
        .arg(&lib_path)
        .arg("-module-name")
        .arg("MusicBridge");

    for src in &swift_sources {
        cmd.arg(src);
    }

    let status = cmd.status().expect("Failed to run swiftc");
    if !status.success() {
        panic!("Swift compilation failed");
    }

    // Tell cargo to link our static library
    println!("cargo:rustc-link-search=native={}", out_dir.display());
    println!("cargo:rustc-link-lib=static=music_bridge");

    // Link Apple frameworks used by the Swift code
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=CoreGraphics");
    println!("cargo:rustc-link-lib=framework=ImageIO");

    // Link Swift runtime
    if let Some(res_path) = &swift_res {
        let lib_dir = res_path.join("macosx");
        if lib_dir.exists() {
            println!("cargo:rustc-link-search=native={}", lib_dir.display());
        }
        // Also try the parent as a search path
        println!("cargo:rustc-link-search=native={}", res_path.display());
    }

    // Link the Swift standard libraries
    // Find the macOS SDK toolchain lib path
    if let Ok(output) = Command::new("xcrun")
        .args(["--show-sdk-path", "--sdk", "macosx"])
        .output()
    {
        let sdk_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !sdk_path.is_empty() {
            // The usr/lib/swift path under the SDK
            let swift_lib = format!("{}/usr/lib/swift", sdk_path);
            println!("cargo:rustc-link-search=native={}", swift_lib);
        }
    }

    // Find Swift toolchain lib directory
    if let Ok(output) = Command::new("xcrun")
        .args(["--find", "swiftc"])
        .output()
    {
        let swiftc_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if let Some(bin_dir) = PathBuf::from(&swiftc_path).parent() {
            let lib_dir = bin_dir.parent().unwrap().join("lib").join("swift").join("macosx");
            if lib_dir.exists() {
                println!("cargo:rustc-link-search=native={}", lib_dir.display());
            }
        }
    }

    // Rerun if Swift sources change
    for src in &swift_sources {
        println!("cargo:rerun-if-changed={}", src.display());
    }
    println!("cargo:rerun-if-changed=swift-bridge/");
}
