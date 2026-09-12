/** 前端软限制；安全限制必须由后端实现。 */

import { currentCopy, formatCurrent } from '../../i18n/localeCopy'

const DEFAULT_QUOTA = {
  platform: {
    readPerMinute: 60,
    writePerMinute: 10,
  },
  apiExecutePerMinute: 30,
  bridgeActionsPerMinute: 180,
  lifecyclePerMinute: 30,
  /** storage/shared/private/settings 热路径单独计，不计入全局 bridge 桶。 */
  storagePerMinute: 600,
  /** 基础 UI 读路径从全局桶剥离。 */
  uiHotPerMinute: 360,
}

type QuotaConfig = typeof DEFAULT_QUOTA

class SlidingWindowRateLimiter {
  private timestamps: number[] = []
  private readonly windowMs: number
  private readonly maxRequests: number

  constructor(windowMs: number, maxRequests: number) {
    this.windowMs = windowMs
    this.maxRequests = maxRequests
  }

  check(): { allowed: boolean; remaining: number; retryAfter?: number } {
    const now = Date.now()
    this.cleanup(now)

    if (this.timestamps.length >= this.maxRequests) {
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

  record(): void {
    this.timestamps.push(Date.now())
  }

  private cleanup(now: number): void {
    const cutoff = now - this.windowMs
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

interface UsageRecord {
  lastUsedAt: number
  rateLimiter: SlidingWindowRateLimiter
}

class TappQuotaManager {
  private readonly quotaConfig: QuotaConfig
  private usageByTapp: Map<string, Record<string, UsageRecord>>

  constructor() {
    this.quotaConfig = { ...DEFAULT_QUOTA }
    this.usageByTapp = new Map()

    // unref 以便 Node 测试退出（浏览器忽略 unref）。
    const timer = setInterval(
      () => this.cleanupExpiredRecords(),
      5 * 60 * 1000,
    )
    if (typeof timer === 'object' && timer && 'unref' in timer) {
      ;(timer as NodeJS.Timeout).unref()
    }
  }

  private cleanupExpiredRecords(): void {
    const now = Date.now()
    const ONE_DAY = 24 * 60 * 60 * 1000

    for (const [tappId, usage] of this.usageByTapp) {
      for (const [type, record] of Object.entries(usage)) {
        if (now - record.lastUsedAt > ONE_DAY) {
          delete usage[type]
        }
      }
      if (Object.keys(usage).length === 0) {
        this.usageByTapp.delete(tappId)
      }
    }
  }

  private getUsage(tappId: string): Record<string, UsageRecord> {
    if (!this.usageByTapp.has(tappId)) {
      this.usageByTapp.set(tappId, {})
    }
    return this.usageByTapp.get(tappId)!
  }

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
    // 四种 KV 共用热路径桶。REST 角色闸另算，不在这里分桶。
    if (
      type.startsWith('storage.') ||
      type.startsWith('shared.') ||
      type.startsWith('private.') ||
      type.startsWith('settings.')
    ) {
      return 'storage'
    }
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
    return 'bridge.action'
  }

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
        reason: formatCurrent(currentCopy().errors.rateLimitedRetry, {
          sec: Math.ceil((rateCheck.retryAfter || 0) / 1000),
        }),
        retryAfter: rateCheck.retryAfter,
      }
    }
    return { allowed: true, remaining: rateCheck.remaining }
  }

  /** 热路径只用自己的桶，避免饿死其他 bridge action。 */
  private isDedicatedBudgetBucket(bucket: string): boolean {
    return (
      bucket === 'bridge.action' ||
      bucket === 'storage' ||
      bucket === 'ui.hot' ||
      bucket === 'lifecycle'
    )
  }

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

  recordUsage(tappId: string, type: string): void {
    const bucket = this.normalizeType(type)
    const record = this.getTypeUsage(tappId, bucket)
    record.lastUsedAt = Date.now()
    record.rateLimiter.record()
    if (!this.isDedicatedBudgetBucket(bucket)) {
      const global = this.getTypeUsage(tappId, 'bridge.action')
      global.lastUsedAt = Date.now()
      global.rateLimiter.record()
    }
  }
}

let quotaManagerInstance: TappQuotaManager | null = null

export function getQuotaManager(): TappQuotaManager {
  if (!quotaManagerInstance) {
    quotaManagerInstance = new TappQuotaManager()
  }
  return quotaManagerInstance
}
