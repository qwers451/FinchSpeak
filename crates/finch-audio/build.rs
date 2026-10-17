use std::{env, fs, path::PathBuf};
fn main() {
    assert_eq!(
        env::var("CARGO_CFG_TARGET_OS").as_deref(),
        Ok("windows"),
        "Native WebRTC build currently targets Windows only"
    );
    let repo = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let profile = if env::var("PROFILE").as_deref() == Ok("release") {
        "Release"
    } else {
        "Debug"
    };
    let root = env::var_os("FINCH_WEBRTC_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("target/native").join(profile));
    let library = root.join("finch_webrtc.lib");
    let dll = root.join("finch_webrtc.dll");
    assert!(
        library.is_file() && dll.is_file(),
        "Run scripts/prepare-webrtc.ps1 -Configuration {profile}; native WebRTC is required at {}",
        root.display()
    );
    println!("cargo:rustc-link-search=native={}", root.display());
    println!("cargo:rustc-link-lib=dylib=finch_webrtc");
    println!("cargo:rerun-if-env-changed=FINCH_WEBRTC_ROOT");
    println!("cargo:rerun-if-changed={}", library.display());
    println!("cargo:rerun-if-changed={}", dll.display());
    // Cargo tests/examples run next to deps or at profile root. Ship the same DLL beside each.
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let profile_root = out.ancestors().nth(3).expect("Cargo output layout");
    let bytes = fs::read(&dll).expect("read native DLL");
    for directory in [
        profile_root.to_path_buf(),
        profile_root.join("deps"),
        profile_root.join("examples"),
    ] {
        fs::create_dir_all(&directory).expect("create native runtime directory");
        let destination = directory.join("finch_webrtc.dll");
        if fs::read(&destination).ok().as_deref() != Some(bytes.as_slice()) {
            fs::write(&destination, &bytes).expect("stage native runtime beside executable");
        }
    }
}
