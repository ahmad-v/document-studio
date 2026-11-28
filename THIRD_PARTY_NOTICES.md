# Third-party notices (incomplete)

No third-party engine binaries, models, icons or fonts are checked into this source snapshot. The manual, unsigned Linux x64 Pandoc development workflow assembles a separate internal bundle containing a verified third-party engine binary. Cargo dependencies are declared in manifests. Their full transitive inventory, licenses, notices, advisories and redistribution obligations are unresolved until dependency resolution and review. Do not publish binaries before completing these checks. See docs/DEPENDENCIES.md.

The manual unsigned Linux x64 Pandoc development bundle downloads the exact Pandoc 3.14 official release asset, verifies its pinned archive SHA-256, and includes the upstream `COPYING.md` and `COPYRIGHT` notices. Pandoc is GPL-2.0-or-later. The 169,845,072-byte executable is an optional private engine; this route is not included in the standard application build. Other target architectures are not yet packaged or verified.

Portable CI previews include `DEPENDENCY_LICENSES.json` and collected Cargo dependency license and notice texts under `third-party/`. The inventory includes normal and build dependency edges. It is not a binary linkage audit, a choice among upstream license alternatives, or a review of separately embedded assets and platform libraries. Empty notice lists require follow-up against the upstream source.
