# thistle-tk-ui-compiler

Host-only declarative UI compiler for `thistle-tk`.

This crate parses strict XML-like markup plus a deliberately small CSS subset and emits Rust source that constructs ordinary `thistle_tk` widget trees. It is for build machines only. Parser dependencies must not be pulled into firmware.

## CLI

```sh
cargo run --manifest-path compiler/Cargo.toml -- \
  --markup compiler/fixtures/weather.ui.xml \
  --css compiler/fixtures/weather.css \
  --out /tmp/weather_ui.rs \
  --struct WeatherUi \
  --fn build_weather
```

## build.rs

Use the crate as a build dependency, then compile into `OUT_DIR`:

```rust
use thistle_tk_ui_compiler::{compile_for_build_script, CompileOptions};

fn main() {
    compile_for_build_script(
        "ui/weather.ui.xml",
        "ui/weather.css",
        "weather_ui.rs",
        &CompileOptions::new("WeatherUi", "build_weather"),
    )
    .expect("compile UI");
}
```

In app code:

```rust
include!(concat!(env!("OUT_DIR"), "/weather_ui.rs"));
```
