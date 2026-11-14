# Local ComfyUI single-image ERP panorama adapter (1.0.3)

Current source is development candidate **1.17.0**. The 1.0.3 inference result below is a retained baseline, not acceptance of every newer feature.

1.17 adds owned image geometry inputs: DA3 mono/multiview, MoGe perspective and MoGe panorama. The compact editor panel discovers model names from the configured resource host, prepares and collects one original task, then explicitly adopts the result. Native undo/redo and reopening retain settings, sources and preview assets. Nested multiview files, depth-grid color proxies and animation metadata are preserved. Differently sized input images are rejected before upload rather than implicitly resized. Unknown extension versions remain read-only.

27 targeted native checks, the production companion/frame browser path and standard SDK build/check/pack/check passed. Actual ComfyUI inference, application EXE service integration and video geometry input remain pending. No weights are downloaded by this package. Node contracts were checked against ComfyUI MoGe eee59185ff4f43ae2d9ad122b245619d27d9daad and DA3 5ece24e73c0785e95a45a45a2773ecd1a6f5339b; Panorama remains pinned in editor/upstream.lock.json.

Version 1.0.3 completed a real single-image ERP generation on LAN ComfyUI 0.45.0 with an RTX 4090. The application queried the original task and saved the 2048×1024 image.

Package ID: `local.thirdparty.comfy-panorama-image`  
Model: `panorama-image-single`  
Operation: `image-to-panorama`  
Capability: `image.generate` with `requiredFeatures: ["image-projection-v1"]`  
Output: `equirectangular360` at 2048×1024

This package is an independent Cargo workspace initialized with the public SDK template (`vendor-sdk` / `template-lock.json`, author SDK **2.0.0**). The frozen runtime verification used SDK 2.1.2, source commit `e903a21`.

On Windows, provenance lives at package-root [SOURCE.json](SOURCE.json). The workflow file is [workflow/source.json](workflow/source.json). Do not add a second provenance file whose name differs only by case.

See `README.zh-CN.md` for model paths, headless sticker_state, and build commands.
# Development candidate 1.12.0

The candidate adds the frozen three-stage LTX 2.3 panorama video operation alongside the existing image and Studio operations. It exports the full ERP decoder output with its audio, rather than labeling the example's perspective cutout as a panorama. Output is currently 2048×1024 at 24 fps. Exact `8n+1` frame counts (121 by default), seed, LoRA strengths and CRF are mapped without rounding frames into seconds. Local graph/query tests and the actual SDK package checks passed; real model inference, installation release and complete upgrade acceptance remain pending. This is not a public stable release claim.


## Development candidate 1.13.1

Model-free Frame capture returns the upstream first_frame, mask and sticker_state as three owned editor assets. The saved document, connected scenes and media are frozen by the host. Camera lanes and fps are retained from upstream output 4, not reconstructed. The compact toolbar submits once and collects the same retained task after reopening; UI and MCP share pending/completed service results. Requires PanoramaStudio plus built-in MaskToImage, PreviewAny and SaveImage. Candidate uses SDK 2.4.0/ddc91f3. Real ComfyUI rendering and manual repair/seam return remain pending; package and browser checks do not claim model acceptance.


### Repair and adoption (1.14.2 candidate)
The compact toolbar exposes capture, frame return, seam preparation and seam return as inline radio modes. Frame return consumes the original upstream pose/fps and rejects mismatched dimensions or changed scene provenance. Seam restoration uses the measured/clamped offset. Results remain owned attachments until explicitly adopted as a new image version through the shared `canvas.editor.asset.publish` owner. Prior versions and original task receipts remain intact; repeated adoption is idempotent. Real ComfyUI/model rendering is pending resource-machine acceptance; mock protocol checks are not inference evidence.


Development candidate 1.15.1 adds saved Studio timeline rendering through its native VIDEO output. Mock SDK submit and original-task query preserved 29.97 fps, 73 frames and ERP360; application video-editor wiring and real Comfy rendering remain pending. Core SaveVideo returns `images`; VHS returns `gifs`. The retained 1.15.0 candidate failed checkpoint validation because appliedParams included fields absent from the frozen input. 1.15.1 reports declared input fields only and leaves timeline values unchanged. No failed original task was resubmitted.

## 1.16.0 H3 editor video candidate

Adds the frozen upstream H3/MoGe/CrossView workflow driven by the saved Studio scene and an owned reference video. Select the source by its editor asset name. Both the scene and measured source must be 24 fps with the same exact 17n+5 frame count (5..3600); no implicit trimming, padding or rate conversion. Reference and generated audio connections are retained. The host must request measured frame timestamps for editor_to_video.

23 targeted tests, standard SDK package validation and one SDK submit/query against a protocol fixture passed. Actual GPU inference, visual quality and audio content remain pending on the resource host. This is a candidate, not a replacement public release.
# Video geometry candidate 1.18.0

The geometry panel selects either owned images or one owned video and retains the choice on reopen. DA3/MoGe consume every decoded video frame through VHS with format=None and no rate, crop, resize, or frame selection override. Measured frame timestamps must match the saved scene timing; panoramic MoGe requires 2:1 input. The original video and audio remain retained. Preparation and explicit adoption are separate actions.

Adapter behavior checks (28), the production editor selection/reopen path, and SDK package validation pass. Browser fixtures do not verify video decoding. Actual host video geometry, real GPU inference, and large-file transport remain pending. This is a candidate, not the public downloadable release.

# Lazy asset candidate 1.19.0

The companion declares `assetTransport: blob-v1`. Opening receives owned metadata and loads Blob assets referenced by the current scene or preview. Unused source videos, geometry inputs and old results are not eagerly materialized. Import uses the same streaming host owner; inherited assets retain their parent revision. A matching application with the 256 KiB transfer implementation is required; SDK 2.4.0 remains unchanged.

Standard SDK package checks and production companion geometry, repair/adoption and parent-child save/reopen checks pass. Native large-file and actual application deployment checks are still pending. This candidate does not replace the public downloadable release; browser fixtures are not evidence of real video decoding or GPU inference.
