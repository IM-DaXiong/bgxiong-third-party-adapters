# MiniMax H3-Image ComfyUI candidate

Version `1.0.0-dev.3`, plugin `local.bgx.minimax-h3-image-comfy`. Two image.generate contracts expose separate text-to-image and reference-edit models. Editing accepts one or two ordered image leases. The initial adapter scope is 2048 x 2048. Prompt and seed pass through unchanged. Sampling uses six steps and CFG 1.

## Build and verify

Run `cargo test --locked` from this directory. Set SDK_ROOT to the sibling bgxiong-adapter-sdk checkout. Its target/debug/bgx-adapter-v2.exe supports these commands (replace placeholders with absolute paths).

```
bgx-adapter-v2 build --sdk SDK_ROOT --project AUTHOR_ROOT
bgx-adapter-v2 check --sdk SDK_ROOT --worker WORKER_EXE --project AUTHOR_ROOT
bgx-adapter-v2 pack --sdk SDK_ROOT --project AUTHOR_ROOT --out NEW_CANDIDATE_DIRECTORY
```

The SDK-generated author kit is pinned in template-lock.json. Ordered uploads, checkpoints, query and download reuse the existing independent FLUX.2 adapter implementation. No SDK or application source changes are required for this stage.

Eight native behavior tests invoke the real Adapter entry point through recorded Host ports using the same frozen native workflows as WASM. They cover LF-stable schema digests, capabilities, 0/1/2 references, order, missing leases/files, invalid inputs, service/node/model preflight, one submission, uncertain acceptance, checkpoint failures, fresh-host recovery, queue states, output nodes and PNG dimensions. These tests simulate HTTP and storage. Live acceptance is recorded separately in the main repository H3-Image handoff plan.

## Frozen resource handoff

SOURCE.json records the source commit and exact workflow/model digests. Both workflows under workflows/ came from the resource host running ComfyUI 0.45.0 (989d93d0). Node 4 owns prompt, dimensions and ordered image conditioning. Node 6 owns seed, node 8 saves the output, nodes 9/10 load references. Single-reference mode removes node 10 and its images.image_2 link. Each node definition is checked separately to stay within the SDK buffered-response budget.

The resource host verified 2K/4K text generation and 2K single/two-reference editing directly. This adapter initially exposes 2K only. All workflow JSON files are pinned byte-for-byte with -text attributes. The prior synthetic workflows and their pending-workflow rejection test were retired when these real graphs became available. Current lifecycle tests exercise these graphs.

Application installation, UI, Creator/MCP and public release acceptance remain separate from SDK live integration. This candidate is outside the current release lock. PNG checking inside the adapter inspects the signature and IHDR dimensions. Live verification must also fully decode downloaded output.
