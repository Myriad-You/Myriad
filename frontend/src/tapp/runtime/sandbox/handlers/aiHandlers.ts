/**
 * AI 与报告处理器
 */

import type { TappInstance } from '../../../types'
import type { TappBridge } from '../../TappBridge'
import type { TappPermissionController } from '../../TappPermission'
import { getQuotaManager } from '../../../services/QuotaManager'
import * as TappApiService from '../../../services/TappApiService'

/**
 * 注册 AI 处理器
 */
export function registerAIHandlers(
  bridge: TappBridge,
  permission: TappPermissionController,
  tappInstance: TappInstance,
): void {
  const quotaManager = getQuotaManager()

  bridge.registerHandler('ai.getQuota', async () => {
    const status = permission.getAIQuotaStatus()
    return { success: true, data: status }
  })

  bridge.registerHandler('ai.canGenerate', async () => {
    const result = permission.canMakeAICall()
    return { success: true, data: result }
  })

  bridge.registerHandler('ai.generate', async (message) => {
    const [request] = (message.payload as { args: unknown[] }).args || []
    if (!request)
      return { success: false, error: 'Request required' }

    const canCall = permission.canMakeAICall()
    if (!canCall.allowed)
      return { success: false, error: canCall.reason || 'AI permission denied' }

    const quotaCheck = quotaManager.checkQuota(tappInstance.id, 'ai.generate')
    if (!quotaCheck.allowed)
      return { success: false, error: quotaCheck.reason, code: 'QUOTA_EXCEEDED' }

    try {
      const response = await TappApiService.aiGenerate(tappInstance.id, request as Parameters<typeof TappApiService.aiGenerate>[1])
      quotaManager.recordUsage(tappInstance.id, 'ai.generate')
      if (response?.usage?.totalTokens) {
        permission.recordAICall(response.usage.totalTokens)
      }
      return { success: true, data: { ...response, quotaRemaining: quotaCheck.remaining - 1 } }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'AI generation failed' }
    }
  })

  bridge.registerHandler('ai.analyze', async (message) => {
    const [request] = (message.payload as { args: unknown[] }).args || []
    if (!request)
      return { success: false, error: 'Request required' }

    const canCall = permission.canMakeAICall()
    if (!canCall.allowed)
      return { success: false, error: canCall.reason || 'AI permission denied' }

    const quotaCheck = quotaManager.checkQuota(tappInstance.id, 'ai.analyze')
    if (!quotaCheck.allowed)
      return { success: false, error: quotaCheck.reason, code: 'QUOTA_EXCEEDED' }

    try {
      const response = await TappApiService.aiAnalyze(tappInstance.id, request as Parameters<typeof TappApiService.aiAnalyze>[1])
      quotaManager.recordUsage(tappInstance.id, 'ai.analyze')
      permission.recordAICall(500)
      return { success: true, data: { ...response, quotaRemaining: quotaCheck.remaining - 1 } }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'AI analysis failed' }
    }
  })

  bridge.registerHandler('ai.image', async (message) => {
    const [request] = (message.payload as { args: unknown[] }).args || []
    const req = request as { prompt?: string } | undefined
    if (!req?.prompt)
      return { success: false, error: 'Prompt required' }

    const canCall = permission.canMakeAICall()
    if (!canCall.allowed)
      return { success: false, error: canCall.reason || 'AI permission denied' }

    const quotaCheck = quotaManager.checkQuota(tappInstance.id, 'ai.image')
    if (!quotaCheck.allowed)
      return { success: false, error: quotaCheck.reason, code: 'QUOTA_EXCEEDED' }

    try {
      const response = await TappApiService.aiImageGenerate(tappInstance.id, request as Parameters<typeof TappApiService.aiImageGenerate>[1])
      quotaManager.recordUsage(tappInstance.id, 'ai.image')
      return { success: true, data: { ...response, quotaRemaining: quotaCheck.remaining - 1 } }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'AI image failed' }
    }
  })

  bridge.registerHandler('ai.chat', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const { messages, context, options } = (params || {}) as {
      messages?: Array<{ role: 'user' | 'assistant' | 'system', content: string }>
      context?: Record<string, unknown>
      options?: Record<string, unknown>
    }
    try {
      const result = await TappApiService.aiChat({
        tappId: tappInstance.id,
        messages: messages || [],
        context,
        options,
      })
      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'AI chat failed' }
    }
  })
}

/**
 * 注册报告处理器
 */
export function registerReportHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): void {
  bridge.registerHandler('report.listReports', async () => {
    try {
      const reports = await TappApiService.listReports()
      return { success: true, data: reports }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('report.getReport', async (message) => {
    const [reportId] = (message.payload as { args: unknown[] }).args || []
    if (!reportId)
      return { success: false, error: 'Report ID required' }
    try {
      const report = await TappApiService.getReport(reportId as string)
      return { success: true, data: report }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('report.getPlatformReport', async (message) => {
    const [platform] = (message.payload as { args: unknown[] }).args || []
    if (!platform)
      return { success: false, error: 'Platform required' }
    try {
      const report = await TappApiService.getPlatformReport(platform as string)
      return { success: true, data: report }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('report.create', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const { title, reportType, content, metadata } = (params || {}) as {
      title?: string
      reportType?: string
      content?: unknown
      metadata?: unknown
    }
    try {
      const result = await TappApiService.createTappReport({
        tappId: tappInstance.id,
        title: title || '',
        reportType: (reportType || 'custom') as 'custom' | 'platform' | 'comprehensive',
        content,
        metadata,
      })
      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('report.list', async () => {
    try {
      const result = await TappApiService.listTappReports(tappInstance.id)
      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('report.get', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const { reportId } = (params || {}) as { reportId?: string }
    if (!reportId)
      return { success: false, error: 'Report ID required' }
    try {
      const result = await TappApiService.getTappReport(tappInstance.id, reportId)
      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('report.update', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const { reportId, title, content, metadata } = (params || {}) as {
      reportId?: string
      title?: string
      content?: unknown
      metadata?: unknown
    }
    if (!reportId)
      return { success: false, error: 'Report ID required' }
    try {
      const result = await TappApiService.updateTappReport(tappInstance.id, reportId, { title, content, metadata })
      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })

  bridge.registerHandler('report.delete', async (message) => {
    const [params] = (message.payload as { args: unknown[] }).args || []
    const { reportId } = (params || {}) as { reportId?: string }
    if (!reportId)
      return { success: false, error: 'Report ID required' }
    try {
      const result = await TappApiService.deleteTappReport(tappInstance.id, reportId)
      return { success: true, data: result }
    }
    catch (error) {
      return { success: false, error: error instanceof Error ? error.message : 'Failed' }
    }
  })
}
