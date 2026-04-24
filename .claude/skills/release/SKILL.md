---
name: release
description: Use when preparing a new crate release, bumping versions, or tagging a release
---

# Release

Guide for creating a new release of cryochamber.

## Step 1: Determine Version Bump

Compare against the last release tag:

```bash
git tag -l 'v0.*' | sort -V          # find latest tag
git log <last-tag>..HEAD --oneline    # review commits
git diff <last-tag>..HEAD --stat      # review scope
```

Apply semver for 0.x (pre-1.0):
- **Patch** (0.x.Y) — bug fixes, docs, CI only
- **Minor** (0.X.0) — new features, new commands, new public API
- **Major** — reserved for post-1.0

## Step 2: Verify Clean State

```bash
make check
```

`make check` runs fmt-check + clippy + test in sequence. All must pass with zero warnings before proceeding.

Also confirm:
- `git status` is clean
- On `main` branch, up to date with origin

## Step 3: Release

```bash
make release V=x.y.z
```

This target bumps `version` in `Cargo.toml`, runs `cargo check`, commits as `release: vX.Y.Z`, tags `vX.Y.Z`, and pushes `main` plus tags. CI publishes the crate to crates.io.
