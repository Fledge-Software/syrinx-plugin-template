#!/usr/bin/env python3
# SPDX-License-Identifier: MPL-2.0
"""Sign or verify a plugin package's `package` file list (the `SPP1` signature).

Why this exists: Syrinx installs a plugin only if `package.sig` verifies
against the key it recorded the first time the plugin was installed. Only the
file list is signed; every file is bound to it by its SHA-256.

The signature is 76 bytes, in the shape of Syrinx's update signature
(docs/update-format.md in Syrinx-app), with its own magic:

    bytes 0-3    ASCII "SPP1" (Syrinx plugin package signature, format 1)
    bytes 4-11   key id: the first 8 bytes of SHA-256 over the public point X || Y
    bytes 12-75  ECDSA P-256 over SHA-256 of the `package` bytes, r then s,
                 each 32 bytes big-endian (the layout Windows CNG uses)

Uses only Python's standard library and the `openssl` command. It never
creates or stores a key except in `self-test`, whose key is generated in a
temporary folder and deleted when the test ends.

    package-signature.py sign      <package-folder> <private-key.pem>
    package-signature.py verify    <package-folder> <public-key.pem>
    package-signature.py key-id    <public-or-private-key.pem>
    package-signature.py self-test
"""

import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

MAGIC = b"SPP1"
SIGNATURE_LENGTH = 76
LIST_NAME = "package"
SIGNATURE_NAME = "package.sig"
LIST_HEADER = b"syrinx-plugin-package 1\n"

# DER prefix of a P-256 SubjectPublicKeyInfo, up to the uncompressed point's 0x04.
P256_SPKI_PREFIX = bytes.fromhex(
    "3059301306072a8648ce3d020106082a8648ce3d030107034200"
)


class SignatureError(Exception):
    """A key, list or signature that must not be accepted."""


def openssl(*args, data=None):
    """Run openssl, returning stdout; any failure is an error, never ignored."""
    result = subprocess.run(
        ["openssl", *args], input=data, capture_output=True, check=False
    )
    if result.returncode != 0:
        message = result.stderr.decode(errors="replace").strip()
        raise SignatureError(f"openssl {args[0]} failed: {message}")
    return result.stdout


def public_point(key_path, private):
    """The 64-byte X || Y of a P-256 key; any other curve is refused."""
    kind = [] if private else ["-pubin"]
    spki = openssl("pkey", *kind, "-in", str(key_path), "-pubout", "-outform", "DER")
    if len(spki) != len(P256_SPKI_PREFIX) + 65 or not spki.startswith(P256_SPKI_PREFIX):
        raise SignatureError("the key is not an ECDSA P-256 key")
    point = spki[len(P256_SPKI_PREFIX):]
    if point[0] != 0x04:
        raise SignatureError("the public point is not uncompressed")
    return point[1:]


def key_id(point):
    return hashlib.sha256(point).digest()[:8]


def read_der_integer(der, offset):
    """One DER INTEGER at `offset`; returns (value bytes, next offset)."""
    if der[offset] != 0x02:
        raise SignatureError("malformed ECDSA signature")
    length = der[offset + 1]
    start = offset + 2
    return der[start:start + length], start + length


def der_to_raw(der):
    """openssl's DER ECDSA-Sig-Value to the 64-byte r || s CNG uses."""
    if der[0] != 0x30:
        raise SignatureError("malformed ECDSA signature")
    r, offset = read_der_integer(der, 2)
    s, _ = read_der_integer(der, offset)
    parts = [value.lstrip(b"\x00").rjust(32, b"\x00") for value in (r, s)]
    if any(len(part) != 32 for part in parts):
        raise SignatureError("ECDSA signature component is too long")
    return parts[0] + parts[1]


def raw_to_der(raw):
    """The reverse, so openssl can verify a 64-byte r || s."""
    def integer(value):
        value = value.lstrip(b"\x00") or b"\x00"
        if value[0] & 0x80:
            value = b"\x00" + value
        return bytes([0x02, len(value)]) + value
    body = integer(raw[:32]) + integer(raw[32:])
    return bytes([0x30, len(body)]) + body


def read_list(folder):
    data = (Path(folder) / LIST_NAME).read_bytes()
    if not data.startswith(LIST_HEADER):
        raise SignatureError("not a plugin package list (wrong first line)")
    return data


def verify_files(folder, data):
    """Every listed file matches its size and hash (the host also refuses extras)."""
    for line in data.decode("ascii").splitlines()[4:]:
        _, digest, size, path = line.split(" ")
        content = (Path(folder) / path).read_bytes()
        if len(content) != int(size) or hashlib.sha256(content).hexdigest() != digest:
            raise SignatureError(f"{path} does not match the list")


def sign(folder, private_key):
    data = read_list(folder)
    point = public_point(private_key, private=True)
    with tempfile.TemporaryDirectory() as scratch:
        list_copy = Path(scratch) / LIST_NAME
        list_copy.write_bytes(data)
        der = openssl("dgst", "-sha256", "-sign", str(private_key), "-binary", str(list_copy))
    signature = MAGIC + key_id(point) + der_to_raw(der)
    (Path(folder) / SIGNATURE_NAME).write_bytes(signature)
    print(f"signed {folder} with key id {key_id(point).hex()}")


def verify(folder, public_key):
    data = read_list(folder)
    signature = (Path(folder) / SIGNATURE_NAME).read_bytes()
    if len(signature) != SIGNATURE_LENGTH or not signature.startswith(MAGIC):
        raise SignatureError("package.sig is not a 76-byte SPP1 signature")
    point = public_point(public_key, private=False)
    if signature[4:12] != key_id(point):
        raise SignatureError("package.sig names a different key")
    with tempfile.TemporaryDirectory() as scratch:
        list_copy = Path(scratch) / LIST_NAME
        der_path = Path(scratch) / "signature.der"
        list_copy.write_bytes(data)
        der_path.write_bytes(raw_to_der(signature[12:]))
        openssl(
            "dgst", "-sha256", "-verify", str(public_key),
            "-signature", str(der_path), str(list_copy),
        )
    verify_files(folder, data)
    print(f"verified {folder}: signed by key id {key_id(point).hex()}")


def self_test():
    """Sign and verify a synthetic package with a throwaway key, then tamper with it."""
    with tempfile.TemporaryDirectory() as scratch:
        scratch = Path(scratch)
        package = scratch / "hello-0.1.0"
        package.mkdir()
        (package / "plugin.wasm").write_bytes(b"synthetic module")
        digest = hashlib.sha256(b"synthetic module").hexdigest()
        (package / LIST_NAME).write_bytes(
            LIST_HEADER + b"id hello\nversion 0.1.0\nworld syrinx:plugin/plugin@0.1.0\n"
            + f"file {digest} 16 plugin.wasm\n".encode()
        )
        private_key, public_key = scratch / "throwaway.pem", scratch / "throwaway.pub.pem"
        openssl("genpkey", "-algorithm", "EC", "-pkeyopt", "ec_paramgen_curve:P-256",
                "-out", str(private_key))
        openssl("pkey", "-in", str(private_key), "-pubout", "-out", str(public_key))
        sign(package, private_key)
        verify(package, public_key)
        if len((package / SIGNATURE_NAME).read_bytes()) != SIGNATURE_LENGTH:
            raise SignatureError("self-test: the signature is not 76 bytes")
        (package / LIST_NAME).write_bytes(
            (package / LIST_NAME).read_bytes().replace(b"0.1.0\n", b"0.1.1\n", 1)
        )
        try:
            verify(package, public_key)
        except SignatureError:
            print("self-test: a changed list is refused")
        else:
            raise SignatureError("self-test: a changed list was accepted")
        for value in (b"\x00" * 31 + b"\x01", b"\x80" + b"\x11" * 31):
            raw = value + value
            if der_to_raw(raw_to_der(raw)) != raw:
                raise SignatureError("self-test: DER conversion does not round-trip")
    print("self-test passed; the throwaway key was deleted with its folder")


def main(argv):
    if shutil.which("openssl") is None:
        raise SignatureError("the openssl command is required")
    match argv:
        case ["sign", folder, key]:
            sign(folder, key)
        case ["verify", folder, key]:
            verify(folder, key)
        case ["key-id", key]:
            text = Path(key).read_text()
            print(key_id(public_point(key, private="PRIVATE KEY" in text)).hex())
        case ["self-test"]:
            self_test()
        case _:
            print(__doc__, file=sys.stderr)
            return 2
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except (SignatureError, OSError, ValueError) as error:
        print(f"package-signature: {error}", file=sys.stderr)
        sys.exit(1)
