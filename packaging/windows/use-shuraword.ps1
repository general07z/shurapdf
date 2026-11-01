# Points the build at the Council's WordCraft fork (general07z/shuraword) instead of the public
# storytold/wordcraft pinned in Cargo.toml: a [patch] in .cargo/config.toml for this checkout only,
# fetched over SSH with the read-only deploy key in $env:SHURAWORD_DEPLOY_KEY (docs/shura-setup.md).
# -Rev is a full commit id; empty means the one in packaging/windows/shuraword-rev.txt.
param([string]$Rev = '')
$ErrorActionPreference = 'Stop'
if (-not $env:SHURAWORD_DEPLOY_KEY) { throw 'SHURAWORD_DEPLOY_KEY is not set' }
$ssh = Join-Path $HOME '.ssh'
New-Item -ItemType Directory -Force -Path $ssh | Out-Null
$key = Join-Path $ssh 'shuraword'
# OpenSSH wants LF line endings and a final newline, and a key only this account can read.
[IO.File]::WriteAllText($key, ($env:SHURAWORD_DEPLOY_KEY.Trim() -replace "`r", '') + "`n")
icacls $key /inheritance:r /grant:r "$($env:USERNAME):(R)" | Out-Null
# GitHub's published SSH host key (docs.github.com, "GitHub's SSH key fingerprints").
$known = Join-Path $ssh 'known_hosts_github'
Set-Content -LiteralPath $known -Value 'github.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl'
$k = $key -replace '\\', '/'; $kh = $known -replace '\\', '/'
git config --global core.sshCommand "ssh -i '$k' -o IdentitiesOnly=yes -o UserKnownHostsFile='$kh' -o StrictHostKeyChecking=yes"
if ($env:GITHUB_ENV) { "CARGO_NET_GIT_FETCH_WITH_CLI=true" >> $env:GITHUB_ENV }
$env:CARGO_NET_GIT_FETCH_WITH_CLI = 'true'
if (-not $Rev) { $Rev = (Get-Content -Raw packaging/windows/shuraword-rev.txt).Trim() }
if ($Rev -notmatch '^[0-9a-f]{40}$') { throw "the shuraword revision must be one full commit id, not '$Rev'" }
$patch = "`n[patch.`"https://github.com/storytold/wordcraft`"]`n"
foreach ($crate in 'wordcraft-docx', 'wordcraft-docbin', 'wordcraft-pdf') {
  $patch += "$crate = { git = `"ssh://git@github.com/general07z/shuraword.git`", rev = `"$Rev`" }`n"
}
Add-Content -LiteralPath .cargo/config.toml -Value $patch
# Record the patched sources in Cargo.lock (this checkout only), so a --locked build uses them.
cargo fetch
if ($LASTEXITCODE -ne 0) { throw 'fetching shuraword failed: check the deploy key and the commit id' }
if (-not (Select-String -LiteralPath Cargo.lock -SimpleMatch 'general07z/shuraword' -Quiet)) { throw 'Cargo.lock does not use shuraword: the patch did not apply' }
Write-Output "WordCraft from general07z/shuraword at $Rev"
