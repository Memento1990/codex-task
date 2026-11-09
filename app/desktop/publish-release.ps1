#requires -Version 7.0
[CmdletBinding()]
param(
 [Parameter(Mandatory)][ValidatePattern('^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$')][string]$Version,
 [Parameter(Mandatory)][string]$NotesFile,
 [ValidateSet('Prepare','Publish')][string]$Mode='Prepare',
 [switch]$Publish,
 [string[]]$StagePaths=@(),
 [string]$PreparedDirectory,
 [string]$GhPath='gh',
 [switch]$SmokeReviewed
)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$repository='Memento1990/codex-task'
$tag="v$Version"
$versionPaths=@('app/desktop/package.json','app/desktop/pnpm-lock.yaml','app/desktop/protocol/Cargo.toml','app/desktop/client/Cargo.toml','app/desktop/agent/Cargo.toml','app/desktop/storage/Cargo.toml','app/desktop/shell/Cargo.toml','app/desktop/Cargo.lock','app/desktop/src-tauri/Cargo.toml','app/desktop/src-tauri/tauri.conf.json')
function Run([string]$Exe,[string[]]$Arguments) {
 Write-Host ('> '+$Exe+' '+($Arguments -join ' '))
 & $Exe @Arguments
 if($LASTEXITCODE -ne 0){throw "External command failed ($LASTEXITCODE): $Exe"}
}
function Capture([string]$Exe,[string[]]$Arguments) {
 $result=& $Exe @Arguments
 if($LASTEXITCODE -ne 0){throw "External command failed ($LASTEXITCODE): $Exe $($Arguments -join ' ')"}
 return ($result -join "`n")
}
function RepoGit([string[]]$Arguments){ Capture 'git' (@('--literal-pathspecs','-c','core.quotepath=false','-C',$repo)+$Arguments) }
function Lines([string]$Text){ @($Text -split "`r?`n" | Where-Object { $_ -ne '' }) }
function Hash([string]$File){(Get-FileHash -LiteralPath $File -Algorithm SHA256).Hash.ToLowerInvariant()}
function WriteUtf8([string]$File,[string]$Text){[IO.File]::WriteAllText($File,$Text,[Text.UTF8Encoding]::new($false))}
function SafeRelative([string]$Relative) {
 if(!$Relative -or $Relative -match '[\r\n*?\[\]:]' -or [IO.Path]::IsPathRooted($Relative)){throw "StagePaths must be literal repo-relative files: $Relative"}
 $normalized=$Relative.Replace('\','/')
 if(@($normalized.Split('/') | Where-Object {$_ -in @('..','.','')}).Count -or $normalized.StartsWith('.git/')){throw "Unsafe stage path: $Relative"}
 $full=[IO.Path]::GetFullPath((Join-Path $repo $normalized))
 if(Test-Path -LiteralPath $full -PathType Container){throw "Directories are not stage allowlist entries: $normalized"}
 if(!(Test-Path -LiteralPath $full -PathType Leaf) -and !(RepoGit @('ls-files','--error-unmatch','--',$normalized))){throw "Not a source file: $normalized"}
 $current=$full
 while($current -ne $repo){if(Test-Path -LiteralPath $current){if((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw "Reparse point refused: $current"}};$current=Split-Path $current}
 return $normalized
}
function AssertRepository([string]$ExpectedHead='') {
 if((RepoGit @('rev-parse','--show-toplevel')).Replace('\','/') -ne $repo.Replace('\','/')){throw 'Unexpected Git repository'}
 if((RepoGit @('branch','--show-current')) -ne 'main'){throw 'Release requires checked-out main branch'}
 $origin=RepoGit @('remote','get-url','origin')
 if($origin -notmatch '^(https://github\.com/Memento1990/codex-task(?:\.git)?|git@github\.com:Memento1990/codex-task(?:\.git)?)$'){throw "Unexpected origin: $origin"}
 $pushOrigins=@(Lines (RepoGit @('remote','get-url','--push','--all','origin')))
 if($pushOrigins.Count -ne 1 -or $pushOrigins[0] -notmatch '^(https://github\.com/Memento1990/codex-task(?:\.git)?|git@github\.com:Memento1990/codex-task(?:\.git)?)$'){throw 'Unexpected/multiple origin push URLs'}
 if($ExpectedHead -and (RepoGit @('rev-parse','HEAD')) -ne $ExpectedHead){throw 'HEAD changed since Prepare'}
 if((RepoGit @('diff','--cached','--name-only'))){throw 'Index must be empty; refusing unrelated staged changes'}
 if((RepoGit @('ls-files','-u'))){throw 'Unmerged index'}
 if((RepoGit @('tag','--list',$tag))){throw "Local tag exists: $tag"}
 if((RepoGit @('ls-remote','--tags','origin',"refs/tags/$tag","refs/tags/$tag^{}"))){throw "Remote tag exists: $tag"}
 $head=RepoGit @('rev-parse','HEAD')
 $remote=RepoGit @('ls-remote','--heads','origin','refs/heads/main')
 if(!$remote -or ($remote -split '\s+')[0] -ne $head){throw 'origin/main must equal local HEAD; reconcile changes first'}
 # Enumerate drafts too: release-by-tag can return 404 for a draft.
 $pages=Capture $GhPath @('api',"repos/$repository/releases?per_page=100",'--paginate','--slurp') | ConvertFrom-Json
 foreach($page in $pages){foreach($release in $page){if($release.tag_name -eq $tag){throw "Release already exists (including draft): $tag"}}}
}
function AssertDirty([string[]]$Allowed) {
 foreach($file in (Lines (RepoGit @('diff','--name-only','HEAD','--')))){if($file -notin $Allowed){throw "Dirty tracked file not explicitly allowed: $file"}}
 foreach($file in (Lines (RepoGit @('ls-files','--others','--exclude-standard')))) {
  if($file.StartsWith('app/') -and $file -notin $Allowed){throw "Untracked application source not allowed: $file"}
 }
}
function AssertWhitespace([string[]]$Allowed) {
 Run 'git' @('-C',$repo,'diff','--check','HEAD','--')
 $untracked=@(Lines (RepoGit @('ls-files','--others','--exclude-standard')))
 foreach($file in $Allowed){if($file -in $untracked){
  & git -C $repo diff --no-index --check -- /dev/null (Join-Path $repo $file)
  # --no-index returns 1 for a clean new file and 3 for whitespace errors.
  if($LASTEXITCODE -notin @(0,1)){throw "Untracked source whitespace check failed: $file"}
 }}
}
function SourceInventory([string[]]$Extra) {
 $files=@((Lines (RepoGit @('ls-files')))+$Extra | Sort-Object -Unique)
 @($files | ForEach-Object {
  $file=Join-Path $repo $_
  if(Test-Path -LiteralPath $file -PathType Leaf){[ordered]@{path=$_;sha256=(Hash $file)}}else{[ordered]@{path=$_;sha256='DELETED'}}
 })
}
function InventoryHash($Inventory){$json=ConvertTo-Json -InputObject @($Inventory) -Depth 5 -Compress;$bytes=[Text.Encoding]::UTF8.GetBytes($json);[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()}
function VerifyAssets([string]$Directory) {
 $json=Capture 'node' @((Join-Path $PSScriptRoot 'verify-release.cjs'),$Directory,$Version,(Join-Path $PSScriptRoot 'update-public-key.txt'))
 return @($json | ConvertFrom-Json)
}
$oldLocation=Get-Location
$oldTarget=$env:CARGO_TARGET_DIR
try {
 Set-Location -LiteralPath $repo
 $notes=(Resolve-Path -LiteralPath $NotesFile).Path
 if(!(Test-Path -LiteralPath $notes -PathType Leaf) -or (Get-Item $notes).Length -eq 0){throw 'NotesFile must be a nonempty existing file'}
 if($Publish -and $Mode -ne 'Publish'){throw '-Publish is valid only with -Mode Publish'}
 if($Mode -eq 'Publish' -and (!$Publish -or !$SmokeReviewed)){throw 'Remote publication requires -Mode Publish -Publish -SmokeReviewed after manual isolated smoke/review'}
 if($Mode -eq 'Prepare') {
  $allowed=@(@($StagePaths | ForEach-Object {SafeRelative $_})+$versionPaths | Sort-Object -Unique)
  AssertRepository
  AssertDirty $allowed
  AssertWhitespace $allowed
  if(!$PreparedDirectory){$PreparedDirectory=Join-Path ([IO.Path]::GetTempPath()) ("floatboard-release-$Version-"+[guid]::NewGuid().ToString('N'))}
  $directory=[IO.Path]::GetFullPath($PreparedDirectory)
  if(Test-Path -LiteralPath $directory){throw 'PreparedDirectory must not exist; never reusing release outputs'}
  # Always outside repo and deployed LOCALAPPDATA: artifacts cannot become production files.
  $local=[IO.Path]::GetFullPath($env:LOCALAPPDATA).TrimEnd('\')+'\'
  if($directory.StartsWith($repo.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase) -or $directory.StartsWith($local,[StringComparison]::OrdinalIgnoreCase)){throw 'Use an external temporary/cache directory, not repo or LOCALAPPDATA'}
  $ancestor=Split-Path $directory
  while($ancestor){if(Test-Path -LiteralPath $ancestor){if((Get-Item -LiteralPath $ancestor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw "Reparse-point output ancestor refused: $ancestor"}};$ancestor=Split-Path $ancestor}
  $currentVersion=(RepoGit @('show','HEAD:app/desktop/package.json') | ConvertFrom-Json).version
  if([version]$Version -le [version]$currentVersion){throw "Version must increase from $currentVersion"}
  New-Item -ItemType Directory -Path $directory | Out-Null
  $base=RepoGit @('rev-parse','HEAD')
  foreach($relative in @('app/desktop/package.json','app/desktop/src-tauri/tauri.conf.json')){
   $file=Join-Path $repo $relative;$text=[IO.File]::ReadAllText($file)
   $rx=[regex]'(?m)("version"\s*:\s*")[^"]+("\s*[,}])'
   if($rx.Matches($text).Count -ne 1){throw "Ambiguous version field: $relative"}
   WriteUtf8 $file ($rx.Replace($text,('${1}'+$Version+'${2}')))
  }
  foreach($relative in @('protocol','client','agent','storage','shell','src-tauri' | ForEach-Object {"app/desktop/$_/Cargo.toml"})){
   $file=Join-Path $repo $relative;$text=[IO.File]::ReadAllText($file)
   $rx=[regex]'(?m)^version = "[^"]+"\r?$'
   if($rx.Matches($text).Count -ne 1){throw "Ambiguous package version: $relative"}
   WriteUtf8 $file ($rx.Replace($text,('version = "'+$Version+'"')))
  }
  Set-Location -LiteralPath $PSScriptRoot
  Run 'pnpm' @('install','--lockfile-only','--offline','--ignore-scripts')
  # Preserve reviewed dependency resolutions; metadata updates only necessary lock entries.
  foreach($manifest in @('Cargo.toml')){$null=Capture 'cargo' @('metadata','--manifest-path',$manifest,'--offline','--filter-platform','x86_64-pc-windows-msvc','--format-version','1')}
  $env:CARGO_TARGET_DIR=Join-Path $directory 'target'
  $before=InventoryHash (SourceInventory $allowed)
  Run 'pnpm' @('install','--offline','--frozen-lockfile','--ignore-scripts')
  Run 'pnpm' @('test')
  Run 'pnpm' @('build')
  Run 'pwsh' @('-NoProfile','-File',(Join-Path $PSScriptRoot 'build-native.ps1'),'-Phase','test-core')
  # Dot-sourcing retains the local MSVC/SDK environment for native Rust tests.
  Run 'pwsh' @('-NoProfile','-Command',(". '"+(Join-Path $PSScriptRoot 'build-native.ps1').Replace("'","''")+"' -Phase check; cargo test --manifest-path src-tauri/Cargo.toml --locked; if (`$LASTEXITCODE -ne 0) { exit `$LASTEXITCODE }"))
  Run 'pwsh' @('-NoProfile','-File',(Join-Path $PSScriptRoot 'build-native.ps1'),'-Phase','test-runtime')
  Run 'pwsh' @('-NoProfile','-File',(Join-Path $PSScriptRoot 'build-native.ps1'),'-Phase','release')
  if((InventoryHash (SourceInventory $allowed)) -ne $before){throw 'Source changed during build; prepare again in a new directory'}
  $assets=Join-Path $directory 'assets'
  Run 'pwsh' @('-NoProfile','-File',(Join-Path $PSScriptRoot 'package-release.ps1'),'-TargetDirectory',(Join-Path $env:CARGO_TARGET_DIR 'release'),'-OutputDirectory',$assets,'-Version',$Version,'-SignUpdate')
  $verified=VerifyAssets $assets
  AssertDirty $allowed
  $receipt=[ordered]@{schema=1;version=$Version;repository=$repository;base_commit=$base;source_hash=$before;stage_paths=$allowed;notes_sha256=(Hash $notes);assets=$verified;target_directory=$env:CARGO_TARGET_DIR;created_at=[DateTimeOffset]::UtcNow.ToString('o')}
  WriteUtf8 (Join-Path $directory 'prepared.json') ($receipt | ConvertTo-Json -Depth 8)
  Write-Host "PREPARED (not committed/pushed/published): $directory"
  Write-Host "Smoke only via dev-isolated.ps1 -Executable `"$(Join-Path $env:CARGO_TARGET_DIR 'release/CodexFloatboard.exe')`" -CodexMode isolated"
 } else {
  if(!$PreparedDirectory){throw 'Publish requires the exact PreparedDirectory from Prepare'}
  $directory=(Resolve-Path -LiteralPath $PreparedDirectory).Path
  $receipt=Get-Content -LiteralPath (Join-Path $directory 'prepared.json') -Raw | ConvertFrom-Json
  if($receipt.schema -ne 1 -or $receipt.version -ne $Version -or $receipt.repository -ne $repository){throw 'Receipt identity mismatch'}
  AssertRepository $receipt.base_commit
  $allowed=@($receipt.stage_paths | ForEach-Object {SafeRelative $_})
  if($StagePaths.Count){throw 'Publish uses the frozen Prepare allowlist; omit StagePaths'}
  AssertDirty $allowed
  if((Hash $notes) -ne $receipt.notes_sha256){throw 'Notes changed since Prepare'}
  if((InventoryHash (SourceInventory $allowed)) -ne $receipt.source_hash){throw 'Source changed since Prepare; stale package refused'}
  $assets=Join-Path $directory 'assets'
  $verified=VerifyAssets $assets
  if((ConvertTo-Json -InputObject @($verified) -Compress) -ne (ConvertTo-Json -InputObject @($receipt.assets) -Compress)){throw 'Assets changed since Prepare'}
  Run 'git' (@('--literal-pathspecs','-C',$repo,'add','--')+$allowed)
  $staged=@(Lines (RepoGit @('diff','--cached','--name-only')))
  if(!$staged.Count){throw 'No release changes staged'}
  foreach($file in $staged){if($file -notin $allowed){throw "Unexpected staged file: $file"}}
  if((InventoryHash (SourceInventory $allowed)) -ne $receipt.source_hash){throw 'Source changed while staging'}
  Run 'git' @('-C',$repo,'diff','--cached','--check')
  Run 'git' @('-C',$repo,'commit','-m',"Release $tag")
  # Hooks may mutate source: never publish a package from a different commit tree.
  if((RepoGit @('diff','--name-only','HEAD','--')) -or (RepoGit @('diff','--cached','--name-only'))){throw 'Commit hook changed source/index; stopping before tag/push'}
  if((InventoryHash (SourceInventory $allowed)) -ne $receipt.source_hash){throw 'Committed source differs from build'}
  $commit=RepoGit @('rev-parse','HEAD')
  if((RepoGit @('rev-parse','HEAD^')) -ne $receipt.base_commit){throw 'Unexpected commit ancestry; stopping before remote changes'}
  foreach($file in (Lines (RepoGit @('diff-tree','--no-commit-id','--name-only','-r',$commit)))){if($file -notin $allowed){throw "Commit includes unapproved file: $file"}}
  if((Hash $notes) -ne $receipt.notes_sha256){throw 'Notes changed during commit'}
  Run 'git' @('-C',$repo,'tag','-a',$tag,'-m',"Release $tag",$commit)
  Run 'git' @('-C',$repo,'push','--atomic','origin','HEAD:refs/heads/main',"refs/tags/$tag")
  $assetPaths=@($verified | ForEach-Object {Join-Path $assets $_.name})
  Run $GhPath (@('release','create',$tag,'--repo',$repository,'--draft','--verify-tag','--title',"Codex Floatboard $tag",'--notes-file',$notes)+$assetPaths)
  $pages=Capture $GhPath @('api',"repos/$repository/releases?per_page=100",'--paginate','--slurp') | ConvertFrom-Json
  $matches=@(foreach($page in $pages){foreach($release in $page){if($release.tag_name -eq $tag){$release}}})
  if($matches.Count -ne 1 -or !$matches[0].draft){throw 'Expected exactly one new draft release'}
  $release=$matches[0]
  $remoteAssets=@(Capture $GhPath @('api',"repos/$repository/releases/$($release.id)/assets?per_page=100") | ConvertFrom-Json)
  if($remoteAssets.Count -ne $verified.Count){throw 'Uploaded asset count mismatch; leaving draft unpublished'}
  foreach($asset in $verified){
   $remote=@($remoteAssets | Where-Object name -eq $asset.name)
   if($remote.Count -ne 1 -or $remote[0].size -ne $asset.size -or $remote[0].digest -ne "sha256:$($asset.sha256)"){throw "Uploaded size/digest mismatch: $($asset.name); draft left unpublished"}
  }
  Run $GhPath @('api','--method','PATCH',"repos/$repository/releases/$($release.id)",'-F','draft=false','-f','make_latest=true')
  $latest=Capture $GhPath @('api',"repos/$repository/releases/latest") | ConvertFrom-Json
  if($latest.id -ne $release.id -or $latest.draft -or $latest.prerelease){throw 'Latest release postcondition failed'}
  Write-Host "PUBLISHED: $($latest.html_url) commit=$commit"
 }
} finally {Set-Location $oldLocation;$env:CARGO_TARGET_DIR=$oldTarget}
