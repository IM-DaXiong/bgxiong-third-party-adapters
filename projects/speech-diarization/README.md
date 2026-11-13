# Local speaker diarization

This package exposes the public SDK capability `speech.diarize`, operation
`std/speech.diarize@1`. It is not a text-to-speech model.

The companion runs 3D-Speaker CAM++ Chinese/English embeddings and FSMN VAD on CPU.
The upstream algorithm is pinned to commit
`065629c313eaf1a01c65c640c46d77e61e9607b4` under Apache-2.0. Vendored code retains
its license. Model revisions and byte hashes are in `companion/resources.lock.json`.
The wrapper disables optional gated overlap imports, uses local fixed model paths,
and calls FSMN through FunASR. No model is downloaded during inference.

Audio stays on its source timeline. Thirty-second VAD windows and batches of up to
64 embeddings are persisted; clustering is performed across the full recording.
Automatic mode uses the upstream AHC distance threshold. An explicit speaker count
uses the same cosine average linkage with SciPy's fixed-count cut. Neither method
identifies a real person's identity. Speakers are anonymous, numbered by first
appearance. The count estimate can split one voice into multiple roles; the host
provides review and role merging. Overlap detection is explicitly unsupported.

The SDK host owns authorized audio streaming, process management, installation,
task checkpoints and cancellation. The companion only listens on loopback and
checks the managed instance header. Its job plans are immutable and idempotent;
queries resume unfinished jobs after process restart. Failed and cancelled jobs
are not silently restarted. Intermediate VAD and embedding artifacts are reusable.

## Build

1. `prepare_source.py --upstream <audited checkout> --models-receipt <download receipt>`
   freezes vendor files and public profile schemas.
2. Build `adapter/Cargo.toml` with Rust 1.95.0 and `--release --target wasm32-wasip2`.
3. `build_package.py` creates the immutable SDK package; run the SDK CLI `check`.
4. Prepare a relocatable Python 3.12 runtime with the recorded dependency inventory,
   then `build_package.py --runtime <directory containing python> --out <new release directory>`.
   The resource ZIP includes the SDK package, companion, model bytes, Python,
   CPU libraries and their installed license metadata. It requires no pip install
   or developer checkout on the user's machine.

The host's frozen resource descriptor is published with the complete ZIP and SHA256.
Source code publication, remote byte verification and application installation are
reported separately; none alone demonstrates end-to-end dubbing quality.
