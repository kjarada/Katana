"""Independent oracle for tests/update/test_signature.cpp: RFC 8032 reference arithmetic in plain Python integers.

Not a test of the program: the script that re-derives the literals the signature tests hold, so that they are not
the C++ under test checked against itself. Run it with any Python 3, no other module: it exits non-zero when it
does not reproduce the RFC 8032 section 7.1 vectors. ctest runs it as update_tools_rfc8032_oracle only so that
it keeps working; the literals in test_signature.cpp are what the C++ is judged against.

Written from RFC 8032 section 6 (the reference code) and section 5.1.3 (decoding), NOT from
the C++ under test. It derives (a) the section 7.1 vectors from the secret keys, to be
compared with the hex the RFC prints, (b) the eight small-order points and their
non-canonical aliases, (c) a mixed-order point, (d) an off-curve y, (e) S + L signatures.
"""
import hashlib
import sys

p = 2**255 - 19
q = 2**252 + 27742317777372353535851937790883648493  # the group order L
d = -121665 * pow(121666, p - 2, p) % p
sqrt_m1 = pow(2, (p - 1) // 4, p)


def sha512(s):
    return hashlib.sha512(s).digest()


def point_add(P, Q):
    A = (P[1] - P[0]) * (Q[1] - Q[0]) % p
    B = (P[1] + P[0]) * (Q[1] + Q[0]) % p
    C = 2 * P[3] * Q[3] * d % p
    D = 2 * P[2] * Q[2] % p
    E, F, G, H = B - A, D - C, D + C, B + A
    return (E * F % p, G * H % p, F * G % p, E * H % p)


def point_mul(s, P):
    Q = (0, 1, 1, 0)
    while s > 0:
        if s & 1:
            Q = point_add(Q, P)
        P = point_add(P, P)
        s >>= 1
    return Q


def point_equal(P, Q):
    if (P[0] * Q[2] - Q[0] * P[2]) % p != 0:
        return False
    if (P[1] * Q[2] - Q[1] * P[2]) % p != 0:
        return False
    return True


def recover_x(y, sign):
    if y >= p:
        return None
    x2 = (y * y - 1) * pow(d * y * y + 1, p - 2, p) % p
    if x2 == 0:
        return None if sign else 0
    x = pow(x2, (p + 3) // 8, p)
    if (x * x - x2) % p != 0:
        x = x * sqrt_m1 % p
    if (x * x - x2) % p != 0:
        return None
    if (x & 1) != sign:
        x = p - x
    return x


g_y = 4 * pow(5, p - 2, p) % p
g_x = recover_x(g_y, 0)
G = (g_x, g_y, 1, g_x * g_y % p)


def point_compress(P):
    zinv = pow(P[2], p - 2, p)
    x = P[0] * zinv % p
    y = P[1] * zinv % p
    return int.to_bytes(y | ((x & 1) << 255), 32, "little")


def point_decompress(s):
    if len(s) != 32:
        raise ValueError
    y = int.from_bytes(s, "little")
    sign = y >> 255
    y &= (1 << 255) - 1
    x = recover_x(y, sign)
    if x is None:
        return None
    return (x, y, 1, x * y % p)


def secret_expand(secret):
    h = sha512(secret)
    a = int.from_bytes(h[:32], "little")
    a &= (1 << 254) - 8
    a |= 1 << 254
    return a, h[32:]


def public_key(secret):
    a, _ = secret_expand(secret)
    return point_compress(point_mul(a, G))


def sign(secret, msg):
    a, prefix = secret_expand(secret)
    A = point_compress(point_mul(a, G))
    r = int.from_bytes(sha512(prefix + msg), "little") % q
    R = point_mul(r, G)
    Rs = point_compress(R)
    h = int.from_bytes(sha512(Rs + A + msg), "little") % q
    s = (r + h * a) % q
    return Rs + int.to_bytes(s, 32, "little")


def verify(public, msg, signature):
    if len(public) != 32 or len(signature) != 64:
        return False
    A = point_decompress(public)
    if not A:
        return False
    Rs = signature[:32]
    R = point_decompress(Rs)
    if not R:
        return False
    s = int.from_bytes(signature[32:], "little")
    if s >= q:
        return False
    h = int.from_bytes(sha512(Rs + public + msg), "little") % q
    sB = point_mul(s, G)
    hA = point_mul(h, A)
    return point_equal(sB, point_add(R, hA))


VECTORS = {
    "TEST 1": ("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60", "",
               "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
               "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"),
    "TEST 2": ("4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb", "72",
               "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
               "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00"),
    "TEST 3": ("c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7", "af82",
               "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
               "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a"),
}


def hexs(b):
    return b.hex()


def cpp_array(name, b):
    items = ", ".join("0x%02x" % c for c in b)
    return "%s = {%s}" % (name, items)


def main():
    print("== RFC 8032 section 7.1: derived from the secret keys vs the hex typed from the RFC")
    ok = True
    for name, (sk, msg, pk, sig) in VECTORS.items():
        sk_b, msg_b = bytes.fromhex(sk), bytes.fromhex(msg)
        pub = public_key(sk_b)
        s = sign(sk_b, msg_b)
        good = hexs(pub) == pk and hexs(s) == sig and verify(bytes.fromhex(pk), msg_b, bytes.fromhex(sig))
        ok &= good
        print(name, "public", hexs(pub) == pk, "signature", hexs(s) == sig, "verify", good)
    if not ok:
        sys.exit("a vector disagrees")

    print("== group order L, little-endian:", int.to_bytes(q, 32, "little").hex())

    print("== the 8-torsion subgroup (all points with 8P = O), by multiplying random points by L")
    torsion = {}
    y = 2
    while len(torsion) < 8 and y < 2000:
        x = recover_x(y, 0)
        if x is not None:
            T = point_mul(q, (x, y, 1, x * y % p))  # L*Q lies in the torsion subgroup (Q has order L*t)
            torsion[point_compress(T)] = T
        y += 1
    print("distinct torsion encodings found:", len(torsion))
    for enc, T in sorted(torsion.items()):
        order = 1
        R = T
        while not point_equal(R, (0, 1, 1, 0)):
            R = point_add(R, T)
            order += 1
        print("  order", order, enc.hex())

    print("== non-canonical aliases")
    # y + p when y < 19 (still < 2^255); the sign bit set on x = 0
    for enc in sorted(torsion):
        yv = int.from_bytes(enc, "little") & ((1 << 255) - 1)
        signbit = enc[31] >> 7
        if yv < 19:
            alias = int.to_bytes((yv + p) | (signbit << 255), 32, "little")
            print("  alias of", enc.hex(), "(y+p):", alias.hex(), "decodes:", point_decompress(alias) is not None)
        if yv == 1 or yv == p - 1:
            flipped = int.to_bytes(yv | (1 << 255), 32, "little")
            print("  x=0 with the sign bit set:", flipped.hex(), "decodes:", point_decompress(flipped) is not None)
    print("  y = p exactly:", int.to_bytes(p, 32, "little").hex(), point_decompress(int.to_bytes(p, 32, "little")))
    print("  all-0xff:", point_decompress(b"\xff" * 32))

    print("== a mixed-order point: TEST 1 public key + an order-8 point")
    pub1 = point_decompress(bytes.fromhex(VECTORS["TEST 1"][2]))
    order8 = None
    for enc, T in torsion.items():
        R, k = T, 1
        while not point_equal(R, (0, 1, 1, 0)):
            R, k = point_add(R, T), k + 1
        if k == 8:
            order8 = T
            break
    mixed = point_compress(point_add(pub1, order8))
    print("  mixed:", mixed.hex(), "L*A == O ?", point_equal(point_mul(q, point_decompress(mixed)), (0, 1, 1, 0)))
    print("  TEST 1 L*A == O ?", point_equal(point_mul(q, pub1), (0, 1, 1, 0)))

    print("== off-curve y values (no x): ", [yy for yy in range(2, 40) if recover_x(yy, 0) is None][:6])

    print("== placeholders")
    for label, text in (("active", b"PLACEHOLDER-ACTIVE-NOT-A-KEY-00-"), ("spare", b"PLACEHOLDER-SPARE-NOT-A-KEY--00-")):
        print(" ", label, text.hex(), len(text), "decodes:", point_decompress(text) is not None)

    print("== all-zero key: ")
    z = point_decompress(bytes(32))
    print("  decodes:", z is not None, "order-4 point; 4P == O ?", point_equal(point_mul(4, z), (0, 1, 1, 0)))

    print("== S + L malleation of TEST 2 (valid mathematically, rejected by RFC 8032 5.1.7)")
    sk, msg, pk, sig = VECTORS["TEST 2"]
    sig_b = bytes.fromhex(sig)
    s = int.from_bytes(sig_b[32:], "little")
    for label, s2 in (("S+L", s + q), ("S+2L", s + 2 * q), ("L", q), ("L-1", q - 1), ("2^256-1", 2**256 - 1), ("S+L (top 3 bits set?)", s + q)):
        if s2 >= 2**256:
            continue
        sig2 = sig_b[:32] + int.to_bytes(s2, 32, "little")
        print(" ", label, "s>=L:", s2 >= q, "bytes:", sig2[32:].hex(), "high 3 bits of byte 63:", sig2[63] >> 5)
    # equation holds for S+L
    sig_ml = sig_b[:32] + int.to_bytes(s + q, 32, "little")
    A = point_decompress(bytes.fromhex(pk))
    Rs = sig_b[:32]
    h = int.from_bytes(sha512(Rs + bytes.fromhex(pk) + bytes.fromhex(msg)), "little") % q
    print("  verification equation holds for S+L:",
          point_equal(point_mul(s + q, G), point_add(point_decompress(Rs), point_mul(h, A))))
    print("  full signature S+L hex:", sig_ml.hex())


if __name__ == "__main__":
    main()
