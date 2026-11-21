fn main() {
    facade_samples();
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

fn facade_samples() {
    use std::{io::Write, path::PathBuf};

    const SAMPLES: &[(&str, usize)] = &[
        ("../game/src/repo_city/world_set.rs", 0),
        ("../game/src/repo_city/city_stream.rs", 18),
        ("../../web/review.mjs", 170),
    ];
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output directory"));
    let mut output =
        std::fs::File::create(output.join("facade_samples.rs")).expect("create facade sample data");
    writeln!(
        output,
        "fn code_samples() -> [Vec<Line>; {}] {{ [",
        SAMPLES.len()
    )
    .unwrap();
    for &(path, start) in SAMPLES {
        println!("cargo:rerun-if-changed={path}");
        let source = std::fs::read_to_string(path).expect("read Repo City facade sample");
        writeln!(output, "vec![").unwrap();
        for line in repo_world::syntax::highlight(path, &source)
            .into_iter()
            .skip(start)
            .take(64)
        {
            writeln!(
                output,
                "Line {{ number: {}, text: String::from({:?}), spans: vec![",
                line.number, line.text
            )
            .unwrap();
            for span in line.spans {
                writeln!(
                    output,
                    "Span {{ bytes: {:?}, kind: Kind::{:?} }},",
                    span.bytes, span.kind
                )
                .unwrap();
            }
            writeln!(output, "] }},").unwrap();
        }
        writeln!(output, "],").unwrap();
    }
    writeln!(output, "] }}").unwrap();
}
