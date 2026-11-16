# 固定 SDK 操作输入测试夹具

本夹具为 T32 提供两份可以安装的合成图片包，包 ID 为 `local.bgx.operation-input-fixture`。它只生成固定的 128×128 PNG，并在结果 `data.received` 回传本次操作收到的输入。安装与执行用空配置，网络来源为空，成果经 SDK 媒体端口回收。

- `1.0.0` 声明提示词、墨色和光泽。陶土墨色与光泽可以同时使用。
- `1.1.0` 增加鼠尾草墨色枚举和中文标注的颗粒强度，加入布局对象和有序处理步骤，调整陶土墨色的光泽规则。拒绝可分别定位到 `/glossEnabled`、`/layout~1options/mix~0ratio` 或具体步骤的 `/passes/1/gain~1value`。
- 两版使用同一个 operation ID。操作版本分别为 1 和 2，输入声明及包内容各有摘要。

使用 Python 3.11 及以上版本、冻结 SDK 构建目录和已有的 crates.io 依赖缓存复验。缓存目录需要含 `index` 和 `cache`，脚本按作者锁复制对应索引与归档，并核对归档摘要。

```powershell
python tests/fixtures/operation-input/verify.py --sdk <frozen-sdk-build> --cargo-registry <prepared-registry> --output <new-evidence-directory>
```

脚本通过公共 CLI 执行作者初始化、离线构建、封包和 SDK 检查。真实 Worker 依次发现操作、运行预检、提交已准备的输入，并写出合成原件。证据目录保存两份安装候选、命令回执、预检、提交和 `summary.json`。

主程序验证使用同一份 EXE、UI 和 SDK，先安装 `candidate-1.0.0`，再安装 `candidate-1.1.0`。分别检查界面新增字段和枚举、保存复开后输入精度和列表顺序、拒绝定位以及旧任务读取。SDK 脚本只验包侧声明、预检、提交和原件。
