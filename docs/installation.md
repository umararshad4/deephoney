# Install DeepHoney on macOS

DeepHoney currently targets macOS 15 or newer. The local packaging command creates an app bundle and ZIP for the architecture of the supplied build. By default it uses ad-hoc signing. Optional Developer ID signing and notarization are supported when the publisher supplies an installed certificate and an existing Keychain profile. It does not produce a universal binary or an automatic updater. A ZIP created locally is not a published release installer.

## Build an app

Install current stable Rust (1.99.1 was validated for this pass), Apple's Command Line Tools (`xcode-select --install`), and Python 3.11 or newer. The native PDF/OCR helper is compiled with Swift during the Rust build and embedded in the executable; users of a completed app do not need Rust, Python, Swift, or a separate helper. Chrome is optional for JavaScript page reading, and model/search services still need configuration.

From the project root:

```sh
python3 scripts/macos/app.py package --output .scratch/macOS-release
```

This runs `cargo build --release --locked --package app --bin deephoney`, derives the version from the root Cargo manifest, and writes:

- `DeepHoney.app`: a native app with the executable, embedded document helper, embedded GPUI assets, and the project MIT license notice.
- `DeepHoney-<version>-macOS.zip`: the same app for transfer.
- `DeepHoney.manifest.json`: version, binary format, signing disclosure, and hashes of every bundle file.
- `SHA256SUMS`: SHA-256 checksums of the ZIP and manifest.

The output directory must not exist. For a development build that has already been compiled, use `--binary target/debug/deephoney` with a new output directory. That flag does not check whether the binary matches current source. Both commands support paths containing spaces. A release build can require substantial disk space; see [development](development.md).

The script applies an **ad-hoc signature**, then checks it. Ad-hoc signing protects local code integrity but does not identify a trusted publisher. The checksums also do not establish publisher identity. Distributed downloads may be blocked by Gatekeeper until a Developer ID signed and notarized release is available. Do not disable Gatekeeper or remove quarantine attributes globally.

## Optional Developer ID distribution

This path follows [Apple's notarization workflow](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution). It requires an available **Developer ID Application** certificate/private key in the publisher's Keychain, current Apple signing tools, and (for notarization) an existing `notarytool` Keychain profile. Developer Program credentials and signing certificates are not included in this project. Never put private keys, passwords, or API key contents into repository files or command arguments here.

To produce a hardened-runtime, timestamped Developer ID signature without uploading anything:

```sh
python3 scripts/macos/app.py package --output .scratch/macOS-signed \
  --sign-identity 'Developer ID Application: Your Publisher Name (YOURTEAMID)'
```

Replace the identity with its exact installed name and 10-character Team ID. Preflight refuses other certificate types or an unavailable identity. It signs the nested app executable first, then the enclosing bundle, without broad runtime exemptions. The manifest explicitly states **Developer ID; not notarized** for this command. That status does not satisfy the notarized installation gate.

**The next command explicitly uploads the packaged app to Apple's service** using the named existing Keychain profile. Run it only as the publisher when you are ready to submit that build:

```sh
python3 scripts/macos/app.py package --output .scratch/macOS-notarized \
  --sign-identity 'Developer ID Application: Your Publisher Name (YOURTEAMID)' \
  --notarize-profile 'your-existing-keychain-profile'
```

The script checks tool availability, waits up to 20 minutes for the submission, requires an `Accepted` result, staples the ticket, validates it and asks Gatekeeper to assess the app. It then produces the final ZIP and checksums **after** stapling. Rejection, timeout, missing credentials, failed signing, failed stapling or failed assessment stops packaging and leaves no completed output directory. A timeout can leave a submission processing at Apple; inspect it with `notarytool` before retrying. The manifest records the accepted submission ID, expected Team ID and observed signing state; it contains no credentials. See Apple's [workflow and log guidance](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow).

For distribution checks and installation that must refuse local/ad-hoc or unnotarized builds:

```sh
python3 scripts/macos/app.py verify .scratch/macOS-notarized/DeepHoney.app --require-notarized
python3 scripts/macos/app.py install .scratch/macOS-notarized/DeepHoney.app --require-notarized
```

The gate verifies the Apple Developer ID certificate chain, app identifier, expected Team ID, hardened runtime, secure timestamp, stapled ticket and Gatekeeper assessment; editing a manifest cannot replace those checks. The manifest and `SHA256SUMS` themselves are unauthenticated integrity records: obtain the artifact from a trusted publisher, and verify the expected Team ID through an independent trusted channel. A plain `verify`/`install` intentionally also accepts the default ad-hoc development build.

**No actual Developer ID or notarized build has been validated in this implementation pass.** The signed/notarized path has mocked command and failure checks; the real ad-hoc path has local signing/archive/install checks. Before publishing, validate a genuine signed build, review Apple's notary log for warnings, and test first launch, Keychain access, native OCR/helper execution and Chrome reading on a Gatekeeper-enabled Mac. A successful notarization is not proof of product quality, source freshness, or research correctness.

## Verify and install

Keep the manifest next to the unpacked app. Verify a ZIP before extracting it:

```sh
cd .scratch/macOS-release
shasum -a 256 -c SHA256SUMS
cd ../..
python3 scripts/macos/app.py verify .scratch/macOS-release/DeepHoney.app
python3 scripts/macos/app.py install .scratch/macOS-release/DeepHoney.app
open "$HOME/Applications/DeepHoney.app"
```

Installation defaults to `~/Applications`, needs no administrator access, and refuses an existing `DeepHoney.app`. The installer checks all bundle hashes, executable permissions, required project license notice, app metadata, and the claimed signature/notarization before and after copying. macOS `ditto` preserves resource forks and stapled-ticket metadata during the verified copy. It rejects symlink paths and system destinations. A custom user directory is supported:

```sh
python3 scripts/macos/app.py install .scratch/macOS-release/DeepHoney.app \
  --destination "$HOME/Applications/Research Tools"
```

To update, quit DeepHoney, move the previous app bundle to a backup location manually, and then install the new bundle. The installer never deletes or replaces an app, closes a running process, migrates data, or changes Keychain entries. Keep the old app until you have verified the new one. Do not run two versions against the same profile at once.

Sessions and settings remain in `~/Library/Application Support/DeepHoney`, unless `DEEPHONEY_DATA_DIR` selects another profile. API keys remain in the macOS Keychain. Removing or moving the `.app` does not remove this data. A changed ad-hoc signature may cause macOS to request Keychain access again; approve only an app you built or otherwise trust.

## Checks and limits

Run the focused packaging checks without touching the installed app or live profile:

```sh
python3 -m unittest discover -s scripts/macos -p 'test_*.py' -v
```

Six checks cover the local packaging and optional distribution paths. The local checks use a copied fixture executable in temporary project scratch directories. They exercise real macOS signing/archive tools, version metadata, manifests, paths with spaces, install preservation, existing-app refusal, symlinks, system destinations, non-executable files, and altered bundles. Mocked distribution checks additionally cover identity/tool preflight, runtime/timestamp requirements, Apple certificate requirements, submission rejection/timeout, stapling failure, final hashes after stapling, output cleanup, and the notarized-only gate. They do not establish source freshness, research quality, real notarization, or cross-architecture compatibility.

Shortcut: public signed distribution remains unproven until the real credential-backed workflow and first-launch checks pass. Use the optional workflow above when publisher credentials are available; add an authenticated update channel when a release distribution process exists.
