//! ONNX Runtime's WebGPU provider loads `libwebgpu_dawn.dylib`, which `ort`
//! copies beside each binary it builds; this lets the binaries find it there.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path");
    }
}
