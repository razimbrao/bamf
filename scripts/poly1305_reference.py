"""Independent Poly1305 / ChaCha20 reference using Python big integers.

Used to cross-check the Rust implementation in crates/bamf-proto/src/aead.rs.
Run: python scripts/poly1305_reference.py
"""

P = (1 << 130) - 5


def poly1305(key: bytes, msg: bytes) -> bytes:
    r = int.from_bytes(key[:16], "little") & 0x0FFFFFFC0FFFFFFC0FFFFFFC0FFFFFFF
    s = int.from_bytes(key[16:], "little")
    acc = 0
    for i in range(0, len(msg), 16):
        block = msg[i : i + 16] + b"\x01"
        acc = (acc + int.from_bytes(block, "little")) * r % P
    return ((acc + s) % (1 << 128)).to_bytes(16, "little")


def xor_of_tags() -> str:
    key = b"\xff" * 32
    msg = b"\xff" * 48
    acc = bytearray(16)
    for n in range(49):
        for i, b in enumerate(poly1305(key, msg[:n])):
            acc[i] ^= b
    return acc.hex()


if __name__ == "__main__":
    rfc_key = bytes.fromhex(
        "85d6be7857556d337f4452fe42d506a80103808afb0db2fd4abff6af4149f51b"
    )
    assert (
        poly1305(rfc_key, b"Cryptographic Forum Research Group").hex()
        == "a8061dc1305136c6c22b8baf0c0127a9"
    )
    print("RFC 8439 2.5.2 vector: ok")
    print("xor of tags (key=ff*32, msg=ff*0..48):", xor_of_tags())
