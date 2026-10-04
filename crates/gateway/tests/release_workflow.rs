//! What the release workflow must keep: the console is built before the
//! binary that embeds it, and nothing is published without a tag.

use std::path::PathBuf;

fn workflow() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/release.yml");
    std::fs::read_to_string(&path).expect("the release workflow is in the repository")
}

/// The text of each job, by name: from its `  name:` line to the next job.
fn jobs(text: &str) -> Vec<(String, String)> {
    let start = text
        .lines()
        .position(|l| l == "jobs:")
        .expect("a jobs section");
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines().skip(start + 1) {
        let is_job = line.starts_with("  ")
            && !line.starts_with("   ")
            && line.trim_end().ends_with(':')
            && !line.trim_start().starts_with('#');
        if is_job {
            out.push((line.trim().trim_end_matches(':').to_string(), String::new()));
        } else if let Some((_, body)) = out.last_mut() {
            body.push_str(line);
            body.push('\n');
        }
    }
    out
}

fn job<'a>(jobs: &'a [(String, String)], name: &str) -> &'a str {
    &jobs
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("no job named {name}"))
        .1
}

/// A job that publishes runs for a push of a version tag and for nothing
/// else: a manual run on a tag ref is `workflow_dispatch`, and publishes
/// nothing.
const PUBLISH_ONLY: &str =
    "if: github.event_name == 'push' && startsWith(github.ref, 'refs/tags/v')";

#[test]
fn it_runs_on_a_version_tag_and_by_hand_and_nothing_else() {
    let text = workflow();
    let on = &text[text.find("\non:").expect("an on: section")..text.find("\njobs:").unwrap()];
    assert!(
        on.contains("push:") && on.contains("tags:") && on.contains("\"v*\""),
        "{on}"
    );
    assert!(on.contains("workflow_dispatch:"), "{on}");
    // Not on a branch, not on a pull request.
    assert!(!on.contains("branches:"), "{on}");
    assert!(!on.contains("pull_request"), "{on}");
}

#[test]
fn the_console_is_built_before_the_binary_that_embeds_it() {
    let text = workflow();
    let jobs = jobs(&text);
    let console = job(&jobs, "console");
    assert!(
        console.contains("pnpm --dir ui install --frozen-lockfile"),
        "{console}"
    );
    assert!(console.contains("pnpm --dir ui build"), "{console}");
    assert!(console.contains("actions/upload-artifact"), "{console}");

    let build = job(&jobs, "build");
    assert!(
        build.contains("needs: console"),
        "the binaries do not wait for the console"
    );
    let fetch = build
        .find("actions/download-artifact")
        .expect("the binaries take the console");
    let compile = build
        .find("cargo build --release --locked -p ultrafast-gateway")
        .expect("a release build");
    assert!(fetch < compile, "the console is fetched after the build");
    assert!(
        build.contains("ui/dist"),
        "the console goes where the gateway reads it"
    );
}

#[test]
fn it_builds_every_target_of_the_plan() {
    let text = workflow();
    let build = job(&jobs(&text), "build").to_string();
    for target in [
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-musl",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
    ] {
        assert!(build.contains(target), "no build for {target}");
    }
    // Each archive has a checksum.
    assert!(build.contains("sha256"), "no checksum");
}

#[test]
fn nothing_is_published_without_a_tag() {
    let text = workflow();
    let jobs = jobs(&text);
    // The jobs that publish are for the push of a tag only, never for a
    // manual run, even one on a tag ref.
    for name in ["release", "docker-publish"] {
        let body = job(&jobs, name);
        assert!(
            body.contains(PUBLISH_ONLY),
            "{name} is not for the push of a tag only:\n{body}"
        );
    }
    let release = job(&jobs, "release");
    assert!(
        release.contains("--draft"),
        "a release is a draft until a person publishes it"
    );
    assert!(release.contains("contents: write"), "{release}");
    // The jobs that can run by hand have no way to write a release or a package.
    for name in ["console", "build"] {
        let body = job(&jobs, name);
        assert!(
            !body.contains("contents: write"),
            "{name} can write a release"
        );
        assert!(
            !body.contains("packages: write"),
            "{name} can write a package"
        );
        assert!(!body.contains("gh release"), "{name} makes a release");
        assert!(!body.contains("push: true"), "{name} pushes an image");
    }
    // The default permissions are read only.
    let top = &text[..text.find("\njobs:").unwrap()];
    assert!(top.contains("permissions:\n  contents: read"), "{top}");
    // The image goes to ghcr only, with no other registry login.
    let docker = job(&jobs, "docker-publish");
    assert!(docker.contains("ghcr.io"), "{docker}");
    assert!(docker.contains("packages: write"), "{docker}");
    assert!(
        !text.to_lowercase().contains("dockerhub"),
        "a second registry"
    );
}

#[test]
fn the_crates_are_not_published_yet() {
    let text = workflow();
    assert!(
        !text.contains("cargo publish"),
        "cargo publish is for later"
    );
    assert!(
        !text.contains("CARGO_REGISTRY_TOKEN"),
        "a crates.io token is for later"
    );
    assert!(
        !text.contains("crates.io") || text.contains("LATER"),
        "crates.io is for later"
    );
}

#[test]
fn the_tag_must_name_the_version_of_the_workspace_before_anything_is_built() {
    let text = workflow();
    let jobs = jobs(&text);
    let version = job(&jobs, "version");
    assert!(
        version.contains("Cargo.toml"),
        "the version of the tag is not checked"
    );
    assert!(version.contains("GITHUB_REF_NAME"), "{version}");
    // The check is a step of a first job, only for a push (a manual run has
    // no tag to check), and the console and so the builds wait for it.
    assert!(
        version.contains("if: github.event_name == 'push'"),
        "{version}"
    );
    assert!(
        !version.contains("needs:"),
        "the check is not the first job: {version}"
    );
    assert!(
        job(&jobs, "console").contains("needs: version"),
        "the console does not wait for the version check"
    );
    assert!(
        job(&jobs, "build").contains("needs: console"),
        "the builds do not wait, through the console, for the check"
    );
    assert!(
        !job(&jobs, "release").contains("Cargo.toml"),
        "the check is still after the builds"
    );
}
