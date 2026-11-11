# 用法

本仓库是比格熊第三方适配器的独立开源真源：
https://github.com/IM-DaXiong/bgxiong-third-party-adapters

旧名 `bgxiong-vendor-adapters` 已更名，指向同一仓库。本仓库不含主程序；主程序按契约导入你交付的插件。许可见根目录 [LICENSE](../LICENSE)（不是 MIT）。

## 导入到比格熊

1. 安装 [BGXiong / 比格熊数字导演工作站](https://www.bgxiong.com)
2. 从[首页下载目录](../README.md#可安装-zip-目录)选择已构建 ZIP 并解压，无需编译；不要下载 GitHub 自动生成的 Source code ZIP 当安装包。
3. 在「设置 → 第三方适配器 → 安装适配包」选择含安装声明和 package 的完整目录。
4. 配置平台 API 密钥后做一次探测/试跑；安装成功不等于厂商侧已经出片

## 给智能体

把本仓库交给智能体，并附上第三方官方 API 文档。密钥在客户端导入时填写，不要写入源码。智能体应先读根目录 [AGENT.md](../AGENT.md) 与 [LICENSE](../LICENSE)，再按合同制作可导入插件。包清单见 [packages/README.zh-CN.md](../packages/README.zh-CN.md)。
