# Local ComfyUI single-image ERP panorama adapter (1.0.3)

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
