interface RetryOptions {
  maxRetries?: number
  /** ms */
  initialDelay?: number
  /** max 10000ms */
  maxDelay?: number
  /** ms */
  timeout?: number
  exponentialBackoff?: boolean
  backoffMultiplier?: number
  shouldRetry?: (error: Error, attempt: number) => boolean
  onRetry?: (error: Error, attempt: number, delay: number) => void
}

interface FetchWithRetryOptions extends RetryOptions {
  fetchOptions?: RequestInit
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

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
  const jitter = exponentialDelay * (0.75 + Math.random() * 0.5)
  return Math.min(Math.floor(jitter), maxDelay)
}

function defaultShouldRetry(error: Error, _attempt: number): boolean {
  if (
    error.message.includes('Failed to fetch') ||
    error.message.includes('Network')
  ) {
    return true
  }

  if (error.message.includes('5')) {
    return true
  }

  if (error.message.includes('429')) {
    return true
  }

  if (error.message.includes('408')) {
    return true
  }

  if (error.message.includes('503')) {
    return true
  }

  return false
}

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

      if (!response.ok) {
        const error = new Error(
          `HTTP ${response.status}: ${response.statusText}`,
        )

        if (attempt < maxRetries && shouldRetry(error, attempt)) {
          lastError = error
          let delay = calculateDelay(
            attempt,
            initialDelay,
            maxDelay,
            exponentialBackoff,
            backoffMultiplier,
          )
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

      if (attempt === maxRetries) {
        throw err
      }

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

  throw lastError ?? new Error('Request failed after all retries')
}

export async function fetchJsonWithRetry<T = any>(
  url: string,
  options: FetchWithRetryOptions = {},
): Promise<T> {
  const response = await fetchWithRetry(url, options)
  return response.json()
}
