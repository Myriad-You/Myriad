import type { TappPlaygroundCode } from '../services/TappPlaygroundService'
import type {
  BackgroundRequirement,
  CustomPlatformConfig,
  RegisteredWidget,
  TappInstance,
  TappManifest,
  TappPermission,
  TappStatus,
  WidgetRegistration,
} from '../types'
import { getDynamicContentProvider } from '../../services/DynamicContentProvider'
import * as TappApiService from '../services/TappApiService'
import { getResourceLoader } from './sandbox/resourceLoader'
import { TappPermissionController } from './TappPermission'

type RuntimeEvent =
  | 'tapp:installed'
  | 'tapp:uninstalled'
  | 'tapp:started'
  | 'tapp:stopped'
  | 'tapp:updated'
  | 'tapp:error'
  | 'widget:registered'
  | 'widget:unregistered'
  | 'platform:registered'
  | 'sync:complete'
  | 'background:changed'

type RuntimeEventCallback = (data: unknown) => void

function samePermissions(
  left: TappPermission[],
  right: TappPermission[],
): boolean {
  return (
    left.length === right.length &&
    new Set(right).isSubsetOf(new Set(left))
  )
}

class RequestDeduplicator {
  private pendingRequests: Map<string, Promise<unknown>> = new Map()

  async dedupe<T>(key: string, factory: () => Promise<T>): Promise<T> {
    const pending = this.pendingRequests.get(key)
    if (pending) {
      return pending as Promise<T>
    }

    const promise = factory().finally(() => {
      this.pendingRequests.delete(key)
    })

    this.pendingRequests.set(key, promise)
    return promise
  }
}

export class TappRuntime {
  private static instance: TappRuntime | null = null

  private installedTapps: Map<string, TappInstance> = new Map()

  private runningTapps: Set<string> = new Set()

  /** 共享管理员 Tapp 的库 status 是站主持久态，不能当成其他用户/访客的运行态。 */
  private sessionRunningTapps: Set<string> = new Set()

  private registeredWidgets: Map<string, RegisteredWidget> = new Map()

  private registeredPlatforms: Map<
    string,
    CustomPlatformConfig & { tappId: string }
  > = new Map()

  private backgroundRequirements: Map<string, Set<BackgroundRequirement>> =
    new Map()

  private manifestBackgroundRequirements: Map<
    string,
    Set<BackgroundRequirement>
  > = new Map()

  private eventListeners: Map<RuntimeEvent, Set<RuntimeEventCallback>> =
    new Map()

  private synced: boolean = false

  private syncError: Error | null = null

  private initialSyncPromise: Promise<void>

  private deduplicator = new RequestDeduplicator()

  /** 同一 Tapp 的 start/stop 串行，避免多窗口并发反转状态。 */
  private lifecycleTransitions = new Map<string, Promise<void>>()
  private uninstallingTapps = new Set<string>()

  private static readonly CACHE_TTL = {
    tappList: 30 * 1000,
  }

  private lastSyncTime: number = 0

  private constructor() {
    this.initialSyncPromise = this.syncFromBackend().catch((err) => {
      console.error('[TappRuntime] Initial sync failed:', err)
      this.syncError = err instanceof Error ? err : new Error(String(err))
      this.synced = true
    })
  }

  static getInstance(): TappRuntime {
    if (!TappRuntime.instance) {
      TappRuntime.instance = new TappRuntime()
    }
    return TappRuntime.instance
  }

  static reset(): void {
    TappRuntime.instance?.dispose()
    TappRuntime.instance = null
    getResourceLoader().clearCache()
  }

  private dispose(): void {
    this.installedTapps.clear()
    this.runningTapps.clear()
    this.sessionRunningTapps.clear()
    this.registeredWidgets.clear()
    this.registeredPlatforms.clear()
    this.backgroundRequirements.clear()
    this.manifestBackgroundRequirements.clear()
    this.eventListeners.clear()
    this.lifecycleTransitions.clear()
    this.uninstallingTapps.clear()
    this.synced = false
    this.syncError = null
    this.lastSyncTime = 0
  }

  async syncFromBackend(force: boolean = false): Promise<void> {
    if (
      !force &&
      this.synced &&
      Date.now() - this.lastSyncTime < TappRuntime.CACHE_TTL.tappList
    ) {
      return
    }

    return this.deduplicator.dedupe('sync', async () => {
      try {
        const [details, backendWidgets] = await Promise.all([
          TappApiService.listTappDetails(),
          TappApiService.getAllWidgets(),
        ])
        const previousTapps = this.installedTapps
        const permissionChanges: TappInstance[] = []
        this.installedTapps = new Map()
        this.runningTapps.clear()

        for (const detail of details) {
          const userRole =
            (detail.user_role as 'guest' | 'user' | 'admin') || 'guest'
          const isAdminTapp = detail.is_admin_tapp ?? false
          const previous = previousTapps.get(detail.id)
          if (previous && previous.userRole !== userRole) {
            this.sessionRunningTapps.delete(detail.id)
          }

          const persistsLifecycle =
            (userRole === 'admin' && isAdminTapp) ||
            (userRole === 'user' && detail.is_temporary === true)
          const installationStatus: TappInstance['installationStatus'] =
            detail.status === 'running'
              ? 'running'
              : detail.status === 'error'
                ? 'error'
                : 'installed'
          const needsReauthorization = detail.needs_reauthorization ?? false
          if (needsReauthorization) {
            this.sessionRunningTapps.delete(detail.id)
          }

          // 访客不能在刷新后保活已停的 Tapp。
          if (isAdminTapp && installationStatus !== 'running') {
            this.sessionRunningTapps.delete(detail.id)
          }

          const isRunning =
            !needsReauthorization &&
            (isAdminTapp
              ? installationStatus === 'running'
              : this.sessionRunningTapps.has(detail.id) ||
                (persistsLifecycle && installationStatus === 'running'))

          const instance: TappInstance = {
            id: detail.id,
            manifest: detail.manifest as TappManifest,
            status: isRunning
              ? 'running'
              : installationStatus === 'error'
                ? 'error'
                : 'installed',
            installationStatus,
            installedAt: detail.installed_at,
            lastRunAt: detail.last_run_at,
            grantedPermissions: detail.granted_permissions as TappPermission[],
            needsReauthorization,
            userRole,
            isTemporary: detail.is_temporary ?? false,
            isAdminTapp,
            visibility:
              detail.visibility === 'admin' ? 'admin' : 'all',
            error: detail.error_message,
          }
          this.installedTapps.set(detail.id, instance)
          if (
            previous &&
            !samePermissions(
              previous.grantedPermissions,
              instance.grantedPermissions,
            )
          ) {
            permissionChanges.push(instance)
          }
          if (isRunning) {
            this.runningTapps.add(detail.id)
            // 否则重载后 headless core 不会被拉起。
            this.registerManifestBackgroundRequirements(instance)
          }
        }

        // 跨标签卸载后不保留同 ID 本地运行标记。
        for (const tappId of this.sessionRunningTapps) {
          if (!this.installedTapps.has(tappId)) {
            this.sessionRunningTapps.delete(tappId)
          }
        }

        this.registeredWidgets.clear()
        for (const widget of backendWidgets) {
          this.registeredWidgets.set(widget.id, widget)
        }

        const backgroundTappIds = new Set(
          this.backgroundRequirements.keys(),
        ).union(new Set(this.manifestBackgroundRequirements.keys()))
        for (const tappId of backgroundTappIds) {
          if (!this.runningTapps.has(tappId)) {
            this.dropStoppedTappHostState(tappId)
          }
        }

        this.synced = true
        this.syncError = null
        this.lastSyncTime = Date.now()
        for (const instance of permissionChanges) {
          this.emit('tapp:updated', {
            id: instance.id,
            instance,
            reason: 'permissions-changed',
          })
        }
        this.emit('sync:complete', {
          tapps: details.length,
          widgets: this.registeredWidgets.size,
        })
      } catch (error) {
        console.error('[TappRuntime] Failed to sync from backend:', error)
        this.syncError =
          error instanceof Error ? error : new Error(String(error))
        throw error
      }
    })
  }

  async waitForSync(): Promise<void> {
    if (!this.synced) {
      let timeout: ReturnType<typeof setTimeout> | undefined
      try {
        await Promise.race([
          this.initialSyncPromise,
          new Promise<never>((_, reject) => {
            timeout = setTimeout(
              () => reject(new Error('Tapp runtime sync timed out after 10s')),
              10000,
            )
          }),
        ])
      } finally {
        if (timeout) clearTimeout(timeout)
      }
    }

    if (this.syncError) throw this.syncError
  }

  async installTapp(
    manifest: TappManifest,
    code: TappPlaygroundCode,
    _requestedPermissions?: TappPermission[],
  ): Promise<TappInstance> {
    const validation =
      TappPermissionController.validateManifestPermissions(manifest)
    if (!validation.valid) {
      throw new Error(`Invalid manifest: ${validation.errors.join(', ')}`)
    }

    if (this.installedTapps.has(manifest.id)) {
      throw new Error(`Tapp ${manifest.id} is already installed`)
    }

    const result = await TappApiService.installFromCode(manifest, code)
    const detail = await TappApiService.getTapp(result.id)

    const userRole = (detail.user_role as 'guest' | 'user' | 'admin') || 'guest'

    const backendPerms = detail.granted_permissions as TappPermission[]

    const instance: TappInstance = {
      id: detail.id,
      manifest: detail.manifest as TappManifest,
      status: detail.status as TappStatus,
      installedAt: detail.installed_at,
      lastRunAt: detail.last_run_at,
      grantedPermissions: backendPerms,
      needsReauthorization: detail.needs_reauthorization ?? false,
      userRole,
      isTemporary: detail.is_temporary ?? result.isTemporary ?? false,
      isAdminTapp: detail.is_admin_tapp ?? result.isAdminTapp ?? false,
      visibility: detail.visibility === 'admin' ? 'admin' : 'all',
      error: detail.error_message,
    }

    this.installedTapps.set(manifest.id, instance)

    getResourceLoader().clearCache(manifest.id)

    await this.syncFromBackend(true)
    const synchronized = this.installedTapps.get(manifest.id) ?? instance
    this.emit('tapp:installed', { id: manifest.id, instance: synchronized })

    return synchronized
  }

  async uninstallTapp(
    tappId: string,
    options?: { keepData?: boolean },
  ): Promise<void> {
    const instance = this.installedTapps.get(tappId)
    if (!instance) {
      throw new Error(`Tapp ${tappId} is not installed`)
    }
    if (this.uninstallingTapps.has(tappId)) {
      throw new Error(`Tapp ${tappId} is already being uninstalled`)
    }
    this.uninstallingTapps.add(tappId)

    try {
      await this.stopTapp(tappId)

      await TappApiService.uninstallTapp(tappId, options)

      for (const [widgetId, widget] of this.registeredWidgets) {
        if (widget.tappId === tappId) {
          this.registeredWidgets.delete(widgetId)
        }
      }

      for (const [platformId, platform] of this.registeredPlatforms) {
        if (platform.tappId === tappId) {
          this.registeredPlatforms.delete(platformId)
        }
      }

      getResourceLoader().clearCache(tappId)
      this.dropStoppedTappHostState(tappId)

      this.installedTapps.delete(tappId)
      this.sessionRunningTapps.delete(tappId)

      this.emit('tapp:uninstalled', { id: tappId })
    } finally {
      this.uninstallingTapps.delete(tappId)
    }
  }

  /** 仅站主公开装或自己的临时装可启停。访客/普通用户不能启动未启动的站主公开 Tapp。 */
  canControlLifecycle(instance: TappInstance): boolean {
    return this.persistsLifecycle(instance)
  }

  async startTapp(tappId: string): Promise<void> {
    return this.enqueueLifecycleTransition(tappId, async () => {
      const instance = this.installedTapps.get(tappId)
      if (!instance) {
        throw new Error(`Tapp ${tappId} is not installed`)
      }
      if (this.uninstallingTapps.has(tappId)) {
        throw new Error(`Tapp ${tappId} is being uninstalled`)
      }
      if (instance.needsReauthorization) {
        throw new Error(`Tapp ${tappId} requires permission reauthorization`)
      }

      if (this.runningTapps.has(tappId)) return

      // 站主公开 Tapp：只有站主可改全站运行态；访客不得会话假启动。
      if (instance.isAdminTapp && !this.persistsLifecycle(instance)) {
        throw new Error(
          'This Tapp is stopped by the site admin and cannot be started by other users',
        )
      }

      const persistsLifecycle = this.persistsLifecycle(instance)
      if (persistsLifecycle) {
        await TappApiService.startTapp(tappId)
        instance.installationStatus = 'running'
      } else {
        this.sessionRunningTapps.add(tappId)
      }
      instance.status = 'running'
      instance.lastRunAt = new Date().toISOString()
      this.runningTapps.add(tappId)
      this.registerManifestBackgroundRequirements(instance)
      this.emit('tapp:started', { id: tappId, instance })
    })
  }

  private enqueueLifecycleTransition(
    tappId: string,
    operation: () => Promise<void>,
  ): Promise<void> {
    const previous = this.lifecycleTransitions.get(tappId) ?? Promise.resolve()
    const transition: Promise<void> = previous
      .catch(() => undefined)
      .then(operation)
      .finally(() => {
        if (this.lifecycleTransitions.get(tappId) === transition) {
          this.lifecycleTransitions.delete(tappId)
        }
      })
    this.lifecycleTransitions.set(tappId, transition)
    return transition
  }

  private persistsLifecycle(instance: TappInstance): boolean {
    return (
      (instance.userRole === 'admin' && instance.isAdminTapp === true) ||
      (instance.userRole === 'user' && instance.isTemporary === true)
    )
  }

  /** 注册 Manifest 声明的后台需求，供 BackgroundRunner 拉起 headless core。 */
  private registerManifestBackgroundRequirements(instance: TappInstance): void {
    this.setManifestBackgroundRequirements(
      instance.id,
      instance.manifest.backgroundRequirements ?? [],
    )
  }

  private setManifestBackgroundRequirements(
    tappId: string,
    requirements: BackgroundRequirement[],
  ): void {
    const hadRequirement = this.hasBackgroundRequirements(tappId)
    if (requirements.length > 0) {
      this.manifestBackgroundRequirements.set(tappId, new Set(requirements))
    } else {
      this.manifestBackgroundRequirements.delete(tappId)
    }
    const hasRequirement = this.hasBackgroundRequirements(tappId)

    if (hadRequirement !== hasRequirement) {
      this.emit('background:changed', {
        tappId,
        requirements: this.getBackgroundRequirements(tappId),
        hasRequirements: hasRequirement,
      })
    }
  }

  private getEffectiveBackgroundRequirements(
    tappId: string,
  ): Set<BackgroundRequirement> {
    return new Set(
      this.manifestBackgroundRequirements.get(tappId) ?? [],
    ).union(new Set(this.backgroundRequirements.get(tappId) ?? []))
  }

  async stopTapp(tappId: string): Promise<void> {
    return this.enqueueLifecycleTransition(tappId, async () => {
      const instance = this.installedTapps.get(tappId)
      if (!instance) {
        throw new Error(`Tapp ${tappId} is not installed`)
      }

      if (!this.runningTapps.has(tappId)) return

      if (instance.isAdminTapp && !this.persistsLifecycle(instance)) {
        throw new Error(
          'This Tapp is managed by the site admin and cannot be stopped by other users',
        )
      }

      if (this.persistsLifecycle(instance)) {
        await TappApiService.stopTapp(tappId)
        instance.installationStatus = 'installed'
      } else {
        this.sessionRunningTapps.delete(tappId)
      }
      this.dropStoppedTappHostState(tappId)
      instance.status = 'installed'
      this.runningTapps.delete(tappId)
      this.emit('tapp:stopped', { id: tappId, instance })
    })
  }

  getTapp(tappId: string): TappInstance | undefined {
    return this.installedTapps.get(tappId)
  }

  getAllTapps(): TappInstance[] {
    return Iterator.from(this.installedTapps.values()).toArray()
  }

  clearCodeCache(tappId?: string): void {
    getResourceLoader().clearCache(tappId)
  }

  async refreshTapp(tappId: string): Promise<void> {
    this.clearCodeCache(tappId)
    await this.syncFromBackend(true)
    const instance = this.installedTapps.get(tappId)
    if (instance) {
      this.emit('tapp:updated', { id: tappId, instance })
    }
  }

  async refreshPermissionGrants(): Promise<void> {
    await this.syncFromBackend(true)
  }

  isRunning(tappId: string): boolean {
    return this.runningTapps.has(tappId)
  }

  async registerWidget(
    tappId: string,
    config: RegisteredWidget['config'],
    runtimeGrant: string,
  ): Promise<RegisteredWidget> {
    const instance = this.installedTapps.get(tappId)
    if (!instance) {
      throw new Error(`Tapp ${tappId} is not installed`)
    }

    if (!instance.grantedPermissions.includes('widget:register')) {
      throw new Error('Permission denied: widget:register')
    }

    const fullId = `tapp.${tappId}.${config.id}`

    const existing = this.registeredWidgets.get(fullId)
    if (existing) {
      return existing
    }

    await TappApiService.registerTappWidget(
      tappId,
      config as WidgetRegistration,
      runtimeGrant,
    )

    const widget: RegisteredWidget = {
      id: fullId,
      tappId,
      config,
      instanceCount: 0,
      registeredAt: new Date().toISOString(),
    }

    this.registeredWidgets.set(fullId, widget)

    this.emit('widget:registered', widget)

    return widget
  }

  async unregisterWidget(
    tappId: string,
    widgetId: string,
    runtimeGrant: string,
  ): Promise<void> {
    const fullId = widgetId.startsWith('tapp.')
      ? widgetId
      : `tapp.${tappId}.${widgetId}`
    const widget = this.registeredWidgets.get(fullId)

    if (!widget) {
      throw new Error(`Widget ${fullId} is not registered`)
    }

    if (widget.tappId !== tappId) {
      throw new Error(
        'Permission denied: cannot unregister widget from another Tapp',
      )
    }

    await TappApiService.unregisterTappWidget(tappId, widgetId, runtimeGrant)

    this.registeredWidgets.delete(fullId)

    this.emit('widget:unregistered', { id: fullId })
  }

  getRegisteredWidgets(): RegisteredWidget[] {
    return Iterator.from(this.registeredWidgets.values()).toArray()
  }

  getWidgetsByTapp(tappId: string): RegisteredWidget[] {
    return Iterator.from(this.registeredWidgets.values())
      .filter((w) => w.tappId === tappId)
      .toArray()
  }

  registerPlatform(tappId: string, config: CustomPlatformConfig): void {
    const instance = this.installedTapps.get(tappId)
    if (!instance) {
      throw new Error(`Tapp ${tappId} is not installed`)
    }

    if (!instance.grantedPermissions.includes('platform:register')) {
      throw new Error('Permission denied: platform:register')
    }

    const fullId = `tapp.${tappId}.${config.id}`

    if (this.registeredPlatforms.has(fullId)) {
      throw new Error(`Platform ${fullId} is already registered`)
    }

    this.registeredPlatforms.set(fullId, { ...config, tappId })

    this.emit('platform:registered', { id: fullId, config })
  }

  getRegisteredPlatforms(): Array<CustomPlatformConfig & { tappId: string }> {
    return Iterator.from(this.registeredPlatforms.values()).toArray()
  }

  on(event: RuntimeEvent, callback: RuntimeEventCallback): () => void {
    let listeners = this.eventListeners.get(event)
    if (!listeners) {
      listeners = new Set()
      this.eventListeners.set(event, listeners)
    }
    listeners.add(callback)

    return () => {
      listeners?.delete(callback)
    }
  }

  private emit(event: RuntimeEvent, data: unknown): void {
    const listeners = this.eventListeners.get(event)
    if (listeners) {
      listeners.forEach((callback) => {
        try {
          callback(data)
        } catch (error) {
          console.error(`[TappRuntime] Event listener error:`, error)
        }
      })
    }
  }

  updateTappStatus(tappId: string, status: TappStatus, error?: string): void {
    const instance = this.installedTapps.get(tappId)
    if (instance) {
      instance.status = status
      instance.error = error

      if (status === 'error') {
        this.emit('tapp:error', { id: tappId, error })
      }
    }
  }

  registerBackgroundRequirement(
    tappId: string,
    requirement: BackgroundRequirement,
  ): void {
    let requirements = this.backgroundRequirements.get(tappId)
    if (!requirements) {
      requirements = new Set()
      this.backgroundRequirements.set(tappId, requirements)
    }

    const hadRequirement = this.hasBackgroundRequirements(tappId)
    requirements.add(requirement)
    const hasRequirement = this.hasBackgroundRequirements(tappId)

    if (!hadRequirement && hasRequirement) {
      this.emit('background:changed', {
        tappId,
        requirements: this.getBackgroundRequirements(tappId),
        hasRequirements: true,
      })
    }
  }

  unregisterBackgroundRequirement(
    tappId: string,
    requirement: BackgroundRequirement,
  ): void {
    const requirements = this.backgroundRequirements.get(tappId)
    if (!requirements) return

    const hadRequirement = this.hasBackgroundRequirements(tappId)
    requirements.delete(requirement)
    const hasRequirement = this.hasBackgroundRequirements(tappId)

    if (requirements.size === 0) {
      this.backgroundRequirements.delete(tappId)
    }

    if (hadRequirement && !hasRequirement) {
      this.emit('background:changed', {
        tappId,
        requirements: this.getBackgroundRequirements(tappId),
        hasRequirements: false,
      })
    }
  }

  private dropStoppedTappHostState(tappId: string): void {
    this.clearBackgroundRequirements(tappId)
    getDynamicContentProvider().unregisterTappProvider(tappId)
  }

  clearBackgroundRequirements(tappId: string): void {
    const had = this.hasBackgroundRequirements(tappId)
    this.backgroundRequirements.delete(tappId)
    this.manifestBackgroundRequirements.delete(tappId)

    if (had) {
      this.emit('background:changed', {
        tappId,
        requirements: [],
        hasRequirements: false,
      })
    }
  }

  getBackgroundRequirements(tappId: string): BackgroundRequirement[] {
    return Iterator.from(this.getEffectiveBackgroundRequirements(tappId)).toArray()
  }

  hasBackgroundRequirements(tappId: string): boolean {
    return this.getEffectiveBackgroundRequirements(tappId).size > 0
  }

  hasBackgroundRequirement(
    tappId: string,
    requirement: BackgroundRequirement,
  ): boolean {
    return this.getEffectiveBackgroundRequirements(tappId).has(requirement)
  }

  getTappsWithBackgroundRequirements(): Array<{
    tappId: string
    requirements: BackgroundRequirement[]
  }> {
    const result: Array<{
      tappId: string
      requirements: BackgroundRequirement[]
    }> = []
    const tappIds = new Set(this.backgroundRequirements.keys()).union(
      new Set(this.manifestBackgroundRequirements.keys()),
    )
    for (const tappId of tappIds) {
      const requirements = this.getEffectiveBackgroundRequirements(tappId)
      if (requirements.size > 0) {
        result.push({ tappId, requirements: Iterator.from(requirements).toArray() })
      }
    }
    return result
  }

  shouldRunInBackground(tappId: string): boolean {
    return this.isRunning(tappId) && this.hasBackgroundRequirements(tappId)
  }

  getBackgroundTapps(): TappInstance[] {
    const result: TappInstance[] = []
    for (const tappId of this.runningTapps) {
      if (this.hasBackgroundRequirements(tappId)) {
        const instance = this.installedTapps.get(tappId)
        if (instance) {
          result.push(instance)
        }
      }
    }
    return result
  }
}

export function getTappRuntime(): TappRuntime {
  return TappRuntime.getInstance()
}

export default TappRuntime
