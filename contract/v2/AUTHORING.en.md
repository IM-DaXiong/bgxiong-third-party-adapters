
## Operation preflight source implementation (unreleased)

The isolated video-speech work adds a metadata-only `probe` kind, `operation_preflight`. Require `operation-preflight-v1` and, for driving-audio video, `video-speech-v1`. Select a discovered operation using the same model/operation/schema identity as submit, provide its schema-valid input and frozen configuration, and provide media summaries only. The runtime permits diagnostic logging only; unknown service/model readiness must be blocked. No uploads, generation or checkpoint writes are authorized. See [video-speech profile](profiles/video-speech-v1/README.en.md). This source increment is uncompiled and untested; generated types and public SDK release are pending.
