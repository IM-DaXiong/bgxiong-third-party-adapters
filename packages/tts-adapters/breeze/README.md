# Breeze TTS 2 托管语音适配器

当前源码版本 `1.0.1`，实现 `plain`、`voice_design`、`reference_clone`，保留原中英文文本、参考音、文字控制、随机种子和参数回执。SDK 协议仍为当前独立 SDK 的 v2，新增依赖正式语义能力 `submission-resolution-v1`。当前源码与本地候选不等于已发布或真实宿主验收通过；正式下载状态以仓库 `release/` 记录为准。

## 持久任务

每次合成由 SDK 原始 requestId、requestDigest 和服务 storeId 共同冻结。伴随服务先提交持久受理，再串行执行模型；相同请求不会再次推理。成功音频按 SHA256 校验，可由 query 重取。宿主在受理响应前中断时，通过正式 probe 核验原 storeId：已有请求恢复查询；确实未受理时先写阻止迟到提交的 tombstone，再回传拒绝证明。404 和更换存储目录均不能当作未受理证明。

取消排队任务立即终止；取消正在计算的任务只记录取消请求，会保持 running；实际成功仍为 succeeded 并保留可复用音频，实际失败为 failed。宿主用户任务持续等待实际结束，再显示已取消，不谎报内核已中断。伴随进程意外退出后，旧 running 请求标记 failed，不能暗中重新推理。原有 `1.0.0-d` 未知受理任务不具备此冻结身份，不能据新合同自动重发。

## 构建与安装

源码仅在此目录维护。`build-package.py --cli <当前SDK CLI> --worker <当前SDK worker> --sdk <本批完整SDK目录> --out <新的候选目录>` 从同级 SDK 编译 WASM，执行正式 package check，生成完整 ZIP、SHA256SUMS、构建回执、companion、provisioning 和许可。输出必须是新目录；不得覆盖已发布版本。

这是托管语音资源包，不是普通云端 adapter-install 信封。ZIP 根包含 `provisioning.json`、`prepare-resources.ps1` 与 `adapter-pack/`，由比格熊当前的托管语音安装 owner 准备资源和登记能力。已有资源沿版本 `1.0.2` 原始 URL、长度及 SHA256 复用，应用脚本由包内清单同步，不提高开源运行时版本。新装所需 Python、模型、依赖与 SoX 完整归档及公开地址均在 provisioning 中声明；ZIP 不重复包含约 11 GB 模型和依赖。

安装升级通过统一 owner 迁移当前选择；历史任务、已生成音频及其冻结包身份保留。不要直接修改任务库、覆盖同版本包，或删除 jobs-v1 存储。

## 定向验证

`python tests/durable_jobs_test.py -v` 验证并发幂等、负面证明与迟到提交竞争、引用快照、成功成果冷恢复、真实 worker 结束前的取消状态、原进程锁及音频摘要。`cargo test --offline --locked --manifest-path adapter/Cargo.toml` 验证三模式原参数/回执和正式 probe/query 映射。真实模型、宿主 UI/MCP、安装迁移和远程恢复分别记录，不能用这些协议测试代替。
