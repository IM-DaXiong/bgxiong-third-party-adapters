# Upgrade guide (authors)

1. Change only this package + managed companion; never the host EXE.
2. Bump `locks/upstream-comfyui-h3-facerefine.sha` and refresh `companion/upstream/`.
3. If picker routes or denoise model wiring change, update `workflows/` and adapter mappings.
4. prepare → probe → atomic activate; never silently fall back.
5. Baseline: `d8521d14fe0d721d80cd9417fff5a559cbc21aba`.

### 1.0.2

Install the WASM and companion from the same envelope. Reload the Comfy nodes after running jobs finish. Verify the 1.0.2 health response, the sharedTracking flag and the BGXFaceTrack declaration. A matching preview and refinement share trajectory geometry. Source frames without a detection pass through. Existing tasks retain their original package version and prompt ID.
