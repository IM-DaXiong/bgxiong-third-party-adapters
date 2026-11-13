# ASCII only; Windows PowerShell 5.1. Resource preparation never starts inference.
param(
    [Parameter(Mandatory = $true)][string]$AppDataRoot,
    [Parameter(Mandatory = $true)][string]$OfflineRoot
)
$ErrorActionPreference = 'Stop'
$config = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'provisioning.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if (-not [IO.Path]::IsPathRooted($AppDataRoot) -or -not [IO.Path]::IsPathRooted($OfflineRoot)) { throw 'AppDataRoot and OfflineRoot must be absolute' }
$AppDataRoot = [IO.Path]::GetFullPath($AppDataRoot).TrimEnd('\')
$OfflineRoot = [IO.Path]::GetFullPath($OfflineRoot).TrimEnd('\')
if (-not (Test-Path -LiteralPath $OfflineRoot -PathType Container)) { throw "OfflineRoot does not exist: $OfflineRoot" }
function Resolve-Under([string]$Root, [string]$Relative) {
    if (-not $Relative -or [IO.Path]::IsPathRooted($Relative) -or $Relative.Contains(':')) { throw 'Resource path must be relative' }
    $path = [IO.Path]::GetFullPath((Join-Path $Root $Relative))
    if (-not $path.StartsWith($Root + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Resource path escapes its root' }
    $ancestor = $path
    while ($ancestor.Length -gt $Root.Length) {
        if (Test-Path -LiteralPath $ancestor) {
            if (((Get-Item -LiteralPath $ancestor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "Resource path contains a link: $ancestor" }
        }
        $ancestor = Split-Path -Parent $ancestor
    }
    return $path
}
function Resolve-Target([string]$Relative) { return Resolve-Under $AppDataRoot $Relative }
function Write-Json([string]$Path, $Value) {
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Path) | Out-Null
    [IO.File]::WriteAllText($Path, (($Value | ConvertTo-Json -Depth 15) + "`n"), [Text.UTF8Encoding]::new($false))
}
# All sources are checked before changing installed resources. No download/cache duplicate.
foreach ($artifact in $config.artifacts) {
    $source = Resolve-Under $OfflineRoot $artifact.relativePath
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw "Resource archive missing: $source" }
    if ((Get-Item -LiteralPath $source).Length -ne [int64]$artifact.bytes) { throw "Resource size mismatch: $source" }
    Write-Host "Verifying $($artifact.id)"
    if ((Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant() -ne $artifact.sha256) { throw "Resource SHA256 mismatch: $source" }
    Resolve-Target $artifact.target | Out-Null
}
foreach ($artifact in $config.artifacts) {
    $target = Resolve-Target $artifact.target
    $receipt = Join-Path $target ('.resource-' + $artifact.id + '.json')
    $owned = Test-Path -LiteralPath $receipt -PathType Leaf
    if ($owned) {
        $saved = Get-Content -LiteralPath $receipt -Raw -Encoding UTF8 | ConvertFrom-Json
        if ($saved.sha256 -ne $artifact.sha256 -or $saved.version -ne $config.version) { throw "Installed resource identity differs: $($artifact.id)" }
        $complete = $true
        foreach ($marker in $artifact.requiredFiles) {
            if (-not (Test-Path -LiteralPath (Join-Path $target $marker) -PathType Leaf)) { $complete = $false }
        }
        if ($complete) { continue }
    } elseif ((Test-Path -LiteralPath $target) -and @(Get-ChildItem -LiteralPath $target -Force).Count -gt 0) {
        throw "Unowned resource target is not empty: $target"
    }
    $source = Resolve-Under $OfflineRoot $artifact.relativePath
    $stage = $target + '.preparing.' + [Guid]::NewGuid().ToString('N')
    New-Item -ItemType Directory -Force -Path $stage | Out-Null
    Write-Host "Extracting $($artifact.id) to $stage"
    if ($artifact.format -eq 'tar.gz') {
        & tar.exe -xf $source -C $stage
        if ($LASTEXITCODE -ne 0) { throw "tar extraction failed: $($artifact.id); incomplete stage=$stage" }
    } elseif ($artifact.format -eq 'zip') {
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        [IO.Compression.ZipFile]::ExtractToDirectory($source, $stage)
    } else { throw "Unsupported archive format: $($artifact.format)" }
    $payload = $stage
    if ($artifact.stripRoot) { $payload = Resolve-Under $stage $artifact.stripRoot }
    foreach ($marker in $artifact.requiredFiles) {
        if (-not (Test-Path -LiteralPath (Join-Path $payload $marker) -PathType Leaf)) { throw "Resource incomplete: $($artifact.id)/$marker; stage=$stage" }
    }
    Write-Json (Join-Path $payload ('.resource-' + $artifact.id + '.json')) @{sha256=$artifact.sha256; version=$config.version}
    # Preserve the old owned resource on repair; never erase user/unknown directories.
    if (Test-Path -LiteralPath $target) {
        $previous = $target + '.previous.' + [Guid]::NewGuid().ToString('N')
        Move-Item -LiteralPath $target -Destination $previous
        Write-Host "Previous resource preserved: $previous"
    }
    Move-Item -LiteralPath $payload -Destination $target
}
$runtimeRoot = Resolve-Target $config.runtimeRoot
$hostConfig = @{speechManaged=@{
    schemaVersion=1; runtimeRoot=$runtimeRoot; storeRoot=(Resolve-Target $config.storeRoot)
    managedPython=(Resolve-Target $config.managedPython); healthPath='/health'
    launcher=$config.launcher; readyTimeoutSecs=900
}}
$python = $hostConfig.speechManaged.managedPython
& $python -c 'import encodings, venv, ensurepip'
if ($LASTEXITCODE -ne 0) { throw 'Managed Python is incomplete' }
$venv = Join-Path $runtimeRoot '.venv'
$requirements = (Get-FileHash -LiteralPath (Resolve-Target $config.dependencyRequirements) -Algorithm SHA256).Hash.ToLowerInvariant()
$installed = Resolve-Target $config.dependencyMarker
$ready = (Test-Path -LiteralPath (Resolve-Target $config.dependencyPython)) -and (Test-Path -LiteralPath $installed)
if ($ready) { $ready = (Get-Content -LiteralPath $installed -Raw).Trim() -eq $requirements }
if (-not $ready) {
    & $python -m venv $venv
    if ($LASTEXITCODE -ne 0) { throw 'Could not create runtime venv' }
    & (Resolve-Target $config.dependencyPython) -m pip install --no-index --only-binary=:all: --disable-pip-version-check --no-input --find-links (Join-Path $hostConfig.speechManaged.storeRoot 'wheelhouse') -r (Join-Path $runtimeRoot 'requirements.lock.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Frozen offline dependencies could not be installed' }
    & (Resolve-Target $config.dependencyPython) -m pip check
    if ($LASTEXITCODE -ne 0) { throw 'Offline dependency consistency check failed' }
    [IO.File]::WriteAllText($installed, $requirements, [Text.Encoding]::ASCII)
}
$hostPath = Resolve-Target $config.hostConfig
Write-Json $hostPath $hostConfig
Write-Host "Host configuration prepared: $hostPath"
