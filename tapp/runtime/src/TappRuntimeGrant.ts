export type RuntimeGrantKind = 'page' | 'widget' | 'headless'

export interface TappRuntimeGrantResponse {
  version: 2
  token: string
  runtimeId: string
  tappId: string
  ownerId: number
  subjectId: number
  instanceId: string
  kind: RuntimeGrantKind
  permissions: string[]
  expiresAt: string
}

/** How the host issues, revokes and checks grants (on the site: its `/api/tapps` routes). */
export interface TappGrantTransport {
  issue: (
    tappId: string,
    instanceId: string,
    kind: RuntimeGrantKind,
  ) => Promise<TappRuntimeGrantResponse>
  revoke: (tappId: string, runtimeId: string) => Promise<void>
  authorize: (tappId: string, permission: string, token: string) => Promise<void>
}

const REFRESH_SKEW_MS = 30_000

interface SharedWidgetEntry {
  grant: TappRuntimeGrant
  refs: number
}

/** 页面内共享 widget grant 的不透明 instanceId。不从 tappId 派生，避免跨标签页互相撤销。 */
export function newSharedWidgetInstanceId(): string {
  const bytes = new Uint8Array(16)
  crypto.getRandomValues(bytes)
  let hex = ''
  for (const byte of bytes) hex += byte.toString(16).padStart(2, '0')
  return `ws.${hex}`
}

/** 一页一份：持有这页全部 grant、它们的 token 归属和共享 widget 池。 */
export class TappGrantRegistry {
  /** 内部：只给 TappRuntimeGrant 用。 */
  readonly tokenOwners = new Map<string, TappRuntimeGrant>()
  /** 内部：只给 TappRuntimeGrant 用。 */
  readonly instances = new Set<TappRuntimeGrant>()
  private readonly sharedWidgetEntries = new Map<string, SharedWidgetEntry>()

  constructor(readonly transport: TappGrantTransport) {}

  create(
    tappId: string,
    instanceId: string,
    kind: RuntimeGrantKind,
  ): TappRuntimeGrant {
    return new TappRuntimeGrant(this, tappId, instanceId, kind)
  }

  /** 同 tappId 的 Widget 共享宿主 grant；各 iframe 仍有独立 session token。 */
  acquireSharedWidget(tappId: string): {
    grant: TappRuntimeGrant
    release: () => void
  } {
    let entry = this.sharedWidgetEntries.get(tappId)
    if (!entry || entry.grant.isDestroyed()) {
      const grant = this.create(tappId, newSharedWidgetInstanceId(), 'widget')
      entry = { grant, refs: 0 }
      this.sharedWidgetEntries.set(tappId, entry)
    }
    entry.refs += 1
    let released = false
    const grant = entry.grant
    return {
      grant,
      release: () => {
        if (released) return
        released = true
        const current = this.sharedWidgetEntries.get(tappId)
        if (!current || current.grant !== grant) return
        current.refs -= 1
        if (current.refs <= 0) {
          this.sharedWidgetEntries.delete(tappId)
          current.grant.destroy()
        }
      },
    }
  }

  sharedWidgetRefCount(tappId: string): number {
    return this.sharedWidgetEntries.get(tappId)?.refs ?? 0
  }

  /** 清共享 widget 池。登录/登出走 destroyAll。 */
  clearSharedWidgetGrants(): void {
    for (const entry of this.sharedWidgetEntries.values()) {
      entry.grant.destroy()
    }
    this.sharedWidgetEntries.clear()
  }

  destroyAll(): void {
    for (const grant of Iterator.from(this.instances).toArray()) grant.destroy()
    this.tokenOwners.clear()
    this.sharedWidgetEntries.clear()
  }

  async recoverRejectedToken(token: string): Promise<string | null> {
    const owner = this.tokenOwners.get(token)
    if (!owner || owner.isDestroyed()) return null
    owner.dropRejected(token)
    return owner.getToken()
  }
}

/**
 * 宿主持有；永不写入 iframe HTML 或 postMessage。多 widget 用 acquireSharedWidget：共享 BE
 * grant，各持自己的 session token。由 TappGrantRegistry.create 创建。
 */
export class TappRuntimeGrant {
  private current: TappRuntimeGrantResponse | null = null
  private refreshPromise: Promise<TappRuntimeGrantResponse> | null = null
  private destroyed = false

  constructor(
    private readonly registry: TappGrantRegistry,
    private readonly tappId: string,
    private readonly instanceId: string,
    private readonly kind: RuntimeGrantKind,
  ) {
    registry.instances.add(this)
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
      this.refreshPromise = this.registry.transport.issue(
        this.tappId,
        this.instanceId,
        this.kind,
      ).finally(() => {
        this.refreshPromise = null
      })
    }

    const grant = await this.refreshPromise
    if (this.destroyed) {
      void this.registry.transport.revoke(this.tappId, grant.runtimeId)
      throw new Error('Tapp runtime stopped while its grant was being issued')
    }

    const previous = this.current
    this.current = grant
    this.registry.tokenOwners.set(grant.token, this)
    if (previous && previous.token !== grant.token) {
      this.registry.tokenOwners.delete(previous.token)
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
    await this.registry.transport.authorize(this.tappId, permission, token)
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

  /** 后端拒收了这个 token：丢掉并撤销它，下一次 getToken 重新签发。 */
  dropRejected(token: string): void {
    if (this.current?.token !== token) return
    const rejected = this.current
    this.current = null
    this.registry.tokenOwners.delete(token)
    void this.registry.transport.revoke(this.tappId, rejected.runtimeId)
  }

  destroy(): void {
    if (this.destroyed) return
    this.destroyed = true
    this.registry.instances.delete(this)
    if (this.current) {
      this.registry.tokenOwners.delete(this.current.token)
      void this.registry.transport.revoke(this.tappId, this.current.runtimeId)
      this.current = null
    }
  }
}
