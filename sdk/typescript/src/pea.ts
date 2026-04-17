// Ciel PEA / 1.0 — TypeScript envelope helpers.
//
// Reference: https://spec.ciel.xyz/ciel-pea-1.0
//
// This module is intentionally minimal. It provides:
//   - PeaEnvelope / PeaAttestation types mirroring the JSON envelope
//   - isPeaEnvelope() shape-level type guard
//   - wireBytesFromEnvelope() base64 -> Uint8Array
//   - A verifyPea() example using @noble/ed25519 (caller provides the Borsh
//     serializer for the attestation object).
//
// Deliberately no runtime dependencies beyond @noble/ed25519. Borsh
// serialization is the caller's responsibility because each consuming app
// will import its own Borsh codec.

export const PEA_SPEC_VERSION = 'ciel-pea/1.0';
export const PEA_FIXTURE_VERSION = 'ciel_attestation_v1';
export const PEA_WIRE_BYTES = 132;

export type PeaVerdict = 0 | 1 | 2 | 3; // APPROVE | WARN | BLOCK | TIMEOUT
export const VERDICT_LABELS: Record<PeaVerdict, string> = {
  0: 'APPROVE',
  1: 'WARN',
  2: 'BLOCK',
  3: 'TIMEOUT',
};

export interface PeaAttestation {
  magic: 'CIEL';
  version: 1;
  tx_hash: string;                // 64 hex chars
  verdict: PeaVerdict;
  safety_score: number;           // 0..10000 normally; 65535 for TIMEOUT
  optimality_score: number;       // same range semantics
  checker_outputs_hash: string;   // 64 hex chars
  slot: number | bigint;          // u64
  expiry_slot: number | bigint;   // u64
  signer: string;                 // 64 hex chars (Ed25519 public key)
  timestamp: number | bigint;     // i64 unix seconds
  timeout_at_ms: number;          // u16, 0 unless verdict=TIMEOUT
}

export type PeaFlagSeverity = 'INFO' | 'WARN' | 'BLOCK';

export interface PeaFlag {
  code: string;
  severity: PeaFlagSeverity;
  detail?: string;
}

export interface PeaIntent {
  intent_nl?: string;
  intent_fingerprint?: string;
  intent_satisfied?: boolean | null;
}

export interface PeaIssuer {
  agent_id?: string;
  operator_pubkey_hash?: string;
  identity_proof?: { kind: 'none' | 'world_id' | 'ens' | 'revoked_world_id'; value?: string };
}

export interface PeaEnvelope {
  spec_version: typeof PEA_SPEC_VERSION;
  attestation: PeaAttestation;
  signature: string;      // 128 hex chars
  wire: {
    borsh_b64: string;
    fixture_version: typeof PEA_FIXTURE_VERSION;
  };
  flags?: PeaFlag[];
  intent?: PeaIntent;
  issuer?: PeaIssuer;
  [additional: string]: unknown; // additive-only per spec §10
}

/**
 * Shape-level type guard for a PEA envelope. Does not verify signatures.
 */
export function isPeaEnvelope(value: unknown): value is PeaEnvelope {
  if (!value || typeof value !== 'object') return false;
  const obj = value as Record<string, unknown>;
  if (obj.spec_version !== PEA_SPEC_VERSION) return false;
  const att = obj.attestation as Record<string, unknown> | undefined;
  if (!att || att.magic !== 'CIEL' || att.version !== 1) return false;
  if (typeof obj.signature !== 'string' || obj.signature.length !== 128) return false;
  const wire = obj.wire as Record<string, unknown> | undefined;
  if (!wire || wire.fixture_version !== PEA_FIXTURE_VERSION) return false;
  if (typeof wire.borsh_b64 !== 'string') return false;
  return true;
}

/**
 * Decode wire.borsh_b64 into raw 132 wire bytes.
 */
export function wireBytesFromEnvelope(envelope: PeaEnvelope): Uint8Array {
  const bin = typeof Buffer !== 'undefined'
    ? Buffer.from(envelope.wire.borsh_b64, 'base64')
    : Uint8Array.from(atob(envelope.wire.borsh_b64), c => c.charCodeAt(0));
  const bytes = bin instanceof Uint8Array ? bin : new Uint8Array(bin);
  if (bytes.length !== PEA_WIRE_BYTES) {
    throw new Error(`wire bytes length ${bytes.length} != ${PEA_WIRE_BYTES}`);
  }
  return bytes;
}

/**
 * Example verifier. Caller must supply `borshSerializeAttestation` because the
 * Borsh codec is runtime-dependent. Returns true iff:
 *  - shape is valid
 *  - wire bytes match a Borsh re-serialization of the attestation object
 *  - Ed25519 strict signature verifies under attestation.signer
 *  - expiry_slot >= currentSlot
 *  - signer is present in trustedSigners (optional)
 *
 * `ed25519Verify` must be a *strict* verifier (rejects small-order points and
 * non-canonical R). Use `@noble/ed25519` which is strict by default.
 */
export async function verifyPea(
  envelope: PeaEnvelope,
  opts: {
    currentSlot: bigint;
    ed25519Verify: (sig: Uint8Array, msg: Uint8Array, pub: Uint8Array) => Promise<boolean>;
    borshSerializeAttestation: (att: PeaAttestation) => Uint8Array;
    trustedSigners?: Uint8Array[];
  },
): Promise<{ valid: boolean; reason?: string }> {
  if (!isPeaEnvelope(envelope)) return { valid: false, reason: 'shape' };
  const wireBytes = wireBytesFromEnvelope(envelope);
  const localWire = opts.borshSerializeAttestation(envelope.attestation);
  if (!bytesEqual(localWire, wireBytes)) return { valid: false, reason: 'wire_mismatch' };

  const signer = hexToBytes(envelope.attestation.signer);
  const signature = hexToBytes(envelope.signature);
  const sigValid = await opts.ed25519Verify(signature, wireBytes, signer);
  if (!sigValid) return { valid: false, reason: 'signature' };

  const expiry = BigInt(envelope.attestation.expiry_slot);
  if (expiry < opts.currentSlot) return { valid: false, reason: 'stale' };

  if (opts.trustedSigners && !opts.trustedSigners.some(s => bytesEqual(s, signer))) {
    return { valid: false, reason: 'untrusted_signer' };
  }
  return { valid: true };
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

function hexToBytes(hex: string): Uint8Array {
  if (hex.length % 2 !== 0) throw new Error('bad hex');
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

function bytesEqual(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}
