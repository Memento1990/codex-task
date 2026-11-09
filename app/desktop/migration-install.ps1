#requires -Version 7.0
[CmdletBinding()]
param([Parameter(Mandatory)][string]$RequestFile,[Parameter(Mandatory)][ValidateSet('Preview','Capture','Apply','Cutover','Activate')][string]$Phase,[switch]$Confirmed)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$OutputEncoding=[Console]::OutputEncoding
$requestPath=(Resolve-Path -LiteralPath $RequestFile).Path
$request=Get-Content -LiteralPath $requestPath -Raw | ConvertFrom-Json
$work=[IO.Path]::GetFullPath($request.work_directory)
$install=[IO.Path]::GetFullPath($request.install_root)
$tools=[IO.Path]::GetFullPath($request.tools_directory)
function JsonFile([string]$Name,$Value){$path=Join-Path $work $Name;[IO.File]::WriteAllText($path,(ConvertTo-Json -InputObject $Value -Depth 50),[Text.UTF8Encoding]::new($false));return $path}
function RunJson([string]$Name,[string[]]$Arguments){$output=& (Join-Path $tools $Name) @Arguments;if($LASTEXITCODE -ne 0){throw "$Name failed ($LASTEXITCODE); no later phase executed"};return ($output -join "`n" | ConvertFrom-Json -AsHashtable)}
if($Phase -ne 'Preview' -and !$Confirmed){throw 'Review the preview and supply -Confirmed for this phase'}
New-Item -ItemType Directory -Path $work -Force | Out-Null
$profilePath=Join-Path $work 'desired-profile.json'
$roots=Join-Path $work 'legacy-roots.json'
$sources=Join-Path $work 'sources.json'
$stateSources=Join-Path $work 'state-sources.json'
$registration=[IO.Path]::GetFullPath($request.registration)
if($Phase -eq 'Preview'){
 if(Test-Path -LiteralPath (Join-Path $work 'request.json')){$old=Get-Content -LiteralPath (Join-Path $work 'request.json') -Raw|ConvertFrom-Json;if(($request|ConvertTo-Json -Depth 50 -Compress) -cne ($old|ConvertTo-Json -Depth 50 -Compress)){throw 'Use a new empty work_directory for a changed request'}}
 $null=JsonFile 'request.json' $request
 $null=JsonFile 'desired-profile.json' $request.profile
 $null=JsonFile 'legacy-roots.json' @($request.legacy_roots)
 $null=JsonFile 'sources.json' @($request.sources)
 $null=JsonFile 'state-sources.json' @($request.state_sources)
 $staged=RunJson 'FloatboardInstaller.exe' @('stage','--install-root',$install,'--archive',$request.archive,'--manifest',$request.manifest,'--signature',$request.signature)
 if($staged.version -ne $request.version){throw 'Signed package version differs from request'}
 $preview=RunJson 'FloatboardInstaller.exe' @('preview','--install-root',$install,'--profile',$profilePath,'--legacy-roots',$roots)
 $null=JsonFile 'startup-confirmation.json' $preview.legacy_startup
 if($request.legacy_receipt){$integration=RunJson 'FloatboardMaintenance.exe' @('migrate-config','--profile',$profilePath,'--legacy-receipt',$request.legacy_receipt);$integration['confirmed']=$true;$null=JsonFile 'integration-confirmation.json' $integration;$preview['integration']=$integration}
 $null=JsonFile 'preview.json' $preview
 $preview
 return
}
$frozen=Get-Content -LiteralPath (Join-Path $work 'request.json') -Raw | ConvertFrom-Json
if(($request|ConvertTo-Json -Depth 50 -Compress) -cne ($frozen|ConvertTo-Json -Depth 50 -Compress)){throw 'Request changed after preview; create a new preview'}
$base=@('--install-root',$install,'--profile',$profilePath,'--legacy-roots',$roots,'--registration',$registration)
switch($Phase){
 'Capture' {
  $preview=RunJson 'FloatboardInstaller.exe' (@('preview')+$base)
  if(@($preview.legacy.processes).Count){throw 'Wait for the listed old sessions and writers to exit normally, then repeat Capture'}
  $capture=Join-Path $work ('history-preview-'+[guid]::NewGuid().ToString('N'))
  $result=RunJson 'FloatboardMaintenance.exe' @('preview','--sources',$sources,'--state-sources',$stateSources,'--output',$capture)
  $null=JsonFile 'history-capture.json' @{directory=$capture}
  $null=JsonFile 'history-summary.json' $result
  if($result.conflicts -or $result.unreadable_sources){Write-Output "Review $capture/preview.json and write $work/resolutions.json before Apply"}
  else{$null=JsonFile 'resolutions.json' @{preview_sha256=$result.preview_sha256;choices=@{}}}
  $result
 }
 'Apply' {
  $capture=Get-Content -LiteralPath (Join-Path $work 'history-capture.json') -Raw|ConvertFrom-Json
  $result=RunJson 'FloatboardMaintenance.exe' @('apply','--preview',(Join-Path $capture.directory 'preview.json'),'--resolutions',(Join-Path $work 'resolutions.json'),'--profile',$profilePath)
  $null=JsonFile 'migration-result.json' $result
  $result
 }
 'Cutover' {
  $arguments=@('cutover')+$base+@('--version',$request.version,'--startup-confirmation',(Join-Path $work 'startup-confirmation.json'),'--confirmed')
  if($request.legacy_receipt){$arguments+=@('--legacy-receipt',$request.legacy_receipt,'--integration-confirmation',(Join-Path $work 'integration-confirmation.json'))}
  RunJson 'FloatboardInstaller.exe' $arguments
 }
 'Activate' {RunJson 'FloatboardInstaller.exe' (@('activate')+$base+@('--confirmed'))}
}
