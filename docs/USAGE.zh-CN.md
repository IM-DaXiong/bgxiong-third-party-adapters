# 用法

本仓库是比格熊第三方适配器的独立开源真源：
https://github.com/IM-DaXiong/bgxiong-third-party-adapters

适配器许可见根目录 [LICENSE](../LICENSE)（不是 MIT）。

## 导入到比格熊

1. 安装 [BGXiong / 比格熊数字导演工作站](https://www.bgxiong.com)
2. 从[首页下载目录](../README.md#可安装-zip-目录)选择已构建 ZIP 并解压，无需编译；不要下载 GitHub 自动生成的 Source code ZIP 当安装包。
3. 在「设置 → 第三方适配器 → 安装适配包」选择含安装声明和 package 的完整目录。
4. 配置平台 API 密钥后做一次探测/试跑；安装成功不等于厂商侧已经出片

## RunningHub H3 视频

安装 `runninghub-h3-all` 后可选择 9 个工作流。三项多图参考支持 1–30 秒、8 种画幅和 0.1–16 MP。固定图数和首帧工作流支持 1–15 秒、4 种画幅和 0.25、0.4、0.6、1 MP 四档。所有工作流采用 24 fps。

1. 下载 [完整安装 ZIP](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/runninghub-h3-all-v1.0.2/local.thirdparty.runninghub-h3-all-1.0.2.zip)，按 [SHA256SUMS.txt](https://github.com/IM-DaXiong/bgxiong-third-party-adapters/releases/download/runninghub-h3-all-v1.0.2/SHA256SUMS.txt) 核对摘要后解压。常见音频扩展识别需要消费 SDK 2.1.3 的主程序版本。
2. 在比格熊打开 **设置 → 第三方适配器 → 安装适配包**，选择同时包含 `adapter-install.json` 和 `package/` 的目录。
3. 按提示绑定 RunningHub 账号，或填写自己的 RunningHub API Key。
4. 在视频工作台的 **RunningHub** 分组选择 `runninghub-h3-all` 下的工作流，按所选项添加素材和设置参数。

Turbo 与官方原生多图参考可选音色参考。「保留原音」要求一段原音作为成片音轨。视频任务提示云端付费积分不足时，请检查 RunningHub 账号的付费积分余额。完整选项和素材要求见[工作流说明](../packages/cloud-adapters/runninghub-h3-all/README.md#工作流与素材)。

## 给智能体

把本仓库交给智能体，并附上第三方官方 API 文档。密钥在客户端导入时填写，不要写入源码。智能体应先读根目录 [AGENT.md](../AGENT.md) 与 [LICENSE](../LICENSE)，再按合同制作可导入插件。包清单见 [packages/README.zh-CN.md](../packages/README.zh-CN.md)。
