# Cardbe Plugin: GitHub Issues

Imports GitHub issues into a Cardbe board. This repository owns the plugin source,
business-logic tests, build script and release package. Cardbe owns the host runtime,
permissions, credentials, scheduling, task persistence and host fixture tests.

## Build and test

Requires PowerShell and the Rust toolchain. Node.js is not required.

```powershell
git submodule update --init
rustup target add wasm32-unknown-unknown
cargo install wasm-tools --version 1.259.0 --locked
cargo test --locked
cargo fmt -- --check
cargo clippy --locked --all-targets -- -D warnings
./scripts/build.ps1
```

The build uses `wasm-tools component new --validate` to wrap the core Wasm module
in a validated WebAssembly Component. The wasm-tools version matches the API
submodule's WIT validation workflow. Output is `dist/manifest.json` and
`dist/component.wasm`, plus `cardbe-plugin-github-issues-<version>.zip` and its
SHA-256 checksum in the repository root. Build outputs are ignored by Git.

## Install

Download a release ZIP and verify its SHA-256 checksum. In Cardbe versions with
ZIP installation support, select the ZIP or enter its HTTPS download URL in the
plugin manager. Otherwise, extract both files into the same directory and select
`manifest.json`. Create an
instance, choose a writable board and approve `api.github.com`. Set the repository
as `owner/name`, choose the open and closed issue columns, and optionally provide
a GitHub bearer token. Public repositories can work without a token.

Cardbe stores the token in the operating system credential store and attaches it
to requests; the plugin never receives it. Scheduling is managed by Cardbe, with
zero for manual-only execution or an interval of 60 to 86,400 seconds. Cardbe must
be running for scheduled imports.

## Import behavior

- Fetches open and closed issues, excludes pull requests, and follows pagination.
- Uses `github:<node-id>` as a stable external key.
- Keeps the newest update when an issue appears on multiple pages.
- Saves a canonical UTC timestamp cursor and overlaps the previous minute.
- Returns no changeset on partial API failure or more than 2,000 unique issues.
- Maps existing labels by name and routes issues to the configured columns.
- Returns manifest-approved errors and bounded retry delays for rate limits.

The host merges updates without overwriting user overrides. Run one importing
device/instance per external source on shared boards. Removing and reinstalling
an instance loses source ownership mappings and can duplicate preserved tasks.

## Compatibility and repository maintenance

`cardbe-plugin-api/wit/plugin.wit` supplies the `cardbe:plugin@1.0.0` plugin
contract through the [API submodule](https://github.com/ldslds449/cardbe-plugin-api).
See `COMPATIBILITY.md` for its pinned revision and checksum. Builds use the pinned
revision; update the submodule deliberately when adopting another supported API.
The manifest requires API `1.0.0` and storage contract `1`.

The plugin is independently versioned. Cardbe releases do not require rebuilding
it when the host contract remains compatible. Before a Cardbe release, verify a
fixed existing plugin artifact against the host; native Rust unit tests alone do
not verify Component compatibility.

Diagnostic translations preserved in `resources/diagnostics` are reference
resources, not an implemented host translation-loading API. Current Cardbe uses
generic localized plugin failures and diagnostic code/count messages.

## Release

Update `version` in both `Cargo.toml` and `manifest.json` using the appropriate
package-manager workflow for any Cargo lock update. Commit and push a matching
`v<version>` tag. CI runs formatting, clippy and tests, builds the component, and
creates a draft release containing `cardbe-plugin-github-issues-<version>.zip`
and its SHA-256 checksum. Publish the draft on GitHub when it is ready.
The ZIP contains only `manifest.json` and `component.wasm`.

No release is published from local builds. This repository does not require a
Cardbe checkout, real token or live GitHub API for its unit tests. Initialize the
API submodule before building or testing.
