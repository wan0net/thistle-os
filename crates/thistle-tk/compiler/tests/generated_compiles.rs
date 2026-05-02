// SPDX-License-Identifier: BSD-3-Clause

use std::process::Command;

use thistle_tk_ui_compiler::{compile_to_rust, CompileOptions};

const MARKUP: &str = include_str!("../fixtures/weather.ui.xml");
const CSS: &str = include_str!("../fixtures/weather.css");

#[test]
fn generated_direct_tree_rust_type_checks_against_local_toolkit() {
    let generated = compile_to_rust(MARKUP, CSS, &CompileOptions::new("WeatherUi", "build_weather"))
        .expect("compile fixture");

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let crate_dir = std::env::temp_dir().join(format!("thistle-tk-generated-check-{stamp}"));
    let src_dir = crate_dir.join("src");
    std::fs::create_dir_all(&src_dir).expect("create temp crate");

    let compiler_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let tk_root = compiler_dir
        .parent()
        .expect("compiler crate should live inside thistle-tk");
    let tk_path = escape_toml_string(&tk_root.display().to_string());

    std::fs::write(
        crate_dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "generated-ui-check"
version = "0.0.0"
edition = "2021"

[dependencies]
thistle-tk = {{ path = "{tk_path}" }}
"#
        ),
    )
    .expect("write Cargo.toml");
    std::fs::write(src_dir.join("generated_ui.rs"), generated).expect("write generated code");
    std::fs::write(
        src_dir.join("lib.rs"),
        r#"mod generated_ui;

pub use generated_ui::*;

pub fn smoke() {
    let (_tree, ui) = build_weather();
    let _ = ui.refresh;
}
"#,
    )
    .expect("write lib.rs");

    let output = Command::new("cargo")
        .arg("check")
        .arg("--offline")
        .env("RUSTFLAGS", "-D warnings")
        .current_dir(&crate_dir)
        .output()
        .expect("run cargo check");

    let _ = std::fs::remove_dir_all(&crate_dir);

    assert!(
        output.status.success(),
        "generated code did not type-check\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn escape_toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
