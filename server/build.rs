//! Windows, CUDA build: candle links cuBLAS and cuRAND at load time (<https://docs.rs/cudarc>, `dynamic-linking`), so
//! without CUDA installed Windows refuses to start the server at all, with an error box. Delay-loading them
//! (<https://learn.microsoft.com/en-us/cpp/build/reference/linker-support-for-delay-loaded-dlls>) lets the server start,
//! look for them first (native-kit's hw) and use the CPU, with the reason, where they are missing. Which libraries comes
//! from native-kit's build script (DEP_NATIVE_KIT_DELAYLOAD; empty unless Windows and CUDA), as only the binary links.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let dlls = std::env::var("DEP_NATIVE_KIT_DELAYLOAD").unwrap_or_default();
    for d in dlls.split(';').filter(|d| !d.is_empty()) {
        println!("cargo:rustc-link-arg=/DELAYLOAD:{d}");
    }
    if !dlls.is_empty() {
        println!("cargo:rustc-link-arg=delayimp.lib");
    }
}
