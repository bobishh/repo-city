fn main() {
    println!("cargo:rerun-if-changed=vendor/stb/adapter.c");
    println!("cargo:rerun-if-changed=vendor/stb/stb_textedit.h");
    let target = std::env::var("TARGET").unwrap_or_default();
    let mut build = cc::Build::new();
    if target == "wasm32-unknown-unknown" {
        // Apple Clang omits WebAssembly. Honor explicit CC first, then Homebrew LLVM.
        if [
            "CC_wasm32-unknown-unknown",
            "CC_wasm32_unknown_unknown",
            "TARGET_CC",
            "CC",
        ]
        .iter()
        .all(|name| std::env::var_os(name).is_none())
            && std::path::Path::new("/opt/homebrew/opt/llvm/bin/clang").exists()
        {
            build.compiler("/opt/homebrew/opt/llvm/bin/clang");
        }
        build.flag("-ffreestanding").flag("-fno-builtin");
    }
    build
        .file("vendor/stb/adapter.c")
        .flag_if_supported("-std=c11")
        .warnings(false)
        .compile("repo_editor");
}
