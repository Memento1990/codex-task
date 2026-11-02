param([ValidateSet('check','release','test-core','reporter')][string]$Phase = 'release')
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path $PSScriptRoot
$taskVc = (Get-ChildItem (Join-Path $taskRoot 'build/local-msvc/VC/Tools/MSVC') -Directory | Select-Object -First 1).FullName
$taskSdk = 'C:\Program Files (x86)\Windows Kits\10'
$taskSdkVersion = '10.0.26100.0'
$env:PATH = (Join-Path $taskVc 'bin\Hostx64\x64') + ';' + (Join-Path $taskSdk ('bin\' + $taskSdkVersion + '\x64')) + ';' + $env:PATH
$env:LIB = (Join-Path $taskVc 'lib\x64') + ';' + (Join-Path $taskSdk ('Lib\' + $taskSdkVersion + '\ucrt\x64')) + ';' + (Join-Path $taskSdk ('Lib\' + $taskSdkVersion + '\um\x64'))
$env:INCLUDE = (Join-Path $taskVc 'include') + ';' + (Join-Path $taskSdk ('Include\' + $taskSdkVersion + '\ucrt')) + ';' + (Join-Path $taskSdk ('Include\' + $taskSdkVersion + '\shared')) + ';' + (Join-Path $taskSdk ('Include\' + $taskSdkVersion + '\um'))
$env:VCToolsInstallDir = $taskVc + '\'
$env:WindowsSdkDir = $taskSdk + '\'
$env:WindowsSDKVersion = $taskSdkVersion + '\'
Set-Location -LiteralPath $PSScriptRoot
$taskManifest = if ($Phase -in @('test-core','reporter')) { 'reporter/Cargo.toml' } else { 'src-tauri/Cargo.toml' }
$taskMetadata = & cargo metadata --manifest-path $taskManifest --no-deps --format-version 1 --locked
if ($LASTEXITCODE -ne 0) { throw '无法解析 Cargo 构建目录' }
$taskTarget = ($taskMetadata | ConvertFrom-Json).target_directory
Write-Output "Cargo target: $taskTarget"
if ($Phase -eq 'test-core') { & cargo test --manifest-path reporter/Cargo.toml --locked }
elseif ($Phase -eq 'reporter') { & cargo build --manifest-path reporter/Cargo.toml --release --locked }
elseif ($Phase -eq 'check') { & cargo check --manifest-path src-tauri/Cargo.toml --locked }
else { & pnpm tauri build --no-bundle }
if ($LASTEXITCODE -ne 0) { throw "原生 $Phase 失败" }
