/**
 * AI 与报告处理器
 */

import type { AITaskRequest, TappInstance } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import { userFacingError } from '../../../../utils/userFacingError'
import * as TappApiService from '../../../services/TappApiService'

/**
 * 注册 AI 处理器
 */
export function registerAIHandlers(bridge: TappBridge): () => void {
  const taskStreams = new Map<string, AbortController>()

  bridge.registerHandler('ai.tasks.create', async (message) => {
    const [request] = (message.payload as { args: unknown[] }).args || []
    if (!request) return { success: false, error: 'Request required' }
    try {
      const task = await TappApiService.createAITask(
        request as AITaskRequest,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: task }
    } catch (error) {
      return {
        success: false,
        error:
          userFacingError(error),
      }
    }
  })

  bridge.registerHandler('ai.tasks.get', async (message) => {
    const [taskId] = (message.payload as { args: unknown[] }).args || []
    if (typeof taskId !== 'string') {
      return { success: false, error: 'Task ID required' }
    }
    try {
      const task = await TappApiService.getAITask(
        taskId,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: task }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('ai.tasks.cancel', async (message) => {
    const [taskId] = (message.payload as { args: unknown[] }).args || []
    if (typeof taskId !== 'string') {
      return { success: false, error: 'Task ID required' }
    }
    try {
      const result = await TappApiService.cancelAITask(
        taskId,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: result }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('ai.tasks.usage', async () => {
    try {
      const usage = await TappApiService.getAIUsage(
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: usage }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('ai.tasks.subscribe', async (message) => {
    const [taskId] = (message.payload as { args: unknown[] }).args || []
    if (typeof taskId !== 'string') {
      return { success: false, error: 'Task ID required' }
    }
    if (taskStreams.has(taskId)) return { success: true, data: true }

    const controller = new AbortController()
    taskStreams.set(taskId, controller)
    void TappApiService.streamAITaskEvents(
      taskId,
      await bridge.getRuntimeGrant(),
      (event) => bridge.emit('aiTaskEvent', { taskId, ...event }),
      controller.signal,
    )
      .catch((error) => {
        if (!controller.signal.aborted) {
          bridge.emit('aiTaskEvent', {
            taskId,
            event: 'error',
            data: {
              code: 'AI_TASK_STREAM_ERROR',
              message: userFacingError(error),
            },
          })
        }
      })
      .finally(() => {
        // 取消后可能已为同 taskId 建立新 stream；旧请求不能删除新 controller。
        if (taskStreams.get(taskId) === controller) {
          taskStreams.delete(taskId)
        }
      })
    return { success: true, data: true }
  })

  bridge.registerHandler('ai.tasks.unsubscribe', async (message) => {
    const [taskId] = (message.payload as { args: unknown[] }).args || []
    if (typeof taskId !== 'string') {
      return { success: false, error: 'Task ID required' }
    }
    taskStreams.get(taskId)?.abort()
    taskStreams.delete(taskId)
    return { success: true, data: true }
  })

  return () => {
    taskStreams.forEach((controller) => controller.abort())
    taskStreams.clear()
  }
}

/**
 * 注册报告处理器
 */
export function registerReportHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
  options: { readOnly?: boolean } = {},
): void {
  bridge.registerHandler('report.listReports', async () => {
    try {
      const reports = await TappApiService.listReports(
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: reports }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('report.getReport', async (message) => {
    const [reportId] = (message.payload as { args: unknown[] }).args || []
    if (!reportId) return { success: false, error: 'Report ID required' }
    try {
      const report = await TappApiService.getReport(
        reportId as string,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: report }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('report.getPlatformReport', async (message) => {
    const [platform] = (message.payload as { args: unknown[] }).args || []
    if (!platform) return { success: false, error: 'Platform required' }
    try {
      const report = await TappApiService.getPlatformReport(
        platform as string,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: report }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  if (!options.readOnly) {
    bridge.registerHandler('report.create', async (message) => {
      const [params] = (message.payload as { args: unknown[] }).args || []
      const { title, reportType, content, metadata } = (params || {}) as {
        title?: string
        reportType?: string
        content?: unknown
        metadata?: unknown
      }
      try {
        const result = await TappApiService.createTappReport(
          {
            tappId: tappInstance.id,
            title: title || '',
            reportType: (reportType || 'custom') as 'custom' | 'platform',
            content,
            metadata,
          },
          await bridge.getRuntimeGrant(),
        )
        return { success: true, data: result }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    })
  }

  bridge.registerHandler('report.list', async () => {
    try {
      const result = await TappApiService.listTappReports(
        tappInstance.id,
        await bridge.getRuntimeGrant(),
      )
      // SDK / docs treat data as the reports array (not { success, reports })
      const reports = Array.isArray(result)
        ? result
        : Array.isArray(result?.reports)
          ? result.reports
          : []
      return { success: true, data: reports }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('report.get', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const { reportId } = (params || {}) as { reportId?: string }
    if (!reportId) return { success: false, error: 'Report ID required' }
    try {
      const result = await TappApiService.getTappReport(
        tappInstance.id,
        reportId,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: result }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  if (!options.readOnly) {
    bridge.registerHandler('report.update', async (message) => {
      const [params] = (message.payload as { args: unknown[] }).args || []
      const { reportId, title, content, metadata } = (params || {}) as {
        reportId?: string
        title?: string
        content?: unknown
        metadata?: unknown
      }
      if (!reportId) return { success: false, error: 'Report ID required' }
      try {
        const result = await TappApiService.updateTappReport(
          tappInstance.id,
          reportId,
          { title, content, metadata },
          await bridge.getRuntimeGrant(),
        )
        return { success: true, data: result }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    })

    bridge.registerHandler('report.delete', async (message) => {
      const [params] = (message.payload as { args: unknown[] }).args || []
      const { reportId } = (params || {}) as { reportId?: string }
      if (!reportId) return { success: false, error: 'Report ID required' }
      try {
        const result = await TappApiService.deleteTappReport(
          tappInstance.id,
          reportId,
          await bridge.getRuntimeGrant(),
        )
        return { success: true, data: result }
      } catch (error) {
        return {
          success: false,
          error: userFacingError(error),
        }
      }
    })
  }
}
