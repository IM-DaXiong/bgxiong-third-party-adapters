param(
    [Parameter(Mandatory=$true)][string]$SdkRoot,
    [Parameter(Mandatory=$true)][string]$SdkCli,
    [Parameter(Mandatory=$true)][string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
$speechProject = $PSScriptRoot
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Output must be a new directory' }
Push-Location $speechProject
try {
    & cargo +1.95.0 build --release --locked --offline --target wasm32-wasip2
    if ($LASTEXITCODE -ne 0) { throw 'WASM build failed' }
    Copy-Item -LiteralPath (Join-Path $speechProject 'target/wasm32-wasip2/release/bgx_wan22_s2v.wasm') -Destination (Join-Path $speechProject 'adapter.wasm')
    $speechManifest = Join-Path $speechProject 'manifest.json'
    $speechData = Get-Content -LiteralPath $speechManifest -Raw | ConvertFrom-Json
    $speechData.artifactSha256 = (Get-FileHash -LiteralPath (Join-Path $speechProject 'adapter.wasm') -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText($speechManifest, ($speechData | ConvertTo-Json -Depth 20) + "`n", [Text.UTF8Encoding]::new($false))
    $speechManifestHash = (Get-FileHash -LiteralPath $speechManifest -Algorithm SHA256).Hash.ToLowerInvariant()
    $speechInventory = $speechData.artifactSha256 + " adapter.wasm`n" + $speechManifestHash + " manifest.json`n"
    [IO.File]::WriteAllText((Join-Path $speechProject 'manifest.sha256'), $speechInventory, [Text.UTF8Encoding]::new($false))
    & $SdkCli pack --sdk $SdkRoot --project $speechProject --out $OutputDirectory
    if ($LASTEXITCODE -ne 0) { throw 'SDK pack failed' }
} finally { Pop-Location }
