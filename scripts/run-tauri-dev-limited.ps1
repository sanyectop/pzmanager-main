$ErrorActionPreference = 'Stop'

$helper = Join-Path $PSScriptRoot 'run-tauri-dev-limited.cmd'
$runas = Join-Path $env:SystemRoot 'System32\runas.exe'

Start-Process -FilePath $runas -ArgumentList @(
    '/trustlevel:0x20000',
    "`"$helper`""
)
