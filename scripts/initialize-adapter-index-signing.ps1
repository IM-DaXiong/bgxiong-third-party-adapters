# Run only in a private interactive Windows PowerShell terminal.
# No password argument, transcript, environment dump, or private-key read.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
if ([Console]::IsInputRedirected -or [Console]::IsOutputRedirected) {
    throw 'Run directly in your private local terminal; redirected input/output is refused.'
}
$key = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.tauri\bgxiong-adapter-index'
$public = $key + '.pub'
if ((Test-Path -LiteralPath $key) -or (Test-Path -LiteralPath $public)) {
    throw 'Refusing to overwrite an existing adapter-index private or public key.'
}
Write-Host 'Use a NON-EMPTY password, save it in your own password manager, and keep the private key private.'
Write-Host 'The native signer will ask for the password without echo. Do not paste any password or private key into chat.'
$ack = Read-Host 'Type READY after choosing and saving a non-empty password'
if ($ack -cne 'READY') { throw 'Cancelled; no key generated.' }
$null = New-Item -ItemType Directory -Path (Split-Path -Parent $key) -Force
$cargo = (Get-Command cargo -CommandType Application -ErrorAction Stop).Source
$start = New-Object System.Diagnostics.ProcessStartInfo
$start.FileName = $cargo
$start.Arguments = 'tauri signer generate --write-keys "' + $key + '"'
$start.UseShellExecute = $false
$start.CreateNoWindow = $false
$start.EnvironmentVariables.Clear()
$allowed = @('SYSTEMROOT','SYSTEMDRIVE','WINDIR','COMSPEC','PATH','PATHEXT','TEMP','TMP','USERPROFILE','APPDATA','LOCALAPPDATA','PROGRAMFILES','PROGRAMFILES(X86)','PROGRAMW6432','COMMONPROGRAMFILES','COMMONPROGRAMFILES(X86)','COMMONPROGRAMW6432','CARGO_HOME','RUSTUP_HOME','RUSTUP_TOOLCHAIN','HOMEDRIVE','HOMEPATH','NUMBER_OF_PROCESSORS','PROCESSOR_ARCHITECTURE')
foreach ($name in $allowed) {
    $value = [Environment]::GetEnvironmentVariable($name, 'Process')
    if ($null -ne $value) { $start.EnvironmentVariables[$name] = $value }
}
# CI and every TAURI_SIGNING_* / BGX password variable are absent in this child.
# --write-keys is mandatory: without it, the native CLI prints key material.
$process = [System.Diagnostics.Process]::Start($start)
$process.WaitForExit()
if ($process.ExitCode -ne 0) { throw 'Native generation failed; no private output is collected by this script.' }
if (-not (Test-Path -LiteralPath $key -PathType Leaf) -or -not (Test-Path -LiteralPath $public -PathType Leaf)) {
    throw 'Key generation did not produce the expected independent files.'
}
# Restrict only the newly generated private file; do not change the .tauri directory or application keys.
$sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User
$acl = New-Object System.Security.AccessControl.FileSecurity
$acl.SetOwner($sid)
$acl.SetAccessRuleProtection($true, $false)
$rule = [System.Security.AccessControl.FileSystemAccessRule]::new($sid, [System.Security.AccessControl.FileSystemRights]::FullControl, [System.Security.AccessControl.AccessControlType]::Allow)
$acl.AddAccessRule($rule)
Set-Acl -LiteralPath $key -AclObject $acl
Write-Host 'Independent adapter-index key files exist; private ACL is restricted to the current user.'
Write-Host ('Public key file (safe to inspect): ' + $public)
Write-Host 'No signing password was persisted in an environment variable or file by this script.'
Write-Host 'Back up the private file and password privately. Reply only: generated. Never send the private contents.'
