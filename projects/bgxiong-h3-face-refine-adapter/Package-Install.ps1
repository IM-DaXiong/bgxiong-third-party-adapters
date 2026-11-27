param([string]$ValidatedWasmPath = '')
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$versionLine = Get-Content -LiteralPath (Join-Path $root 'Cargo.toml') | Where-Object { $_ -match '^version = "([^"]+)"$' } | Select-Object -First 1
if ($versionLine -notmatch '^version = "([^"]+)"$') { throw 'Missing package version' }
$version = $Matches[1]
$repo = Split-Path -Parent (Split-Path -Parent $root)
$archive = Join-Path $repo 'artifacts/current/local.thirdparty.h3-face-refine'
$destination = Join-Path $archive $version
if (Test-Path -LiteralPath $destination) { throw "Refusing to overwrite distribution: $destination" }
$wasm = if ($ValidatedWasmPath) { [IO.Path]::GetFullPath($ValidatedWasmPath) } else { Join-Path $root 'adapter.wasm' }
if (-not (Test-Path -LiteralPath $wasm)) { throw 'Build/check with the public SDK first, then provide its validated WASM.' }
$manifestPath = Join-Path $root 'manifest.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
if ($manifest.version -ne $version -or $manifest.artifactSha256 -ne (Get-FileHash -LiteralPath $wasm -Algorithm SHA256).Hash.ToLowerInvariant()) { throw 'Validated manifest, source version and WASM differ.' }
$utf8 = [Text.UTF8Encoding]::new($false)
# Preserve the SDK-validated manifest bytes; reserialization changes its identity.
if ([IO.Path]::GetFullPath($wasm) -ne [IO.Path]::GetFullPath((Join-Path $root 'adapter.wasm'))) { Copy-Item -LiteralPath $wasm -Destination (Join-Path $root 'adapter.wasm') }
$manifestHash = (Get-FileHash -LiteralPath $manifestPath -Algorithm SHA256).Hash.ToLowerInvariant()
$inventory = "$manifestHash manifest.json`n$($manifest.artifactSha256) adapter.wasm`n"
[IO.File]::WriteAllText((Join-Path $root 'manifest.sha256'), $inventory, $utf8)
$sourceRecord = Get-Content -LiteralPath (Join-Path $root 'SOURCE.json') -Raw | ConvertFrom-Json
if ($sourceRecord.version -ne $version) { throw 'SOURCE.json and the package version differ.' }
$out = Join-Path $archive ('.stage-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path (Join-Path $out 'package') -Force | Out-Null
foreach ($name in @('manifest.json','manifest.sha256','adapter.wasm')) { Copy-Item -LiteralPath (Join-Path $root $name) -Destination (Join-Path $out 'package') }
$code = [Collections.Generic.List[object]]::new()
foreach ($mount in @(@{source='companion/upstream/ComfyUI-H3-FaceRefine'; name='ComfyUI-H3-FaceRefine'}, @{source='companion/custom_nodes/BGXFaceRefineContract'; name='BGXFaceRefineContract'})) {
    $source = (Resolve-Path -LiteralPath (Join-Path $root $mount.source)).Path
    foreach ($file in Get-ChildItem -LiteralPath $source -File -Recurse) {
        $relative = $file.FullName.Substring($source.Length + 1).Replace('\','/')
        if ($relative -match '(^|/)(__pycache__|\.git)(/|$)' -or $relative.EndsWith('.pyc')) { continue }
        $path = 'comfy-nodes/' + $mount.name + '/' + $relative
        $dest = Join-Path $out $path
        New-Item -ItemType Directory -Path (Split-Path $dest -Parent) -Force | Out-Null
        Copy-Item -LiteralPath $file.FullName -Destination $dest
        $code.Add(@{path=$path; sha256=(Get-FileHash -LiteralPath $dest -Algorithm SHA256).Hash.ToLowerInvariant()})
    }
}
$descriptor = @{
    schemaVersion=1; packageDirectory='package'; manifestSha256=$manifestHash
    authentication=@{source='none'}; managedMediaProbe=$true
    comfyui=@{customNodes='comfy-nodes'; files=@($code.ToArray() | Sort-Object path)}
    hostConfig=@{
        bindingId='h3-face-refine-saved-comfy'; bindingRevision=1; configurationRef='h3-face-refine-saved-comfy-v1'
        configuration=@{timeoutMs=600000}
        network=@{origins=@(); allowPrivate=$true; allowEmbeddedAuthorization=$false; deniedHosts=@(); deniedNetworks=@()}
        credentials=@(); trust=@{publishers=@(); revokedPlugins=@(); revokedVersions=@(); revokedKeys=@(); denylistedHosts=@(); isFresh=$false; supportedFeatures=@('media-target-range-repair-v1','opaque-file-upload-name-v1','media-target-selection-v1','operation-input-preflight-v2','submission-rejection-v1')}
        mediaTtlMs=3600000; transport=@{handshakeTimeoutMs=10000; writeTimeoutMs=60000; shutdownTimeoutMs=10000; cancelGraceMs=100}
        budgetMs=@{capabilities=60000; probe=60000; submit=600000; query=600000; cancel=60000}; probe=$null
    }
}
[IO.File]::WriteAllText((Join-Path $out 'adapter-install.json'), (($descriptor | ConvertTo-Json -Depth 40).Replace("`r`n", "`n")) + "`n", $utf8)
Copy-Item -LiteralPath (Join-Path $root 'README.zh-CN.md') -Destination (Join-Path $out 'README.zh-CN.md')
Copy-Item -LiteralPath (Join-Path $root 'LICENSE-NOTES.md') -Destination (Join-Path $out 'LICENSE-NOTES.md')
foreach ($item in $code) {
    if ((Get-FileHash -LiteralPath (Join-Path $out $item.path) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $item.sha256) {
        throw "Companion digest mismatch: $($item.path)"
    }
}
if ((Get-FileHash -LiteralPath (Join-Path $out 'package/manifest.json')).Hash.ToLowerInvariant() -ne $manifestHash -or
    (Get-FileHash -LiteralPath (Join-Path $out 'package/adapter.wasm')).Hash.ToLowerInvariant() -ne $manifest.artifactSha256) {
    throw 'Staged package identity differs from the validated package.'
}
[IO.Directory]::Move($out, $destination)
Write-Output "INSTALL_PACKAGE=$destination NODE_FILES=$($code.Count) VERSION=$version"
