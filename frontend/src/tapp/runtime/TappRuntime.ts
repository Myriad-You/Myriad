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

import type { TappCodeStructure } from '../examples/tapps/types'
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
import * as TappApiService from '../services/TappApiService'
import { TappPermissionController } from './TappPermission'

/** 运行时事件类型 */
type RuntimeEvent
  = | 'tapp:installed'
    | 'tapp:uninstalled'
    | 'tapp:started'
    | 'tapp:stopped'
    | 'tapp:error'
    | 'widget:registered'
    | 'widget:unregistered'
    | 'platform:registered'
    | 'sync:complete'
    | 'background:changed' // 后台需求变化

type RuntimeEventCallback = (data: unknown) => void

/** 缓存项接口 */
interface CacheEntry<T> {
  data: T
  timestamp: number
  ttl: number
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

  /** 已注册的小组件（内存缓存） */
  private registeredWidgets: Map<string, RegisteredWidget> = new Map()

  /** 已注册的自定义平台 */
  private registeredPlatforms: Map<string, CustomPlatformConfig & { tappId: string }> = new Map()

  /** 代码缓存（分离结构） */
  private codeCache: Map<string, CacheEntry<TappCodeStructure>> = new Map()

  /** 后台运行需求（tappId -> 需求集合） */
  private backgroundRequirements: Map<string, Set<BackgroundRequirement>> = new Map()

  /** 事件监听器 */
  private eventListeners: Map<RuntimeEvent, Set<RuntimeEventCallback>> = new Map()

  /** 是否已从后端同步 */
  private synced: boolean = false

  /** 是否正在同步 */
  private syncing: boolean = false

  /** 同步错误（用于 waitForSync 超时或失败处理） */
  private syncError: Error | null = null

  /** 请求去重器 */
  private deduplicator = new RequestDeduplicator()

  /** 缓存 TTL（毫秒） */
  private static readonly CACHE_TTL = {
    code: 5 * 60 * 1000, // 代码缓存 5 分钟
    tappList: 30 * 1000, // Tapp 列表 30 秒
    widgets: 60 * 1000, // Widget 列表 60 秒
  }

  /** 上次同步时间 */
  private lastSyncTime: number = 0

  private constructor() {
    // 异步从后端同步状态
    this.syncFromBackend().catch((err) => {
      console.error('[TappRuntime] Initial sync failed:', err)
      this.syncError = err
      // 即使失败也标记为已同步，避免无限等待
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

  /**
   * 从后端同步状态（带请求去重和缓存检查）
   */
  async syncFromBackend(force: boolean = false): Promise<void> {
    // 检查缓存是否仍有效
    if (!force && this.synced && Date.now() - this.lastSyncTime < TappRuntime.CACHE_TTL.tappList) {
      return
    }

    // 使用请求去重
    return this.deduplicator.dedupe('sync', async () => {
      if (this.syncing)
        return
      this.syncing = true

      try {
        // 获取 Tapp 列表
        const tapps = await TappApiService.listTapps()
        this.installedTapps.clear()
        this.runningTapps.clear()

        // 并行获取所有 Tapp 详情，限制并发数为 5
        const CONCURRENCY_LIMIT = 5
        const chunks: typeof tapps[] = []
        for (let i = 0; i < tapps.length; i += CONCURRENCY_LIMIT) {
          chunks.push(tapps.slice(i, i + CONCURRENCY_LIMIT))
        }

        for (const chunk of chunks) {
          const detailPromises = chunk.map(async (tapp) => {
            try {
              const detail = await TappApiService.getTapp(tapp.id)
              // 将后端返回的 user_role 转换为 UserRole 类型
              const userRole = (detail.user_role as 'guest' | 'user' | 'admin') || 'guest'
              const instance: TappInstance = {
                id: detail.id,
                manifest: detail.manifest as TappManifest,
                status: detail.status as TappStatus,
                installedAt: detail.installed_at,
                lastRunAt: detail.last_run_at,
                grantedPermissions: detail.granted_permissions as TappPermission[],
                userRole,
                isTemporary: detail.is_temporary ?? tapp.is_temporary ?? false,
                isAdminTapp: detail.is_admin_tapp ?? tapp.is_admin_tapp ?? false,
              }
              return { success: true, tappId: tapp.id, instance, isRunning: detail.status === 'running' }
            }
            catch (error) {
              console.warn(`[TappRuntime] Failed to get details for ${tapp.id}:`, error)
              return { success: false, tappId: tapp.id }
            }
          })

          const results = await Promise.all(detailPromises)
          for (const result of results) {
            if (result.success && result.instance) {
              this.installedTapps.set(result.tappId, result.instance)
              if (result.isRunning) {
                this.runningTapps.add(result.tappId)
              }
            }
          }
        }

        // 获取后端已注册的小组件
        const backendWidgets = await TappApiService.getAllWidgets()
        this.registeredWidgets.clear()
        for (const widget of backendWidgets) {
          this.registeredWidgets.set(widget.id, widget)
        }

        // 从 manifest 补充注册缺失的 widgets（批量处理优化）
        const widgetsToSync: Array<{ tappId: string, widget: RegisteredWidget }> = []

        for (const [tappId, instance] of this.installedTapps) {
          const { manifest } = instance
          if (!manifest.widgets || manifest.widgets.length === 0)
            continue

          for (const widgetDef of manifest.widgets) {
            const fullId = `tapp.${tappId}.${widgetDef.id}`
            if (!this.registeredWidgets.has(fullId)) {
              const widget: RegisteredWidget = {
                id: fullId,
                tappId,
                config: {
                  id: widgetDef.id,
                  name: widgetDef.name,
                  description: widgetDef.description || '',
                  icon: widgetDef.icon || manifest.icon || '📦',
                  sizes: widgetDef.sizes,
                  defaultSize: widgetDef.defaultSize,
                  category: widgetDef.category || 'utility',
                  configSchema: widgetDef.configSchema,
                  refreshInterval: widgetDef.refreshInterval,
                },
                instanceCount: 0,
                registeredAt: new Date().toISOString(),
              }
              this.registeredWidgets.set(fullId, widget)
              widgetsToSync.push({ tappId, widget })
            }
          }
        }

        // 批量同步 widgets 到后端（限制并发）
        const WIDGET_SYNC_CONCURRENCY = 3
        for (let i = 0; i < widgetsToSync.length; i += WIDGET_SYNC_CONCURRENCY) {
          const batch = widgetsToSync.slice(i, i + WIDGET_SYNC_CONCURRENCY)
          await Promise.allSettled(
            batch.map(({ tappId, widget }) =>
              TappApiService.registerTappWidget(tappId, widget.config as WidgetRegistration)
                .catch(() => { /* 静默失败，widget 可在下次同步时重试 */ }),
            ),
          )
        }

        this.synced = true
        this.lastSyncTime = Date.now()
        this.emit('sync:complete', { tapps: tapps.length, widgets: this.registeredWidgets.size })
      }
      catch (error) {
        console.error('[TappRuntime] Failed to sync from backend:', error)
        throw error
      }
      finally {
        this.syncing = false
      }
    })
  }

  /**
   * 等待同步完成
   * 包含超时保护（10秒）
   */
  async waitForSync(): Promise<void> {
    if (this.synced)
      return

    return new Promise((resolve, reject) => {
      // 超时保护：10秒后自动解决
      const timeout = setTimeout(() => {
        console.warn('[TappRuntime] waitForSync timed out after 10s')
        this.synced = true
        resolve()
      }, 10000)

      const unsubscribe = this.on('sync:complete', () => {
        clearTimeout(timeout)
        unsubscribe()
        resolve()
      })

      // 检查是否在等待期间已完成同步
      if (this.synced) {
        clearTimeout(timeout)
        unsubscribe()
        if (this.syncError) {
          reject(this.syncError)
        }
        else {
          resolve()
        }
      }
    })
  }

  /**
   * 安装 Tapp
   */
  async installTapp(
    manifest: TappManifest,
    code: TappCodeStructure,
    requestedPermissions?: TappPermission[],
  ): Promise<TappInstance> {
    // 验证 Manifest
    const validation = TappPermissionController.validateManifestPermissions(manifest)
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
    const instance: TappInstance = {
      id: detail.id,
      manifest: detail.manifest as TappManifest,
      status: detail.status as TappStatus,
      installedAt: detail.installed_at,
      lastRunAt: detail.last_run_at,
      grantedPermissions: detail.granted_permissions as TappPermission[],
      userRole,
      isTemporary: detail.is_temporary ?? result.is_temporary ?? false,
      isAdminTapp: detail.is_admin_tapp ?? result.is_admin_tapp ?? false,
    }

    // 添加到内存缓存（保留完整的分离结构，带 TTL）
    this.installedTapps.set(manifest.id, instance)
    this.codeCache.set(manifest.id, {
      data: code,
      timestamp: Date.now(),
      ttl: TappRuntime.CACHE_TTL.code,
    })

    // 从 manifest 预注册 widgets（无需运行 Tapp 代码）
    await this.registerWidgetsFromManifest(instance)

    // 触发事件
    this.emit('tapp:installed', instance)

    return instance
  }

  /**
   * 从 manifest 预注册 widgets
   */
  private async registerWidgetsFromManifest(instance: TappInstance): Promise<void> {
    const { manifest } = instance
    if (!manifest.widgets || manifest.widgets.length === 0) {
      return
    }

    let widgetsRegistered = 0

    for (const widgetDef of manifest.widgets) {
      const fullId = `tapp.${manifest.id}.${widgetDef.id}`

      // 检查是否已注册
      if (this.registeredWidgets.has(fullId)) {
        widgetsRegistered++
        continue
      }

      const widget: RegisteredWidget = {
        id: fullId,
        tappId: manifest.id,
        config: {
          id: widgetDef.id,
          name: widgetDef.name,
          description: widgetDef.description || '',
          icon: widgetDef.icon || manifest.icon || '📦',
          sizes: widgetDef.sizes,
          defaultSize: widgetDef.defaultSize,
          category: widgetDef.category || 'utility',
          configSchema: widgetDef.configSchema,
          refreshInterval: widgetDef.refreshInterval,
        },
        instanceCount: 0,
        registeredAt: new Date().toISOString(),
      }

      this.registeredWidgets.set(fullId, widget)
      widgetsRegistered++

      // 同步到后端
      try {
        await TappApiService.registerTappWidget(manifest.id, widget.config as WidgetRegistration)
      }
      catch (error) {
        console.error(`[TappRuntime] Failed to sync widget ${fullId} to backend:`, error)
      }

      this.emit('widget:registered', widget)
    }

    // 如果有 widget 被注册，自动声明 widget 后台需求
    if (widgetsRegistered > 0) {
      this.registerBackgroundRequirement(manifest.id, 'widget')
    }
  }

  /**
   * 卸载 Tapp
   * @param tappId Tapp ID
   * @param options 卸载选项，包含 keepData 可选参数
   */
  async uninstallTapp(tappId: string, options?: { keepData?: boolean }): Promise<void> {
    const instance = this.installedTapps.get(tappId)
    if (!instance) {
      throw new Error(`Tapp ${tappId} is not installed`)
    }

    // 如果正在运行，先停止
    if (this.runningTapps.has(tappId)) {
      await this.stopTapp(tappId)
    }

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

    // 清除代码缓存
    this.codeCache.delete(tappId)

    // 从列表中移除
    this.installedTapps.delete(tappId)

    // 触发事件
    this.emit('tapp:uninstalled', { id: tappId })
  }

  /**
   * 启动 Tapp
   */
  async startTapp(tappId: string): Promise<void> {
    const instance = this.installedTapps.get(tappId)
    if (!instance) {
      throw new Error(`Tapp ${tappId} is not installed`)
    }

    if (this.runningTapps.has(tappId)) {
      return
    }

    // 调用 API 启动
    await TappApiService.startTapp(tappId)

    // 更新状态
    instance.status = 'running'
    instance.lastRunAt = new Date().toISOString()
    this.runningTapps.add(tappId)

    // 触发事件
    this.emit('tapp:started', instance)
  }

  /**
   * 停止 Tapp
   */
  async stopTapp(tappId: string): Promise<void> {
    const instance = this.installedTapps.get(tappId)
    if (!instance) {
      throw new Error(`Tapp ${tappId} is not installed`)
    }

    if (!this.runningTapps.has(tappId)) {
      return
    }

    // 调用 API 停止
    await TappApiService.stopTapp(tappId)

    // 清除后台需求（停止时重置）
    this.clearBackgroundRequirements(tappId)

    // 更新状态
    instance.status = 'installed'
    this.runningTapps.delete(tappId)

    // 触发事件
    this.emit('tapp:stopped', { id: tappId })
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
   * 获取 Tapp 代码（从内存缓存，带 TTL 验证）
   */
  getTappCode(tappId: string): TappCodeStructure | null {
    const cached = this.codeCache.get(tappId)
    if (cached && Date.now() - cached.timestamp < cached.ttl) {
      return cached.data
    }
    // 缓存过期，删除
    if (cached) {
      this.codeCache.delete(tappId)
    }
    return null
  }

  /**
   * 获取 Tapp 代码（从 API，带请求去重）
   * 支持混合渲染模式：自动获取 HTML 模板和 CSS
   */
  async fetchTappCode(tappId: string, widgetSize?: string): Promise<TappCodeStructure> {
    // 缓存 key 包含 widgetSize，因为不同尺寸可能有不同的 HTML 模板
    const cacheKey = widgetSize ? `${tappId}:${widgetSize}` : tappId

    // 优先从缓存获取（带 TTL 验证）
    const cached = this.codeCache.get(cacheKey)
    if (cached && Date.now() - cached.timestamp < cached.ttl) {
      return cached.data
    }

    // 使用请求去重
    return this.deduplicator.dedupe(`code:${cacheKey}`, async () => {
      try {
        // 尝试使用新的资源 API（获取完整结构）
        const resources = await TappApiService.getTappResources(tappId)

        const codeStructure: TappCodeStructure = {
          core: resources.code,
          styles: resources.styles,
          pageHtml: resources.pageTemplate,
          widgetCSS: resources.widgetCSS,
          pageCSS: resources.pageCSS,
        }

        // 🔍 调试：构建的代码结构
        console.log('[TappRuntime.fetchTappCode] 构建代码结构:', {
          tappId,
          widgetSize,
          hasCore: !!codeStructure.core,
          hasStyles: !!codeStructure.styles,
          stylesLength: codeStructure.styles?.length || 0,
          hasWidgetCSS: !!codeStructure.widgetCSS,
          widgetCSSLength: codeStructure.widgetCSS?.length || 0,
          hasPageCSS: !!codeStructure.pageCSS,
          pageCSSLength: codeStructure.pageCSS?.length || 0,
        })

        // 根据 widgetSize 选择对应的 HTML 模板
        if (resources.widgetTemplates && widgetSize) {
          codeStructure.widgetHtml = resources.widgetTemplates[widgetSize]
        }
        else if (resources.widgetTemplates) {
          // 如果没有指定尺寸，取第一个模板
          const firstKey = Object.keys(resources.widgetTemplates)[0]
          if (firstKey) {
            codeStructure.widgetHtml = resources.widgetTemplates[firstKey]
          }
        }

        // 存入缓存（带 TTL）
        this.codeCache.set(cacheKey, {
          data: codeStructure,
          timestamp: Date.now(),
          ttl: TappRuntime.CACHE_TTL.code,
        })

        return codeStructure
      }
      catch {
        // 回退到旧 API（只获取代码）
        const codeString = await TappApiService.getTappCode(tappId)
        const codeStructure: TappCodeStructure = {
          core: codeString,
        }

        this.codeCache.set(cacheKey, {
          data: codeStructure,
          timestamp: Date.now(),
          ttl: TappRuntime.CACHE_TTL.code,
        })

        return codeStructure
      }
    })
  }

  /**
   * 设置 Tapp 代码（用于预装 Tapp）
   */
  setTappCode(tappId: string, code: TappCodeStructure): void {
    this.codeCache.set(tappId, {
      data: code,
      timestamp: Date.now(),
      ttl: TappRuntime.CACHE_TTL.code,
    })
  }

  /**
   * 清除代码缓存
   * 如果提供 tappId，会删除该 Tapp 所有尺寸的缓存
   */
  clearCodeCache(tappId?: string): void {
    if (tappId) {
      // 删除所有以该 tappId 开头的缓存（包括带尺寸后缀的）
      for (const key of this.codeCache.keys()) {
        if (key === tappId || key.startsWith(`${tappId}:`)) {
          this.codeCache.delete(key)
        }
      }
    }
    else {
      this.codeCache.clear()
    }
  }

  /**
   * 预加载指定 Tapp 的代码（后台加载，不阻塞）
   */
  prefetchTappCode(tappId: string): void {
    this.fetchTappCode(tappId).catch(() => {
      // 静默失败，预加载不影响主流程
    })
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
  async registerWidget(tappId: string, config: RegisteredWidget['config']): Promise<RegisteredWidget> {
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
    await TappApiService.registerTappWidget(tappId, config as WidgetRegistration)

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
  async unregisterWidget(tappId: string, widgetId: string): Promise<void> {
    const fullId = widgetId.startsWith('tapp.') ? widgetId : `tapp.${tappId}.${widgetId}`
    const widget = this.registeredWidgets.get(fullId)

    if (!widget) {
      throw new Error(`Widget ${fullId} is not registered`)
    }

    if (widget.tappId !== tappId) {
      throw new Error('Permission denied: cannot unregister widget from another Tapp')
    }

    // 同步到后端
    await TappApiService.unregisterTappWidget(tappId, widgetId)

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
    return Array.from(this.registeredWidgets.values()).filter(w => w.tappId === tappId)
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
        }
        catch (error) {
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
  registerBackgroundRequirement(tappId: string, requirement: BackgroundRequirement): void {
    let requirements = this.backgroundRequirements.get(tappId)
    if (!requirements) {
      requirements = new Set()
      this.backgroundRequirements.set(tappId, requirements)
    }

    const hadRequirements = requirements.size > 0
    requirements.add(requirement)

    // 如果从无需求变为有需求，触发事件
    if (!hadRequirements && requirements.size > 0) {
      this.emit('background:changed', { tappId, requirements: Array.from(requirements), hasRequirements: true })
    }
  }

  /**
   * 注销后台运行需求
   */
  unregisterBackgroundRequirement(tappId: string, requirement: BackgroundRequirement): void {
    const requirements = this.backgroundRequirements.get(tappId)
    if (!requirements)
      return

    requirements.delete(requirement)

    // 如果没有需求了，触发事件
    if (requirements.size === 0) {
      this.backgroundRequirements.delete(tappId)
      this.emit('background:changed', { tappId, requirements: [], hasRequirements: false })
    }
  }

  /**
   * 清除 Tapp 的所有后台需求
   */
  clearBackgroundRequirements(tappId: string): void {
    const had = this.backgroundRequirements.has(tappId)
    this.backgroundRequirements.delete(tappId)

    if (had) {
      this.emit('background:changed', { tappId, requirements: [], hasRequirements: false })
    }
  }

  /**
   * 获取 Tapp 的后台运行需求
   */
  getBackgroundRequirements(tappId: string): BackgroundRequirement[] {
    const requirements = this.backgroundRequirements.get(tappId)
    return requirements ? Array.from(requirements) : []
  }

  /**
   * 检查 Tapp 是否有后台运行需求
   */
  hasBackgroundRequirements(tappId: string): boolean {
    const requirements = this.backgroundRequirements.get(tappId)
    return requirements ? requirements.size > 0 : false
  }

  /**
   * 检查 Tapp 是否有特定的后台运行需求
   */
  hasBackgroundRequirement(tappId: string, requirement: BackgroundRequirement): boolean {
    const requirements = this.backgroundRequirements.get(tappId)
    return requirements ? requirements.has(requirement) : false
  }

  /**
   * 获取所有需要后台运行的 Tapp（有任何后台需求的）
   */
  getTappsWithBackgroundRequirements(): Array<{ tappId: string, requirements: BackgroundRequirement[] }> {
    const result: Array<{ tappId: string, requirements: BackgroundRequirement[] }> = []
    for (const [tappId, requirements] of this.backgroundRequirements) {
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
