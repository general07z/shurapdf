<#
.SYNOPSIS
  Create a code-signing certificate for ShuraPDF pilot builds and hand it to GitHub Actions.

.DESCRIPTION
  Pilot only. A self-signed certificate is trusted only on PCs where its public part (.cer) is
  installed under Trusted Publishers and Trusted Root Certification Authorities, by hand
  (-TrustOnThisPc) or by Group Policy. For production, use a certificate from the Council's
  internal CA (AD CS, Code Signing template) or a public CA: see docs/code-signing.md.

  Run it on your own Windows PC. The private key stays in your certificate store and in the
  password-protected .pfx; GitHub only ever gets the .pfx as an encrypted repository secret
  (WINDOWS_CERTIFICATE, base64) with its password (WINDOWS_CERTIFICATE_PASSWORD), which
  packaging/windows/sign.ps1 reads. Never commit the .pfx, its base64 or the password.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File packaging\windows\new-signing-certificate.ps1 -Upload
  Creates the certificate, stores both secrets in general07z/shurapdf with gh, and writes the
  public .cer to %USERPROFILE%\shurapdf-signing.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File packaging\windows\new-signing-certificate.ps1 -TrustOnThisPc
  Creates the certificate without gh: paste the two secrets into GitHub by hand (the script says
  where), and trust it on this PC (run as administrator).
#>
param(
  [string] $Subject = 'CN=ShuraPDF Pilot Code Signing, O=Shura Council, C=BH',
  [ValidateRange(1, 5)] [int] $Years = 3,
  [string] $OutDir = (Join-Path $HOME 'shurapdf-signing'),
  # Store the secrets in the repository with the GitHub CLI (gh auth login first; needs admin).
  [switch] $Upload,
  [string] $Repo = 'general07z/shurapdf',
  # Also trust the certificate on this PC (Trusted Publishers and Trusted Root; administrator).
  [switch] $TrustOnThisPc
)
$ErrorActionPreference = 'Stop'

if ([Environment]::OSVersion.Platform -ne 'Win32NT') { throw 'Run this on Windows: it uses the Windows certificate store.' }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$pfx = Join-Path $OutDir 'shurapdf-signing.pfx'
$cer = Join-Path $OutDir 'shurapdf-signing.cer'
if (Test-Path -LiteralPath $pfx) { throw "$pfx exists already: move it away first, so a working key is never overwritten." }

$password = Read-Host -AsSecureString 'Password for the .pfx (at least 16 characters; it becomes WINDOWS_CERTIFICATE_PASSWORD)'
$bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($password)
try { $plain = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr) } finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }
if ($plain.Length -lt 16) { throw 'Use a password of at least 16 characters.' }

Write-Output "Creating '$Subject' (RSA 3072, SHA-256, $Years years) in Cert:\CurrentUser\My"
$cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject $Subject -CertStoreLocation 'Cert:\CurrentUser\My' `
  -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 -KeyExportPolicy Exportable -NotAfter (Get-Date).AddYears($Years)
Export-PfxCertificate -Cert $cert -FilePath $pfx -Password $password | Out-Null
Export-Certificate -Cert $cert -FilePath $cer | Out-Null
$base64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($pfx))

if ($Upload) {
  if (-not (Get-Command gh -ErrorAction SilentlyContinue)) { throw 'gh not found: install it (winget install GitHub.cli), run gh auth login, or run without -Upload.' }
  # --body rather than a pipe: Windows PowerShell 5.1 adds a line break to piped text, which
  # would become part of the password.
  gh secret set WINDOWS_CERTIFICATE --repo $Repo --body $base64
  if ($LASTEXITCODE -ne 0) { throw 'gh secret set WINDOWS_CERTIFICATE failed' }
  gh secret set WINDOWS_CERTIFICATE_PASSWORD --repo $Repo --body $plain
  if ($LASTEXITCODE -ne 0) { throw 'gh secret set WINDOWS_CERTIFICATE_PASSWORD failed' }
  Write-Output "Stored WINDOWS_CERTIFICATE and WINDOWS_CERTIFICATE_PASSWORD in $Repo."
}
else {
  $secretFile = Join-Path $OutDir 'WINDOWS_CERTIFICATE.base64.txt'
  Set-Content -LiteralPath $secretFile -Value $base64 -NoNewline
  Write-Output @"
Add two repository secrets at https://github.com/$Repo/settings/secrets/actions (New repository secret):
  WINDOWS_CERTIFICATE           the whole content of $secretFile
  WINDOWS_CERTIFICATE_PASSWORD  the password you just typed
Then delete $secretFile.
"@
}
$plain = $null

if ($TrustOnThisPc) {
  Import-Certificate -FilePath $cer -CertStoreLocation 'Cert:\LocalMachine\Root' | Out-Null
  Import-Certificate -FilePath $cer -CertStoreLocation 'Cert:\LocalMachine\TrustedPublisher' | Out-Null
  Write-Output 'Trusted on this PC (Trusted Root and Trusted Publishers).'
}

Write-Output @"
Thumbprint: $($cert.Thumbprint)
Valid until: $($cert.NotAfter.ToString('yyyy-MM-dd'))
Keep $pfx and its password safe (a password manager or the ICT key store): it signs software in the Council's name.
Give $cer (public, safe to share) to the PCs that run the pilot, or to ICT for a Group Policy
(Computer Configuration > Windows Settings > Security Settings > Public Key Policies >
Trusted Root Certification Authorities and Trusted Publishers).
"@
