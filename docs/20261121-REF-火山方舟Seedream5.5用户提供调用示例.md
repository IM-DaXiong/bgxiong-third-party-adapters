# 火山方舟 Seedream 5.5 用户提供调用示例（脱敏）

- 文档 ID：REF-VOLC-SEEDREAM-55-USER-CURL-20261121
- 类型：REF
- 创建时间：2026-11-21T18:18:18+08:00
- 更新时间：2026-11-21T18:18:18+08:00
- 状态：用户提供示例归档（未测试、未作为有效调用证据）
- 适用范围：Seedream 5.5 第三方适配器规划与后续实现对照
- 来源：用户于 2026-11-21 粘贴的 cURL 调用示例（t238u）
- 归档日期：2026-11-21（Asia/Shanghai）
- 官方接口文档（本地全文权威参考）：[20261121-REF-火山方舟图片生成API-用户提供全文.md](./20261121-REF-火山方舟图片生成API-用户提供全文.md)
- 官方页面：https://ark.volcengine.com/region:cn-beijing/docs/image-generation-api
- 敏感信息处理：原消息中的真实 Bearer 密钥已删除，统一替换为占位符 `${VOLCENGINE_ARK_API_KEY}`。本文、日志、源码与命令均不得回写原密钥。本归档未读取账号中心、应用数据库或保险库中的真实密钥，也未向该接口发送请求

## 说明

以下参数按用户提供原样保存，包括 `background: "transparent"`、`watermark: true`、`output_format: "png"`、`response_format: "url"`、`size: "2K"`、`stream: false` 以及完整提示词正文。这些字段在用户提供的官方全文中均有定义，但**本示例本身未经本轮实测**，不能把本段当作已验证有效的调用。

模型 ID 与区域以用户示例及官方全文为准：`doubao-seedream-5-5-261028`，`POST https://ark.cn-beijing.volces.com/api/v3/images/generations`。

提示词正文是用户示例素材，不是产品通用默认提示词，后续适配器不得把它硬编码为选择该模型时的自动填充词。

## 脱敏调用示例

```bash
curl -X POST https://ark.cn-beijing.volces.com/api/v3/images/generations \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${VOLCENGINE_ARK_API_KEY}" \
  -d '{
    "model": "doubao-seedream-5-5-261028",
    "prompt": "刺绣贴布款复古贴纸，透明背景。每一枚贴布都极度写实地还原了真实刺绣工艺，具有密实立体的浮雕感，边缘一圈厚实的米白色锁边刺绣；部分元素（星星、口袋等）还表现出真实丹宁牛仔布纹理 + 黄色双明线缝迹。图案均为少女心经典元素：爱心、雏菊、樱桃、彩虹、泰迪熊、蝴蝶等；文字包含黑色手写花体草书“Good VIBES”“By Cecy Papper”。",
    "size": "2K",
    "response_format": "url",
    "output_format": "png",
    "background": "transparent",
    "stream": false,
    "watermark": true
}'
```

## 参数对照（仅描述用户示例，不扩范围）

| 字段 | 用户示例值 | 用户提供官方全文中的位置与含义（摘录） |
|------|------------|----------------------------------------|
| 方法与 URL | `POST https://ark.cn-beijing.volces.com/api/v3/images/generations` | 全文文首接口地址 |
| Authorization | `Bearer ${VOLCENGINE_ARK_API_KEY}` | API Key 鉴权；原文示例使用环境变量占位 |
| model | `doubao-seedream-5-5-261028` | Seedream 5.5 的 Model ID |
| prompt | 刺绣贴布款复古贴纸…… | 图片生成场景必选提示词 |
| size | `2K` | 5.5 图片生成场景推荐档位之一，默认值亦为 `2K` |
| response_format | `url` | 默认值 `url`；链接生成后 24 小时内有效 |
| output_format | `png` | 5.5 默认值 `png`；与透明背景兼容 |
| background | `transparent` | 5.5 可选 `auto` / `transparent` / `opaque`，默认 `auto`；`transparent` 不可与 `jpeg` 同用 |
| stream | `false` | 5.5 暂不支持流式输出 |
| watermark | `true` | 官方默认值 `true`；`true` 时右下角添加“AI 生成”水印 |

## 明确不做

- 不发送付费 HTTP 请求，不测试真实密钥
- 不把本示例视为已跑通
- 不把示例提示词写成通用默认提示词
