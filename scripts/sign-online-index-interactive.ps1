# First use may capture the password via the user's private terminal.
# Remembered passwords are Windows DPAPI-protected for this account.
[CmdletBinding()]
param(
    [switch]$Preflight,
    [switch]$LocalOnly,
    [switch]$RememberPassword,
    [ValidatePattern('^[a-z0-9._-]+@[0-9]+\.[0-9]+\.[0-9]+(?:\+[0-9A-Za-z.-]+)?$')]
    [string[]]$RequireNewInstall = @()
)
$ErrorActionPreference = 'Stop'

function Test-RealExecutable([string]$Path) {
    if ([string]::IsNullOrWhiteSpace($Path)) { return $false }
    if ($Path -match '[<>"|?*]') { return $false }
    if ($Path -like '*\AppData\Local\Microsoft\WindowsApps\*') { return $false }
    $item = Get-Item -LiteralPath $Path -ErrorAction SilentlyContinue
    if ($null -eq $item -or $item.PSIsContainer) { return $false }
    if ($item.Length -le 0 -and -not $item.LinkType) { return $false }
    return $true
}

function Resolve-RealPython {
    $commands = @(Get-Command python, python3, py -CommandType Application -ErrorAction SilentlyContinue)
    foreach ($command in $commands) {
        $candidate = [string]$command.Source
        if (Test-RealExecutable $candidate) { return $candidate }
    }
    $fallbacks = @(
        (Join-Path ${env:ProgramFiles} 'Python312\python.exe'),
        (Join-Path ${env:ProgramFiles} 'Python311\python.exe'),
        (Join-Path ${env:LocalAppData} 'Programs\Python\Python312\python.exe'),
        'C:\Windows\py.exe'
    )
    foreach ($candidate in $fallbacks) {
        if (Test-RealExecutable $candidate) { return $candidate }
    }
    throw 'A real Python interpreter was not found. Install Python 3 and keep it on PATH, then rerun this script. No password was requested.'
}

function Resolve-CargoBin {
    $commands = @(Get-Command cargo -CommandType Application -ErrorAction SilentlyContinue)
    foreach ($command in $commands) {
        $candidate = [string]$command.Source
        if (Test-Path -LiteralPath $candidate) { return (Split-Path -Parent $candidate) }
    }
    $fallback = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.cargo\bin'
    if (Test-Path -LiteralPath (Join-Path $fallback 'cargo.exe')) { return $fallback }
    throw 'cargo.exe was not found. Install rustup/cargo and keep ~/.cargo/bin on PATH, then rerun this script. No password was requested.'
}

function New-ChildStartInfo([string]$FileName, [string]$Arguments, [string]$WorkingDirectory, [string]$CargoBin) {
    if (-not (Test-Path -LiteralPath $WorkingDirectory -PathType Container)) {
        throw ('Working directory does not exist: ' + $WorkingDirectory)
    }
    $start = New-Object System.Diagnostics.ProcessStartInfo
    $start.FileName = $FileName
    $start.Arguments = $Arguments
    $start.WorkingDirectory = $WorkingDirectory
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $false
    $start.EnvironmentVariables.Clear()
    $allowed = @('SYSTEMROOT','SYSTEMDRIVE','WINDIR','COMSPEC','PATH','PATHEXT','TEMP','TMP','USERPROFILE','APPDATA','LOCALAPPDATA','PROGRAMFILES','PROGRAMFILES(X86)','PROGRAMW6432','COMMONPROGRAMFILES','COMMONPROGRAMFILES(X86)','COMMONPROGRAMW6432','CARGO_HOME','RUSTUP_HOME','RUSTUP_TOOLCHAIN','HOMEDRIVE','HOMEPATH','NUMBER_OF_PROCESSORS','PROCESSOR_ARCHITECTURE')
    foreach ($name in $allowed) {
        $value = [Environment]::GetEnvironmentVariable($name, 'Process')
        if ($null -ne $value) { $start.EnvironmentVariables[$name] = $value }
    }
    $path = $start.EnvironmentVariables['PATH']
    if ([string]::IsNullOrWhiteSpace($path)) { $path = '' }
    $system32 = Join-Path $env:WINDIR 'System32'
    foreach ($extra in @($CargoBin, $system32)) {
        if ($path -notlike ('*' + $extra + '*')) { $path = $extra + ';' + $path }
    }
    $start.EnvironmentVariables['PATH'] = $path
    if (-not $start.EnvironmentVariables['CARGO_HOME']) {
        $cargoHome = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.cargo'
        if (Test-Path -LiteralPath $cargoHome) { $start.EnvironmentVariables['CARGO_HOME'] = $cargoHome }
    }
    if (-not $start.EnvironmentVariables['RUSTUP_HOME']) {
        $rustupHome = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.rustup'
        if (Test-Path -LiteralPath $rustupHome) { $start.EnvironmentVariables['RUSTUP_HOME'] = $rustupHome }
    }
    return $start
}

$root = Split-Path -Parent $PSScriptRoot
$key = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.tauri\bgxiong-adapter-index'
$public = $key + '.pub'
$protectedPassword = $key + '.password.dpapi'
if (-not (Test-Path -LiteralPath $public -PathType Leaf)) {
    throw 'The independent adapter-index public key is missing.'
}
$source = Join-Path (Split-Path -Parent $root) 'bgxiong-ai-story\src-tauri\src\comfyui\workflow\adapter_online_update\mod.rs'
# Only public trust configuration is read. No private-file content is read by this wrapper.
$text = [IO.File]::ReadAllText($source)
$match = [regex]::Match($text, 'INDEX_PUBKEY_B64:\s*&str\s*=\s*"([^"]*)"')
$publicText = [IO.File]::ReadAllText($public).Trim()
if (-not $match.Success -or -not $publicText -or $match.Groups[1].Value -cne $publicText) {
    throw 'Formal host public trust is not bound to this public key; ask the executor to finish that step first.'
}

$python = Resolve-RealPython
$cargoBin = Resolve-CargoBin
$publisher = Join-Path $PSScriptRoot 'publish-online-index.py'
$arguments = '"' + $publisher + '" --prepare-upload --pubkey "' + $public + '"'
foreach ($identity in $RequireNewInstall) {
    $arguments += ' --require-new-install "' + $identity + '"'
}

if ($LocalOnly) {
    $arguments += ' --local-only'
}
if ($Preflight) {
    $arguments += ' --check'
}
$prepare = New-ChildStartInfo $python $arguments $root $cargoBin
$prepareProcess = [Diagnostics.Process]::Start($prepare)
$prepareProcess.WaitForExit()
if ($prepareProcess.ExitCode -eq 0 -and (-not $RememberPassword -or $Preflight)) { return }
if ($prepareProcess.ExitCode -ne 0 -and ($prepareProcess.ExitCode -ne 3 -or $Preflight)) {
    throw 'CDN upload preparation failed. See the publisher output above.'
}
$tauri = Join-Path $cargoBin 'cargo-tauri.exe'
if (-not (Test-Path -LiteralPath $tauri -PathType Leaf) -or -not (Test-Path -LiteralPath $key -PathType Leaf)) {
    throw 'The Tauri signer or independent adapter-index private key is missing.'
}

# Import the module from this PowerShell installation, independent of inherited PSModulePath.
$securityModule = Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Security\Microsoft.PowerShell.Security.psd1'
Import-Module -Name $securityModule -ErrorAction Stop
$interactive = -not [Console]::IsInputRedirected -and -not [Console]::IsOutputRedirected
$signArguments = $arguments + ' --private-key-path "' + $key + '"'
if ($RememberPassword) { $signArguments += ' --verify-signing-password' }

do {
    if ((Test-Path -LiteralPath $protectedPassword -PathType Leaf) -and -not $RememberPassword) {
        $secure = ConvertTo-SecureString -String ([IO.File]::ReadAllText($protectedPassword).Trim())
    } else {
        if (-not $interactive) {
            throw 'Initialize the signing password once in your private terminal with -RememberPassword.'
        }
        $secure = Read-Host 'Adapter-index signing password' -AsSecureString
    }
    $pointer = [IntPtr]::Zero
    $sign = $null
    try {
        if ($secure.Length -eq 0) { throw 'A non-empty password is required; nothing signed.' }
        $pointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure)
        $sign = New-ChildStartInfo $python $signArguments $root $cargoBin
        $sign.EnvironmentVariables['BGX_ADAPTER_INDEX_KEY_PASSWORD'] = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer)
        $process = [Diagnostics.Process]::Start($sign)
        $sign.EnvironmentVariables.Remove('BGX_ADAPTER_INDEX_KEY_PASSWORD')
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer)
        $pointer = [IntPtr]::Zero
        $process.WaitForExit()
        $result = $process.ExitCode
        if ($result -eq 0 -and $RememberPassword) {
            [IO.File]::WriteAllText($protectedPassword, (ConvertFrom-SecureString -SecureString $secure))
            Write-Host 'Saved the DPAPI-protected password for this Windows account.'
        }
    } finally {
        if ($null -ne $sign) { $sign.EnvironmentVariables.Remove('BGX_ADAPTER_INDEX_KEY_PASSWORD') }
        if ($pointer -ne [IntPtr]::Zero) { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer) }
        $secure.Dispose()
    }
} while ($result -eq 4 -and $RememberPassword -and $interactive)
if ($result -ne 0) { throw 'CDN upload preparation failed. See the publisher output above.' }
