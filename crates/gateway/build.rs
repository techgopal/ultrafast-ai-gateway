//! Puts the console where `src/web.rs` embeds it from: `$OUT_DIR/console`.
//!
//! The console is built by `pnpm --dir ui build` into `ui/dist`. This script
//! only copies that output. It never runs Node, so the gateway builds on a
//! machine without it; the binary then serves a page that says how to build
//! the console.

use std::path::{Path, PathBuf};
use std::{env, fs, io};

const PLACEHOLDER: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Ultrafast Gateway</title>
  </head>
  <body>
    <h1>Ultrafast Gateway</h1>
    <p>The gateway is running, but the console was not built into this binary.</p>
    <p>To build the console and the gateway together, run in the repository:</p>
    <pre>pnpm --dir ui install --frozen-lockfile &amp;&amp; pnpm --dir ui build &amp;&amp; cargo build --release -p ultrafast-gateway</pre>
    <p>The API at /api and /v1 works without the console.</p>
  </body>
</html>
"#;

fn main() -> io::Result<()> {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("set by cargo"));
    let dist = manifest.join("../../ui/dist");
    let console = out.join("console");

    println!("cargo:rerun-if-changed=build.rs");
    // Cargo takes a watched path that is missing as changed, so watching a
    // missing `ui/dist` compiles the gateway again on every build. Without a
    // console build, `ui` is watched instead: a build of the console changes
    // nothing outside `ui/dist` but the directory `ui` itself, so nothing
    // narrower would notice it. Cargo then reads all of `ui`, `node_modules`
    // included, which takes about a second until the console is built.
    let ui = manifest.join("../../ui");
    if dist.exists() {
        println!("cargo:rerun-if-changed={}", dist.display());
    } else if ui.exists() {
        println!("cargo:rerun-if-changed={}", ui.display());
    }
    println!("cargo:rustc-check-cfg=cfg(console_built)");

    // Nothing of an earlier build may stay in the binary.
    match fs::remove_dir_all(&console) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    fs::create_dir_all(&console)?;
    // The page served when the console cannot be.
    fs::write(out.join("console_placeholder.html"), PLACEHOLDER)?;

    if dist.join("index.html").is_file() {
        copy_dir(&dist, &console)?;
        println!("cargo:rustc-cfg=console_built");
    } else {
        fs::write(console.join("index.html"), PLACEHOLDER)?;
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        // Of the entry itself: a symbolic link is neither, so it is skipped
        // and nothing outside the build gets into the binary.
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
