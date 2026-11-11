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
