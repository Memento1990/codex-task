param([Parameter(Mandatory=$true)][string]$TargetDirectory,[Parameter(Mandatory=$true)][string]$OutputDirectory,[string]$Version,[switch]$SignUpdate,[string]$MinimumVersion)
$ErrorActionPreference='Stop'
if(!$Version){$Version=(Get-Content -LiteralPath (Join-Path $PSScriptRoot 'package.json') -Raw | ConvertFrom-Json).version}
if(!$MinimumVersion){$MinimumVersion=$Version}
if($MinimumVersion -notmatch '^\d+\.\d+\.\d+$' -or [version]$MinimumVersion -gt [version]$Version){throw 'Explicit compatible minimum version is invalid'}
if($Version -notmatch '^\d+\.\d+\.\d+$'){throw 'Only stable semantic versions supported'}
$target=(Resolve-Path -LiteralPath $TargetDirectory).Path
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$pinned=(Get-Content -LiteralPath (Join-Path $PSScriptRoot 'update-public-key.txt') -Raw).Trim()
foreach($name in @('CodexFloatboard.exe','FloatboardAgent.exe','FloatboardHook.exe','FloatboardCtl.exe','FloatboardMaintenance.exe','FloatboardInstaller.exe','FloatboardUpdater.exe','FloatboardLauncher.exe')){
 $identityFile=Join-Path ([System.IO.Path]::GetTempPath()) ('floatboard-build-'+[guid]::NewGuid().ToString()+'.json')
 $process=Start-Process -FilePath (Join-Path $target $name) -ArgumentList @('--build-identity',('"'+$identityFile+'"')) -WindowStyle Hidden -PassThru -Wait
 if($process.ExitCode -ne 0 -or -not(Test-Path -LiteralPath $identityFile)){throw "Cannot verify build identity: $name"}
 $identity=Get-Content -LiteralPath $identityFile -Raw|ConvertFrom-Json
 if($identity.version -ne $Version -or $identity.update_public_key -ne $pinned -or $identity.updater_protocol -ne 2){throw "Mismatched version/key/protocol in $name; rebuild all binaries before packaging"}
}
$env:FLOATBOARD_UPDATE_PUBLIC_KEY=$pinned
$out=$ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
if(Test-Path -LiteralPath $out){throw 'Output directory must not exist; refusing to overwrite a release'}
New-Item -ItemType Directory -Path $out | Out-Null
$portable=Join-Path $out 'CodexFloatboard';$versionDir=Join-Path $portable "versions/$Version"
New-Item -ItemType Directory -Path $versionDir -Force | Out-Null
foreach($name in @('CodexFloatboard.exe','FloatboardAgent.exe','FloatboardHook.exe','FloatboardCtl.exe','FloatboardMaintenance.exe','FloatboardInstaller.exe','FloatboardUpdater.exe','FloatboardLauncher.exe')){Copy-Item -LiteralPath (Join-Path $target $name) -Destination $versionDir}
Copy-Item -LiteralPath (Join-Path $target 'FloatboardLauncher.exe') -Destination (Join-Path $portable 'CodexFloatboard.exe')
Copy-Item -LiteralPath (Join-Path $target 'FloatboardUpdater.exe') -Destination $portable
Copy-Item -LiteralPath (Join-Path $repo 'LICENSE') -Destination $versionDir
Copy-Item -LiteralPath (Join-Path $repo 'docs/THIRD-PARTY-NOTICES.txt') -Destination $versionDir
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'migration-install.ps1') -Destination $versionDir
Copy-Item -LiteralPath (Join-Path $repo 'docs/migration-install.md') -Destination (Join-Path $versionDir 'MIGRATION.md')
$readme="Codex Floatboard $Version`r`nMigration from 1.x: read versions/$Version/MIGRATION.md and use migration-install.ps1 there. Do not overwrite an existing installation.`r`nAfter profile registration, run the root CodexFloatboard.exe. Keep the complete folder intact.`r`nUpdates require a signed GitHub Release. Updating never terminates Codex.`r`nDo not remove old version directories while a session is using them.`r`nRelease notes: https://github.com/Memento1990/codex-task/releases/tag/v$Version`r`n"
[System.IO.File]::WriteAllText((Join-Path $versionDir 'README.txt'),$readme,[System.Text.UTF8Encoding]::new($false))
[System.IO.File]::WriteAllText((Join-Path $portable 'current.json'),(@{version=$Version}|ConvertTo-Json -Compress),[System.Text.UTF8Encoding]::new($false))
Copy-Item -LiteralPath (Join-Path $versionDir 'README.txt') -Destination $portable
$zip=Join-Path $out "CodexFloatboard-$Version-windows-x64.zip"
Compress-Archive -Path $portable -DestinationPath $zip
$update=Join-Path $out "Floatboard-update-$Version-windows-x64.zip"
Compress-Archive -Path (Join-Path $versionDir '*') -DestinationPath $update
$manifest=[ordered]@{schema=2;version=$Version;platform='windows-x86_64';minimum_version=$MinimumVersion;updater_protocol=2;database_schema_min=2;database_schema_max=2;agent_protocol=2;url="https://github.com/Memento1990/codex-task/releases/download/v$Version/Floatboard-update-$Version-windows-x64.zip";size=(Get-Item $update).Length;sha256=(Get-FileHash $update -Algorithm SHA256).Hash.ToLower()}
$manifestPath=Join-Path $out 'update-manifest.json'
[System.IO.File]::WriteAllText($manifestPath,($manifest|ConvertTo-Json -Compress),[System.Text.UTF8Encoding]::new($false))
if($SignUpdate){if(-not $env:FLOATBOARD_UPDATE_PUBLIC_KEY){throw 'Build public key must be present for signing'};& (Join-Path $target 'FloatboardRelease.exe') sign $manifestPath (Join-Path $out 'update-manifest.json.sig');if($LASTEXITCODE -ne 0){throw 'Signing failed; do not publish unsigned assets'}}
$lines=Get-ChildItem -LiteralPath $out -File | Where-Object Name -ne 'SHA256SUMS.txt' | ForEach-Object {'{0}  {1}' -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(),$_.Name}
$lines|Set-Content (Join-Path $out 'SHA256SUMS.txt') -Encoding utf8
Write-Output "Packaged $Version at $out"
if(-not $SignUpdate){Write-Warning 'Unsigned development package: NOT eligible for automatic updates or production publication.'}
