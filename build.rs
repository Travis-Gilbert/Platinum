//! ONNX Runtime's WebGPU provider loads `libwebgpu_dawn.dylib`, which `ort`
//! copies beside each binary it builds; this lets the binaries find it there.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path");
        let plist = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("assets/VoiceInfo.plist");
        println!("cargo:rerun-if-changed={}", plist.display());
        println!(
            "cargo:rustc-link-arg-bin=frame=-Wl,-sectcreate,__TEXT,__info_plist,{}",
            plist.display()
        );
    }
}
