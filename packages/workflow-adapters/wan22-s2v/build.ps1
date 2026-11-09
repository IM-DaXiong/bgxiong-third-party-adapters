param(
    [Parameter(Mandatory=$true)][string]$SdkRoot,
    [Parameter(Mandatory=$true)][string]$SdkCli,
    [Parameter(Mandatory=$true)][string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
$speechProject = $PSScriptRoot
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Output must be a new directory' }
$previousFlags = $env:CARGO_ENCODED_RUSTFLAGS
$cargoCache = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$publicRepo = (Resolve-Path (Join-Path $speechProject '../../..')).Path
$publicSdk = (Resolve-Path -LiteralPath $SdkRoot).Path
$remap = @('--remap-path-prefix==/bgx-author')
foreach ($mapping in @(@($cargoCache, '/bgx-cargo'), @($publicRepo, '/bgx-adapters'), @($publicSdk, '/bgx-sdk'))) {
    $path = [IO.Path]::GetFullPath($mapping[0])
    $remap += "--remap-path-prefix=$path=$($mapping[1])"
    $remap += "--remap-path-prefix=$($path.Replace('\', '/'))=$($mapping[1])"
}
$env:CARGO_ENCODED_RUSTFLAGS = $remap -join [char]31
Push-Location $speechProject
try {
    & cargo +1.95.0 build --release --locked --offline --target wasm32-wasip2
    if ($LASTEXITCODE -ne 0) { throw 'WASM build failed' }
    Copy-Item -LiteralPath (Join-Path $speechProject 'target/wasm32-wasip2/release/bgx_wan22_s2v.wasm') -Destination (Join-Path $speechProject 'adapter.wasm')
    $speechManifest = Join-Path $speechProject 'manifest.json'
    $manifestText = Get-Content -LiteralPath $speechManifest -Raw -Encoding UTF8
    $previousDigest = ($manifestText | ConvertFrom-Json).artifactSha256
    $artifactDigest = (Get-FileHash -LiteralPath (Join-Path $speechProject 'adapter.wasm') -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($previousDigest -notmatch '^[0-9a-f]{64}$') { throw 'Invalid manifest artifact digest' }
    [IO.File]::WriteAllText($speechManifest, $manifestText.Replace($previousDigest, $artifactDigest).Replace("`r`n", "`n"), [Text.UTF8Encoding]::new($false))
    $speechManifestHash = (Get-FileHash -LiteralPath $speechManifest -Algorithm SHA256).Hash.ToLowerInvariant()
    $speechInventory = $artifactDigest + " adapter.wasm`n" + $speechManifestHash + " manifest.json`n"
    [IO.File]::WriteAllText((Join-Path $speechProject 'manifest.sha256'), $speechInventory, [Text.UTF8Encoding]::new($false))
    & $SdkCli pack --sdk $SdkRoot --project $speechProject --out $OutputDirectory
    if ($LASTEXITCODE -ne 0) { throw 'SDK pack failed' }
} finally {
    $env:CARGO_ENCODED_RUSTFLAGS = $previousFlags
    Pop-Location
}
