#!/usr/bin/env bash
#
# Creates the local code-signing identity Mavoca is built with, once per machine.
#
# Why this exists: with no signing identity, `tauri build` leaves the bundle
# ad-hoc, linker-signed, with no sealed resources and no stable designated
# requirement. TCC then pins the Accessibility grant to the binary's exact
# cdhash, so every rebuild silently voids it — the toggle stays switched on in
# System Settings while `AXIsProcessTrusted()` returns false and the selected
# word is never read. Signed with a certificate, the grant is stored against
# `identifier "com.vocablearning.app" and certificate leaf`, which survives a
# rebuild: granted once, granted for good.
#
# The certificate is self-signed and trusted in the login keychain only. That is
# enough for codesign and for TCC; it is not a Developer ID and does nothing for
# Gatekeeper on anyone else's machine.
#
# Expect two prompts from macOS: one to unlock the login keychain while the
# certificate is trusted, and, the first time a build signs with it, one to let
# codesign use the private key. Choose "Always Allow" for the second.

set -euo pipefail

# Must match `bundle.macOS.signingIdentity` in src-tauri/tauri.conf.json.
IDENTITY_NAME="Mavoca Local Signing"
VALID_DAYS=3650
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"
# Guards the PKCS#12 file between openssl and the keychain, inside a temporary
# directory this script deletes on the way out. It is not a secret and is not
# what protects the key afterwards; the login keychain is.
TRANSFER_PASSWORD="mavoca-transfer"

if security find-identity -v -p codesigning | grep -qF "$IDENTITY_NAME"; then
  echo "\"$IDENTITY_NAME\" already exists and is valid for code signing. Nothing to do."
  exit 0
fi

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

echo "Creating a self-signed code-signing certificate…"
openssl req -x509 -newkey rsa:2048 -sha256 -days "$VALID_DAYS" -nodes \
  -keyout "$workdir/key.pem" -out "$workdir/cert.pem" \
  -subj "/CN=$IDENTITY_NAME" \
  -addext "basicConstraints=critical,CA:false" \
  -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning" \
  >/dev/null 2>&1

# Apple's Security framework cannot read the PKCS#12 defaults OpenSSL 3 writes:
# the import fails with "MAC verification failed during PKCS12 import", which
# reads like a wrong password and is not. The MAC and both PBE algorithms have to
# be named, and the password has to be non-empty.
openssl pkcs12 -export \
  -macalg sha1 -certpbe PBE-SHA1-3DES -keypbe PBE-SHA1-3DES \
  -inkey "$workdir/key.pem" -in "$workdir/cert.pem" \
  -name "$IDENTITY_NAME" -out "$workdir/identity.p12" -passout pass:"$TRANSFER_PASSWORD"

echo "Importing it into the login keychain…"
# -T lets codesign reach the private key; -A keeps any other tool from being
# asked about separately.
security import "$workdir/identity.p12" -k "$KEYCHAIN" -P "$TRANSFER_PASSWORD" \
  -T /usr/bin/codesign -A

echo "Trusting it for code signing (macOS will ask for your keychain password)…"
# codesign only offers identities the trust settings call valid, so this step is
# not optional. User domain, so it needs no administrator password.
security add-trusted-cert -r trustRoot -p codeSign -k "$KEYCHAIN" "$workdir/cert.pem"

if security find-identity -v -p codesigning | grep -qF "$IDENTITY_NAME"; then
  echo
  echo "Done. \"$IDENTITY_NAME\" is ready; 'bun run tauri build' will use it."
  echo
  echo "The Accessibility grant belongs to the old unsigned builds, so reset it once:"
  echo "  sudo tccutil reset Accessibility com.vocablearning.app"
  echo "Then install the newly signed build, launch it, and grant access when asked."
else
  echo
  echo "The certificate was imported but is not valid for code signing yet." >&2
  echo "Open Keychain Access, find \"$IDENTITY_NAME\" in the login keychain," >&2
  echo "and set Get Info -> Trust -> Code Signing to \"Always Trust\"." >&2
  exit 1
fi
