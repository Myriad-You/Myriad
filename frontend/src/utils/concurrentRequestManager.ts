import { ApiError } from '../services/api'
import { httpStatusMessage } from './userFacingError'

interface QueuedRequest {
  key: string
  fetcher: () => Promise<any>
  controller: AbortController
  priority: number
  resolve: (value: any) => void
  reject: (reason: any) => void
  timeout?: number
  timeoutId: ReturnType<typeof setTimeout> | null
  timedOut: boolean
}

class ConcurrentRequestManager {
  private activeRequests: Map<string, AbortController> = new Map()
  private requestQueue: QueuedRequest[] = []
  private maxConcurrent: number
  private currentCount: number = 0

  constructor(maxConcurrent: number = 6) {
    this.maxConcurrent = maxConcurrent
  }

  /** ms */
  async fetch<T>(
    key: string,
    fetcher: (signal: AbortSignal) => Promise<T>,
    priority: number = 0,
    timeout?: number,
  ): Promise<T> {
    if (
      this.activeRequests.has(key) ||
      this.requestQueue.some((request) => request.key === key)
    ) {
      this.cancelRequest(key)
    }

    const controller = new AbortController()

    const { promise, resolve, reject } = Promise.withResolvers<T>()
    const request: QueuedRequest = {
      key,
      fetcher: () => fetcher(controller.signal),
      controller,
      priority,
      resolve,
      reject,
      timeout,
      timeoutId: null,
      timedOut: false,
    }

    if (this.currentCount >= this.maxConcurrent) {
      this.requestQueue.push(request)
      this.requestQueue = this.requestQueue.toSorted(
        (a, b) => b.priority - a.priority,
      )
    } else {
      this.executeRequest(request)
    }
    return promise
  }

  private async executeRequest(request: QueuedRequest) {
    this.currentCount++
    this.activeRequests.set(request.key, request.controller)

    // Queue time is not part of the network timeout.
    if (request.timeout) {
      request.timeoutId = setTimeout(() => {
        request.timedOut = true
        request.controller.abort()
      }, request.timeout)
    }

    try {
      const result = await request.fetcher()
      request.resolve(result)
    } catch (error) {
      if (error instanceof Error && error.name === 'AbortError') {
        request.reject(
          new Error(
            request.timedOut ? 'Request timed out' : 'Request was cancelled',
          ),
        )
      } else {
        request.reject(error)
      }
    } finally {
      if (request.timeoutId !== null) {
        clearTimeout(request.timeoutId)
        request.timeoutId = null
      }
      if (this.activeRequests.get(request.key) === request.controller) {
        this.activeRequests.delete(request.key)
      }
      this.currentCount--
      this.processQueue()
    }
  }

  private processQueue() {
    if (
      this.requestQueue.length > 0 &&
      this.currentCount < this.maxConcurrent
    ) {
      const nextRequest = this.requestQueue.shift()
      if (nextRequest) {
        this.executeRequest(nextRequest)
      }
    }
  }

  cancelRequest(key: string): void {
    const controller = this.activeRequests.get(key)
    if (controller) {
      controller.abort()
    }

    // Reject queued promises so callers do not hang.
    const queued = this.requestQueue.filter((request) => request.key === key)
    this.requestQueue = this.requestQueue.filter(
      (request) => request.key !== key,
    )
    for (const request of queued) {
      request.controller.abort()
      request.reject(new Error('Request was cancelled'))
    }
  }

  cancelAll(): void {
    for (const [_key, controller] of this.activeRequests) {
      controller.abort()
    }
    this.activeRequests.clear()

    for (const request of this.requestQueue) {
      request.controller.abort()
      request.reject(new Error('Request was cancelled'))
    }
    this.requestQueue = []
  }

  getStatus() {
    return {
      active: this.currentCount,
      queued: this.requestQueue.length,
      maxConcurrent: this.maxConcurrent,
    }
  }
}

export const requestManager = new ConcurrentRequestManager(6)

export async function managedFetch<T = any>(
  url: string,
  options: RequestInit = {},
  config: {
    key?: string
    priority?: number
    timeout?: number
  } = {},
): Promise<T> {
  const key = config.key || url
  const priority = config.priority || 0
  const timeout = config.timeout

  return requestManager.fetch(
    key,
    async (signal) => {
      const response = await fetch(url, {
        ...options,
        signal,
      })

      if (!response.ok) {
        throw new ApiError(httpStatusMessage(response.status), response.status)
      }

      return response.json()
    },
    priority,
    timeout,
  )
}
