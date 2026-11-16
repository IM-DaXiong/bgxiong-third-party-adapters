param(
    [Parameter(Mandatory = $true)][string]$SdkBuild,
    [Parameter(Mandatory = $true)][string]$FrozenPackageDirectory,
    [Parameter(Mandatory = $true)][string]$ExpectedArtifactSha256,
    [string]$ApiOrigin,
    [Parameter(Mandatory = $true)][string[]]$ResultOrigins,
    [Parameter(Mandatory = $true)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'

function Read-HttpsOrigin([string]$Value) {
    $origin = $null
    if (-not [Uri]::TryCreate($Value, [UriKind]::Absolute, [ref]$origin) -or
        $origin.Scheme -ne 'https' -or
        $origin.HostNameType -ne [UriHostNameType]::Dns -or
        -not $origin.IsDefaultPort -or
        $origin.AbsolutePath -ne '/' -or $origin.UserInfo -or $origin.Query -or $origin.Fragment) {
        throw 'Expected an exact HTTPS DNS origin without credentials, path, query, fragment or custom port.'
    }
    return $origin.GetLeftPart([UriPartial]::Authority)
}

if (-not [IO.Path]::IsPathFullyQualified($OutputDirectory) -or (Test-Path -LiteralPath $OutputDirectory)) {
    throw 'OutputDirectory must be an absolute path that does not already exist.'
}
$api = $null
if ($PSBoundParameters.ContainsKey('ApiOrigin')) {
    $api = Read-HttpsOrigin $ApiOrigin
    if (([Uri]$api).Host -notmatch '^[a-z0-9][a-z0-9-]*\.[a-z0-9][a-z0-9-]*\.maas\.aliyuncs\.com$') {
        throw 'Expected the HTTPS workspace origin from Alibaba Cloud Model Studio.'
    }
}
$results = @($ResultOrigins | ForEach-Object { Read-HttpsOrigin $_ } | Select-Object -Unique)
if ($results.Count -eq 0) { throw 'Supply the approved DashScope result origins.' }

$sdk = (Resolve-Path -LiteralPath $SdkBuild).Path
$cli = Join-Path $sdk 'runtime/bgx-adapter-v2.exe'
$runtime = Join-Path $sdk 'runtime/adapter-sdk-v2'
$worker = Join-Path $runtime 'bgx-adapter-v2-worker.exe'
$package = (Resolve-Path -LiteralPath $FrozenPackageDirectory).Path
$authority = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$frozenManifest = Join-Path $package 'manifest.json'
$manifest = Get-Content -LiteralPath $frozenManifest -Raw -Encoding UTF8 | ConvertFrom-Json
$wasmSha256 = (Get-FileHash -LiteralPath (Join-Path $package 'adapter.wasm') -Algorithm SHA256).Hash.ToLowerInvariant()
if ($manifest.pluginId -ne $authority.pluginId -or $manifest.version -ne $authority.version -or
    $manifest.entry -ne 'adapter.wasm' -or $manifest.artifactSha256 -ne $wasmSha256 -or
    $authority.artifactSha256 -ne $wasmSha256 -or $ExpectedArtifactSha256 -ne $wasmSha256) {
    throw 'The package identity or approved Wasm digest differs from the DashScope author manifest.'
}
& $cli check --sdk $runtime --worker $worker --project $package
if ($LASTEXITCODE -ne 0) { throw 'SDK package check failed.' }

$envelope = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'installation/adapter-install.template.json') -Raw -Encoding UTF8 | ConvertFrom-Json -AsHashtable
$envelope.manifestSha256 = (Get-FileHash -LiteralPath $frozenManifest -Algorithm SHA256).Hash.ToLowerInvariant()
$envelope.hostConfig.network.origins = $results
$binding = 'selectedAccountOrigin'
if ($api) {
    $binding = 'fixedWorkspaceOrigin'
    $envelope.hostConfig.configuration['endpointBaseUrl'] = $api
    $envelope.hostConfig.network.origins = @(@($api) + $results | Select-Object -Unique)
    $envelope.authentication.fields[0].origins = @($api)
    $envelope.authentication.fields[0].Remove('accountOriginHostSuffix')
}

New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
$installPackage = Join-Path $OutputDirectory 'package'
Copy-Item -LiteralPath $package -Destination $installPackage -Recurse
if ((Get-FileHash -LiteralPath (Join-Path $installPackage 'adapter.wasm') -Algorithm SHA256).Hash.ToLowerInvariant() -ne $wasmSha256) {
    throw 'Copied Wasm bytes differ.'
}
$license = Join-Path $PSScriptRoot '../../../LICENSE'
Copy-Item -LiteralPath $license,(Join-Path $PSScriptRoot 'README.md') -Destination $OutputDirectory
$json = ($envelope | ConvertTo-Json -Depth 30) -replace "`r`n", "`n"
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'adapter-install.json'), $json + "`n", [Text.UTF8Encoding]::new($false))
ConvertTo-Json -Compress -InputObject @{directory = $OutputDirectory; pluginId = $manifest.pluginId; version = $manifest.version; wasmSha256 = $wasmSha256; binding = $binding}
