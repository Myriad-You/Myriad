/**
 * 客户端请求限流工具
 * 防止短时间内大量请求导致的性能问题
 */

interface RateLimitConfig {
  maxRequests: number // 时间窗口内最大请求数
  windowMs: number // 时间窗口（毫秒）
}

interface RateLimitEntry {
  timestamps: number[]
  blocked: boolean
  blockedUntil?: number
}

const rateLimitStore = new Map<string, RateLimitEntry>()

/**
 * 默认限流配置
 */
const DEFAULT_CONFIGS: Record<string, RateLimitConfig> = {
  login: { maxRequests: 5, windowMs: 5 * 60 * 1000 }, // 登录: 5次/5分钟
  api: { maxRequests: 100, windowMs: 60 * 1000 }, // API: 100次/分钟
  fetch: { maxRequests: 10, windowMs: 60 * 1000 }, // 数据获取: 10次/分钟
  analysis: { maxRequests: 5, windowMs: 60 * 1000 }, // 分析: 5次/分钟
}

/**
 * 检查是否超出限流
 */
export function checkRateLimit(key: string, configName: keyof typeof DEFAULT_CONFIGS = 'api'): boolean {
  const config = DEFAULT_CONFIGS[configName]
  const now = Date.now()

  let entry = rateLimitStore.get(key)

  if (!entry) {
    entry = { timestamps: [], blocked: false }
    rateLimitStore.set(key, entry)
  }

  // 检查是否在封禁期
  if (entry.blocked && entry.blockedUntil) {
    if (now < entry.blockedUntil) {
      return false // 仍在封禁期
    }
    else {
      // 封禁期结束，重置
      entry.blocked = false
      entry.blockedUntil = undefined
      entry.timestamps = []
    }
  }

  // 清理过期的时间戳
  entry.timestamps = entry.timestamps.filter(
    timestamp => now - timestamp < config.windowMs,
  )

  // 检查是否超出限制
  if (entry.timestamps.length >= config.maxRequests) {
    // 超出限制，封禁一段时间
    entry.blocked = true
    entry.blockedUntil = now + config.windowMs
    return false
  }

  // 记录本次请求
  entry.timestamps.push(now)
  return true
}

/**
 * 获取剩余请求次数
 */
export function getRemainingRequests(key: string, configName: keyof typeof DEFAULT_CONFIGS = 'api'): number {
  const config = DEFAULT_CONFIGS[configName]
  const entry = rateLimitStore.get(key)

  if (!entry)
    return config.maxRequests

  const now = Date.now()
  const validTimestamps = entry.timestamps.filter(
    timestamp => now - timestamp < config.windowMs,
  )

  return Math.max(0, config.maxRequests - validTimestamps.length)
}

/**
 * 获取重置时间（毫秒）
 */
export function getResetTime(key: string, configName: keyof typeof DEFAULT_CONFIGS = 'api'): number {
  const config = DEFAULT_CONFIGS[configName]
  const entry = rateLimitStore.get(key)

  if (!entry || entry.timestamps.length === 0)
    return 0

  const oldestTimestamp = Math.min(...entry.timestamps)
  const resetTime = oldestTimestamp + config.windowMs

  return Math.max(0, resetTime - Date.now())
}

/**
 * 重置限流计数器
 */
export function resetRateLimit(key: string): void {
  rateLimitStore.delete(key)
}

/**
 * 清理所有限流记录
 */
export function clearAllRateLimits(): void {
  rateLimitStore.clear()
}

/**
 * Rate Limit 错误类
 */
export class RateLimitError extends Error {
  constructor(
    message: string,
    public retryAfter: number,
    public remaining: number = 0,
  ) {
    super(message)
    this.name = 'RateLimitError'
  }
}

/**
 * 执行带限流检查的操作
 */
export async function withRateLimit<T>(
  key: string,
  operation: () => Promise<T>,
  configName: keyof typeof DEFAULT_CONFIGS = 'api',
): Promise<T> {
  if (!checkRateLimit(key, configName)) {
    const resetTime = getResetTime(key, configName)
    const remaining = getRemainingRequests(key, configName)

    throw new RateLimitError(
      `请求过于频繁，请在 ${Math.ceil(resetTime / 1000)} 秒后重试`,
      resetTime,
      remaining,
    )
  }

  return operation()
}
