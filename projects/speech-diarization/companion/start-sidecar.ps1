$ErrorActionPreference = 'Stop'
$env:PYTHONNOUSERSITE = '1'
$env:PYTHONDONTWRITEBYTECODE = '1'
& $env:BGXIONG_MANAGED_PYTHON -I (Join-Path $PSScriptRoot 'server.py')
exit $LASTEXITCODE
