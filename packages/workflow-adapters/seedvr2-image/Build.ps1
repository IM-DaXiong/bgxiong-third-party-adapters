[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Sdk,
    [Parameter(Mandatory)][string]$RuntimeSdk,
    [Parameter(Mandatory)][string]$Cli,
    [Parameter(Mandatory)][string]$Worker,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [switch]$Offline
)
$ErrorActionPreference = 'Stop'
foreach ($path in @($Sdk,$RuntimeSdk,$Cli,$Worker,$OutputDirectory)) {
    if (-not [IO.Path]::IsPathFullyQualified($path)) { throw 'All paths must be absolute.' }
}
if (Test-Path -LiteralPath $OutputDirectory) { throw 'OutputDirectory already exists; preserve accepted artifacts.' }
$buildArgs = @('build','--sdk',$Sdk,'--project',$PSScriptRoot)
if ($Offline) { $buildArgs += '--offline' }
& $Cli @buildArgs
if ($LASTEXITCODE -ne 0) { throw "SDK author build failed: $LASTEXITCODE" }
& $Cli check --sdk $RuntimeSdk --worker $Worker --project $PSScriptRoot
if ($LASTEXITCODE -ne 0) { throw "SDK package check failed: $LASTEXITCODE" }
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
$runtime = Join-Path $OutputDirectory 'runtime'
& $Cli pack --sdk $Sdk --project $PSScriptRoot --out $runtime
if ($LASTEXITCODE -ne 0) { throw "SDK pack failed: $LASTEXITCODE" }
& $Cli check --sdk $RuntimeSdk --worker $Worker --project $runtime
if ($LASTEXITCODE -ne 0) { throw "Packed runtime check failed: $LASTEXITCODE" }
$install = Join-Path $OutputDirectory 'install'
$package = Join-Path $install 'package'
$workflow = Join-Path $install 'workflow'
$templateFile = Join-Path $PSScriptRoot 'workflow-templates.json'
$sourceGraph = Join-Path $PSScriptRoot 'workflow/source.json'
$sourceDigest = (Get-FileHash -LiteralPath $sourceGraph -Algorithm SHA256).Hash.ToLowerInvariant()
$templates = Get-Content -Raw -LiteralPath $templateFile | ConvertFrom-Json
if ($sourceDigest -notin @($templates.templates.sha256)) {
    throw 'Workflow source bytes differ from the declared template.'
}
New-Item -ItemType Directory -Path $package,$workflow | Out-Null
Get-ChildItem -LiteralPath $runtime -File | Copy-Item -Destination $package
Copy-Item -LiteralPath $templateFile -Destination $package
Copy-Item -LiteralPath $sourceGraph -Destination (Join-Path $workflow 'source.json')
$descriptor = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot 'installation/adapter-install.template.json') | ConvertFrom-Json
$descriptor.manifestSha256 = (Get-FileHash -LiteralPath (Join-Path $package 'manifest.json') -Algorithm SHA256).Hash.ToLowerInvariant()
$descriptor | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath (Join-Path $install 'adapter-install.json') -Encoding utf8
Write-Output "Install directory: $install"
