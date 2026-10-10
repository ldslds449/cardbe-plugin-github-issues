# Host contract provenance

- Source: https://github.com/ldslds449/cardbe-plugin-api, `wit/plugin.wit`.
- Submodule: `cardbe-plugin-api`, pinned to commit `bdee0a4f00a71e9b4b89bbfd36b420f816106190`.
- API: `cardbe:plugin@1.0.0`, world `importer`.
- WIT SHA-256 (Git blob, LF line endings): `fb4e08d0d6d5423329fbbf1e37cf304deba04a87b1f9d5a541a2c3440a7024b6`.
- Required manifest API version: `1.0.0`.
- Required storage contract: `1`.

The WIT is consumed directly from the pinned API submodule. Initialize it with
`git submodule update --init`; builds do not download host definitions.
Compatibility also includes configuration and board JSON, HTTP policy and changeset
semantics; unchanged WIT alone is not a complete compatibility test.
