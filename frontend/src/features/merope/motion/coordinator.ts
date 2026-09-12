import type { MotionChannel, MotionSourceId } from './channels'
import { noteTurnTraceLeaseExpiry } from '../turnTrace'
import { channelPriority } from './channels'

export interface MotionLeaseHandle {
  leaseId: string
  ownerToken: string
  source: MotionSourceId
  generation: number
}

export interface MotionLease {
  leaseId: string
  source: MotionSourceId
  channels: readonly MotionChannel[]
  generation: number
  expiresAtMs: number | null
}

interface PrivateLease extends MotionLease {
  ownerToken: string
}

export interface MotionSnapshot {
  generation: number
  owners: Record<MotionChannel, MotionSourceId>
  leases: readonly MotionLease[]
}

export interface MotionClaimOptions {
  nowMs?: number
  ttlMs?: number | null
}

const IDLE: MotionSourceId = 'idle'

export class RigMotionCoordinator {
  private readonly leases = new Map<string, PrivateLease>()
  private generation = 0
  private clockMs = 0
  private nextLeaseSeq = 1

  claim(
    source: MotionSourceId,
    channels: readonly MotionChannel[],
    options: MotionClaimOptions = {},
  ): MotionLeaseHandle | null {
    if (source === IDLE) return null
    const unique = uniqueChannels(channels)
    if (unique.length === 0) return null
    const nowMs = options.nowMs ?? this.clockMs
    this.clockMs = nowMs
    this.generation += 1
    const leaseId = `lease-${this.nextLeaseSeq}`
    this.nextLeaseSeq += 1
    const ownerToken = createOwnerToken()
    const handle: MotionLeaseHandle = {
      leaseId,
      ownerToken,
      source,
      generation: this.generation,
    }
    this.leases.set(leaseId, {
      ...handle,
      channels: unique,
      expiresAtMs: expiry(nowMs, options.ttlMs),
    })
    return handle
  }

  renew(
    handle: MotionLeaseHandle | null | undefined,
    channels: readonly MotionChannel[],
    options: MotionClaimOptions = {},
  ): MotionLeaseHandle | null {
    if (!handle) return null
    const current = this.authenticated(handle)
    if (!current) return null
    const unique = uniqueChannels(channels)
    const nowMs = options.nowMs ?? this.clockMs
    this.clockMs = nowMs
    if (unique.length === 0) {
      this.leases.delete(handle.leaseId)
      this.generation += 1
      return null
    }
    this.generation += 1
    const next: MotionLeaseHandle = {
      leaseId: current.leaseId,
      ownerToken: current.ownerToken,
      source: current.source,
      generation: this.generation,
    }
    this.leases.set(current.leaseId, {
      ...next,
      channels: unique,
      expiresAtMs: expiry(nowMs, options.ttlMs),
    })
    return next
  }

  release(
    handle: MotionLeaseHandle | null | undefined,
    channels?: readonly MotionChannel[],
  ): boolean {
    if (!handle) return false
    const current = this.authenticated(handle)
    if (!current) return false
    if (!channels || channels.length === 0) {
      this.leases.delete(handle.leaseId)
      this.generation += 1
      return true
    }
    const drop = new Set(channels)
    const next = Iterator.from(
      new Set(current.channels).difference(drop),
    ).toArray()
    if (next.length === 0) this.leases.delete(handle.leaseId)
    else this.leases.set(handle.leaseId, { ...current, channels: next })
    this.generation += 1
    return true
  }

  tick(nowMs: number): void {
    this.clockMs = nowMs
    let expired = 0
    for (const [leaseId, lease] of this.leases) {
      if (lease.expiresAtMs !== null && lease.expiresAtMs <= nowMs) {
        this.leases.delete(leaseId)
        expired += 1
      }
    }
    if (expired > 0) {
      this.generation += 1
      noteTurnTraceLeaseExpiry(expired)
    }
  }

  leaseCount(): number {
    return this.leases.size
  }

  owner(
    channel: MotionChannel,
    nowMs: number = this.clockMs,
  ): MotionSourceId {
    this.tick(nowMs)
    return this.winner(channel)
  }

  private winner(channel: MotionChannel): MotionSourceId {
    let winner: MotionSourceId = IDLE
    let best = 0
    let bestGeneration = 0
    for (const lease of this.leases.values()) {
      if (!lease.channels.includes(channel)) continue
      const priority = channelPriority(channel, lease.source)
      if (
        priority > best ||
        (priority === best && lease.generation > bestGeneration)
      ) {
        winner = lease.source
        best = priority
        bestGeneration = lease.generation
      }
    }
    return winner
  }

  snapshot(nowMs: number = this.clockMs): MotionSnapshot {
    this.tick(nowMs)
    const owners = {
      mouth: this.winner('mouth'),
      expression: this.winner('expression'),
      gaze: this.winner('gaze'),
      headBody: this.winner('headBody'),
    }
    const publicLeases: MotionLease[] = []
    for (const lease of this.leases.values()) {
      publicLeases.push(toPublicLease(lease))
    }
    return {
      generation: this.generation,
      owners,
      leases: publicLeases,
    }
  }

  private authenticated(handle: MotionLeaseHandle): PrivateLease | null {
    const current = this.leases.get(handle.leaseId)
    if (!current) return null
    if (current.ownerToken !== handle.ownerToken) return null
    if (current.source !== handle.source) return null
    return current
  }
}

const runtime = { current: new RigMotionCoordinator() }

export function getRigMotionCoordinator(): RigMotionCoordinator {
  return runtime.current
}

function uniqueChannels(channels: readonly MotionChannel[]): MotionChannel[] {
  const seen = new Set<MotionChannel>()
  const unique: MotionChannel[] = []
  for (const channel of channels) {
    if (seen.has(channel)) continue
    seen.add(channel)
    unique.push(channel)
  }
  return unique
}

function expiry(nowMs: number, ttlMs: number | null | undefined): number | null {
  return typeof ttlMs === 'number' && ttlMs > 0 ? nowMs + ttlMs : null
}

function createOwnerToken(): string {
  const cryptoObj = globalThis.crypto
  if (cryptoObj && typeof cryptoObj.randomUUID === 'function') {
    return cryptoObj.randomUUID()
  }
  return `tok-${Math.random().toString(36).slice(2, 12)}-${Date.now().toString(36)}`
}

function toPublicLease(lease: PrivateLease): MotionLease {
  return {
    leaseId: lease.leaseId,
    source: lease.source,
    channels: lease.channels,
    generation: lease.generation,
    expiresAtMs: lease.expiresAtMs,
  }
}
