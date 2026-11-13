# Breeze 1.0.1 offline installation metadata publication

Scope: public Breeze runtime 1.0.2 metadata still identifies adapter 1.0.0-a, and an older local drop identifies 1.0.0-d. The current application correctly requires the bundled adapter 1.0.1 identity. Installing the old sidecar on a new computer therefore fails before resource installation.

Atomic file map:

- Derive a new `breeze-tts-h1-1.0.2.zip.offline-bundle.json` from the already frozen Breeze 1.0.1 `provisioning.json`. Preserve runtime 1.0.2 archive URL, SHA256 and bytes exactly.
- Fetch and preserve the original published runtime JSON bytes. Do not substitute the local development manifest, whose `sourceDir` has changed.
- Append these two files and a separate `BREEZE-INSTALL-METADATA-SHA256SUMS.txt` to the existing `breeze-durable-1.0.1` Release, without overwriting any asset. Preserve the existing complete adapter ZIP, aggregate ZIP and `SHA256SUMS` byte-for-byte.
- Add `release/breeze-durable-1.0.1-metadata.json`, extend the Breeze release/download records, and update the repository README with exact public links and the existing offline-drop directory procedure. No package, runtime, protocol or application Rust changes belong to this atom.

Self-review: the resource archive identity remains runtime 1.0.2; only the separately published adapter/resource association identifies Breeze 1.0.1. Application discovery uses `OfflineBundlePanel -> offlineBundleScan -> matching archive sidecar -> strict builtin locator`; it does not silently fetch or migrate an old sidecar. Users must put the newly linked sidecar beside the original runtime ZIP and select that resource folder. New URLs must appear in the public user entry, not only in an internal upload receipt.

Validation: inspect the three prior Release assets before/after, re-download every new metadata asset through its public HTTPS URL and compare full bytes/SHA256, verify the sidecar against frozen provisioning and the original runtime JSON archive digest/size, then commit/push the source records and confirm remote containment. This does not claim a new native installation run or change the existing package's earlier acceptance record.

Result: all three new assets were uploaded without overwrite and re-downloaded from their public Release URLs. Their bytes matched exactly: sidecar SHA256 `c3677921a1a65b12d0a12cbe353f051d7019a874f524e136fdbff9f70dc6b1f3` (406 bytes), original runtime JSON `a6f0c3509b3b7c06bc09941a1f8a8f7f68de9928bfe1d78e856c8ace1328e33b` (9794 bytes), independent sums `4f56bae36e4f9ba3e16d574de24c96b7fdbb426ca6d77fc6ee66d736008930e3` (201 bytes). The three prior asset IDs, timestamps, sizes and GitHub digests were unchanged. The public README and installable download entry now expose the correct URLs and placement workflow; metadata is not fetched automatically by the application. The structured receipt is `release/breeze-durable-1.0.1-metadata.json`. Main Rust and all package/runtime bytes remain unchanged.
