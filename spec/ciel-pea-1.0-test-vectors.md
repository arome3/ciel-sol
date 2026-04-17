# Ciel PEA / 1.0 — Test Vectors

**Companion to:** [ciel-pea-1.0.md](./ciel-pea-1.0.md)
**Purpose:** Provide an implementable end-to-end verification walkthrough. If you can work through this document and arrive at `signature_valid = true`, your verifier is conformant.

---

## 1. Canonical test vector — BLOCK verdict

The canonical wire-byte test vector for `ciel-pea/1.0` is the committed binary fixture `ciel_attestation_v1.bin` in the Ciel reference repository. This fixture is load-bearing: both the off-chain signer and the on-chain `CielAssert` verifier must produce and consume these exact bytes (Ciel Key Invariant #6). If your implementation roundtrips these bytes byte-for-byte, you are wire-compatible.

### 1.1 Field values (from `attestation_fixtures.json`)

| Field | Value |
|---|---|
| `magic` | `CIEL` (ASCII, bytes `0x43 0x49 0x45 0x4c`) |
| `version` | `1` |
| `tx_hash` | `0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20` |
| `verdict` | `2` (BLOCK) |
| `safety_score` | `3500` |
| `optimality_score` | `0` |
| `checker_outputs_hash` | `2122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f40` |
| `slot` | `350000000` |
| `expiry_slot` | `350000002` |
| `signer` | `4142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f60` |
| `timestamp` | `1712000000` |
| `timeout_at_ms` | `0` |

### 1.2 Wire bytes (hex, 132 bytes)

The Borsh-serialized wire bytes for the above field values, in spec §4 field order:

```
4349454c 01
0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20
02
ac0d
0000
2122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f40
80e63c14 00000000
82e63c14 00000000
4142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f60
00e7 0d66 00000000
0000
```

Whitespace is illustrative. Actual wire bytes are 132 contiguous octets.

Notes on endianness (per §4):

- `safety_score = 3500 = 0x0dac` → LE bytes `ac 0d`
- `optimality_score = 0` → LE bytes `00 00`
- `slot = 350000000 = 0x14 3ce6 80` → LE bytes `80 e6 3c 14 00 00 00 00`
- `expiry_slot = 350000002` → LE bytes `82 e6 3c 14 00 00 00 00`
- `timestamp = 1712000000` → LE bytes `00 e7 0d 66 00 00 00 00`
- `timeout_at_ms = 0` → LE bytes `00 00`

### 1.3 Base64 encoding (`wire.borsh_b64`)

Take the 132 wire bytes above, encode as base64 per RFC 4648 with no line breaks, and populate `wire.borsh_b64` with the result. That string is the exact payload signed by the issuer.

To compute the signature, the issuer runs `ed25519_sign(issuer_priv_key, wire_bytes)` where `wire_bytes` is the 132-byte sequence from §1.2. The `signer` field in the attestation MUST equal `ed25519_pub_key(issuer_priv_key)`.

## 2. Step-by-step verification

Given a received PEA JSON object, a conformant verifier executes the following sequence. Any step that fails MUST cause the verifier to reject the PEA.

### Step 1 — Schema validation

Validate the received JSON against `ciel-pea-1.0.schema.json`. Reject if invalid.

```
ajv validate -s ciel-pea-1.0.schema.json -d received.json
```

### Step 2 — Extract wire bytes

```
wire_bytes = base64_decode(pea.wire.borsh_b64)
assert len(wire_bytes) == 132
```

### Step 3 — Re-serialize and compare

Serialize `pea.attestation` to Borsh using the field order in spec §4. Byte-compare against `wire_bytes`. Any mismatch is a tamper indicator.

```
local_wire = borsh_serialize(pea.attestation)
assert local_wire == wire_bytes
```

### Step 4 — Parse signer

```
signer_pubkey = hex_decode(pea.attestation.signer)
assert len(signer_pubkey) == 32
```

### Step 5 — Parse signature

```
signature = hex_decode(pea.signature)
assert len(signature) == 64
```

### Step 6 — Strict Ed25519 verification

```
# RFC 8032, with strict verification (reject non-canonical R, small-order points)
valid = ed25519_verify_strict(
    public_key = signer_pubkey,
    message    = wire_bytes,
    signature  = signature,
)
assert valid == True
```

### Step 7 — Freshness check

```
assert pea.attestation.expiry_slot >= current_slot_of_target_network
```

### Step 8 — Trust check (out of band)

```
# Consumer-specific allowlist of trusted issuers.
assert signer_pubkey in consumer.trusted_issuers
```

If all eight steps pass, the PEA is **accepted**.

## 3. Language-specific verifier snippets

### 3.1 Rust (ed25519-dalek)

```rust
use ed25519_dalek::{Signature, VerifyingKey};

pub fn verify_pea(pea: &Pea, current_slot: u64, trusted: &[VerifyingKey]) -> Result<(), VerifyError> {
    let wire_bytes = base64::decode(&pea.wire.borsh_b64)?;
    if wire_bytes.len() != 132 { return Err(VerifyError::WireLen); }

    let local_wire = borsh::to_vec(&pea.attestation)?;
    if local_wire != wire_bytes { return Err(VerifyError::WireMismatch); }

    let signer_bytes: [u8; 32] = hex::decode(&pea.attestation.signer)?.try_into()?;
    let signer = VerifyingKey::from_bytes(&signer_bytes)?;

    let sig_bytes: [u8; 64] = hex::decode(&pea.signature)?.try_into()?;
    let signature = Signature::from_bytes(&sig_bytes);

    signer.verify_strict(&wire_bytes, &signature)?;

    if pea.attestation.expiry_slot < current_slot {
        return Err(VerifyError::Stale);
    }
    if !trusted.contains(&signer) {
        return Err(VerifyError::UntrustedSigner);
    }
    Ok(())
}
```

### 3.2 TypeScript (Node crypto / tweetnacl)

```ts
import { verify } from '@noble/ed25519';

export async function verifyPea(pea: Pea, currentSlot: bigint, trusted: Uint8Array[]): Promise<boolean> {
  const wireBytes = Buffer.from(pea.wire.borsh_b64, 'base64');
  if (wireBytes.length !== 132) return false;

  const localWire = borshSerialize(pea.attestation); // caller-provided Borsh codec
  if (!wireBytes.equals(localWire)) return false;

  const signer = Buffer.from(pea.attestation.signer, 'hex');
  const signature = Buffer.from(pea.signature, 'hex');
  const ok = await verify(signature, wireBytes, signer);
  if (!ok) return false;

  if (BigInt(pea.attestation.expiry_slot) < currentSlot) return false;
  if (!trusted.some(k => k.equals(signer))) return false;
  return true;
}
```

### 3.3 Python (PyNaCl)

```python
import base64, binascii
from nacl.signing import VerifyKey

def verify_pea(pea, current_slot, trusted_keys):
    wire_bytes = base64.b64decode(pea["wire"]["borsh_b64"])
    assert len(wire_bytes) == 132
    local_wire = borsh_serialize(pea["attestation"])  # caller-provided
    assert local_wire == wire_bytes
    signer = binascii.unhexlify(pea["attestation"]["signer"])
    signature = binascii.unhexlify(pea["signature"])
    VerifyKey(signer).verify(wire_bytes, signature)
    assert pea["attestation"]["expiry_slot"] >= current_slot
    assert signer in trusted_keys
```

## 4. Conformance checklist

A conformant verifier:

- [ ] Parses the JSON envelope and validates it against `ciel-pea-1.0.schema.json`.
- [ ] Extracts 132 bytes from `wire.borsh_b64` and rejects payloads of any other length.
- [ ] Byte-compares the decoded wire bytes against a re-serialization of `attestation`.
- [ ] Uses **strict** Ed25519 verification (`verify_strict` or equivalent).
- [ ] Checks `expiry_slot >= current_slot` where `current_slot` is obtained from the target chain, not from the PEA itself.
- [ ] Applies a consumer-controlled issuer allowlist; a valid signature alone is not sufficient for trust.
- [ ] Treats `verdict = 3` (TIMEOUT) as distinct from WARN and does not permit override.
- [ ] Passes through unknown top-level fields and unknown flag codes without rejecting.

## 5. Regenerating signed examples

The four example JSON files under `./examples/` are regenerated by a small signing harness in the Ciel reference repository:

```bash
cargo run --package ciel-signer --example gen_pea_examples -- \
  --out spec/examples \
  --keyfile spec/test-private-key.json
```

The test private key is **only for spec example signatures**. The corresponding public key is committed at [test-public-key.txt](./test-public-key.txt). Anyone regenerating signatures must use the committed test key so that example signatures verify against the published public key. Production issuers MUST use independently generated keys.
