# Requires PowerShell 7. Launches an already-built GUI; never builds or starts Vite.
[CmdletBinding()]
param(
    [string]$Executable,
    [ValidateSet('debug','release')][string]$Configuration = 'release',
    [ValidateRange(0,65535)][int]$DebugPort=0,
    [ValidateSet('local','isolated')][string]$CodexMode='local',
    [string]$CodexHome,
    [switch]$CheckOnly
)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..')).TrimEnd('\','/')
$hostLocal = [Environment]::GetFolderPath('LocalApplicationData')
if (-not $hostLocal -or -not [IO.Path]::IsPathFullyQualified($hostLocal)) { throw 'Cannot determine the real user LOCALAPPDATA.' }
$hostLocal = [IO.Path]::GetFullPath($hostLocal)
function Assert-NoReparse([string]$Path) {
    $cursor = [IO.Path]::GetFullPath($Path)
    while ($cursor) {
        if (Test-Path -LiteralPath $cursor) {
            $item = Get-Item -LiteralPath $cursor -Force
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "Refusing a junction/symlink in DEV path: $cursor" }
        }
        $parent = Split-Path -Path $cursor -Parent
        if ($parent -eq $cursor) { break }
        $cursor = $parent
    }
}
if (-not $Executable) { $Executable = "D:\Cache\Rust\targets\codex-floatboard\$Configuration\CodexFloatboard.exe" }
$exe = [IO.Path]::GetFullPath($Executable)
$installed = [IO.Path]::GetFullPath((Join-Path $hostLocal 'Programs'))
if ($exe.StartsWith($installed.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase) -or
    $exe -match '(?i)[\\/]releases[\\/]' -or
    [IO.Path]::GetFileName($exe) -ne 'CodexFloatboard.exe') {
    throw 'DEV refuses installed Programs executables and public releases packages. Select a compiled CodexFloatboard.exe from a build target.'
}
Assert-NoReparse $exe
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw "Precompiled DEV executable missing: $exe. Build explicitly first; no stable fallback is allowed." }
$hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($repo.ToLowerInvariant()))).Substring(0,12).ToLowerInvariant()
$root = Join-Path $hostLocal "CodexFloatboard-Dev\$hash"
if ($CodexMode -eq 'local') { $root=Join-Path $root 'local-codex' }
$local = Join-Path $root 'local'
$data = Join-Path $local 'CodexFloatboard/data'
$control = Join-Path $local 'CodexFloatboard/control'
$codex = Join-Path $root 'codex'
if ($CodexMode -eq 'local') {
    $codex=if($CodexHome){$CodexHome}elseif($env:CODEX_HOME){$env:CODEX_HOME}else{Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codex'}
    if(-not [IO.Path]::IsPathFullyQualified($codex)){throw 'Local Codex home must be absolute.'}
    $codex=[IO.Path]::GetFullPath($codex)
}
$config = Join-Path $codex 'config.toml'
$roaming = Join-Path $root 'roaming'
$webview = Join-Path $root 'webview'
$updates = Join-Path $control 'updates/preferences.json'
$options = Join-Path $data 'profile.json'
$agent=Join-Path (Split-Path $exe -Parent) 'FloatboardAgent.exe'
$ctl=Join-Path (Split-Path $exe -Parent) 'FloatboardCtl.exe'
$maintenance=Join-Path (Split-Path $exe -Parent) 'FloatboardMaintenance.exe'
$paths = @($root,$local,$data,$control,$codex,$config,$roaming,$webview,$updates,$options)
foreach ($p in $paths) { Assert-NoReparse $p }
$isolatedEnv = [ordered]@{
    LOCALAPPDATA=$local; APPDATA=$roaming; CODEX_HOME=$codex
    CODEX_FLOATBOARD_DATA=$data; WEBVIEW2_USER_DATA_FOLDER=$webview
    FLOATBOARD_PROFILE_CONFIG=$options; FLOATBOARD_DEV_ISOLATED='1'; FLOATBOARD_DEVELOPMENT='1'; FLOATBOARD_CODEX_MODE=$CodexMode
}
if ($DebugPort -ne 0) { if($DebugPort -lt 1024){throw 'DebugPort must be >=1024'}; $isolatedEnv['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS']="--remote-debugging-address=127.0.0.1 --remote-debugging-port=$DebugPort" }
$summary = [ordered]@{
    mode='DEV'; codex_mode=$CodexMode; check_only=[bool]$CheckOnly; executable=$exe; workspace=$repo
    sandbox=$root; config=$config; data=$data; control=$control; webview=$webview
    arguments=@('--isolated-instance','--data-dir',$data)
    environment=$isolatedEnv; update_preferences=$updates; auto_check=$false; auto_download=$false
    desktop_preferences=$options; auto_start=$false; config_reset_to_empty=$false
    clears_inherited_patterns=@('CODEX_*','FLOATBOARD_*','OPENAI_*','AZURE_OPENAI_*','ANTHROPIC_*','WEBVIEW2_*')
    writes_registry=$false; starts_dev_server=$false; writes_files=(-not $CheckOnly)
}
if ($CheckOnly) { $summary | ConvertTo-Json -Depth 5; return }
# Reuse this workspace's existing DEV window before touching its sandbox.
$existing=Get-CimInstance Win32_Process -Filter "Name='CodexFloatboard.exe'" | Where-Object { $_.ExecutablePath -eq $exe -and $_.CommandLine -like "*$data*" } | Select-Object -First 1
if ($existing) {
    Write-Host "DEV already running (PID $($existing.ProcessId)); no resources changed."
    return
}
Write-Host '[DEV-ISOLATED] Not the production installation.' -ForegroundColor Yellow
Write-Host "Executable: $exe"
Write-Host "Codex mode: $CodexMode"
Write-Host "Config:     $config (not reset/copied; local mode connects only on explicit confirmation)"
Write-Host "Data:       $data"
Write-Host "Control:    $control"
Write-Host "WebView:    $webview"
$dirs=@($data,$control,$roaming,$webview,(Split-Path $updates -Parent))
if($CodexMode -eq 'isolated'){$dirs+=@($codex)}
foreach ($dir in $dirs) {
    New-Item -ItemType Directory -Path $dir -Force | Out-Null
}
# Initialize once; preserve DEV hooks/consent across restarts. Never copy production auth/config.
if ($CodexMode -eq 'isolated' -and -not (Test-Path -LiteralPath $config)) { [IO.File]::WriteAllText($config,'',[Text.UTF8Encoding]::new($false)) }
[IO.File]::WriteAllText($updates,'{"auto_check":false,"auto_download":false}',[Text.UTF8Encoding]::new($false))
foreach($binary in @($agent,$ctl,$maintenance)){Assert-NoReparse $binary;if(-not(Test-Path -LiteralPath $binary -PathType Leaf)){throw "Build the Agent, client and maintenance tools first: $binary"}}
if(-not(Test-Path -LiteralPath $options)){
    $listener=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0);$listener.Start();$port=([Net.IPEndPoint]$listener.LocalEndpoint).Port;$listener.Stop()
    $profile=[ordered]@{profile_id=[guid]::NewGuid().ToString();environment='development';owner_sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value;root=$data;port=$port;agent_exe=$agent;gui_exe=$exe;codex_home=$codex}
    [IO.File]::WriteAllText($options,($profile|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
    & $maintenance initialize --profile $options
    if($LASTEXITCODE -ne 0){throw 'DEV database initialization failed; retained for diagnosis.'}
}else{
    $profile=Get-Content -LiteralPath $options -Raw|ConvertFrom-Json
    if($profile.agent_exe -ne $agent -or $profile.gui_exe -ne $exe -or $profile.codex_home -ne $codex){throw 'DEV profile belongs to another build or Codex mode. Use a separate workspace sandbox.'}
}
$agentStart=[Diagnostics.ProcessStartInfo]::new();$agentStart.FileName=$agent;$agentStart.UseShellExecute=$false;$agentStart.CreateNoWindow=$true
$agentStart.ArgumentList.Add('--profile');$agentStart.ArgumentList.Add($options)
foreach ($name in @($agentStart.Environment.Keys)) { if ($name -match '^(?i:CODEX_|FLOATBOARD_|OPENAI_|AZURE_OPENAI_|ANTHROPIC_|WEBVIEW2_)') { [void]$agentStart.Environment.Remove($name) } }
foreach ($entry in $isolatedEnv.GetEnumerator()) { $agentStart.Environment[$entry.Key]=$entry.Value }
$agentProcess=$null
& $ctl --profile $options --method identity 2>$null |Out-Null
if($LASTEXITCODE -ne 0){$agentProcess=[Diagnostics.Process]::Start($agentStart)}
$deadline=[DateTime]::UtcNow.AddSeconds(20);$ready=$false
while([DateTime]::UtcNow -lt $deadline){
    if($agentProcess -and $agentProcess.HasExited){throw "DEV Agent exited ($($agentProcess.ExitCode)); no GUI started."}
    $identity=& $ctl --profile $options --method identity 2>$null
    if($LASTEXITCODE -eq 0){$health=$identity|ConvertFrom-Json;if($health.storage.status -ne 'ok'){throw 'DEV storage is unavailable; retained without replacement.'};$ready=$true;break}
    Start-Sleep -Milliseconds 100
}
if(-not $ready){throw 'DEV Agent readiness timed out.'}
Write-Host "DEV Agent ready: profile $($profile.profile_id), endpoint $($profile.port)"
$start = [Diagnostics.ProcessStartInfo]::new()
$start.FileName=$exe
$start.WorkingDirectory=Split-Path $exe -Parent
$start.UseShellExecute=$false
$start.ArgumentList.Add('--isolated-instance')
$start.ArgumentList.Add('--data-dir')
$start.ArgumentList.Add($data)
# No inherited production generation, preflight, private-key file, debug port, or API credentials.
foreach ($name in @($start.Environment.Keys)) {
    if ($name -match '^(?i:CODEX_|FLOATBOARD_|OPENAI_|AZURE_OPENAI_|ANTHROPIC_|WEBVIEW2_)') { [void]$start.Environment.Remove($name) }
}
foreach ($entry in $isolatedEnv.GetEnumerator()) { $start.Environment[$entry.Key]=$entry.Value }
$process = [Diagnostics.Process]::Start($start)
if (-not $process) { throw 'DEV process did not start.' }
if ($process.WaitForExit(2500)) { throw "DEV GUI exited during startup (exit $($process.ExitCode)); check the build and isolated settings." }
Write-Host "DEV GUI started (PID $($process.Id)); only close this instance normally."
