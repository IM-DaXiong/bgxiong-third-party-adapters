param(
    [Parameter(Mandatory=$true)][string]$Sdk,
    [Parameter(Mandatory=$true)][string]$OutputDirectory,
    [Parameter(Mandatory=$true)][string]$EditorDirectory,
    [string]$Cli,
    [string]$Worker,
    [switch]$Online
)
$ErrorActionPreference = 'Stop'
$Sdk = (Resolve-Path -LiteralPath $Sdk).Path
$OutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory, (Get-Location).Path)
$Project = $PSScriptRoot
if (Test-Path -LiteralPath $OutputDirectory) { throw 'OutputDirectory must not exist.' }
$TemplatePath = Join-Path $Project 'installation/adapter-install.template.json'
$Installation = Get-Content -LiteralPath $TemplatePath -Raw | ConvertFrom-Json
if ($Installation.schemaVersion -ne 1 -or $Installation.packageDirectory -ne 'package') { throw 'Invalid installation layout.' }
if ($Installation.hostConfig.credentials.Count -ne 0 -or $Installation.authentication.source -ne 'none') {
    throw 'This local ComfyUI adapter does not request credentials.'
}
if (-not $Cli) { $Cli = Join-Path $Sdk 'bin/bgx-adapter-v2.exe' }
if (-not $Worker) { $Worker = Join-Path $Sdk 'bin/bgx-adapter-v2-worker.exe' }
foreach ($tool in @($Cli, $Worker)) {
    if (-not (Test-Path -LiteralPath $tool -PathType Leaf)) { throw "Required SDK tool missing: $tool" }
}
$Cli = (Resolve-Path -LiteralPath $Cli).Path
$Worker = (Resolve-Path -LiteralPath $Worker).Path
$EditorDirectory = (Resolve-Path -LiteralPath $EditorDirectory).Path
$EditorReceipt = Get-Content -LiteralPath (Join-Path $EditorDirectory 'editor-build.json') -Raw | ConvertFrom-Json
$SourceLock = Get-Content -LiteralPath (Join-Path $Project 'editor/upstream.lock.json') -Raw | ConvertFrom-Json
if ($EditorReceipt.upstream.commit -ne $SourceLock.commit) { throw 'Editor source differs from the pinned upstream.' }
if ($EditorReceipt.upstreamLockSha256 -ne (Get-FileHash -LiteralPath (Join-Path $Project 'editor/upstream.lock.json') -Algorithm SHA256).Hash.ToLowerInvariant()) { throw 'Editor upstream/patch lock differs from this source batch.' }
foreach ($source in $EditorReceipt.bridgeSources) {
    if ($source.path -notin @('build.mjs','studio.js','actions.js','state-preservation.js','connected-scenes.js','embedded.css')) { throw 'Unexpected bridge source in editor receipt.' }
    if ($source.sha256 -ne (Get-FileHash -LiteralPath (Join-Path $Project "editor/$($source.path)") -Algorithm SHA256).Hash.ToLowerInvariant()) { throw "Editor bridge source changed: $($source.path)" }
}
if (@($EditorReceipt.bridgeSources).Count -ne 6 -or @($EditorReceipt.bridgeSources.path | Sort-Object -Unique).Count -ne 6) { throw 'Editor bridge source receipt is incomplete or duplicated.' }
$EditorFile = Join-Path $EditorDirectory 'editor-bundle.json'
$EditorEntry = @($EditorReceipt.files | Where-Object { $_.path -eq 'editor-bundle.json' })
$EditorHash = (Get-FileHash -LiteralPath $EditorFile -Algorithm SHA256).Hash.ToLowerInvariant()
if ($EditorEntry.Count -ne 1 -or $EditorEntry[0].sha256 -ne $EditorHash) { throw 'Editor companion digest mismatch.' }
$BuildArguments = @('build', '--sdk', $Sdk, '--project', $Project)
if (-not $Online) { $BuildArguments += '--offline' }
& $Cli @BuildArguments
if ($LASTEXITCODE -ne 0) { throw 'Standard SDK build failed.' }
& $Cli check --sdk $Sdk --worker $Worker --project $Project
if ($LASTEXITCODE -ne 0) { throw 'Source package validation failed.' }
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
$PackageDirectory = Join-Path $OutputDirectory 'package'
& $Cli pack --sdk $Sdk --project $Project --out $PackageDirectory
if ($LASTEXITCODE -ne 0) { throw 'Standard SDK pack failed.' }
& $Cli check --sdk $Sdk --worker $Worker --project $PackageDirectory
if ($LASTEXITCODE -ne 0) { throw 'Runtime package validation failed.' }
$Installation.manifestSha256 = (Get-FileHash -LiteralPath (Join-Path $PackageDirectory 'manifest.json') -Algorithm SHA256).Hash.ToLowerInvariant()
$Installation | Add-Member -NotePropertyName editor -NotePropertyValue @{ file='editor-bundle.json'; sha256=$EditorHash } -Force
Copy-Item -LiteralPath $EditorFile -Destination (Join-Path $OutputDirectory 'editor-bundle.json')
Copy-Item -LiteralPath (Join-Path $EditorDirectory 'UPSTREAM-LICENSE.txt') -Destination (Join-Path $OutputDirectory 'PANORAMA-LICENSE.txt')
Copy-Item -LiteralPath (Join-Path $EditorDirectory 'fonts/OFL.txt') -Destination (Join-Path $OutputDirectory 'FONT-LICENSE.txt')
Copy-Item -LiteralPath (Join-Path $EditorDirectory 'upstream.lock.json') -Destination (Join-Path $OutputDirectory 'upstream.lock.json')
$Installation | ConvertTo-Json -Depth 32 | Set-Content -LiteralPath (Join-Path $OutputDirectory 'adapter-install.json') -Encoding utf8NoBOM
$Saved = Get-Content -LiteralPath (Join-Path $OutputDirectory 'adapter-install.json') -Raw | ConvertFrom-Json
if ($Saved.manifestSha256 -ne (Get-FileHash -LiteralPath (Join-Path $PackageDirectory 'manifest.json') -Algorithm SHA256).Hash.ToLowerInvariant()) { throw 'Installation manifest digest mismatch.' }
Write-Output "Installable package: $OutputDirectory"
