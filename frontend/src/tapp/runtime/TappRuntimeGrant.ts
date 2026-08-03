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

interface SharedWidgetEntry {
  grant: TappRuntimeGrant
  refs: number
}

/**
 * Backend `MAX_INSTANCE_ID_LENGTH` is 100; tapp ids may be up to 128.
 * Never embed the full tappId in instance_id — use a stable longer hash form.
 * Charset must stay compatible with BE: [A-Za-z0-9_.-]
 */
export function sharedWidgetInstanceId(tappId: string): string {
  // Dual FNV-1a 32-bit streams → 16 hex chars of entropy (collision-resistant
  // enough for the share table, which is also keyed by tappId).
  let h1 = 0x811C9DC5
  let h2 = 0x811C9DC5 ^ 0x9E3779B9
  for (let i = 0; i < tappId.length; i++) {
    const c = tappId.charCodeAt(i)
    h1 ^= c
    h1 = Math.imul(h1, 0x01000193)
    h2 ^= c + i
    h2 = Math.imul(h2, 0x01000193)
  }
  const hex =
    (h1 >>> 0).toString(16).padStart(8, '0') +
    (h2 >>> 0).toString(16).padStart(8, '0')
  // Compact stable slug (not the full id) for debuggability, then hash.
  const slug = tappId
    .replace(/[^\w.-]/g, '_')
    .replace(/_+/g, '_')
    .slice(0, 48)
  const id = `ws.${slug}.${hex}`
  return id.length <= 100 ? id : id.slice(0, 100)
}

/**
 * Host-owned runtime identity. The token stays in the parent application and is
 * never serialized into iframe HTML or postMessage payloads.
 *
 * Multi-widget same Tapp: use {@link TappRuntimeGrant.acquireSharedWidget} so N
 * sandboxes share one BE grant (refcount) while each keeps its own session token.
 */
export class TappRuntimeGrant {
  private static readonly tokenOwners = new Map<string, TappRuntimeGrant>()
  private static readonly instances = new Set<TappRuntimeGrant>()
  /** Refcounted grants for widget sandboxes of the same tappId. */
  private static readonly sharedWidgetEntries = new Map<string, SharedWidgetEntry>()
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

  /**
   * Acquire a refcounted host grant for all Widget sandboxes of `tappId`.
   * Isolation: iframes still have distinct session tokens; only host BE identity is shared.
   */
  static acquireSharedWidget(tappId: string): {
    grant: TappRuntimeGrant
    release: () => void
  } {
    let entry = TappRuntimeGrant.sharedWidgetEntries.get(tappId)
    if (!entry || entry.grant.isDestroyed()) {
      const grant = new TappRuntimeGrant(
        tappId,
        sharedWidgetInstanceId(tappId),
        'widget',
      )
      entry = { grant, refs: 0 }
      TappRuntimeGrant.sharedWidgetEntries.set(tappId, entry)
    }
    entry.refs += 1
    let released = false
    const grant = entry.grant
    return {
      grant,
      release: () => {
        if (released) return
        released = true
        const current = TappRuntimeGrant.sharedWidgetEntries.get(tappId)
        if (!current || current.grant !== grant) return
        current.refs -= 1
        if (current.refs <= 0) {
          TappRuntimeGrant.sharedWidgetEntries.delete(tappId)
          current.grant.destroy()
        }
      },
    }
  }

  /** Test helper */
  static sharedWidgetRefCount(tappId: string): number {
    return TappRuntimeGrant.sharedWidgetEntries.get(tappId)?.refs ?? 0
  }

  /**
   * Destroy every shared-widget pool entry and clear the table.
   * Targeted helper for tests / hot-reload; login/logout uses {@link destroyAll}.
   */
  static clearSharedWidgetGrants(): void {
    for (const entry of TappRuntimeGrant.sharedWidgetEntries.values()) {
      entry.grant.destroy()
    }
    TappRuntimeGrant.sharedWidgetEntries.clear()
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

  isDestroyed(): boolean {
    return this.destroyed
  }

  getTappId(): string {
    return this.tappId
  }

  getInstanceId(): string {
    return this.instanceId
  }

  getKind(): RuntimeGrantKind {
    return this.kind
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
    TappRuntimeGrant.sharedWidgetEntries.clear()
  }

  /**
   * Reissue once after backend restart, revocation, or subject mismatch
   * (`INVALID_RUNTIME_GRANT` / `RUNTIME_GRANT_SUBJECT_MISMATCH` from host clients).
   */
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
