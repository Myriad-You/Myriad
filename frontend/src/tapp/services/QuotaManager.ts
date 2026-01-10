/**
 * Tapp 配额管理服务
 * 管理 Tapp 的 API 调用配额和使用统计
 *
 * 安全特性：
 * - 滑动窗口速率限制，防止突发请求
 * - 配额数据使用内存缓存，页面刷新后会重置
 * - 这是安全设计，防止用户绕过前端配额检查
 * - 真正的配额限制应该在后端实现
 *
 * 性能特性：
 * - 高效的滑动窗口算法
 * - 自动清理过期记录
 */

import type { TappQuotaConfig, TappUsageStats } from '../types'

/** 默认配额配置 */
const DEFAULT_QUOTA: TappQuotaConfig = {
  ai: {
    dailyLimit: 100, // 每日 AI 调用次数限制
    monthlyLimit: 2000, // 每月 AI 调用次数限制
    maxTokensPerRequest: 2000, // 每次请求最大 token
  },
  platform: {
    readPerMinute: 60, // 每分钟读取次数
    writePerMinute: 10, // 每分钟写入次数
    maxItemsPerBatch: 100, // 批量操作最大条目数
  },
  storage: {
    maxKeys: 1000, // 最大键数量
    maxValueSize: 1024 * 1024, // 单个值最大大小 (1MB)
    maxTotalSize: 10 * 1024 * 1024, // 总存储大小 (10MB)
  },
  widget: {
    maxRegistrations: 10, // 最大注册小组件数
    minRefreshInterval: 1000, // 最小刷新间隔 (ms)
  },
}

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
  check(): { allowed: boolean, remaining: number, retryAfter?: number } {
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
      }
      else {
        right = mid
      }
    }
    if (left > 0) {
      this.timestamps = this.timestamps.slice(left)
    }
  }

  /**
   * 获取当前窗口内的请求数
   */
  getCount(): number {
    this.cleanup(Date.now())
    return this.timestamps.length
  }

  /**
   * 重置限制器
   */
  reset(): void {
    this.timestamps = []
  }
}

/** 使用记录 */
interface UsageRecord {
  count: number
  lastReset: number
  history: { timestamp: number, count: number }[]
  rateLimiter?: SlidingWindowRateLimiter
}

/** 配额管理器 */
class TappQuotaManager {
  private quotaConfig: TappQuotaConfig
  private usageByTapp: Map<string, Record<string, UsageRecord>>

  /** 全局速率限制器（防止单个 Tapp 滥用） */
  private globalRateLimiters: Map<string, SlidingWindowRateLimiter> = new Map()

  /** 自动清理定时器 */
  private cleanupInterval: ReturnType<typeof setInterval> | null = null

  constructor() {
    this.quotaConfig = { ...DEFAULT_QUOTA }
    this.usageByTapp = new Map()

    // 启动自动清理（每 5 分钟清理一次过期数据）
    this.cleanupInterval = setInterval(() => this.cleanupExpiredRecords(), 5 * 60 * 1000)
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
        if (now - record.lastReset > ONE_DAY && record.count === 0) {
          delete usage[type]
        }
        // 限制历史记录长度
        if (record.history.length > 30) {
          record.history = record.history.slice(-30)
        }
      }
      // 如果 Tapp 没有任何使用记录，删除它
      if (Object.keys(usage).length === 0) {
        this.usageByTapp.delete(tappId)
      }
    }
  }

  /**
   * 销毁管理器（清理定时器）
   */
  destroy(): void {
    if (this.cleanupInterval) {
      clearInterval(this.cleanupInterval)
      this.cleanupInterval = null
    }
  }

  /** 获取或创建 Tapp 的使用记录 */
  private getUsage(tappId: string): Record<string, UsageRecord> {
    if (!this.usageByTapp.has(tappId)) {
      this.usageByTapp.set(tappId, {})
    }
    return this.usageByTapp.get(tappId)!
  }

  /** 获取或创建特定类型的使用记录（带滑动窗口限制器） */
  private getTypeUsage(tappId: string, type: string): UsageRecord {
    const usage = this.getUsage(tappId)
    if (!usage[type]) {
      // 根据类型创建对应的速率限制器
      let rateLimiter: SlidingWindowRateLimiter | undefined

      if (type === 'platform.read') {
        rateLimiter = new SlidingWindowRateLimiter(60 * 1000, this.quotaConfig.platform.readPerMinute)
      }
      else if (type === 'platform.write') {
        rateLimiter = new SlidingWindowRateLimiter(60 * 1000, this.quotaConfig.platform.writePerMinute)
      }
      else if (type.startsWith('ai.')) {
        // AI 请求使用更严格的短期限制（10秒内最多5次）
        rateLimiter = new SlidingWindowRateLimiter(10 * 1000, 5)
      }
      else if (type === 'http.fetch') {
        // HTTP 请求限制（每分钟30次）
        rateLimiter = new SlidingWindowRateLimiter(60 * 1000, 30)
      }

      usage[type] = {
        count: 0,
        lastReset: Date.now(),
        history: [],
        rateLimiter,
      }
    }
    return usage[type]
  }

  /** 检查是否需要重置计数器 */
  private checkReset(record: UsageRecord, period: 'minute' | 'day' | 'month'): void {
    const now = Date.now()
    const periodMs = {
      minute: 60 * 1000,
      day: 24 * 60 * 60 * 1000,
      month: 30 * 24 * 60 * 60 * 1000,
    }[period]

    if (now - record.lastReset > periodMs) {
      // 保存历史记录
      record.history.push({
        timestamp: record.lastReset,
        count: record.count,
      })
      // 只保留最近 30 条历史
      if (record.history.length > 30) {
        record.history = record.history.slice(-30)
      }
      // 重置计数器
      record.count = 0
      record.lastReset = now
    }
  }

  /**
   * 检查配额是否允许操作（增强版：包含滑动窗口检查）
   */
  checkQuota(tappId: string, type: string, amount: number = 1): {
    allowed: boolean
    remaining: number
    reason?: string
    retryAfter?: number
  } {
    const record = this.getTypeUsage(tappId, type)

    // 首先检查滑动窗口速率限制
    if (record.rateLimiter) {
      const rateCheck = record.rateLimiter.check()
      if (!rateCheck.allowed) {
        return {
          allowed: false,
          remaining: 0,
          reason: `速率限制：请求过于频繁，请在 ${Math.ceil((rateCheck.retryAfter || 0) / 1000)} 秒后重试`,
          retryAfter: rateCheck.retryAfter,
        }
      }
    }

    let limit: number
    let period: 'minute' | 'day' | 'month'

    switch (type) {
      case 'ai.generate':
      case 'ai.analyze':
        limit = this.quotaConfig.ai.dailyLimit
        period = 'day'
        break
      case 'platform.read':
        limit = this.quotaConfig.platform.readPerMinute
        period = 'minute'
        break
      case 'platform.write':
        limit = this.quotaConfig.platform.writePerMinute
        period = 'minute'
        break
      case 'http.fetch':
        limit = 30 // 每分钟30次 HTTP 请求
        period = 'minute'
        break
      default:
        // 默认不限制
        return { allowed: true, remaining: Infinity }
    }

    this.checkReset(record, period)

    const remaining = limit - record.count
    if (record.count + amount > limit) {
      return {
        allowed: false,
        remaining,
        reason: `超出 ${type} 配额限制 (${record.count}/${limit})`,
      }
    }

    return { allowed: true, remaining: remaining - amount }
  }

  /**
   * 记录使用（同时更新滑动窗口）
   */
  recordUsage(tappId: string, type: string, amount: number = 1): void {
    const record = this.getTypeUsage(tappId, type)
    record.count += amount

    // 记录到滑动窗口
    if (record.rateLimiter) {
      for (let i = 0; i < amount; i++) {
        record.rateLimiter.record()
      }
    }
  }

  /**
   * 批量检查多个操作的配额
   */
  checkMultipleQuotas(tappId: string, operations: Array<{ type: string, amount?: number }>): {
    allowed: boolean
    results: Array<{ type: string, allowed: boolean, remaining: number, reason?: string }>
  } {
    const results = operations.map(op => ({
      type: op.type,
      ...this.checkQuota(tappId, op.type, op.amount || 1),
    }))

    return {
      allowed: results.every(r => r.allowed),
      results,
    }
  }

  /**
   * 获取 Tapp 的使用统计
   */
  getStats(tappId: string): TappUsageStats {
    const usage = this.getUsage(tappId)

    const aiRecord = this.getTypeUsage(tappId, 'ai.generate')
    const readRecord = this.getTypeUsage(tappId, 'platform.read')
    const writeRecord = this.getTypeUsage(tappId, 'platform.write')

    // 检查重置
    this.checkReset(aiRecord, 'day')
    this.checkReset(readRecord, 'minute')
    this.checkReset(writeRecord, 'minute')

    return {
      tappId,
      ai: {
        used: aiRecord.count,
        limit: this.quotaConfig.ai.dailyLimit,
        remaining: Math.max(0, this.quotaConfig.ai.dailyLimit - aiRecord.count),
        resetAt: new Date(aiRecord.lastReset + 24 * 60 * 60 * 1000).toISOString(),
      },
      platformRead: {
        used: readRecord.count,
        limit: this.quotaConfig.platform.readPerMinute,
        remaining: Math.max(0, this.quotaConfig.platform.readPerMinute - readRecord.count),
        resetAt: new Date(readRecord.lastReset + 60 * 1000).toISOString(),
      },
      platformWrite: {
        used: writeRecord.count,
        limit: this.quotaConfig.platform.writePerMinute,
        remaining: Math.max(0, this.quotaConfig.platform.writePerMinute - writeRecord.count),
        resetAt: new Date(writeRecord.lastReset + 60 * 1000).toISOString(),
      },
      history: Object.entries(usage).map(([type, record]) => ({
        type,
        count: record.count,
        lastReset: new Date(record.lastReset).toISOString(),
      })),
    }
  }

  /**
   * 获取所有 Tapp 的使用统计
   */
  getAllStats(): TappUsageStats[] {
    const stats: TappUsageStats[] = []
    this.usageByTapp.forEach((_, tappId) => {
      stats.push(this.getStats(tappId))
    })
    return stats
  }

  /**
   * 重置 Tapp 的配额
   */
  resetQuota(tappId: string, type?: string): void {
    if (type) {
      const record = this.getTypeUsage(tappId, type)
      record.count = 0
      record.lastReset = Date.now()
    }
    else {
      this.usageByTapp.delete(tappId)
    }
    // 不再保存到 localStorage
  }

  /**
   * 更新配额配置
   */
  updateConfig(config: Partial<TappQuotaConfig>): void {
    this.quotaConfig = {
      ...this.quotaConfig,
      ...config,
      ai: { ...this.quotaConfig.ai, ...config.ai },
      platform: { ...this.quotaConfig.platform, ...config.platform },
      storage: { ...this.quotaConfig.storage, ...config.storage },
      widget: { ...this.quotaConfig.widget, ...config.widget },
    }
  }

  /**
   * 获取当前配额配置
   */
  getConfig(): TappQuotaConfig {
    return { ...this.quotaConfig }
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

export { DEFAULT_QUOTA, TappQuotaManager }
export type { TappQuotaConfig, TappUsageStats }
