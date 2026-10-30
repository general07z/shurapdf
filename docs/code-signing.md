# Code signing for ShuraPDF

> **Status:** live · **Last updated:** 2026-10-30 · **Change:** first version (pilot certificate script, GitHub build signing)

Signing tells Windows who published `pdfcraft.exe` and the MSI, and proves nobody changed them after the build. An unsigned build still runs, but Windows shows "Unknown publisher" (and SmartScreen warns when the file came from the internet). It also makes AppLocker or WDAC allow-listing by publisher impossible.

The build signs automatically when two repository secrets exist:

| Secret | Content |
|---|---|
| `WINDOWS_CERTIFICATE` | the certificate with its private key, as a password-protected `.pfx`, base64-encoded |
| `WINDOWS_CERTIFICATE_PASSWORD` | the `.pfx` password |

`packaging/windows/sign.ps1` reads them (through `package.ps1`, run by `.github/workflows/shura-windows.yml`), signs both executables and the MSI with SHA-256 and an RFC 3161 timestamp, then verifies the signatures. Without the secrets the build still succeeds, unsigned, with a warning.

## Which certificate

| Option | Trusted on | Cost | When |
|---|---|---|---|
| **1. Self-signed (pilot)** | only PCs where ICT installs its `.cer` (Trusted Root + Trusted Publishers), by hand or Group Policy | free | **now**, for the pilot group |
| **2. Council internal CA (AD CS)**, Code Signing template | every domain-joined Council PC automatically | free if the Council runs AD CS; ICT effort to issue | **recommended for internal production** |
| **3. Public CA** (OV/EV code signing) | every Windows PC | roughly USD 300–700 per year, plus a cloud-HSM signing service | only if ShuraPDF is distributed outside the Council |

Notes:

- **Option 3 does not fit this pipeline as is.** Since June 2023 public CAs only issue code-signing keys on a hardware token or HSM, so there is no `.pfx` to store as a secret. It needs a cloud signing service (for example the CA's own, or Azure Trusted Signing, about USD 10 per month, whose eligibility for organisations outside its launch countries must be checked). `sign.ps1` already supports Azure Trusted Signing through the `AZURE_*` secrets.
- **SmartScreen** judges files downloaded from the internet. Copies installed by Intune, SCCM or Group Policy carry no download mark, so options 1 and 2 avoid its warning for internal deployment.
- **The key is a credential.** Whoever holds the `.pfx` and its password can sign software in the Council's name. Keep it in the ICT key store or a password manager, never in email, chat, the repository or a shared drive. Rotate it (new certificate, update both secrets) when someone who had it leaves.

## Option 1: pilot certificate (now)

On your Windows PC, in the `shurapdf` folder, with the [GitHub CLI](https://cli.github.com/) signed in (`gh auth login`):

```powershell
powershell -ExecutionPolicy Bypass -File packaging\windows\new-signing-certificate.ps1 -Upload -TrustOnThisPc
```

It creates an RSA 3072 / SHA-256 certificate valid for 3 years ("ShuraPDF Pilot Code Signing, Shura Council, BH") and stores both secrets in `general07z/shurapdf`. It also trusts the certificate on this PC (run PowerShell as administrator for `-TrustOnThisPc`) and writes the public `shurapdf-signing.cer` to `%USERPROFILE%\shurapdf-signing`. Without `-Upload`, it writes the base64 to a file and says where to paste it.

On the other pilot PCs, as administrator:

```powershell
Import-Certificate -FilePath shurapdf-signing.cer -CertStoreLocation Cert:\LocalMachine\Root; Import-Certificate -FilePath shurapdf-signing.cer -CertStoreLocation Cert:\LocalMachine\TrustedPublisher
```

Or ICT deploys the `.cer` by Group Policy: Computer Configuration ▸ Windows Settings ▸ Security Settings ▸ Public Key Policies ▸ *Trusted Root Certification Authorities* and *Trusted Publishers*. The workflow also attaches the public `.cer` to each release.

## Option 2: Council internal CA (production)

Ask ICT security for a code-signing certificate from the Council's enterprise CA:

1. Template: *Code Signing* (Enhanced Key Usage 1.3.6.1.5.5.7.3.3), RSA 3072 or more, SHA-256, private key exportable (needed for the `.pfx` secret) or kept on an HSM that `sign.ps1` can reach.
2. Subject: the department (for example `CN=Shura Council - Digital Systems and Solutions, O=Shura Council, C=BH`).
3. Export it as a `.pfx` with a strong password, and store it with the same two secrets (`gh secret set WINDOWS_CERTIFICATE --repo general07z/shurapdf < base64.txt`).
4. Add the issuing CA's certificate to *Trusted Publishers* by Group Policy, if the publisher should be trusted without prompts.

Domain PCs already trust the enterprise root, so nothing else needs installing on them.

## Checking a signed build

```powershell
Get-AuthenticodeSignature .\pdfcraft.exe | Format-List Status, SignerCertificate, TimeStamperCertificate
```

`Status` must be `Valid` on a PC that trusts the certificate.
