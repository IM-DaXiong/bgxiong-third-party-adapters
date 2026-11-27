# H3 FaceRefine Companion Contract (A03/A04)

Upstream: Carasibana/ComfyUI-H3-FaceRefine@d8521d14fe0d721d80cd9417fff5a559cbc21aba (v1.1.2)

## Isolation (hard rule)

- Package code lives under the **managed runtime root** for this plugin version.
- Managed nodes are exposed via `--extra-model-paths-config` → `extra-paths/extra_model_paths.yaml`. Standalone servers use the verified [node installer](../COMFYUI-INSTALL.zh-CN.md).
- The standalone installer stages both versioned node trees and retains their predecessors for rollback. The managed launcher should avoid mounting a second copy of an installed node set.
- Python deps for the companion may use a version-scoped venv; large unchanged weights may be reused read-only from shared model stores.
- Activation is prepare → probe → atomic enable. Failure leaves previous activation untouched.

## Public picker backend (no browser automation)

The upstream UI and older adapter versions use HTTP routes registered by `picker_api.register()`:

| Method | Path | Purpose |
|--------|------|---------|
| POST | `/h3_facerefine/scan` | Analyze shots + ranked candidates (JPEG cards) |
| POST | `/h3_facerefine/preview_xy` | Optional XY preview |
| GET | `/h3_facerefine/videos` | Input folder listing |
| GET | `/h3_facerefine/preview` | Range video preview |

Host/SDK must call these via adapter operations; never drive the Comfy web UI.

## Queued target analysis in 1.0.6

The companion exposes `BGXFaceAnalyze` as a ComfyUI output node. The adapter uses the active
server's `/prompt` queue and checks `queuedAnalysis` at `/bgx_face_refine/health`.
The node accepts an uploaded input name, the detector and cut settings, and a UUID request
identity. It uses the same detector and shot boundaries as the legacy picker. The dedicated
queue path imports automatic cut detection from the bundled wheel. The legacy picker keeps
its busy guard.

History contains only the normalized request ID at `outputs["1"]["bgx_analysis"][0]`.
`GET /bgx_face_refine/queued-analysis/{request_id}` returns the scan geometry and candidates
for that completed execution. The video filesystem path and legacy picker JPEG previews
stay out of the response. Results use ComfyUI's temporary directory and follow its lifetime.
`POST /bgx_face_refine/queued-analysis/{request_id}/cancel` accepts `{"promptId":"..."}` and
confirms removal only if the matching analysis remains pending. Running and missing jobs
return `unsupported`.

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
