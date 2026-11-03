# Desktop entry: keep failures visible instead of closing the console silently.
$ErrorActionPreference='Stop'
try {
    & (Join-Path $PSScriptRoot 'dev-isolated.ps1')
} catch {
    Write-Host "DEV 启动失败：$($_.Exception.Message)" -ForegroundColor Red
    Write-Host '正式版未被修改。请把以上错误反馈给维护者。'
    [void](Read-Host '按 Enter 关闭')
    exit 1
}
