# RunningHub H3 全工作流适配器

一个安装包，十一项独立工作流。旧适配器由用户自行卸载，本包不迁移或兼容旧身份。

- 多图参考 · Turbo（1–9图）
- 多图参考 · Spectrum 均衡（1–9图）
- 多图参考 · 官方原生（1–9图）
- 多图参考 · 官方原生·保留原音（1–9图）
- 固定1图参考 · Turbo
- 固定2图参考 · Turbo
- 固定3图参考 · Turbo
- 固定4图参考 · Turbo
- 首帧生视频 · 官方原生
- 首帧生视频 · Turbo
- 参考音对口型 · Spectrum（1图）

完整安装目录包含 adapter-install.json 与 package。导入后配置自己的 RunningHub API Key。

平台映射独立维护：classic 为七个 AI App，dynamic 为四个整图工作流。各工作流的素材、参数、音轨和原任务续查规则分别声明，不自动切换。

## 参数边界

四项多图参考：1–9 张图；Turbo / Spectrum 均衡 / 官方原生支持 0–3 段音色参考，保留原音项要求恰好 1 段原音。时长 1–30 秒、8 种画幅、0.1–16 MP、24 fps。各图与采样节点独立保留。

七项固定应用：固定 1/2/3/4 图 Turbo、首帧原生、首帧 Turbo、1 图加 1 段参考音对口型。时长 1–15 秒、4 种画幅、0.25/0.4/0.6/1 MP、24 fps。固定参考图不会按图数自动改选其他应用。

## 构建

使用独立 SDK 当前正式作者套件（根含 bin、sdk、templates、protocol）。执行 `./Build.ps1 -Sdk <作者套件绝对路径> -OutputDirectory <尚不存在的完整安装目录>`；独立缓存首次缺依赖时显式加 `-Online`。SDK 输入须在本批开始固定，不在构建中途更新。

定向测试：`cargo test --manifest-path Cargo.toml --locked --offline`。不需要构建主程序。包检查不等于已通过 RunningHub 付费生成；真实账户、计费、生成内容及应用播放由实际使用验收。
