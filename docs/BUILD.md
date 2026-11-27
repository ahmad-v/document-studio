# Building the alpha snapshot

Run these commands from the repository root. Rustup selects the toolchain in `rust-toolchain.toml`. The first Cargo build fetches dependencies, including the exact Localwright Git revision recorded in `Cargo.toml` and `integration/localwright.json`.

```sh
cargo fmt --all -- --check
cargo check --locked --all-targets
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

On Linux, install the X11 and Wayland development libraries listed in the check workflow. Run `cargo run --locked` from a graphical session to launch the app. Use `cargo run --locked --no-default-features -- --cli --help` for the headless interface.

Update both Localwright revision records and regenerate `Cargo.lock` when advancing the shared dependency.

## GitHub Actions

- `check.yml` is the stable Linux x64 required check for pull requests and pushes to `main`. It runs formatting, locked metadata/build/tests and strict Clippy for source changes, plus JSON, Python syntax, and whitespace checks. Documentation-only changes skip Cargo compilation while keeping the same check context successful.
- `integration.yml` runs on demand or when a pull request receives the `integration` label. It exercises the default GUI build, a bounded Xvfb startup smoke, the headless CLI, regression fixtures, and compilation/tests of the optional OCR adapter. The adapter check does not prove an OCR engine pack is installed.
- `native.yml` runs on demand or with the `platform-validation` pull-request label. It verifies Windows x64/ARM64, Linux x64/ARM64 and Apple Silicon macOS on matching native runners.
- `distribution.yml` is manually triggered. Its default mode builds and smoke-tests unsigned portable previews. Its configured package mode derives all 15 target-format pairs from `packaging/config.json` and remains held until the provisional package identity and maintainer are approved. Several packaging tools and device checks remain incomplete.
- `security.yml` runs weekly or on demand. It audits the committed lockfile, emits a CycloneDX SBOM, and archives Cargo license metadata. The metadata inventory is not legal license clearance or a native binary linkage audit.

All workflows use read-only repository permissions, pinned actions, and run cancellation for obsolete development/integration checks. Cargo caches are keyed by platform, toolchain, build profile/features and the lockfile. Manual native/distribution runs do not cancel each other.
