# SDK signed URL audit gap

Confirmed against SDK 050b7f1 / 2.1.3 using real HttpBroker and BeforeSendSink: one ordinary HTTP test passed; the unregistered signed-query redaction test failed. No live provider request was made.

Authoritative SDK remediation plan: [SDK patch plan](https://github.com/IM-DaXiong/bgxiong-adapter-sdk/blob/a014675cbc04c234e4c40981ae4488111cc8b5e2/docs/20261112-PLAN-SDK%E5%87%BA%E7%AB%99%E7%AD%BE%E5%90%8DURL%E8%84%B1%E6%95%8F%E8%A1%A5%E4%B8%81%E5%8D%87%E7%BA%A7.md). SDK implementation is unchanged. Do not bypass broker/audit in this adapter.

Reproduce from this package: `cargo test --manifest-path tests/sdk-transport/Cargo.toml --locked --offline --test sdk_transport`. Expected acceptance is both tests passing; the current failed assertion is intentionally retained, not ignored.

Large media-body, credential redirects and worker integration remain NOT_RUN. Video input additionally lacks a verified upload/URL source. The current shell exposes no usable operation and is not an installable package.

## SDK 2.1.4 retest

Both original SDK transport probes pass against a014675. The candidate adds the text and ordered-image operation contracts. The large body native fixture covers chunked media reads and writes beyond the HTTP buffer limit. Real Kling account generation and application import remain separate acceptance steps.
