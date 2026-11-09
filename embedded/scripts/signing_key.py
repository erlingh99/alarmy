# /// script
# requires-python = ">=3.11"
# dependencies = ["cryptography>=44"]
# [tool.uv]
# exclude-newer = "10 days"
# ///
"""
Ed25519 signing of OTA firmware images.

The firmware embeds the public key (`firmware/signing_key.pub`, 32 raw bytes) and only installs
an update whose signature verifies against it (embassy-boot's `verify_and_mark_updated`). That
signs the SHA-512 digest of the image, not the image itself, so `sign` does the same.

The private key is a 64 character hex seed. It lives in the `FIRMWARE_SIGNING_KEY`
GitHub secret and reaches the Docker build as a build secret.

Usage (from embedded/):
    uv run scripts/signing_key.py generate --private-out ~/.alarmy/firmware_signing.key
    uv run scripts/signing_key.py sign --key KEY_FILE firmware.bin firmware.sig
"""
import argparse
import hashlib
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import (
    Encoding,
    NoEncryption,
    PrivateFormat,
    PublicFormat,
)

PUBLIC_KEY = Path(__file__).resolve().parent.parent / "firmware" / "signing_key.pub"


def generate(private_out: Path) -> None:
    if private_out.exists():
        raise SystemExit(f"{private_out} exists, refusing to overwrite a signing key")
    key = Ed25519PrivateKey.generate()
    seed = key.private_bytes(Encoding.Raw, PrivateFormat.Raw, NoEncryption())
    private_out.parent.mkdir(parents=True, exist_ok=True)
    private_out.write_text(seed.hex())
    PUBLIC_KEY.write_bytes(key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw))
    print(f"Private key written to {private_out}, public key to {PUBLIC_KEY}")


def sign(key_file: Path, image: Path, signature_out: Path) -> None:
    key = Ed25519PrivateKey.from_private_bytes(bytes.fromhex(key_file.read_text().strip()))
    public = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
    if public != PUBLIC_KEY.read_bytes():
        raise SystemExit(f"The key doesn't match {PUBLIC_KEY}, the firmware would reject it")
    digest = hashlib.sha512(image.read_bytes()).digest()
    signature_out.write_text(key.sign(digest).hex())
    print(f"Signed {image} -> {signature_out}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    commands = parser.add_subparsers(dest="command", required=True)

    gen = commands.add_parser("generate", help="create a key pair")
    gen.add_argument("--private-out", type=Path, required=True)

    sig = commands.add_parser("sign", help="sign a firmware image")
    sig.add_argument("--key", type=Path, required=True, help="file holding the hex seed")
    sig.add_argument("image", type=Path)
    sig.add_argument("signature_out", type=Path)

    args = parser.parse_args()
    if args.command == "generate":
        generate(args.private_out.expanduser())
    else:
        sign(args.key, args.image, args.signature_out)


if __name__ == "__main__":
    main()
