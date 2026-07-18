import type {
  RuntimeGrantKind,
  TappRuntimeGrantResponse,
} from '../services/TappApiService'
import {
  authorizeTappRuntimePermission,
  issueTappRuntimeGrant,
  revokeTappRuntimeGrant,
} from '../services/TappApiService'

const REFRESH_SKEW_MS = 30_000

/**
 * Host-owned runtime identity. The token stays in the parent application and is
 * never serialized into iframe HTML or postMessage payloads.
 */
export class TappRuntimeGrant {
  private static readonly tokenOwners = new Map<string, TappRuntimeGrant>()
  private static readonly instances = new Set<TappRuntimeGrant>()
  private current: TappRuntimeGrantResponse | null = null
  private refreshPromise: Promise<TappRuntimeGrantResponse> | null = null
  private destroyed = false

  constructor(
    private readonly tappId: string,
    private readonly instanceId: string,
    private readonly kind: RuntimeGrantKind,
  ) {
    TappRuntimeGrant.instances.add(this)
  }

  async getToken(): Promise<string> {
    if (this.destroyed) {
      throw new Error('Tapp runtime has already stopped')
    }

    const expiresAt = this.current
      ? Date.parse(this.current.expiresAt)
      : Number.NaN
    if (
      this.current &&
      Number.isFinite(expiresAt) &&
      expiresAt - Date.now() > REFRESH_SKEW_MS
    ) {
      return this.current.token
    }

    if (!this.refreshPromise) {
      this.refreshPromise = issueTappRuntimeGrant(
        this.tappId,
        this.instanceId,
        this.kind,
      ).finally(() => {
        this.refreshPromise = null
      })
    }

    const grant = await this.refreshPromise
    if (this.destroyed) {
      void revokeTappRuntimeGrant(this.tappId, grant.runtimeId)
      throw new Error('Tapp runtime stopped while its grant was being issued')
    }

    const previous = this.current
    this.current = grant
    TappRuntimeGrant.tokenOwners.set(grant.token, this)
    if (previous && previous.token !== grant.token) {
      TappRuntimeGrant.tokenOwners.delete(previous.token)
      if (previous.runtimeId !== grant.runtimeId) {
        void revokeTappRuntimeGrant(this.tappId, previous.runtimeId)
      }
    }
    return grant.token
  }

  async getOwnerId(): Promise<number> {
    await this.getToken()
    if (!this.current) {
      throw new Error('Tapp runtime grant is not initialized')
    }
    return this.current.ownerId
  }

  async getRuntimeId(): Promise<string> {
    await this.getToken()
    if (!this.current) {
      throw new Error('Tapp runtime grant is not initialized')
    }
    return this.current.runtimeId
  }

  async authorize(permission: string): Promise<void> {
    const token = await this.getToken()
    await authorizeTappRuntimePermission(this.tappId, permission, token)
  }

  destroy(): void {
    if (this.destroyed) return
    this.destroyed = true
    TappRuntimeGrant.instances.delete(this)
    if (this.current) {
      TappRuntimeGrant.tokenOwners.delete(this.current.token)
      void revokeTappRuntimeGrant(this.tappId, this.current.runtimeId)
      this.current = null
    }
  }

  static destroyAll(): void {
    for (const grant of [...TappRuntimeGrant.instances]) grant.destroy()
    TappRuntimeGrant.tokenOwners.clear()
  }

  /** Reissue once after a backend restart or explicit server-side revocation. */
  static async recoverRejectedToken(token: string): Promise<string | null> {
    const owner = TappRuntimeGrant.tokenOwners.get(token)
    if (!owner || owner.destroyed) return null
    if (owner.current?.token === token) {
      const rejected = owner.current
      owner.current = null
      TappRuntimeGrant.tokenOwners.delete(token)
      void revokeTappRuntimeGrant(owner.tappId, rejected.runtimeId)
    }
    return owner.getToken()
  }
}
