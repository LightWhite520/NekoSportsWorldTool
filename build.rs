fn main() {
    println!("cargo:rerun-if-changed=src/location/macos.m");
    println!("cargo:rerun-if-changed=src/location/Info.plist");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let output =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("location-helper");
    let architecture = match std::env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        other => panic!("Unsupported macOS architecture: {other}"),
    };
    let status = std::process::Command::new("xcrun")
        .args([
            "clang",
            "-arch",
            architecture,
            "-mmacosx-version-min=11.0",
            "-fobjc-arc",
            "src/location/macos.m",
            "-framework",
            "Foundation",
            "-framework",
            "CoreLocation",
            "-Wl,-sectcreate,__TEXT,__info_plist,src/location/Info.plist",
            "-o",
        ])
        .arg(&output)
        .status()
        .expect("macOS build requires Xcode Command Line Tools");
    assert!(status.success(), "Cannot compile CoreLocation helper");
    let status = std::process::Command::new("codesign")
        .args([
            "--force",
            "--sign",
            "-",
            "--identifier",
            "org.nekosportsworld.tool.location",
        ])
        .arg(output)
        .status()
        .expect("macOS codesign unavailable");
    assert!(status.success(), "Cannot sign CoreLocation helper");
}
