/**
 * Live federation size caps for the host bridge.
 *
 * Defaults match backend `memory_profile` DEFAULT_* (not the pre-#316 36/64/80).
 * `refreshFederationLimits` reads GET /api/federation/public/limits so saver
 * hosts reject oversized TAPP payloads before the request leaves the browser.
 */

import { federationApi } from '../../services/federationApi'

export type FederationLiveLimits = {
  profile: 'default' | 'saver'
  messagePayloadBytes: number
  noteImageBytes: number
  noteVideoBytes: number
}

export const DEFAULT_FEDERATION_LIVE_LIMITS: FederationLiveLimits = {
  profile: 'default',
  messagePayloadBytes: 4 * 1024 * 1024,
  noteImageBytes: 32 * 1024 * 1024,
  noteVideoBytes: 256 * 1024 * 1024,
}

const MESSAGE_ENVELOPE_HEADROOM_BYTES = 64 * 1024

let cached: FederationLiveLimits = { ...DEFAULT_FEDERATION_LIVE_LIMITS }
let inflight: Promise<void> | null = null

export function federationLiveLimits(): FederationLiveLimits {
  return cached
}

export function federationMessageEnvelopeBytes(): number {
  return cached.messagePayloadBytes + MESSAGE_ENVELOPE_HEADROOM_BYTES
}

export function applyFederationLimitsPayload(
  raw: unknown,
): FederationLiveLimits | null {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null
  const o = raw as Record<string, unknown>
  const profile = o.profile === 'saver' ? 'saver' : o.profile === 'default' ? 'default' : null
  const messagePayloadBytes = asPositiveInt(o.message_payload_bytes)
  const noteImageBytes = asPositiveInt(o.note_image_bytes)
  const noteVideoBytes = asPositiveInt(o.note_video_bytes)
  if (
    profile === null ||
    messagePayloadBytes === null ||
    noteImageBytes === null ||
    noteVideoBytes === null
  ) {
    return null
  }
  cached = {
    profile,
    messagePayloadBytes,
    noteImageBytes,
    noteVideoBytes,
  }
  return cached
}

/** Test helper: restore process-local cache to product defaults. */
export function resetFederationLimitsForTests(): void {
  cached = { ...DEFAULT_FEDERATION_LIVE_LIMITS }
  inflight = null
}

export function refreshFederationLimits(): Promise<void> {
  if (!inflight) {
    inflight = Promise.resolve()
      .then(() => federationApi.getPublicLimits())
      .then((payload) => {
        applyFederationLimitsPayload(payload)
      })
      .catch(() => {
        // Keep last-known / defaults. Backend remains authoritative.
      })
      .finally(() => {
        inflight = null
      })
  }
  return inflight
}

function asPositiveInt(value: unknown): number | null {
  if (typeof value !== 'number' || !Number.isFinite(value) || value <= 0) {
    return null
  }
  return Math.floor(value)
}
