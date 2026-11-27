param(
    [Parameter(Mandatory = $true)][string]$SdkBuild,
    [string[]]$OssOrigins = @()
)
& (Join-Path $PSScriptRoot '../Build-InstallCandidate.ps1') -Project $PSScriptRoot -SdkBuild $SdkBuild -OssOrigins $OssOrigins
