//! Puts the dashboard bundle where `rust-embed` can find it.
//!
//! Two situations, one output directory:
//!
//! * **Working in this repository.** `templates/three-d` is present, so the
//!   Vite build runs and its `dist/` is copied into `assets/`.
//! * **Consuming the published crate.** `templates/` was never packaged, but
//!   `assets/` was. Nothing to build; the bytes are already there. This is what
//!   keeps Node.js off a user's machine, and it is also how docs.rs builds us.
//!
//! With feature `viz` off there is no server to serve anything, so this does
//! nothing at all.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where `rust-embed` reads from. Gitignored; see the `include` list in
/// Cargo.toml for how it reaches the published crate.
const ASSETS_DIR: &str = "assets";

/// The template whose build output is embedded.
const TEMPLATE: &str = "templates/three-d";

/// Escape hatch for a working tree without Node.js. Reuses whatever is already
/// in `assets/` rather than failing the build.
const SKIP_VAR: &str = "PIPELINE_VIZ_SKIP_UI_BUILD";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed={SKIP_VAR}");

    // Build scripts do not see `cfg(feature = ...)`; Cargo passes features in
    // the environment instead.
    if env::var_os("CARGO_FEATURE_VIZ").is_none() {
        return;
    }

    let manifest_dir = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR").expect("Cargo always sets CARGO_MANIFEST_DIR"),
    );
    let assets = manifest_dir.join(ASSETS_DIR);
    let template = manifest_dir.join(TEMPLATE);

    for path in [
        "src",
        "public",
        "index.html",
        "package.json",
        "vite.config.ts",
    ] {
        let watched = template.join(path);
        if watched.exists() {
            println!("cargo:rerun-if-changed={}", watched.display());
        }
    }

    let have_prebuilt = assets.join("index.html").is_file();

    // The published crate: no template to build from, only the packaged bytes.
    if !template.join("package.json").is_file() {
        assert!(
            have_prebuilt,
            "feature \"viz\" is on, but neither {TEMPLATE} nor prebuilt {ASSETS_DIR}/index.html \
             is present. A published pipeline-viz always ships {ASSETS_DIR}/; if you are building \
             from a git checkout, fetch the templates/ directory."
        );
        return;
    }

    if env::var_os(SKIP_VAR).is_some() {
        assert!(
            have_prebuilt,
            "{SKIP_VAR} is set but {ASSETS_DIR}/index.html does not exist yet. Run \
             `npm run three-d:build` once, or unset {SKIP_VAR}."
        );
        println!("cargo:warning=pipeline-viz: {SKIP_VAR} set, reusing existing {ASSETS_DIR}/");
        return;
    }

    if let Err(error) = build_template(&manifest_dir) {
        // A missing toolchain should not be a hard stop when usable assets are
        // already on disk — it only means they may be stale.
        assert!(
            have_prebuilt,
            "pipeline-viz could not build the dashboard ({error}). Install Node.js and run \
             `npm install`, or set {SKIP_VAR}=1 once {ASSETS_DIR}/ has been populated."
        );
        println!("cargo:warning=pipeline-viz: dashboard build failed ({error}); reusing existing {ASSETS_DIR}/");
        return;
    }

    let dist = template.join("dist");
    assert!(
        dist.join("index.html").is_file(),
        "the dashboard build reported success but produced no {}/index.html",
        dist.display()
    );

    sync_dir(&dist, &assets).expect("copying the dashboard bundle into assets/");
}

/// Runs the Vite build for the embedded template through the workspace root, so
/// npm resolves the `@pipeline-viz/protocol` workspace dependency.
fn build_template(manifest_dir: &Path) -> io::Result<()> {
    if !manifest_dir.join("node_modules").is_dir() {
        run(manifest_dir, "npm", &["install", "--silent"])?;
    }
    run(
        manifest_dir,
        "npm",
        &[
            "run",
            "--silent",
            "build",
            "--workspace=@pipeline-viz/three-d",
        ],
    )
}

fn run(dir: &Path, program: &str, args: &[&str]) -> io::Result<()> {
    let output = Command::new(program).args(args).current_dir(dir).output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "`{program} {}` failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

/// Replaces `dest` with the contents of `src`.
///
/// Wholesale replacement rather than a merge: a stale hashed bundle left behind
/// from a previous build would otherwise be embedded forever.
fn sync_dir(src: &Path, dest: &Path) -> io::Result<()> {
    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    copy_dir(src, dest)
}

fn copy_dir(src: &Path, dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();

        // Editor and tooling droppings land in dist/ from time to time. They
        // must not become part of a published artifact.
        if name.to_string_lossy().starts_with('.') {
            continue;
        }

        let from = entry.path();
        let to = dest.join(&name);
        if entry.file_type()?.is_dir() {
            copy_dir(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}
