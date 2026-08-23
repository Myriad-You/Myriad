/**
 * Tapp 配额管理服务
 * 管理 Tapp 的 API 调用配额和使用统计
 *
 * 安全特性：
 * - 滑动窗口速率限制，防止突发请求
 * - 配额数据使用内存缓存，页面刷新后会重置
 * - 只用于快速失败和减少误操作；安全限制必须由后端实现
 *
 * 性能特性：
 * - 高效的滑动窗口算法
 * - 自动清理过期记录
 */

import { currentCopy } from '../../i18n/localeCopy'

/** 默认配额配置 */
const DEFAULT_QUOTA = {
  platform: {
    readPerMinute: 60, // 每分钟读取次数
    writePerMinute: 10, // 每分钟写入次数
  },
  apiExecutePerMinute: 30,
  /** 所有 Bridge API 的全局软上限（不含 storage / UI 热路径） */
  bridgeActionsPerMinute: 180,
  /** lifecycle.ready 等控制面信号 */
  lifecyclePerMinute: 30,
  /**
   * storage.* 是 Widget/Page 热路径，单独宽限且不计入全局 bridge 桶，
   * 避免正常读写把其它 API 顶到 180/min 上限。
   */
  storagePerMinute: 600,
  /**
   * ui.getTheme / locale / animation 等基础读路径同样从全局桶剥离。
   */
  uiHotPerMinute: 360,
}

type QuotaConfig = typeof DEFAULT_QUOTA

/** 滑动窗口速率限制器 */
class SlidingWindowRateLimiter {
  private timestamps: number[] = []
  private readonly windowMs: number
  private readonly maxRequests: number

  constructor(windowMs: number, maxRequests: number) {
    this.windowMs = windowMs
    this.maxRequests = maxRequests
  }

  /**
   * 检查是否允许请求
   * @returns 是否允许，以及剩余配额
   */
  check(): { allowed: boolean; remaining: number; retryAfter?: number } {
    const now = Date.now()
    this.cleanup(now)

    if (this.timestamps.length >= this.maxRequests) {
      // 计算需要等待的时间
      const oldestTimestamp = this.timestamps[0]
      const retryAfter = Math.max(0, oldestTimestamp + this.windowMs - now)
      return {
        allowed: false,
        remaining: 0,
        retryAfter,
      }
    }

    return {
      allowed: true,
      remaining: this.maxRequests - this.timestamps.length - 1,
    }
  }

  /**
   * 记录一次请求
   */
  record(): void {
    this.timestamps.push(Date.now())
  }

  /**
   * 清理过期的时间戳
   */
  private cleanup(now: number): void {
    const cutoff = now - this.windowMs
    // 使用二分查找优化清理
    let left = 0
    let right = this.timestamps.length
    while (left < right) {
      const mid = Math.floor((left + right) / 2)
      if (this.timestamps[mid] <= cutoff) {
        left = mid + 1
      } else {
        right = mid
      }
    }
    if (left > 0) {
      this.timestamps = this.timestamps.slice(left)
    }
  }
}

/** 使用记录 */
interface UsageRecord {
  lastUsedAt: number
  rateLimiter: SlidingWindowRateLimiter
}

/** 配额管理器 */
class TappQuotaManager {
  private readonly quotaConfig: QuotaConfig
  private usageByTapp: Map<string, Record<string, UsageRecord>>

  constructor() {
    this.quotaConfig = { ...DEFAULT_QUOTA }
    this.usageByTapp = new Map()

    // 启动自动清理（每 5 分钟清理一次过期数据）
    // unref so Node unit tests can exit (browser ignores unref).
    const timer = setInterval(
      () => this.cleanupExpiredRecords(),
      5 * 60 * 1000,
    )
    if (typeof timer === 'object' && timer && 'unref' in timer) {
      ;(timer as NodeJS.Timeout).unref()
    }
  }

  /**
   * 清理过期记录
   */
  private cleanupExpiredRecords(): void {
    const now = Date.now()
    const ONE_DAY = 24 * 60 * 60 * 1000

    for (const [tappId, usage] of this.usageByTapp) {
      for (const [type, record] of Object.entries(usage)) {
        // 清理超过一天没有活动的记录
        if (now - record.lastUsedAt > ONE_DAY) {
          delete usage[type]
        }
      }
      // 如果 Tapp 没有任何使用记录，删除它
      if (Object.keys(usage).length === 0) {
        this.usageByTapp.delete(tappId)
      }
    }
  }

  /** 获取或创建 Tapp 的使用记录 */
  private getUsage(tappId: string): Record<string, UsageRecord> {
    if (!this.usageByTapp.has(tappId)) {
      this.usageByTapp.set(tappId, {})
    }
    return this.usageByTapp.get(tappId)!
  }

  /** Bridge 使用具体 action 名；配额按能力族聚合。 */
  private normalizeType(type: string): string {
    if (
      [
        'platform.listEnabled',
        'platform.getData',
        'platform.getStats',
        'platform.getDistribution',
      ].includes(type)
    ) {
      return 'platform.read'
    }
    if (
      [
        'platform.addItem',
        'platform.addItems',
        'platform.registerPlatform',
      ].includes(type)
    ) {
      return 'platform.write'
    }
    if (type === 'api.execute' || type === 'api.list') {
      return 'api.execute'
    }
    if (type.startsWith('lifecycle.')) {
      return 'lifecycle'
    }
    if (type.startsWith('storage.')) {
      return 'storage'
    }
    // Basic UI/animation/user reads that fire on every paint/theme tick
    if (
      type === 'ui.getTheme' ||
      type === 'ui.getPrimaryColor' ||
      type === 'ui.getLocale' ||
      type === 'animation.getLevel' ||
      type === 'animation.shouldAnimate' ||
      type === 'animation.getConfig' ||
      type === 'animation.getStaggerDelay' ||
      type === 'user.getRole' ||
      type === 'user.isAdmin' ||
      type === 'user.isGuest' ||
      type === 'user.isLoggedIn'
    ) {
      return 'ui.hot'
    }
    // 其余 bridge action 走全局桶
    return 'bridge.action'
  }

  /** 获取或创建特定类型的使用记录（带滑动窗口限制器） */
  private getTypeUsage(tappId: string, type: string): UsageRecord {
    const usage = this.getUsage(tappId)
    if (!usage[type]) {
      let maxRequests: number

      if (type === 'platform.read') {
        maxRequests = this.quotaConfig.platform.readPerMinute
      } else if (type === 'platform.write') {
        maxRequests = this.quotaConfig.platform.writePerMinute
      } else if (type === 'api.execute') {
        maxRequests = this.quotaConfig.apiExecutePerMinute
      } else if (type === 'lifecycle') {
        maxRequests = this.quotaConfig.lifecyclePerMinute
      } else if (type === 'storage') {
        maxRequests = this.quotaConfig.storagePerMinute
      } else if (type === 'ui.hot') {
        maxRequests = this.quotaConfig.uiHotPerMinute
      } else {
        maxRequests = this.quotaConfig.bridgeActionsPerMinute
      }

      usage[type] = {
        lastUsedAt: Date.now(),
        rateLimiter: new SlidingWindowRateLimiter(60 * 1000, maxRequests),
      }
    }
    return usage[type]
  }

  private evaluateLimiter(
    tappId: string,
    bucket: string,
  ): {
    allowed: boolean
    remaining: number
    reason?: string
    retryAfter?: number
  } {
    const record = this.getTypeUsage(tappId, bucket)
    const rateCheck = record.rateLimiter.check()
    if (!rateCheck.allowed) {
      return {
        allowed: false,
        remaining: 0,
        reason: currentCopy().errors.rateLimitedRetry.replace(
          '{sec}',
          String(Math.ceil((rateCheck.retryAfter || 0) / 1000)),
        ),
        retryAfter: rateCheck.retryAfter,
      }
    }
    return { allowed: true, remaining: rateCheck.remaining }
  }

  /**
   * Buckets that use only their own limiter (no secondary debit of the shared
   * `bridge.action` budget). Includes:
   * - `bridge.action` itself (already the shared budget — avoid double-check)
   * - hot paths (storage / basic UI reads / lifecycle) with dedicated caps so
   *   they cannot starve unrelated bridge actions
   */
  private isDedicatedBudgetBucket(bucket: string): boolean {
    return (
      bucket === 'bridge.action' ||
      bucket === 'storage' ||
      bucket === 'ui.hot' ||
      bucket === 'lifecycle'
    )
  }

  /**
   * 检查配额是否允许操作（增强版：包含滑动窗口检查）
   */
  checkQuota(
    tappId: string,
    type: string,
  ): {
    allowed: boolean
    remaining: number
    reason?: string
    retryAfter?: number
  } {
    const bucket = this.normalizeType(type)

    // 先过能力族桶；platform/api 等仍同时计入全局 bridge 桶
    const primary = this.evaluateLimiter(tappId, bucket)
    if (!primary.allowed) return primary

    if (!this.isDedicatedBudgetBucket(bucket)) {
      const global = this.evaluateLimiter(tappId, 'bridge.action')
      if (!global.allowed) return global
      return {
        allowed: true,
        remaining: Math.min(primary.remaining, global.remaining),
      }
    }

    return primary
  }

  /**
   * 记录使用（同时更新滑动窗口）
   */
  recordUsage(tappId: string, type: string): void {
    const bucket = this.normalizeType(type)
    const record = this.getTypeUsage(tappId, bucket)
    record.lastUsedAt = Date.now()
    record.rateLimiter.record()
    // Non-dedicated buckets also debit the shared global bridge budget
    if (!this.isDedicatedBudgetBucket(bucket)) {
      const global = this.getTypeUsage(tappId, 'bridge.action')
      global.lastUsedAt = Date.now()
      global.rateLimiter.record()
    }
  }
}

/** 全局配额管理器实例 */
let quotaManagerInstance: TappQuotaManager | null = null

/**
 * 获取配额管理器实例
 */
export function getQuotaManager(): TappQuotaManager {
  if (!quotaManagerInstance) {
    quotaManagerInstance = new TappQuotaManager()
  }
  return quotaManagerInstance
}
