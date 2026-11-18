# Local release-input rebuild checkpoint (2026-11-18)

This is a source/freeze checkpoint, not a binary publication or host/cloud acceptance claim.

- Seven changed package identities were prepared with the current clean SDK source cc60a6bd77e96c559765afc22cbb41c89d5d5588 and its verified release tools: seedvr2 1.0.4, seedream 1.0.8, flux2 1.0.4, Breeze 1.0.2, face-refine 1.0.0-a33, RunningHub H3-all 1.0.5, Qwen Image 2.1 0.1.4.
- Breeze was rebuilt locally from source; the historical 1.0.2 ZIP was not available. The first rebuild failed the private-path audit. A fresh source build with explicit rustc path-remapping passed the unchanged audit and SDK validation. It is not the historical accepted byte identity. Model/host inference acceptance for the new bytes remains NOT_RUN.
- Face-refine's template lock/generated contract mirror was stale relative to the current public SDK. Only the generated mirror and its matching generated lock were restored from official SDK init output; editable author code and original Cargo.lock were preserved. Standard SDK build/check and the official install-envelope producer passed. Missing isolated Cargo dependencies were fetched through the standard online build; no credentials/configuration were copied.
- H3-all reproduced its declared WASM 8874a429a611f234187895171efdf49c2ee41e22917f48c5b2317f8f9e35764b. Qwen and face-refine now record their actual locally rebuilt digests, rather than asserting the unavailable historical byte identities.
- Official prepare/check/pack and local-current staging succeeded. The full lock contains 26 packages, 262 files. Only these seven package rows changed; existing blocked VoxCPM status is unchanged.
- Canonical LF lock SHA256: c1d00010170c87ba930a6c082b02381c7f479105ac047949857978a57335956f.
- Complete local archive: third-party-frozen-resources-c1d00010170c.zip; 7069048 bytes; SHA256 5ba4f7d4fe59be86ce3229596fe411862606faa7e751b56e1297cdc3539b6da2. An empty isolated restore using the unchanged official restore tool restored and verified all 262 files.
- Archive upload/public Release creation was NOT authorized in this task and has NOT occurred. release/current-artifact.json intentionally remains the historical published receipt. The official download gate currently FAILS because that historical receipt does not match the new lock. No download URL, published status, host acceptance or credential certification has been fabricated.
- The local-only receipt and prepared-lock scratch file are not public source deliverables. Source push, artifact publication and installer acceptance are separate stages.
## Publication and download verification completed

The user subsequently explicitly authorized the resource Release/tag release-inputs-0.6.1119-20261118 and this exact ZIP. It was created at source commit 50d52721db972df55773e46cddce70ae1fc083f2 without changing any historical release or latest-release designation.

Release: https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/tag/release-inputs-0.6.1119-20261118

Asset: https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/release-inputs-0.6.1119-20261118/third-party-frozen-resources-c1d00010170c.zip

GitHub reports the asset uploaded, 7069048 bytes, SHA256 5ba4f7d4fe59be86ce3229596fe411862606faa7e751b56e1297cdc3539b6da2. It was downloaded again from that release; its size, archive digest and all 262 locked file digests matched. The unchanged official verify-release-downloads.py adapters entry then independently downloaded and restored/verified the archive successfully. current-artifact.json now records the real published URL and verified size/digests/counts. This resolves the previous publication blocker, not installer acceptance, new model inference acceptance or upgrade-tool certification.