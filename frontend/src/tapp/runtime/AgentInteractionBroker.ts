import type { AgentInteractionV2, TappInstance } from '../types'
import type { TappBridge } from './TappBridge'
import { currentCopy } from '../../i18n/localeCopy'
import { executeFrontendAction } from '../../services/agent'
import * as TappApiService from '../services/TappApiService'
import { userFacingError } from '../../utils/userFacingError'
import { requestDataExchangeFromHost } from './DataExchangeBroker'

const RECONNECT_DELAY_MS = 500

function objectParams(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('Intent params must be an object')
  }
  return value as Record<string, unknown>
}

async function executeHostIntent(
  bridge: TappBridge,
  tappInstance: TappInstance,
  type: string,
  rawParams: unknown,
): Promise<unknown> {
  const params = objectParams(rawParams)
  switch (type) {
    case 'ui.open': {
      const tappId = params.tappId
      if (typeof tappId !== 'string' || !/^[\w.-]{1,128}$/.test(tappId)) {
        throw new Error('ui.open requires a valid tappId')
      }
      const result = await executeFrontendAction({
        type: 'open_window',
        tappId,
        timestamp: Date.now(),
      })
      if (result === null) {
        throw new Error(currentCopy().errors.operationFailed)
      }
      return result
    }
    case 'report.create': {
      const title = params.title
      const reportType = params.reportType
      if (
        typeof title !== 'string' ||
        title.trim().length === 0 ||
        title.length > 200 ||
        !['platform', 'custom'].includes(String(reportType))
      ) {
        throw new Error('report.create requires title and a valid reportType')
      }
      return TappApiService.createTappReport(
        {
          tappId: tappInstance.id,
          title,
          reportType: reportType as 'platform' | 'custom',
          content: params.content ?? null,
          metadata: params.metadata,
        },
        await bridge.getRuntimeGrant(),
      )
    }
    case 'dataExchange.request': {
      const targetTappId = params.targetTappId
      const exportId = params.exportId
      const purpose = params.purpose
      if (
        typeof targetTappId !== 'string' ||
        typeof exportId !== 'string' ||
        typeof purpose !== 'string'
      ) {
        throw new TypeError(
          'dataExchange.request requires targetTappId, exportId and purpose',
        )
      }
      // This adapter deliberately delegates confirmation to DataExchangeBroker
      // so a cross-Tapp read has exactly one detailed, one-shot consent popup.
      return requestDataExchangeFromHost(bridge, {
        targetTappId,
        exportId,
        params: params.params,
        purpose,
      })
    }
    default:
      throw new Error(`Unsupported host intent: ${type}`)
  }
}

export function registerAgentInteractionHandlers(
  bridge: TappBridge,
  tappInstance: TappInstance,
): () => void {
  let stopped = false
  let streamController: AbortController | null = null

  bridge.registerHandler('agent.v2.accept', async (message) => {
    const [interactionId] = (message.payload as { args: unknown[] }).args || []
    try {
      const result = await TappApiService.acceptAgentInteraction(
        String(interactionId || ''),
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

  bridge.registerHandler('agent.v2.result', async (message) => {
    const [interactionId, result] =
      (message.payload as { args: unknown[] }).args || []
    try {
      const value = await TappApiService.submitAgentInteractionResult(
        String(interactionId || ''),
        result as { data: unknown; summary?: string; idempotencyKey: string },
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: value }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('agent.v2.reject', async (message) => {
    const [interactionId, reason] =
      (message.payload as { args: unknown[] }).args || []
    try {
      const value = await TappApiService.rejectAgentInteraction(
        String(interactionId || ''),
        String(reason || ''),
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data: value }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  bridge.registerHandler('agent.v2.intent', async (message) => {
    const [interactionId, request] =
      (message.payload as { args: unknown[] }).args || []
    const intent = (request || {}) as {
      type?: string
      params?: unknown
      reason?: string
    }
    const confirmed =
      intent.type === 'dataExchange.request' ||
      window.confirm(
        currentCopy()
          .tapp.agentIntentConfirm.replace(
            '{name}',
            tappInstance.manifest.name,
          )
          .replace('{type}', intent.type || 'unknown')
          .replace('{reason}', intent.reason || ''),
      )
    if (!confirmed) return { success: false, error: 'User denied Agent intent' }
    try {
      const authorization = await TappApiService.requestAgentIntent(
        String(interactionId || ''),
        {
          type: String(intent.type || ''),
          params: intent.params,
          reason: String(intent.reason || ''),
        },
        await bridge.getRuntimeGrant(),
      )
      const result = await executeHostIntent(
        bridge,
        tappInstance,
        String(intent.type || ''),
        intent.params,
      )
      return {
        success: true,
        data: { authorization, status: 'executed', executed: true, result },
      }
    } catch (error) {
      return {
        success: false,
        error: userFacingError(error),
      }
    }
  })

  if (tappInstance.manifest.agent?.protocolVersion === 2) {
    void (async () => {
      for (;;) {
        if (stopped) break
        streamController = new AbortController()
        try {
          await TappApiService.streamAgentInteractions(
            await bridge.getRuntimeGrant(),
            (interaction: AgentInteractionV2) =>
              bridge.emit('agentInteractionV2', interaction),
            streamController.signal,
          )
        } catch (error) {
          if (!stopped && !streamController.signal.aborted) {
            console.warn(
              '[TappAgentBroker] Interaction stream disconnected',
              error,
            )
          }
        } finally {
          streamController = null
        }
        if (!stopped) {
          await new Promise((resolve) =>
            setTimeout(resolve, RECONNECT_DELAY_MS),
          )
        }
      }
    })()
  }

  return () => {
    stopped = true
    streamController?.abort()
    streamController = null
  }
}
