// SPDX-License-Identifier: BSD-3-Clause
//! Command-line entry point for the host-only thistle-tk UI compiler.

use std::path::PathBuf;

use thistle_tk_ui_compiler::{compile_files_to_path, CompileOptions};

fn main() {
    if let Err(err) = run() {
        eprintln!("thistle-tk-ui-compiler: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut markup = None;
    let mut css = None;
    let mut out = None;
    let mut struct_name = None;
    let mut fn_name = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--markup" => markup = Some(next_value(&mut args, "--markup")?),
            "--css" => css = Some(next_value(&mut args, "--css")?),
            "--out" => out = Some(next_value(&mut args, "--out")?),
            "--struct" => struct_name = Some(next_value(&mut args, "--struct")?),
            "--fn" => fn_name = Some(next_value(&mut args, "--fn")?),
            "-h" | "--help" => {
                print_usage();
                return Ok(());
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }

    let markup = markup.ok_or_else(|| "missing --markup <path>".to_owned())?;
    let css = css.ok_or_else(|| "missing --css <path>".to_owned())?;
    let out = out.ok_or_else(|| "missing --out <path>".to_owned())?;
    let struct_name = struct_name.unwrap_or_else(|| "GeneratedUi".to_owned());
    let fn_name = fn_name.unwrap_or_else(|| "build_ui".to_owned());

    let options = CompileOptions::new(struct_name, fn_name);
    compile_files_to_path(PathBuf::from(markup), PathBuf::from(css), PathBuf::from(out), &options)
        .map_err(|err| err.to_string())
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value after {flag}"))
}

fn print_usage() {
    println!(
        "Usage: thistle-tk-ui-compiler --markup <ui.xml> --css <ui.css> --out <generated.rs> [--struct <Name>] [--fn <name>]"
    );
}
