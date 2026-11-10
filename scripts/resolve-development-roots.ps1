# Resolve the product workflow checkout and one validated SDK build input.
function Get-AdapterDevelopmentRoots {
    param(
        [string]$FromDirectory,
        [string]$HostRoot,
        [string]$SdkRoot,
        [string]$ArtifactRoot,
        [string]$Cli,
        [string]$Worker
    )
    $adapterRoot = (& git -C $FromDirectory rev-parse --show-toplevel) -join ''
    if ($LASTEXITCODE -ne 0) { throw 'Run the exporter from the adapter Git checkout' }
    $adapterRemote = (& git -C $adapterRoot remote get-url origin) -join ''
    if ($LASTEXITCODE -ne 0 -or $adapterRemote -notmatch '^(https://github\.com/IM-DaXiong/|git@github\.com:IM-DaXiong/)bgxiong-third-party-adapters(\.git)?$') {
        throw 'Unexpected adapter repository'
    }
    if (-not $HostRoot) { $HostRoot = Join-Path (Split-Path -Parent $adapterRoot) 'bgxiong-ai-story' }
    $hostPath = [IO.Path]::GetFullPath($HostRoot)
    $actualHost = (& git -C $hostPath rev-parse --show-toplevel) -join ''
    if ($LASTEXITCODE -ne 0 -or [IO.Path]::GetFullPath($actualHost) -ne $hostPath) { throw "Missing host Git checkout: $hostPath" }
    $hostRemote = (& git -C $hostPath remote get-url origin) -join ''
    if ($LASTEXITCODE -ne 0 -or $hostRemote -notmatch '^(https://github\.com/IM-DaXiong/|git@github\.com:IM-DaXiong/)bgxiong-ai-story-dev(\.git)?$') {
        throw 'Unexpected host repository'
    }
    $dependency = Join-Path $hostPath 'scripts/adapter-sdk-dependency.py'
    $arguments = @($dependency, '--read')
    if ($ArtifactRoot) { $arguments = @($dependency, '--inspect-artifact', $ArtifactRoot) }
    $raw = & python @arguments
    if ($LASTEXITCODE -ne 0) { throw 'Prepare the SDK in the host checkout before exporting author kits' }
    $sdk = ($raw -join "`n") | ConvertFrom-Json
    if ($SdkRoot -and [IO.Path]::GetFullPath($SdkRoot) -ne [IO.Path]::GetFullPath($sdk.sourceRoot)) {
        throw 'SdkRoot differs from the selected build receipt'
    }
    if (-not $Cli) { $Cli = $sdk.cli }
    if (-not $Worker) { $Worker = $sdk.worker }
    if ((Get-FileHash -LiteralPath $Cli).Hash -ne (Get-FileHash -LiteralPath $sdk.cli).Hash -or
        (Get-FileHash -LiteralPath $Worker).Hash -ne (Get-FileHash -LiteralPath $sdk.worker).Hash) {
        throw 'CLI or worker differs from the selected SDK build receipt'
    }
    $hostRevision = (& git -C $hostPath rev-parse HEAD) -join ''
    if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve the host workflow commit' }
    $adapterRevision = (& git -C $adapterRoot rev-parse HEAD) -join ''
    if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve the adapter source commit' }
    $adapterChanges = (& git -C $adapterRoot status --porcelain --untracked-files=normal) -join "`n"
    if ($LASTEXITCODE -ne 0) { throw 'Cannot verify the adapter source state' }
    return [pscustomobject]@{
        AdapterRoot = [IO.Path]::GetFullPath($adapterRoot)
        HostRoot = $hostPath
        SdkRoot = $sdk.sourceRoot
        ArtifactRoot = $sdk.artifactRoot
        Cli = $Cli
        Worker = $Worker
        HostRevision = $hostRevision
        AdapterRevision = $adapterRevision
        AdapterDirty = [bool]$adapterChanges
        SdkRevision = $sdk.sourceCommit
        SdkBuildReceiptSha256 = $sdk.receiptSha256
    }
}

function Copy-AdapterAuthorSdk($Roots, [string]$Destination) {
    & robocopy $Roots.SdkRoot $Destination /E /XJ /XD target node_modules .git .bgx-sdk __pycache__ /XF AGENTS.md .gitattributes /NFL /NDL /NJH /NJS /NP | Out-Null
    if ($LASTEXITCODE -ge 8) { throw "Public SDK copy failed: $LASTEXITCODE" }
    $bin = New-Item -ItemType Directory -Path (Join-Path $Destination 'bin') -Force
    Copy-Item -LiteralPath $Roots.Cli -Destination (Join-Path $bin.FullName 'bgx-adapter-v2.exe')
    Copy-Item -LiteralPath $Roots.Worker -Destination (Join-Path $bin.FullName 'bgx-adapter-v2-worker.exe')
}

function Write-AdapterExportSource($Roots, [string]$OutputDirectory, [hashtable]$Details = @{}) {
    $identity = @{
        schemaVersion = 2
        hostWorkflowCommit = $Roots.HostRevision
        adapterCommit = $Roots.AdapterRevision
        adapterWorkingTreeDirty = $Roots.AdapterDirty
        sdkCommit = $Roots.SdkRevision
        sdkBuildReceiptSha256 = $Roots.SdkBuildReceiptSha256
    }
    foreach ($key in $Details.Keys) { $identity.Add($key, $Details[$key]) }
    [IO.File]::WriteAllText(
        (Join-Path $OutputDirectory 'SOURCE.json'),
        (($identity | ConvertTo-Json -Depth 8) + "`n"),
        [Text.UTF8Encoding]::new($false)
    )
}
