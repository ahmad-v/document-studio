# Dependencies

Cargo manifests list the exact direct requirements. The committed lockfile records the resolved versions and registry checksums. Builds use Rust 1.88.0 from `rust-toolchain.toml`. The CI checkout action is pinned to its upstream v4.2.2 commit.

A preliminary RustSec review used advisory database revision `c9f6c91a44dbad9d71742329e9df061279f9c4be`. The pinned Localwright revision uses Iced’s Tiny Skia software renderer and removes the affected `lru 0.12.5` dependency path (RUSTSEC-2026-0002 and RUSTSEC-2026-0253). Full license review, SBOM and native dependency review remain open.

The weekly/manual `security.yml` workflow runs pinned `cargo-audit` 0.23.0 against the committed lockfile and generates an all-features CycloneDX 1.5 SBOM with `cargo-cyclonedx` 0.5.10. It also creates a transitive Cargo license metadata inventory and fails if a dependency declares neither a license expression nor a license file. This inventory supports review; it does not determine which side of an upstream license alternative applies, approve redistribution, inspect linked native libraries, or replace human legal review. The audit and inventory are separate jobs so advisory findings do not suppress the inventory artifact.

The document stack uses lopdf 0.42.0 with optional features disabled and ZIP 9.0.5. These pins address advisories in the previous parser candidates. The updated ZIP dependency requires Rust 1.88. PDF import still needs process isolation and separate active-content review.
