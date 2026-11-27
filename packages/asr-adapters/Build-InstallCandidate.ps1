param(
    [Parameter(Mandatory = $true)][string]$Project,
    [Parameter(Mandatory = $true)][string]$SdkBuild,
    [string[]]$OssOrigins = @()
)
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath $Project).Path
$sdkBuildRoot = (Resolve-Path -LiteralPath $SdkBuild).Path
$repo = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
$sdkSource = Join-Path (Split-Path $repo) 'bgxiong-adapter-sdk'
$receipt = Get-Content -LiteralPath (Join-Path $sdkBuildRoot 'build-receipt.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$sourceCommit = git -C $sdkSource rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $sourceCommit -ne $receipt.sourceCommit) { throw 'SDK source and build receipt differ.' }
$pendingSdk = git -C $sdkSource status --porcelain
if ($LASTEXITCODE -ne 0 -or $pendingSdk) { throw 'SDK source must be clean.' }

$manifest = Get-Content -LiteralPath (Join-Path $projectRoot 'manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json -AsHashtable
$async = $manifest.pluginId -ne 'local.bgx.bailian-qwen3-asr-flash'
if ($async -and $OssOrigins.Count -lt 2) { throw 'Supply the exact approved OSS upload and result origins.' }
if (-not $async -and $OssOrigins.Count) { throw 'The synchronous Qwen package does not use OSS.' }
foreach ($origin in $OssOrigins) {
    if ($origin -cnotmatch '^https://dashscope-[a-z0-9-]+\.oss-cn-beijing\.aliyuncs\.com$') {
        throw 'An OSS origin must be an exact Beijing DashScope HTTPS origin.'
    }
}

$buildCache = Join-Path $PSScriptRoot 'bailian-asr-client/target'
$target = Join-Path $buildCache 'wasm32-wasip2/release'
cargo build --locked --release --target wasm32-wasip2 --target-dir $buildCache --manifest-path (Join-Path $projectRoot 'Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw 'Wasm build failed.' }
$name = (Split-Path $projectRoot -Leaf) -replace '-', '_'
$wasm = Join-Path $target ($name + '.wasm')
function Get-Sha([string]$File) { (Get-FileHash -LiteralPath $File -Algorithm SHA256).Hash.ToLowerInvariant() }
function Write-Json([string]$File, $Value) {
    $json = ($Value | ConvertTo-Json -Depth 30) -replace "`r`n", "`n"
    [IO.File]::WriteAllText($File, $json + [char]10, [Text.UTF8Encoding]::new($false))
}

$wasmSha = Get-Sha $wasm
if ($manifest.artifactSha256 -notmatch '^0{64}$' -and $manifest.artifactSha256 -ne $wasmSha) {
    throw 'Built Wasm differs from the source manifest. Verify the build inputs or increment the package version.'
}
$manifest.artifactSha256 = $wasmSha
$stage = Join-Path $buildCache ('install-' + [guid]::NewGuid().ToString('N'))
$package = Join-Path $stage 'package'
New-Item -ItemType Directory -Path $package -Force | Out-Null
$entry = Join-Path $package 'adapter.wasm'
Copy-Item -LiteralPath $wasm -Destination $entry
$manifestPath = Join-Path $package 'manifest.json'
Write-Json $manifestPath $manifest
$manifestSha = Get-Sha $manifestPath
[IO.File]::WriteAllLines((Join-Path $package 'manifest.sha256'), @("$($manifest.artifactSha256) adapter.wasm", "$manifestSha manifest.json"))

$runtime = Join-Path $sdkBuildRoot 'runtime/adapter-sdk-v2'
$cli = Join-Path $sdkBuildRoot 'runtime/bgx-adapter-v2.exe'
$worker = Join-Path $runtime 'bgx-adapter-v2-worker.exe'
& $cli check --sdk $runtime --worker $worker --project $package
if ($LASTEXITCODE -ne 0) { throw "SDK package check failed for $($manifest.pluginId)." }

$envelope = Get-Content -LiteralPath (Join-Path $projectRoot 'installation/adapter-install.template.json') -Raw -Encoding UTF8 | ConvertFrom-Json -AsHashtable
$envelope.manifestSha256 = $manifestSha
$envelope.hostConfig.network.origins = @($OssOrigins | Select-Object -Unique)
Write-Json (Join-Path $stage 'adapter-install.json') $envelope
Write-Json (Join-Path $stage 'sdk-source-lock.json') @{sourceCommit = $sourceCommit; sdkRelease = $receipt.sdkRelease }
Copy-Item -LiteralPath (Join-Path $repo 'LICENSE'),(Join-Path $projectRoot 'README.md') -Destination $stage

$output = Join-Path $repo "artifacts/current/$($manifest.pluginId)/$($manifest.version)"
if (Test-Path -LiteralPath $output) {
    $oldManifest = Join-Path $output 'package/manifest.json'
    $oldEnvelope = Join-Path $output 'adapter-install.json'
    if ((Get-Sha $oldManifest) -ne $manifestSha -or (Get-Sha $oldEnvelope) -ne (Get-Sha (Join-Path $stage 'adapter-install.json'))) {
        throw 'Different bytes already exist for this version. Build a new version.'
    }
} else {
    New-Item -ItemType Directory -Path (Split-Path $output) -Force | Out-Null
    Move-Item -LiteralPath $stage -Destination $output
}
[pscustomobject]@{ directory = $output; pluginId = $manifest.pluginId; manifestSha256 = $manifestSha; wasmSha256 = $manifest.artifactSha256 } | ConvertTo-Json -Compress
