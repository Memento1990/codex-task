$ErrorActionPreference = 'Stop'
Set-Location -LiteralPath $PSScriptRoot
$taskPython = 'D:\Env\anaconda3\python.exe'
& $taskPython -m PyInstaller --noconfirm --clean floatboard.spec
if ($LASTEXITCODE -ne 0) { throw '构建失败' }
Write-Output (Join-Path $PSScriptRoot 'dist\CodexFloatboard\CodexFloatboard.exe')
