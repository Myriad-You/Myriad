/**
 * API 重试工具
 * 提供自动重试、指数退避、超时控制等功能
 */

interface RetryOptions {
  /** 最大重试次数，默认 3 */
  maxRetries?: number
  /** 初始延迟（毫秒），默认 1000ms */
  initialDelay?: number
  /** 最大延迟（毫秒），默认 10000ms */
  maxDelay?: number
  /** 超时时间（毫秒），默认 30000ms */
  timeout?: number
  /** 是否使用指数退避，默认 true */
  exponentialBackoff?: boolean
  /** 退避倍数，默认 2 */
  backoffMultiplier?: number
  /** 自定义错误判断函数（返回 true 表示应该重试） */
  shouldRetry?: (error: Error, attempt: number) => boolean
  /** 重试前的回调 */
  onRetry?: (error: Error, attempt: number, delay: number) => void
}

interface FetchWithRetryOptions extends RetryOptions {
  /** fetch 的 RequestInit 选项 */
  fetchOptions?: RequestInit
}

/**
 * 延迟指定毫秒数
 */
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/**
 * 计算重试延迟时间
 */
function calculateDelay(
  attempt: number,
  initialDelay: number,
  maxDelay: number,
  exponentialBackoff: boolean,
  backoffMultiplier: number,
): number {
  if (!exponentialBackoff) {
    return Math.min(initialDelay, maxDelay)
  }

  const exponentialDelay = initialDelay * backoffMultiplier ** attempt
  // 添加随机抖动（±25%）减少并发冲突
  const jitter = exponentialDelay * (0.75 + Math.random() * 0.5)
  return Math.min(Math.floor(jitter), maxDelay)
}

/**
 * 默认的错误重试判断
 */
function defaultShouldRetry(error: Error, _attempt: number): boolean {
  // 网络错误总是重试
  if (
    error.message.includes('Failed to fetch') ||
    error.message.includes('Network')
  ) {
    return true
  }

  // HTTP 5xx 错误重试
  if (error.message.includes('5')) {
    return true
  }

  // 429 Too Many Requests 重试
  if (error.message.includes('429')) {
    return true
  }

  // 408 Request Timeout 重试
  if (error.message.includes('408')) {
    return true
  }

  // 503 Service Unavailable 重试
  if (error.message.includes('503')) {
    return true
  }

  return false
}

/**
 * 带超时的 fetch 包装
 */
async function fetchWithTimeout(
  url: string,
  options: RequestInit = {},
  timeoutMs: number,
): Promise<Response> {
  const controller = new AbortController()
  const timeoutId = setTimeout(() => controller.abort(), timeoutMs)

  try {
    const response = await fetch(url, {
      ...options,
      signal: controller.signal,
    })
    clearTimeout(timeoutId)
    return response
  } catch (error) {
    clearTimeout(timeoutId)
    if (error instanceof Error && error.name === 'AbortError') {
      throw new Error(`Request timeout after ${timeoutMs}ms`)
    }
    throw error
  }
}

/**
 * 带重试的 fetch 请求
 *
 * @example
 * ```typescript
 * const response = await fetchWithRetry('https://api.example.com/data', {
 *   maxRetries: 3,
 *   timeout: 5000,
 *   fetchOptions: {
 *     method: 'POST',
 *     headers: { 'Content-Type': 'application/json' },
 *     body: JSON.stringify({ key: 'value' })
 *   }
 * });
 * ```
 */
export async function fetchWithRetry(
  url: string,
  options: FetchWithRetryOptions = {},
): Promise<Response> {
  const {
    maxRetries = 3,
    initialDelay = 1000,
    maxDelay = 10000,
    timeout = 30000,
    exponentialBackoff = true,
    backoffMultiplier = 2,
    shouldRetry = defaultShouldRetry,
    onRetry,
    fetchOptions = {},
  } = options

  let lastError: Error | null = null

  for (let attempt = 0; attempt <= maxRetries; attempt++) {
    try {
      const response = await fetchWithTimeout(url, fetchOptions, timeout)

      // 检查 HTTP 状态码
      if (!response.ok) {
        const error = new Error(
          `HTTP ${response.status}: ${response.statusText}`,
        )

        // 判断是否应该重试
        if (attempt < maxRetries && shouldRetry(error, attempt)) {
          lastError = error
          let delay = calculateDelay(
            attempt,
            initialDelay,
            maxDelay,
            exponentialBackoff,
            backoffMultiplier,
          )
          // Honor Retry-After on 429 (seconds or HTTP-date) without exceeding maxDelay
          if (response.status === 429) {
            const ra = response.headers.get('Retry-After')
            if (ra) {
              const asInt = Number.parseInt(ra, 10)
              if (Number.isFinite(asInt) && asInt >= 0) {
                delay = Math.min(asInt * 1000, maxDelay)
              } else {
                const when = Date.parse(ra)
                if (Number.isFinite(when)) {
                  delay = Math.min(
                    Math.max(0, when - Date.now()),
                    maxDelay,
                  )
                }
              }
            }
          }

          if (onRetry) {
            onRetry(error, attempt + 1, delay)
          } else {
            console.warn(
              `Request failed (attempt ${attempt + 1}/${maxRetries + 1}): ${error.message}. Retrying in ${delay}ms...`,
            )
          }

          await sleep(delay)
          continue
        }

        throw error
      }

      return response
    } catch (error) {
      const err = error instanceof Error ? error : new Error(String(error))

      // 最后一次尝试，直接抛出错误
      if (attempt === maxRetries) {
        throw err
      }

      // 判断是否应该重试
      if (!shouldRetry(err, attempt)) {
        throw err
      }

      lastError = err
      const delay = calculateDelay(
        attempt,
        initialDelay,
        maxDelay,
        exponentialBackoff,
        backoffMultiplier,
      )

      if (onRetry) {
        onRetry(err, attempt + 1, delay)
      } else {
        console.warn(
          `Request failed (attempt ${attempt + 1}/${maxRetries + 1}): ${err.message}. Retrying in ${delay}ms...`,
        )
      }

      await sleep(delay)
    }
  }

  // 理论上不会到达这里，但为了类型安全
  throw lastError || new Error('Request failed after all retries')
}

/**
 * 带重试的 JSON 请求（便捷方法）
 *
 * @example
 * ```typescript
 * const data = await fetchJsonWithRetry<UserData>('https://api.example.com/user', {
 *   maxRetries: 3
 * });
 * ```
 */
export async function fetchJsonWithRetry<T = any>(
  url: string,
  options: FetchWithRetryOptions = {},
): Promise<T> {
  const response = await fetchWithRetry(url, options)
  return response.json()
}
