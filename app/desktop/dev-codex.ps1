# Launch an independent Codex CLI using the SAME workspace DEV home as the GUI.
[CmdletBinding()]
param([switch]$CheckOnly)
$ErrorActionPreference='Stop'
$info=& (Join-Path $PSScriptRoot 'dev-isolated.ps1') -CodexMode isolated -CheckOnly | ConvertFrom-Json
$command=Get-Command codex.exe -CommandType Application -ErrorAction Stop
$workspace=Join-Path $info.sandbox 'workspace'
if($CheckOnly){[ordered]@{mode='DEV-CODEX';executable=$command.Source;codex_home=$info.environment.CODEX_HOME;local=$info.environment.LOCALAPPDATA;workspace=$workspace;copies_production_auth=$false;bypasses_hook_trust=$false}|ConvertTo-Json;return}
# The GUI wrapper performs path/reparse validation; the workspace below is kept
# in that sandbox and is not the source repository or a production project.
$cursor=$workspace
while($cursor){if(Test-Path -LiteralPath $cursor){if((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'DEV workspace contains a reparse path'}};$parent=Split-Path $cursor -Parent;if($parent -eq $cursor){break};$cursor=$parent}
New-Item -ItemType Directory -Path $workspace -Force|Out-Null
New-Item -ItemType Directory -Path $info.environment.CODEX_HOME -Force|Out-Null
Write-Host '[DEV CODEX] Separate test session; NOT the daily-use Codex Desktop.' -ForegroundColor Yellow
Write-Host "CODEX_HOME: $($info.environment.CODEX_HOME)"
Write-Host 'First configure the DEV GUI connection. Login and hook trust must be completed normally in this isolated Codex; no credentials or trust are copied.'
$previous=@{}
foreach($item in Get-ChildItem Env:){if($item.Name -match '^(?i:CODEX_|FLOATBOARD_|OPENAI_|AZURE_OPENAI_|ANTHROPIC_|WEBVIEW2_)'){$previous[$item.Name]=$item.Value}}
$keys=@('LOCALAPPDATA','APPDATA')+@($info.environment.PSObject.Properties.Name)
foreach($key in $keys|Select-Object -Unique){if(-not $previous.ContainsKey($key)){$previous[$key]=[Environment]::GetEnvironmentVariable($key)}}
try{
 foreach($item in @(Get-ChildItem Env:)){if($item.Name -match '^(?i:CODEX_|FLOATBOARD_|OPENAI_|AZURE_OPENAI_|ANTHROPIC_|WEBVIEW2_)'){[Environment]::SetEnvironmentVariable($item.Name,$null,'Process')}}
 foreach($property in $info.environment.PSObject.Properties){[Environment]::SetEnvironmentVariable($property.Name,[string]$property.Value,'Process')}
 & $command.Source -C $workspace --sandbox workspace-write --ask-for-approval on-request --no-alt-screen
 if($LASTEXITCODE -ne 0){throw "Isolated Codex exited with code $LASTEXITCODE"}
}finally{foreach($key in $previous.Keys){[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}
