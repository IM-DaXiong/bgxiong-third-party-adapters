# RunningHub H3 全工作流适配器

`runninghub-h3-all` 通过 RunningHub 生成视频。一个安装包提供 9 个工作流，使用同一个 RunningHub 账号。

## 安装与使用

1. 下载 [完整安装 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/runninghub-h3-all-v1.0.1/local.thirdparty.runninghub-h3-all-1.0.1.zip)，按 [SHA256SUMS.txt](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/runninghub-h3-all-v1.0.1/SHA256SUMS.txt) 核对摘要后解压。
2. 在比格熊打开 **设置 → 第三方适配器 → 安装适配包**，选择同时包含 `adapter-install.json` 和 `package/` 的目录。
3. 按提示绑定 RunningHub 账号，或填写自己的 RunningHub API Key。
4. 在视频工作台的 **RunningHub** 分组选择 `runninghub-h3-all` 下的工作流，按所选项添加素材和设置参数。

预编译安装包通过 RunningHub 云端执行，安装时只需准备 RunningHub API Key。

## 工作流与素材

- **多图参考 · Turbo（1–9图）**。1–9 张图，可选 0–3 段音色参考。
- **多图参考 · 官方原生（1–9图）**。1–9 张图，可选 0–3 段音色参考。
- **多图参考 · 官方原生·保留原音（1–9图）**。1–9 张图，另选 1 段原音作为成片音轨。
- **固定1图参考 · Turbo**。恰好 1 张参考图。
- **固定2图参考 · Turbo**。恰好 2 张参考图。
- **固定3图参考 · Turbo**。恰好 3 张参考图。
- **固定4图参考 · Turbo**。恰好 4 张参考图。
- **首帧生视频 · 官方原生**。1 张图作为首帧。
- **首帧生视频 · Turbo**。1 张图作为首帧。

Turbo 与官方原生多图参考可用音频作为音色条件，成片使用模型生成的音轨。「保留原音」将所选音频作为成片音轨。

三项多图参考支持 1–30 秒、8 种画幅和 0.1–16 MP。固定图数和首帧工作流支持 1–15 秒、4 种画幅和 0.25、0.4、0.6、1 MP 四档。所有工作流采用 24 fps。

## 源码入口

- [src/lib.rs](src/lib.rs) 发布能力目录并按所选模型分派请求。
- [src/dynamic/routes.rs](src/dynamic/routes.rs) 声明三项多图参考的标签、输入合同、云端工作流和音轨策略。
- [src/classic/mod.rs](src/classic/mod.rs) 声明六项固定应用。[应用节点映射](docs/AI-APP-MAPPING.zh-CN.md)列出各应用的素材、参数和节点。
- [工作流资料](workflows/README.md)记录提交方式与源码位置。

## 构建

使用独立 SDK 当前正式作者套件（根含 bin、sdk、templates、protocol）。执行 `./Build.ps1 -Sdk <作者套件绝对路径> -OutputDirectory <尚不存在的完整安装目录>`；独立缓存首次缺依赖时显式加 `-Online`。SDK 输入须在本批开始固定，不在构建中途更新。

定向测试：`cargo test --manifest-path Cargo.toml --locked --offline`。不需要构建主程序。

1.0.1 提供上述 9 项能力。云端生成与播放由实际任务验证。
