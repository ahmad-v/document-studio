# Packaging preparation

The target matrix retains all 15 combinations. The installable formats in this matrix still require device validation. This is an unsigned development payload. The experimental driver checks native host and executable architecture, requires Cargo.lock and refuses packaging until the maintainer approves the candidate application identity in config.json. It does not itself prove that a lockfile, dependencies or output are safe.

| Recipe | Status |
|---|---|
| Windows portable ZIP x64/ARM64 | Driver present; unverified |
| Windows MSI x64/ARM64 | WiX source candidate; unverified |
| Windows offline setup EXE x64/ARM64 | Burn template only; native tool validation blocked |
| Linux DEB/RPM x64/ARM64 | Driver present; dependencies and distro QA pending |
| Linux AppImage x64/ARM64 | Driver present; reviewed pinned tool/runtime and ABI testing required |
| Linux Flatpak x64/ARM64 | Manifest template; vendor source and pinned runtime required |
| macOS ARM64 DMG | APP/DMG driver; ad hoc signing, not notarized, unverified |

`distribution.yml` is manually triggered. Portable preview mode builds an optimized, unsigned preview on each of the five native targets, extracts it, verifies CLI help and uploads it as a short-lived Actions artifact. Configured-package mode derives the 15 target-format pairs directly from `packaging/config.json`; it is held until the provisional application identity and maintainer are approved. The matrix is a preparation path, not evidence that each package can currently be built or installed. RPM, WiX, AppImage and Flatpak work needs reviewed tooling or implementation before those formats can pass. No public release job exists and none was introduced. Flatpak template omits network permission and grants no blanket filesystem access; portal integration is missing. Standard payloads include no private engine or model. No unavailable functionality is advertised in portable previews.

Windows MSI and EXE must use the same product identity and payload, separately verify repair/upgrade/uninstall and architecture, and embed all payloads. Candidate application ID `org.localwright.DocumentStudio` is not a verified owner namespace or trademark. Product GUID generated deterministically as a proposed identifier, not an existing registered product. Review before fixing identity. Linux and macOS require real-device validation. No signing keys, service accounts or secrets are included.

## Portable CI previews
The Linux OCR development variant can bundle a reviewed private engine and execute the packaged fixture. See [OCR engine documentation](../docs/OCR_ENGINE.md).


Manually run the `Distribution preparation` workflow with `portable-preview` to create an unsigned archive on each native target. The workflow extracts every archive, runs CLI help from the extracted payload, and includes a SHA-256 checksum. Download the artifacts from the completed workflow run. These archives are for evaluation only. Linux may require system libraries, and macOS Gatekeeper may block an unsigned executable.
