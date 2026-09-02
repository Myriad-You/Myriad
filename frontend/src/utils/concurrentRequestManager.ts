/**
 * 并发请求管理器
 *
 * 功能：
 * 1. 控制同时进行的请求数量
 * 2. 自动取消过期或重复的请求
 * 3. 支持请求优先级
 */

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

  /**
   * 执行请求（带并发控制）
   * @param key 请求唯一标识
   * @param fetcher 请求函数
   * @param priority 优先级（数值越大越优先）
   * @param timeout 超时时间（毫秒）
   */
  async fetch<T>(
    key: string,
    fetcher: (signal: AbortSignal) => Promise<T>,
    priority: number = 0,
    timeout?: number,
  ): Promise<T> {
    // 如果已有相同请求正在进行，取消旧请求
    if (
      this.activeRequests.has(key) ||
      this.requestQueue.some((request) => request.key === key)
    ) {
      this.cancelRequest(key)
    }

    const controller = new AbortController()

    return new Promise<T>((resolve, reject) => {
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

      // 如果达到并发限制，加入队列
      if (this.currentCount >= this.maxConcurrent) {
        this.requestQueue.push(request)
        // 按优先级排序（高优先级在前）
        this.requestQueue.sort((a, b) => b.priority - a.priority)
      } else {
        this.executeRequest(request)
      }
    })
  }

  /**
   * 执行单个请求
   */
  private async executeRequest(request: QueuedRequest) {
    this.currentCount++
    this.activeRequests.set(request.key, request.controller)

    // 排队时间不计入网络超时；任务真正开始后才启动计时。
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

  /**
   * 处理队列中的下一个请求
   */
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

  /**
   * 取消指定请求
   */
  cancelRequest(key: string): void {
    const controller = this.activeRequests.get(key)
    if (controller) {
      controller.abort()
    }

    // 同时从队列中移除并结束其 Promise，避免调用方永久等待。
    const queued = this.requestQueue.filter((request) => request.key === key)
    this.requestQueue = this.requestQueue.filter(
      (request) => request.key !== key,
    )
    for (const request of queued) {
      request.controller.abort()
      request.reject(new Error('Request was cancelled'))
    }
  }

  /**
   * 取消所有请求
   */
  cancelAll(): void {
    // 取消所有活动请求
    for (const [_key, controller] of this.activeRequests) {
      controller.abort()
    }
    this.activeRequests.clear()

    // 清空队列并结束所有尚未执行的 Promise
    for (const request of this.requestQueue) {
      request.controller.abort()
      request.reject(new Error('Request was cancelled'))
    }
    this.requestQueue = []
  }

  /**
   * 获取当前状态
   */
  getStatus() {
    return {
      active: this.currentCount,
      queued: this.requestQueue.length,
      maxConcurrent: this.maxConcurrent,
    }
  }
}

// 创建全局实例
export const requestManager = new ConcurrentRequestManager(6)

/**
 * 简化的fetch封装（自动处理AbortSignal）
 */
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
