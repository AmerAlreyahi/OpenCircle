# Signing your own build

OpenCircle's release files are not code-signed. Signing needs a certificate
that costs money and ties to an identity, and this project is free. Instead:

1. Verify the download with `SHA256SUMS.txt` from the release page.
2. Or build from source yourself ([README](../README.md#build-from-source)).
3. Or sign the files with your own certificate, as below. This is useful for a
   company that wants to roll OpenCircle out without SmartScreen warnings.

## Windows

Without a signature, SmartScreen shows "Windows protected your PC" on first
run. A user can click **More info**, then **Run anyway**.

To sign, you need a code-signing certificate (from a certificate authority, or
your organisation's in-house CA for managed PCs) and `signtool` from the
Windows SDK. Sign the installers (`.exe` and `.msi`) and, if you distribute the
program on its own, `opencircle.exe`:

```powershell
signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 `
  /f my-certificate.pfx /p $env:PFX_PASSWORD OpenCircle-setup.exe
signtool verify /pa /v OpenCircle-setup.exe
```

Use your certificate store (`/sha1 <thumbprint>` or `/n "Your Company"`) or a
cloud signing service instead of a `.pfx` file where you can. Keep the
certificate and its password out of the repository. For managed PCs you can
instead trust your in-house CA through Group Policy and skip the public
certificate. Signing changes the file, so publish new checksums for the signed
files.

## Linux

Linux has no SmartScreen. You can still give users a way to check that a file
came from you. With GPG:

```bash
sha256sum opencircle_*.deb opencircle-*.AppImage > SHA256SUMS.txt
gpg --armor --detach-sign SHA256SUMS.txt          # writes SHA256SUMS.txt.asc
```

Publish your public key. Users verify with:

```bash
gpg --verify SHA256SUMS.txt.asc SHA256SUMS.txt
sha256sum -c SHA256SUMS.txt --ignore-missing
```

For your own APT or RPM repository, sign the packages and repository metadata
with the same key (`dpkg-sig`/`debsign`, `rpm --addsign`), and distribute the
public key so clients can trust it.
