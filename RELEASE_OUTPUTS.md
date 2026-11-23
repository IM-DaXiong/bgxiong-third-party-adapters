> **BAS-3 修订 / 用户 t372–t374 / 2026-11-23**：唯一交付根 `L:\00Dev\bgxiong-00-release` 下必须直接平级 `dev-项目名`、`release-项目名`、`cdn-项目名`、`deploy-项目名`，不再有编号分类/主机名/client/ai-story导航外壳。进入项目目录就选/拖成品；包内模块/平台结构照旧，CDN实际对象键不变。按本仓 RELEASE_OUTPUTS.md；同项目/用途/类型/平台/渠道只保留有版本/构建证据的最新完整成品，核验后删旧版，不按复制mtime判断；运行安装目录、源码、配置、缓存不迁移。

# 本仓交付归集入口（BAS-3，2026-11-23）

用户指定唯一最终交付根 `L:\00Dev\bgxiong-00-release`。所有智能体完成打包发行、开发/测试版编译或开发人员拿走测试前，必须按下表归集已完成成果；只报target/dist/旧artifacts路径不算交付。

完整权威合同（本机开发文档，不是对外SDK构建依赖）：`L:/00Dev/bgxiong-ai-story/docs/dev/guides/20261123-GUIDE-unified-delivery-root.md`。本仓源码、缓存、运行路径与用户安装布局不改，外部用户无需此L盘。密钥、私有配置、数据库/备份、源码checkout、缓存和日志不入交付目录；不移动正在运行的程序。

- **本机适配器安装归档**：旧 `artifacts/current/<pluginId>/<version>/` → 唯一根下 `原安装归档根不迁移；拿走测试ZIP归dev-bgxiong-third-party-adapters`。入口 `packages/*/Build*.ps1等; release/current.json`。源码packages和--development安装归档唯一根保持；统一根是完成后的交付副本，不是第二源码/安装真源。
- **生产CDN整批适配器**：旧 `artifacts/cdn/; artifacts/cdn-index/` → 唯一根下 `cdn-bgxiong-third-party-adapters/`。入口 `scripts/pack-cdn-directory.py; stage-cdn-upload-directory.py; sign-online-index-interactive.ps1`。原目录仅组装/验签工作区；完整归集本批全部ZIP、生产签名latest.json及SHA256SUMS.txt，序号/资格/安装信封不变；不能只拷ZIP。

CDN项目平级目录只放该远端前缀的载荷，拖**其中内容**；本机 cdn-项目名 不是远端对象目录，不要作为包装层上传。ZIP/manifest/latest/index/signature/checksum必须同批，生产验签/资格/序号按原规范，不能用测试签名冒充生产。

若入口有输出参数，显式传对应目标；否则构建成功并完成现行验收后，用主仓`scripts/stage-delivery-output.py --source <完整已验成品> --relative <上表相对路径> --check`核对，再去掉--check归集。主仓不在本机时按相同规则人工白名单复制并核每文件摘要，不因此要求外部SDK用户安装主程序。自动归集入口与显式stage边界见权威合同；本次包含已确认交付成品迁移，但没有构建、打包、签名、上传或部署。源码内部工作目录/运行安装档案不因归集要求迁移。

完成前回报真实归集路径、版本/源码/平台、文件闭包与摘要、实际验证层及NOT_RUN；空目录/复制成功不能代替发布/真实运行验收。待签名、缺包、未归集或依赖原checkout的启动器均标待交付。

自动CDN成品输出/校验已改到 `L:/00Dev/bgxiong-00-release/cdn-bgxiong-third-party-adapters`（pack-cdn-directory.py / stage-cdn-upload-directory.py / publish-online-index.py）；`artifacts/cdn-index` 只保留签名/资格工作区，`artifacts/current` 是仍在使用的安装档案，均不是待上传导航壳。
