# H3 FaceRefine Companion Contract (A03/A04)

Upstream: Carasibana/ComfyUI-H3-FaceRefine@d8521d14fe0d721d80cd9417fff5a559cbc21aba (v1.1.2)

## Isolation (hard rule)

- Package code lives under the **managed runtime root** for this plugin version.
- Nodes are exposed to ComfyUI only via `--extra-model-paths-config` → `extra-paths/extra_model_paths.yaml`.
- **Forbidden**: copying/overwriting into shared `ComfyUI/custom_nodes/ComfyUI-H3-FaceRefine`.
- Python deps for the companion may use a version-scoped venv; large unchanged weights may be reused read-only from shared model stores.
- Activation is prepare → probe → atomic enable. Failure leaves previous activation untouched.

## Public picker backend (no browser automation)

Prefer upstream HTTP routes registered by `picker_api.register()`:

| Method | Path | Purpose |
|--------|------|---------|
| POST | `/h3_facerefine/scan` | Analyze shots + ranked candidates (JPEG cards) |
| POST | `/h3_facerefine/preview_xy` | Optional XY preview |
| GET | `/h3_facerefine/videos` | Input folder listing |
| GET | `/h3_facerefine/preview` | Range video preview |

Host/SDK must call these via adapter operations; never drive the Comfy web UI.

## Local-runtime mapping

Uses public `local-runtime-v1` RuntimeSpec + CodeInventory overlay. **No new host ports** were required for A03: existing ensure/probe/lease + code overlay suffice when the launch args include the extra-model-paths config and package-owned root.

## Tracking in 1.0.2

Both API workflows use `BGXFaceTrack` and the refinement crop settings. The first
execution resolves the selected faces and saves validated geometry and per-frame
detection flags. With matching source, selection, detector settings, identity
image and component versions, subsequent executions recreate the crops from
that trajectory. Comfy keeps it in its temporary directory until eviction
or restart. Explicit preview renders a WebP.

Refinement composites only source frames with tracker detections. Target
occlusion can produce a ready preview with reported lost ranges. Missing
tracking data or a range with no selected face prevents sampling.
