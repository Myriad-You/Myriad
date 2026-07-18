/**
 * Tapp Runtime - 运行时主控制器
 * 管理 Tapp 的生命周期、加载和执行
 *
 * 数据存储策略：
 * - 所有数据存储在后端数据库
 * - 内存缓存用于快速访问
 * - 通过 API 同步状态
 *
 * 性能优化：
 * - 请求去重防止并发重复请求
 * - 智能缓存策略减少 API 调用
 * - 懒加载按需获取 Tapp 详情
 */

import type {
  BackgroundRequirement,
  CustomPlatformConfig,
  RegisteredWidget,
  TappCodeStructure,
  TappInstance,
  TappManifest,
  TappPermission,
  TappStatus,
  WidgetRegistration,
} from '../types'
import * as TappApiService from '../services/TappApiService'
import { getResourceLoader } from './sandbox/resourceLoader'
import { TappPermissionController } from './TappPermission'

/** 运行时事件类型 */
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
  | 'background:changed' // 后台需求变化

type RuntimeEventCallback = (data: unknown) => void

function samePermissions(
  left: TappPermission[],
  right: TappPermission[],
): boolean {
  if (left.length !== right.length) return false
  const expected = new Set(left)
  return right.every((permission) => expected.has(permission))
}

/** 请求去重管理器 */
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

/**
 * Tapp Runtime 类
 */
export class TappRuntime {
  private static instance: TappRuntime | null = null

  /** 已安装的 Tapp（内存缓存） */
  private installedTapps: Map<string, TappInstance> = new Map()

  /** 运行中的 Tapp */
  private runningTapps: Set<string> = new Set()

  /**
   * 当前浏览器会话主动启动的共享管理员 Tapp。
   *
   * 管理员安装记录的数据库 status 表示管理员自己的持久运行态，不能直接
   * 传播成其他用户或访客的运行态。共享 Tapp 对非所有者只在当前会话中运行。
   */
  private sessionRunningTapps: Set<string> = new Set()

  /** 已注册的小组件（内存缓存） */
  private registeredWidgets: Map<string, RegisteredWidget> = new Map()

  /** 已注册的自定义平台 */
  private registeredPlatforms: Map<
    string,
    CustomPlatformConfig & { tappId: string }
  > = new Map()

  /** 后台运行需求（tappId -> 需求集合） */
  private backgroundRequirements: Map<string, Set<BackgroundRequirement>> =
    new Map()

  /** Manifest 声明的后台需求；与运行时 require/release 分开计源。 */
  private manifestBackgroundRequirements: Map<
    string,
    Set<BackgroundRequirement>
  > = new Map()

  /** 事件监听器 */
  private eventListeners: Map<RuntimeEvent, Set<RuntimeEventCallback>> =
    new Map()

  /** 是否已从后端同步 */
  private synced: boolean = false

  /** 同步错误（用于 waitForSync 超时或失败处理） */
  private syncError: Error | null = null

  /** 构造时启动的首次同步；waitForSync 直接等待它，不靠事件猜测完成状态。 */
  private initialSyncPromise: Promise<void>

  /** 请求去重器 */
  private deduplicator = new RequestDeduplicator()

  /** 同一 Tapp 的 start/stop 串行执行，避免多窗口并发反转最终状态。 */
  private lifecycleTransitions = new Map<string, Promise<void>>()
  private uninstallingTapps = new Set<string>()

  /** 缓存 TTL（毫秒） */
  private static readonly CACHE_TTL = {
    tappList: 30 * 1000, // Tapp 列表 30 秒
  }

  /** 上次同步时间 */
  private lastSyncTime: number = 0

  private constructor() {
    // 异步从后端同步状态
    this.initialSyncPromise = this.syncFromBackend().catch((err) => {
      console.error('[TappRuntime] Initial sync failed:', err)
      this.syncError = err instanceof Error ? err : new Error(String(err))
      // 首次请求已经结束，但 waitForSync 会明确抛出 syncError。
      this.synced = true
    })
  }

  /**
   * 获取单例实例
   */
  static getInstance(): TappRuntime {
    if (!TappRuntime.instance) {
      TappRuntime.instance = new TappRuntime()
    }
    return TappRuntime.instance
  }

  /** Drop every subject-scoped cache when the authenticated identity changes. */
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

  /**
   * 从后端同步状态（带请求去重和缓存检查）
   */
  async syncFromBackend(force: boolean = false): Promise<void> {
    // 检查缓存是否仍有效
    if (
      !force &&
      this.synced &&
      Date.now() - this.lastSyncTime < TappRuntime.CACHE_TTL.tappList
    ) {
      return
    }

    // 使用请求去重
    return this.deduplicator.dedupe('sync', async () => {
      try {
        const details = await TappApiService.listTappDetails()
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
          const isRunning =
            this.sessionRunningTapps.has(detail.id) ||
            (persistsLifecycle && detail.status === 'running')

          const instance: TappInstance = {
            id: detail.id,
            manifest: detail.manifest as TappManifest,
            status: isRunning
              ? 'running'
              : detail.status === 'error'
                ? 'error'
                : 'installed',
            installedAt: detail.installed_at,
            lastRunAt: detail.last_run_at,
            grantedPermissions: detail.granted_permissions as TappPermission[],
            userRole,
            isTemporary: detail.is_temporary ?? false,
            isAdminTapp,
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
            // 恢复运行态的 Tapp 也要补注册 manifest 后台需求，
            // 否则重载后 headless core 不会被拉起。
            this.registerManifestBackgroundRequirements(instance)
          }
        }

        // 跨标签页或其他副本卸载后，不保留同 ID 的本地会话运行标记；否则将来
        // 重新安装同名 Tapp 会被误判为已经运行。
        for (const tappId of this.sessionRunningTapps) {
          if (!this.installedTapps.has(tappId)) {
            this.sessionRunningTapps.delete(tappId)
          }
        }

        // 获取后端已注册的小组件
        const backendWidgets = await TappApiService.getAllWidgets()
        this.registeredWidgets.clear()
        for (const widget of backendWidgets) {
          this.registeredWidgets.set(widget.id, widget)
        }

        // 外部停止/卸载或跨标签页状态变化后，不应残留动态或 Manifest 后台来源。
        const backgroundTappIds = new Set([
          ...this.backgroundRequirements.keys(),
          ...this.manifestBackgroundRequirements.keys(),
        ])
        for (const tappId of backgroundTappIds) {
          if (!this.runningTapps.has(tappId)) {
            this.clearBackgroundRequirements(tappId)
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

  /**
   * 等待同步完成
   * 包含超时保护（10秒）
   */
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

  /**
   * 安装 Tapp
   */
  async installTapp(
    manifest: TappManifest,
    code: TappCodeStructure,
    _requestedPermissions?: TappPermission[],
  ): Promise<TappInstance> {
    // 验证 Manifest
    const validation =
      TappPermissionController.validateManifestPermissions(manifest)
    if (!validation.valid) {
      throw new Error(`Invalid manifest: ${validation.errors.join(', ')}`)
    }

    // 检查是否已安装
    if (this.installedTapps.has(manifest.id)) {
      throw new Error(`Tapp ${manifest.id} is already installed`)
    }

    // 通过 API 安装（传递完整的代码结构，包括 CSS 和 HTML 模板）
    const result = await TappApiService.installFromCode(manifest, code)
    const detail = await TappApiService.getTapp(result.id)

    // 将后端返回的 user_role 转换为 UserRole 类型
    const userRole = (detail.user_role as 'guest' | 'user' | 'admin') || 'guest'

    const backendPerms = detail.granted_permissions as TappPermission[]

    const instance: TappInstance = {
      id: detail.id,
      manifest: detail.manifest as TappManifest,
      status: detail.status as TappStatus,
      installedAt: detail.installed_at,
      lastRunAt: detail.last_run_at,
      grantedPermissions: backendPerms,
      userRole,
      isTemporary: detail.is_temporary ?? result.isTemporary ?? false,
      isAdminTapp: detail.is_admin_tapp ?? result.isAdminTapp ?? false,
    }

    // 添加到内存缓存
    this.installedTapps.set(manifest.id, instance)

    // 安装内容可能覆盖同 ID 的资源，确保真实资源加载器不会返回旧页面/widget/core。
    getResourceLoader().clearCache(manifest.id)

    // 后端在安装事务中对账 Manifest Widget；同步一次取得权威注册表。
    await this.syncFromBackend(true)
    const synchronized = this.installedTapps.get(manifest.id) || instance
    this.emit('tapp:installed', { id: manifest.id, instance: synchronized })

    return synchronized
  }

  /**
   * 卸载 Tapp
   * @param tappId Tapp ID
   * @param options 卸载选项，包含 keepData 可选参数
   */
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
      // 即使 start 仍在途也要排队 stop，确保卸载前没有活跃 runtime。
      await this.stopTapp(tappId)

      // 调用 API 卸载
      await TappApiService.uninstallTapp(tappId, options)

      // 删除注册的小组件
      for (const [widgetId, widget] of this.registeredWidgets) {
        if (widget.tappId === tappId) {
          this.registeredWidgets.delete(widgetId)
        }
      }

      // 删除注册的平台
      for (const [platformId, platform] of this.registeredPlatforms) {
        if (platform.tappId === tappId) {
          this.registeredPlatforms.delete(platformId)
        }
      }

      // 清除该 Tapp 的全部模式资源缓存
      getResourceLoader().clearCache(tappId)

      // 从列表中移除
      this.installedTapps.delete(tappId)
      this.sessionRunningTapps.delete(tappId)

      // 触发事件
      this.emit('tapp:uninstalled', { id: tappId })
    } finally {
      this.uninstallingTapps.delete(tappId)
    }
  }

  /**
   * 启动 Tapp
   */
  async startTapp(tappId: string): Promise<void> {
    return this.enqueueLifecycleTransition(tappId, async () => {
      const instance = this.installedTapps.get(tappId)
      if (!instance) {
        throw new Error(`Tapp ${tappId} is not installed`)
      }
      if (this.uninstallingTapps.has(tappId)) {
        throw new Error(`Tapp ${tappId} is being uninstalled`)
      }

      if (this.runningTapps.has(tappId)) return

      const persistsLifecycle = this.persistsLifecycle(instance)
      if (persistsLifecycle) {
        await TappApiService.startTapp(tappId)
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
    const previous = this.lifecycleTransitions.get(tappId) || Promise.resolve()
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

  /** 是否由当前查看者拥有并可把生命周期状态持久化到安装记录。 */
  private persistsLifecycle(instance: TappInstance): boolean {
    return (
      (instance.userRole === 'admin' && instance.isAdminTapp === true) ||
      (instance.userRole === 'user' && instance.isTemporary === true)
    )
  }

  /**
   * 注册 manifest 声明的后台需求。
   * 声明需求的 Tapp 由此在运行期被 getBackgroundTapps 收入，
   * 并由 TappBackgroundRunner 拉起 headless core。
   */
  private registerManifestBackgroundRequirements(instance: TappInstance): void {
    this.setManifestBackgroundRequirements(
      instance.id,
      instance.manifest.backgroundRequirements || [],
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
    return new Set([
      ...(this.manifestBackgroundRequirements.get(tappId) || []),
      ...(this.backgroundRequirements.get(tappId) || []),
    ])
  }

  /**
   * 停止 Tapp
   */
  async stopTapp(tappId: string): Promise<void> {
    return this.enqueueLifecycleTransition(tappId, async () => {
      const instance = this.installedTapps.get(tappId)
      if (!instance) {
        throw new Error(`Tapp ${tappId} is not installed`)
      }

      if (!this.runningTapps.has(tappId)) return

      if (this.persistsLifecycle(instance)) {
        await TappApiService.stopTapp(tappId)
      } else {
        this.sessionRunningTapps.delete(tappId)
      }
      this.clearBackgroundRequirements(tappId)
      instance.status = 'installed'
      this.runningTapps.delete(tappId)
      this.emit('tapp:stopped', { id: tappId, instance })
    })
  }

  /**
   * 获取 Tapp 实例
   */
  getTapp(tappId: string): TappInstance | undefined {
    return this.installedTapps.get(tappId)
  }

  /**
   * 获取所有已安装的 Tapp
   */
  getAllTapps(): TappInstance[] {
    return Array.from(this.installedTapps.values())
  }

  /**
   * 清除真实资源加载器中的 core/page/widget 缓存。
   */
  clearCodeCache(tappId?: string): void {
    getResourceLoader().clearCache(tappId)
  }

  /** 使资源缓存失效、重新同步清单，并通知现有运行实例重建。 */
  async refreshTapp(tappId: string): Promise<void> {
    this.clearCodeCache(tappId)
    await this.syncFromBackend(true)
    const instance = this.installedTapps.get(tappId)
    if (instance) {
      this.emit('tapp:updated', { id: tappId, instance })
    }
  }

  /** Refresh effective grants and rebuild only runtimes whose permissions changed. */
  async refreshPermissionGrants(): Promise<void> {
    await this.syncFromBackend(true)
  }

  /**
   * 检查 Tapp 是否在运行
   */
  isRunning(tappId: string): boolean {
    return this.runningTapps.has(tappId)
  }

  /**
   * 注册小组件
   */
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

    // 检查是否已存在（避免重复注册）
    const existing = this.registeredWidgets.get(fullId)
    if (existing) {
      return existing
    }

    // 同步到后端
    await TappApiService.registerTappWidget(
      tappId,
      config as WidgetRegistration,
      runtimeGrant,
    )

    // 更新内存缓存
    const widget: RegisteredWidget = {
      id: fullId,
      tappId,
      config,
      instanceCount: 0,
      registeredAt: new Date().toISOString(),
    }

    this.registeredWidgets.set(fullId, widget)

    // 触发事件
    this.emit('widget:registered', widget)

    return widget
  }

  /**
   * 注销小组件
   */
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

    // 同步到后端
    await TappApiService.unregisterTappWidget(tappId, widgetId, runtimeGrant)

    // 从内存缓存移除
    this.registeredWidgets.delete(fullId)

    // 触发事件
    this.emit('widget:unregistered', { id: fullId })
  }

  /**
   * 获取所有已注册的小组件
   */
  getRegisteredWidgets(): RegisteredWidget[] {
    return Array.from(this.registeredWidgets.values())
  }

  /**
   * 获取指定 Tapp 的小组件
   */
  getWidgetsByTapp(tappId: string): RegisteredWidget[] {
    return Array.from(this.registeredWidgets.values()).filter(
      (w) => w.tappId === tappId,
    )
  }

  /**
   * 注册自定义平台
   */
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

    // 触发事件
    this.emit('platform:registered', { id: fullId, config })
  }

  /**
   * 获取所有已注册的自定义平台
   */
  getRegisteredPlatforms(): Array<CustomPlatformConfig & { tappId: string }> {
    return Array.from(this.registeredPlatforms.values())
  }

  /**
   * 监听事件
   */
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

  /**
   * 触发事件
   */
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

  /**
   * 更新 Tapp 状态
   */
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

  // ============ 后台运行需求管理 ============

  /**
   * 注册后台运行需求
   * Tapp 在需要后台运行时调用此方法声明需求
   */
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

  /**
   * 注销后台运行需求
   */
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

  /**
   * 清除 Tapp 的所有后台需求
   */
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

  /**
   * 获取 Tapp 的后台运行需求
   */
  getBackgroundRequirements(tappId: string): BackgroundRequirement[] {
    return Array.from(this.getEffectiveBackgroundRequirements(tappId))
  }

  /**
   * 检查 Tapp 是否有后台运行需求
   */
  hasBackgroundRequirements(tappId: string): boolean {
    return this.getEffectiveBackgroundRequirements(tappId).size > 0
  }

  /**
   * 检查 Tapp 是否有特定的后台运行需求
   */
  hasBackgroundRequirement(
    tappId: string,
    requirement: BackgroundRequirement,
  ): boolean {
    return this.getEffectiveBackgroundRequirements(tappId).has(requirement)
  }

  /**
   * 获取所有需要后台运行的 Tapp（有任何后台需求的）
   */
  getTappsWithBackgroundRequirements(): Array<{
    tappId: string
    requirements: BackgroundRequirement[]
  }> {
    const result: Array<{
      tappId: string
      requirements: BackgroundRequirement[]
    }> = []
    const tappIds = new Set([
      ...this.backgroundRequirements.keys(),
      ...this.manifestBackgroundRequirements.keys(),
    ])
    for (const tappId of tappIds) {
      const requirements = this.getEffectiveBackgroundRequirements(tappId)
      if (requirements.size > 0) {
        result.push({ tappId, requirements: Array.from(requirements) })
      }
    }
    return result
  }

  /**
   * 检查 Tapp 是否应该在后台运行
   * 条件：Tapp 正在运行 + 有后台需求
   */
  shouldRunInBackground(tappId: string): boolean {
    return this.isRunning(tappId) && this.hasBackgroundRequirements(tappId)
  }

  /**
   * 获取所有应该在后台运行的 Tapp
   */
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

/**
 * 获取 Runtime 实例
 */
export function getTappRuntime(): TappRuntime {
  return TappRuntime.getInstance()
}

export default TappRuntime
