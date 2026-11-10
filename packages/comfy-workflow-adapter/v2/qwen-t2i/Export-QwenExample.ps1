[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$OutputDirectory,
    [string]$Cli,
    [string]$Worker,
    [string]$HostRoot,
    [string]$SdkRoot,
    [string]$ArtifactRoot
)
$ErrorActionPreference = 'Stop'
if (-not [IO.Path]::IsPathFullyQualified($OutputDirectory)) { throw 'OutputDirectory must be absolute.' }
if (Test-Path -LiteralPath $OutputDirectory) { throw 'OutputDirectory already exists; preserve earlier artifacts.' }
. (Join-Path $PSScriptRoot '../../../../scripts/resolve-development-roots.ps1')
$roots = Get-AdapterDevelopmentRoots -FromDirectory $PSScriptRoot -HostRoot $HostRoot -SdkRoot $SdkRoot -ArtifactRoot $ArtifactRoot -Cli $Cli -Worker $Worker
$Cli = $roots.Cli
$Worker = $roots.Worker
foreach ($file in @($Cli, $Worker)) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Missing tool: $file" }
}
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
$sdkOut = Join-Path $OutputDirectory 'vendor-adapter-v2'
Copy-AdapterAuthorSdk $roots $sdkOut
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'sample-source') -Destination $OutputDirectory -Recurse
Copy-Item -LiteralPath (Join-Path $PSScriptRoot '../tools/Build-ComfyExample.ps1') -Destination $OutputDirectory
$newScript = [IO.File]::ReadAllText((Join-Path $PSScriptRoot '../tools/New-ComfyExample.ps1')).Replace('bgx-comfy-wan22-example','bgx-comfy-qwen-t2i-example')
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'New-ComfyExample.ps1'), $newScript, [Text.UTF8Encoding]::new($false))
foreach ($name in @('README.zh-CN.md','request-t2i.json','Invoke-QwenExample.ps1')) {
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination $OutputDirectory
}
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'sample-source/workflow/source.json') -Destination (Join-Path $OutputDirectory 'qwen-image-2.1-t2i.source.json')
Write-AdapterExportSource $roots $OutputDirectory
Write-Output "Qwen T2I author kit: $OutputDirectory"
