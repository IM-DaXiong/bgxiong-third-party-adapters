# 百炼图像与万相视频适配包

包以独立操作声明百炼图像和万相视频。图像覆盖现行设置中的千问图像 3.0、千问图像编辑 Plus、Z-Image Turbo。视频使用万相 3.0 的工作空间端点。图像同步响应保留供应商返回的原件，视频按已受理原任务号查询并下载。

使用阿里云百炼以 `https://<WorkspaceId>.<Region>.maas.aliyuncs.com` 为源站的绑定。API Key 的授权范围是工作空间 API 源站。保存成果所需的对象存储源站单独授予只读下载，使用 SDK 的受管网络与媒体租约。

## 工作空间安装候选

作者用 `Build-InstallCandidate.ps1` 配合已有 SDK 包、冻结 SDK 和精确 Wasm SHA256 生成完整候选。脚本需要已批准的 HTTPS 结果下载源站。生成的目录含 SDK 包、安装信封、仓库许可证和本说明。

```powershell
./Build-InstallCandidate.ps1 -SdkBuild <SDK-build> -FrozenPackageDirectory <package> `
  -ExpectedArtifactSha256 <verified-sha256> `
  -ResultOrigins <approved-https-result-origin> -OutputDirectory <new-absolute-directory>
```

在「设置 → 第三方适配器」导入目录，选择带工作空间地址和 API Key 的百炼账号。支持账号源站绑定的主程序会使用该账号的 HTTPS 工作空间源站。面向固定工作空间的本机候选可显式传入 `-ApiOrigin`，再选对应账号。结果下载不带 API Key。文件型视频与音频上传的对象存储配置属于独立能力。

[万相 3.0 视频接口](https://help.aliyun.com/zh/model-studio/wan3-video-generation-api-reference) · [百炼图像接口](https://help.aliyun.com/zh/model-studio/qwen-image-generation-and-editing-api-reference)
