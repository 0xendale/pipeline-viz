#!/usr/bin/env bash
#
# Proves that the published .crate is what a consumer actually needs: the
# dashboard bytes are inside it, the development-only trees are not, and a
# project depending on it compiles and runs with no Node.js anywhere in sight.
#
# The headline claim of this crate is "add a dependency, get a dashboard". This
# script is what stops that claim from silently becoming false.
#
# Usage:
#   verify-package.sh --build [--require-file PATH]... [--no-allow-dirty]
#   verify-package.sh --verify-archive FILE.crate [--require-file PATH]...
#
#   --build            Package from this checkout and run every check against
#                      the resulting archive.
#   --verify-archive   Check an archive that already exists, and nothing else.
#   --require-file     Additional archive member that must be present, relative
#                      to the archive root. Repeatable.
#   --no-allow-dirty   Package without --allow-dirty. For the release gate,
#                      which runs from a clean committed tree.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Members every published archive must carry. `assets/index.html` is the whole
# point: without it a consumer needs the template sources, and the crate stops
# being a drop-in.
BASE_REQUIRED="Cargo.toml build.rs src/lib.rs assets/index.html README.md LICENSE"

# Development sources. Packaging them would bloat the crate and imply that a
# consumer is expected to build them.
FORBIDDEN_DIRS="templates packages"

EXTRA_REQUIRED=()
MODE=""
ARCHIVE=""
ALLOW_DIRTY=1
WORK_DIR=""

cleanup() {
    if [ -n "$WORK_DIR" ] && [ -d "$WORK_DIR" ]; then
        rm -rf "$WORK_DIR"
    fi
}
trap cleanup EXIT INT TERM

step() {
    printf '\n== %s\n' "$*"
}

fail() {
    printf 'verify-package: %s\n' "$*" >&2
    exit 1
}

usage() {
    sed -n '3,24p' "$0" | sed 's/^# \{0,1\}//'
}

while [ $# -gt 0 ]; do
    case "$1" in
        --build)
            MODE="build"
            shift
            ;;
        --verify-archive)
            [ $# -ge 2 ] || fail "--verify-archive needs a path"
            MODE="verify-archive"
            ARCHIVE="$2"
            shift 2
            ;;
        --require-file)
            [ $# -ge 2 ] || fail "--require-file needs a path"
            EXTRA_REQUIRED[${#EXTRA_REQUIRED[@]}]="$2"
            shift 2
            ;;
        --no-allow-dirty)
            ALLOW_DIRTY=0
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            usage >&2
            fail "unknown argument: $1"
            ;;
    esac
done

[ -n "$MODE" ] || { usage >&2; fail "one of --build or --verify-archive is required"; }

require_tool() {
    command -v "$1" >/dev/null 2>&1 || fail "$1 is required but was not found on PATH"
}

# ---------------------------------------------------------------------------
# Archive contract
# ---------------------------------------------------------------------------

verify_archive() {
    archive="$1"
    shift

    [ -f "$archive" ] || fail "archive not found: $archive"

    listing="$(tar -tzf "$archive")" || fail "archive is not readable as a gzip tarball: $archive"
    root="$(printf '%s\n' "$listing" | head -n 1 | cut -d/ -f1)"
    [ -n "$root" ] || fail "archive has no root directory: $archive"

    for member in $BASE_REQUIRED "$@"; do
        if ! printf '%s\n' "$listing" | grep -q -x "$root/$member"; then
            fail "archive is missing required file: $member (in $archive)"
        fi
    done

    for forbidden in $FORBIDDEN_DIRS; do
        if printf '%s\n' "$listing" | grep -q "^$root/$forbidden/"; then
            fail "archive contains development-only tree: $forbidden/ (in $archive)"
        fi
    done

    if printf '%s\n' "$listing" | grep -q "node_modules/"; then
        fail "archive contains node_modules/ (in $archive)"
    fi

    printf 'archive contract satisfied: %s\n' "$archive"
    printf '  root: %s\n' "$root"
    printf '  members: %s\n' "$(printf '%s\n' "$listing" | grep -c .)"
}

if [ "$MODE" = "verify-archive" ]; then
    verify_archive "$ARCHIVE" ${EXTRA_REQUIRED[@]+"${EXTRA_REQUIRED[@]}"}
    exit 0
fi

# ---------------------------------------------------------------------------
# Build mode
# ---------------------------------------------------------------------------

require_tool cargo
require_tool npm
require_tool tar
require_tool git

cd "$REPO_ROOT"

TRACKED_BEFORE="$(git status --porcelain)"

step "Installing workspace dependencies (npm ci)"
npm ci

step "Rebuilding dashboard assets from the templates"
# Removing assets/ and touching build.rs forces a rebuild, so a stale bundle
# from an earlier tree state can never reach the archive.
rm -rf "$REPO_ROOT/assets"
touch "$REPO_ROOT/build.rs"
cargo check --features viz,macros >/dev/null
[ -f "$REPO_ROOT/assets/index.html" ] || fail "build.rs produced no assets/index.html"

step "Generating a lockfile for --locked packaging"
# Cargo.lock is gitignored: a library pins nothing for its consumers. It exists
# only so this verification is reproducible.
cargo generate-lockfile

step "Packaging"
if [ "$ALLOW_DIRTY" -eq 1 ]; then
    cargo package --locked --allow-dirty
else
    cargo package --locked
fi

# `cargo pkgid` prints one of `<url>#<version>` or `<url>#<name>@<version>`,
# so cut to the last `#` and then to the last `@`. Reading the version out of
# `cargo metadata` instead would pick whichever workspace member came first.
VERSION="$(cargo pkgid -p pipeline-viz)"
VERSION="${VERSION##*#}"
VERSION="${VERSION##*@}"
case "$VERSION" in
    [0-9]*) ;;
    *) fail "could not determine the package version (cargo pkgid gave '$VERSION')" ;;
esac

ARCHIVE="$REPO_ROOT/target/package/pipeline-viz-$VERSION.crate"
[ -f "$ARCHIVE" ] || fail "expected archive not produced: $ARCHIVE"
printf 'packaged version: %s\n' "$VERSION"

WORK_DIR="$(mktemp -d)"

step "Verifying archive contents"
verify_archive "$ARCHIVE" ${EXTRA_REQUIRED[@]+"${EXTRA_REQUIRED[@]}"}

step "Verifying that a damaged archive is rejected"
# The verifier is only worth anything if it fails when it should. Rebuild the
# archive without the dashboard entry point and require a readable complaint.
DAMAGED_DIR="$WORK_DIR/damaged"
mkdir -p "$DAMAGED_DIR"
tar -xzf "$ARCHIVE" -C "$DAMAGED_DIR"
rm -f "$DAMAGED_DIR/pipeline-viz-$VERSION/assets/index.html"
tar -czf "$WORK_DIR/damaged.crate" -C "$DAMAGED_DIR" "pipeline-viz-$VERSION"

if damaged_output="$(verify_archive "$WORK_DIR/damaged.crate" 2>&1)"; then
    fail "an archive without assets/index.html was accepted"
fi
case "$damaged_output" in
    *"missing required file: assets/index.html"*)
        printf 'damaged archive rejected: %s\n' "$damaged_output"
        ;;
    *)
        fail "damaged archive was rejected, but not readably: $damaged_output"
        ;;
esac

# ---------------------------------------------------------------------------
# Consumers
# ---------------------------------------------------------------------------

step "Unpacking the archive for consumer builds"
CRATE_DIR="$WORK_DIR/unpacked"
mkdir -p "$CRATE_DIR"
tar -xzf "$ARCHIVE" -C "$CRATE_DIR"
UNPACKED="$CRATE_DIR/pipeline-viz-$VERSION"
[ -d "$UNPACKED" ] || fail "archive did not unpack to pipeline-viz-$VERSION"
[ ! -d "$UNPACKED/templates" ] || fail "unpacked crate still contains templates/"

# Node.js must never be reached during a consumer build. Rather than trusting
# that, put tools on PATH that record being called and then fail.
MARKER_DIR="$WORK_DIR/markers"
FAKE_BIN="$WORK_DIR/fake-bin"
mkdir -p "$MARKER_DIR" "$FAKE_BIN"
for tool in npm node npx; do
    cat > "$FAKE_BIN/$tool" <<FAKE
#!/bin/sh
touch "$MARKER_DIR/$tool-was-called"
echo "verify-package: a packaged consumer must never invoke $tool" >&2
exit 1
FAKE
    chmod +x "$FAKE_BIN/$tool"
done

# One target directory for every consumer, so dependencies compile once.
CONSUMER_TARGET="$REPO_ROOT/target/package-verify"

consumer_manifest() {
    name="$1"
    features="$2"
    extra="$3"

    {
        printf '[package]\nname = "%s"\nversion = "0.0.0"\nedition = "2021"\n\n' "$name"
        # An empty table keeps the consumer out of any surrounding workspace.
        printf '[workspace]\n\n'
        printf '[dependencies]\n'
        if [ -z "$features" ]; then
            printf 'pipeline-viz = { path = "%s" }\n' "$UNPACKED"
        else
            printf 'pipeline-viz = { path = "%s", features = [%s] }\n' "$UNPACKED" "$features"
        fi
        printf '%s' "$extra"
    }
}

build_consumer() {
    name="$1"
    features="$2"
    extra_deps="$3"
    source="$4"

    dir="$WORK_DIR/$name"
    mkdir -p "$dir/src"
    consumer_manifest "$name" "$features" "$extra_deps" > "$dir/Cargo.toml"
    printf '%s' "$source" > "$dir/src/main.rs"

    PATH="$FAKE_BIN:$PATH" cargo build \
        --manifest-path "$dir/Cargo.toml" \
        --target-dir "$CONSUMER_TARGET" \
        || fail "consumer $name failed to build"

    printf '%s\n' "$dir"
}

step "Building a consumer with default features (viz off)"
build_consumer consumer-default "" "" 'fn main() {
    let tracker = pipeline_viz::PipelineTracker::builder()
        .bind_port(0)
        .start_background()
        .expect("compiled out, never fails");
    let mut job = tracker.job("committer").id("block_42").start();
    job.hold("Waiting for finality");
    assert_eq!(tracker.snapshot().jobs.len(), 0, "viz off records nothing");
    assert!(!tracker.is_serving(), "viz off opens no port");
    println!("default consumer ok");
}' >/dev/null

step "Building a consumer with feature viz"
VIZ_CONSUMER="$(build_consumer consumer-viz '"viz"' 'tokio = { version = "1", features = ["rt", "time", "macros"] }
' 'use std::time::Duration;

use pipeline_viz::{JobPhase, PipelineTracker};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let tracker = PipelineTracker::builder()
        .bind_port(0)
        .tick(Duration::from_millis(10))
        .start_background()
        .expect("started inside a runtime");

    let mut job = tracker.job("committer").id("block_42").job_type("Block").start();
    job.hold("Waiting for finality (2/12 confirmations)");

    // Let the collector drain the channel before asking it anything.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let snapshot = tracker.snapshot();
    let found = snapshot
        .jobs
        .iter()
        .find(|state| state.job_id == "block_42")
        .expect("block_42 is in the packaged consumers snapshot");

    match &found.phase {
        JobPhase::Held { reason } => {
            assert_eq!(reason, "Waiting for finality (2/12 confirmations)");
            println!("viz consumer ok: block_42 held, reason = {reason}");
        }
        other => panic!("expected block_42 to be held, got {other:?}"),
    }

    assert_eq!(tracker.dropped_events(), 0, "nothing should be dropped here");
}' | tail -n 1)"

step "Building a consumer with features viz and macros"
build_consumer consumer-viz-macros '"viz", "macros"' 'tokio = { version = "1", features = ["rt", "time", "macros"] }
' 'use pipeline_viz::{track_job, track_node};

#[track_node(id = "committer", kind = Sink, name = "Database Committer", inputs = ["indexer"])]
#[track_job(node = "committer", id = number, job_type = "Block")]
async fn commit(number: u64) -> u64 {
    number
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // No tracker is installed, so the generated calls do nothing. What is being
    // proven here is that the macro surface resolves from the published crate.
    assert_eq!(commit(42).await, 42);
    println!("viz+macros consumer ok");
}' >/dev/null

step "Running the packaged consumer"
"$CONSUMER_TARGET/debug/consumer-viz" || fail "the packaged consumer did not report a held block_42"

step "Checking that no consumer reached for Node.js"
for tool in npm node npx; do
    [ ! -f "$MARKER_DIR/$tool-was-called" ] || fail "a consumer build invoked $tool"
done
printf 'no marker files were written: consumers need no Node.js toolchain\n'

step "Checking that nothing tracked was modified"
TRACKED_AFTER="$(git status --porcelain)"
if [ "$TRACKED_BEFORE" != "$TRACKED_AFTER" ]; then
    printf 'before:\n%s\nafter:\n%s\n' "$TRACKED_BEFORE" "$TRACKED_AFTER" >&2
    fail "verification changed tracked files"
fi

printf '\nverify-package: all checks passed for %s\n' "$ARCHIVE"
