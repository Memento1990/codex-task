param([ValidateSet('check','release','test-core','test-runtime','agent')][string]$Phase = 'release')
$ErrorActionPreference = 'Stop'
# Public trust anchor is source-controlled; the private key is never a build input.
$taskPublicKey = (Get-Content -LiteralPath (Join-Path $PSScriptRoot 'update-public-key.txt') -Raw).Trim()
if ($env:FLOATBOARD_UPDATE_PUBLIC_KEY -and $env:FLOATBOARD_UPDATE_PUBLIC_KEY -ne $taskPublicKey) { throw 'Update public key differs from pinned release key' }
$env:FLOATBOARD_UPDATE_PUBLIC_KEY = $taskPublicKey
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
Push-Location -LiteralPath $PSScriptRoot
try {
$taskManifest = 'Cargo.toml'
$taskMetadata = & cargo metadata --manifest-path $taskManifest --no-deps --format-version 1 --locked
if ($LASTEXITCODE -ne 0) { throw '无法解析 Cargo 构建目录' }
$taskTarget = ($taskMetadata | ConvertFrom-Json).target_directory
Write-Output "Cargo target: $taskTarget"
if ($Phase -eq 'agent') { & cargo build --manifest-path Cargo.toml -p floatboard-agent -p floatboard-client -p floatboard-shell --locked }
elseif ($Phase -eq 'test-runtime') { & cargo test --manifest-path Cargo.toml -p floatboard-protocol -p floatboard-client -p floatboard-agent --locked }
elseif ($Phase -eq 'test-core') { & cargo build --manifest-path Cargo.toml -p floatboard-agent -p floatboard-client -p floatboard-shell --locked; if($LASTEXITCODE -ne 0){throw 'Installer test prerequisites failed'}; & cargo test --manifest-path Cargo.toml -p floatboard-storage -p floatboard-shell --locked }
elseif ($Phase -eq 'check') { & cargo check --manifest-path src-tauri/Cargo.toml --locked }
else { & cargo build --manifest-path Cargo.toml -p floatboard-agent -p floatboard-client -p floatboard-shell --release --locked; if($LASTEXITCODE -ne 0){throw 'Agent/tools release build failed'}; & node (Join-Path $PSScriptRoot 'node_modules/@tauri-apps/cli/tauri.js') build --no-bundle '--' --locked }
if ($LASTEXITCODE -ne 0) { throw "原生 $Phase 失败" }
} finally { Pop-Location }
