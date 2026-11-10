[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$SourceDirectory,
    [Parameter(Mandatory)][string]$ProjectDirectory,
    [Parameter(Mandatory)][string]$SdkDirectory,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [string]$Cli,
    [string]$Worker,
    [string]$HostRoot,
    [string]$SdkRoot,
    [string]$ArtifactRoot
)
$ErrorActionPreference = 'Stop'
if (-not [IO.Path]::IsPathFullyQualified($OutputDirectory)) { throw 'OutputDirectory must be absolute.' }
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Preserve the previous output; use a new directory.' }
. (Join-Path $PSScriptRoot '../../../../scripts/resolve-development-roots.ps1')
$roots = Get-AdapterDevelopmentRoots -FromDirectory $PSScriptRoot -HostRoot $HostRoot -SdkRoot $SdkRoot -ArtifactRoot $ArtifactRoot -Cli $Cli -Worker $Worker
$Cli = $roots.Cli
$Worker = $roots.Worker
foreach ($file in @($Cli, $Worker, (Join-Path $SourceDirectory "$ProjectDirectory/manifest.json"))) {
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Missing input: $file" }
}
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
$sdkOut = Join-Path $OutputDirectory $SdkDirectory
Copy-AdapterAuthorSdk $roots $sdkOut
$projectOut = Join-Path $OutputDirectory $ProjectDirectory
& robocopy (Join-Path $SourceDirectory $ProjectDirectory) $projectOut /E /XJ /XD target .bgx-sdk __pycache__ /XF adapter.wasm manifest.sha256 /NFL /NDL /NJH /NJS /NP | Out-Null
if ($LASTEXITCODE -ge 8) { throw "Author project copy failed: $LASTEXITCODE" }
# Export only current author instructions. Historical runs remain in project records.
foreach ($name in @('README.md', 'README.zh-CN.md', 'FINAL-ACCEPTANCE.zh-CN.md', 'Build-Flux2.ps1', 'Invoke-Flux2.ps1', 'Build-Continuum.ps1', 'Invoke-Continuum.ps1', 'request-two-reference.json', 'request-continuum.json', 'host-config.app.json')) {
    $source = Join-Path $SourceDirectory $name
    if (Test-Path -LiteralPath $source -PathType Leaf) { Copy-Item -LiteralPath $source -Destination $OutputDirectory }
}
Write-AdapterExportSource $roots $OutputDirectory
Write-Output "Independent author kit: $OutputDirectory"
