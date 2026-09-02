import type { OneShotDataAccessGrant } from '../services/TappApiService'
import type { TappInstance, TappMessage } from '../types'
import type { TappBridge } from './TappBridge'
import { userFacingError } from '../../utils/userFacingError'
import {
  authorizeDataExchange,
  cancelDataExchange,
  consumeDataExchange,
  prepareDataExchange,
} from '../services/TappApiService'
import { requestDataExchangeConsent } from './DataExchangeConsent'

const PROVIDER_TIMEOUT_MS = 30_000
const MAX_ACTIVE_REQUESTS_PER_RUNTIME = 3
const DATA_EXCHANGE_ID = /^[\w.-]{1,128}$/

interface RuntimeRegistration {
  bridge: TappBridge
  instance: TappInstance
  exports: Set<string>
  activeRequests: number
}

interface PendingConsentInvocation {
  requester: RuntimeRegistration
  provider: RuntimeRegistration
  exportId: string
  controller: AbortController
}

interface DataExchangeRequest {
  targetTappId: string
  exportId: string
  params?: unknown
  purpose: string
}

interface ProviderResponse {
  requestId: string
  ok: boolean
  data?: unknown
  error?: string
}

interface PendingInvocation {
  requester: RuntimeRegistration
  provider: RuntimeRegistration
  requesterRuntimeGrant: string
  access: OneShotDataAccessGrant
  resolve: (value: unknown) => void
  reject: (error: Error) => void
  timeout: ReturnType<typeof setTimeout>
}

function getArgs(message: TappMessage): unknown[] {
  return (message.payload as { args?: unknown[] } | undefined)?.args ?? []
}

class DataExchangeBroker {
  private readonly runtimes = new Set<RuntimeRegistration>()
  private readonly pendingConsents = new Set<PendingConsentInvocation>()
  private readonly pending = new Map<string, PendingInvocation>()

  register(bridge: TappBridge, instance: TappInstance): () => void {
    const runtime: RuntimeRegistration = {
      bridge,
      instance,
      exports: new Set(),
      activeRequests: 0,
    }
    this.runtimes.add(runtime)

    bridge.registerHandler('dataExchange.registerProvider', async (message) => {
      const [exportId] = getArgs(message) as [string]
      if (!DATA_EXCHANGE_ID.test(exportId || '')) {
        return { success: false, error: 'Invalid Data Exchange export id' }
      }
      const declared = instance.manifest.dataExchange?.exports?.some(
        (item) => item.id === exportId,
      )
      if (!declared) {
        return {
          success: false,
          error: `Data Exchange export is not declared: ${exportId}`,
        }
      }
      runtime.exports.add(exportId)
      return { success: true, data: null }
    })

    bridge.registerHandler(
      'dataExchange.unregisterProvider',
      async (message) => {
        const [exportId] = getArgs(message) as [string]
        runtime.exports.delete(exportId)
        for (const consent of this.pendingConsents) {
          if (consent.provider === runtime && consent.exportId === exportId) {
            consent.controller.abort()
            this.pendingConsents.delete(consent)
          }
        }
        return { success: true, data: null }
      },
    )

    bridge.registerHandler('dataExchange.request', async (message) => {
      const [request] = getArgs(message) as [DataExchangeRequest]
      try {
        const data = await this.request(runtime, request)
        return { success: true, data }
      } catch (error) {
        return {
          success: false,
          error:
            userFacingError(error),
        }
      }
    })

    bridge.registerHandler('dataExchange.respond', async (message) => {
      const [response] = getArgs(message) as [ProviderResponse]
      try {
        await this.respond(runtime, response)
        return { success: true, data: null }
      } catch (error) {
        return {
          success: false,
          error:
            userFacingError(error),
        }
      }
    })

    return () => {
      this.runtimes.delete(runtime)
      for (const consent of this.pendingConsents) {
        if (consent.requester === runtime || consent.provider === runtime) {
          consent.controller.abort()
          this.pendingConsents.delete(consent)
        }
      }
      for (const [requestId, invocation] of this.pending) {
        if (
          invocation.provider === runtime ||
          invocation.requester === runtime
        ) {
          clearTimeout(invocation.timeout)
          invocation.reject(new Error('Data Exchange runtime stopped'))
          this.pending.delete(requestId)
          void cancelDataExchange(
            requestId,
            invocation.requesterRuntimeGrant,
          ).catch(() => {})
        }
      }
      bridge.unregisterHandler('dataExchange.registerProvider')
      bridge.unregisterHandler('dataExchange.unregisterProvider')
      bridge.unregisterHandler('dataExchange.request')
      bridge.unregisterHandler('dataExchange.respond')
    }
  }

  async requestForHost(
    bridge: TappBridge,
    request: DataExchangeRequest,
  ): Promise<unknown> {
    const requester = [...this.runtimes].find(
      (runtime) => runtime.bridge === bridge,
    )
    if (!requester) {
      throw new Error('Requester runtime is not registered')
    }
    return this.request(requester, request)
  }

  private async findProvider(
    targetTappId: string,
    exportId: string,
    providerOwnerId: number,
  ): Promise<RuntimeRegistration | undefined> {
    const candidates = [...this.runtimes].filter(
      (runtime) =>
        runtime.instance.id === targetTappId && runtime.exports.has(exportId),
    )
    for (const candidate of candidates) {
      try {
        if ((await candidate.bridge.getRuntimeOwnerId()) === providerOwnerId) {
          return candidate
        }
      } catch {
        // A runtime may disappear while a prepared request is being matched.
        // Continue to any other online instance of the same installation.
      }
    }
    return undefined
  }

  private async request(
    requester: RuntimeRegistration,
    request: DataExchangeRequest,
  ): Promise<unknown> {
    if (
      !request ||
      typeof request !== 'object' ||
      !DATA_EXCHANGE_ID.test(request.targetTappId || '') ||
      !DATA_EXCHANGE_ID.test(request.exportId || '') ||
      typeof request.purpose !== 'string' ||
      request.purpose.trim().length === 0 ||
      request.purpose.length > 500
    ) {
      throw new Error('Invalid Data Exchange request')
    }

    if (requester.activeRequests >= MAX_ACTIVE_REQUESTS_PER_RUNTIME) {
      throw new Error('Too many active Data Exchange requests')
    }
    requester.activeRequests += 1

    try {
      const runtimeGrant = await requester.bridge.getRuntimeGrant()
      const prepared = await prepareDataExchange(
        {
          targetTappId: request.targetTappId,
          exportId: request.exportId,
          params: request.params ?? null,
          purpose: request.purpose,
        },
        runtimeGrant,
      )

      const provider = await this.findProvider(
        request.targetTappId,
        request.exportId,
        prepared.providerOwnerId,
      )
      if (!provider) {
        await cancelDataExchange(prepared.requestId, runtimeGrant).catch(
          () => {},
        )
        throw new Error(
          `Data provider is not running: ${request.targetTappId}/${request.exportId}`,
        )
      }

      const consentController = new AbortController()
      const pendingConsent: PendingConsentInvocation = {
        requester,
        provider,
        exportId: request.exportId,
        controller: consentController,
      }
      this.pendingConsents.add(pendingConsent)
      const consent = await requestDataExchangeConsent(
        prepared,
        consentController.signal,
      ).finally(() => this.pendingConsents.delete(pendingConsent))
      if (consent !== 'allow') {
        await cancelDataExchange(prepared.requestId, runtimeGrant).catch(
          () => {},
        )
        if (consent === 'expired') {
          throw new Error('Data Exchange authorization expired')
        }
        if (consent === 'cancelled') {
          throw new Error('Data Exchange runtime stopped')
        }
        throw new Error('Data Exchange request was denied')
      }
      if (!this.runtimes.has(requester)) {
        await cancelDataExchange(prepared.requestId, runtimeGrant).catch(
          () => {},
        )
        throw new Error('Data Exchange requester runtime stopped')
      }
      if (
        !this.runtimes.has(provider) ||
        !provider.exports.has(request.exportId)
      ) {
        await cancelDataExchange(prepared.requestId, runtimeGrant).catch(
          () => {},
        )
        throw new Error('Data provider stopped before authorization completed')
      }

      const access = await authorizeDataExchange(
        prepared.requestId,
        runtimeGrant,
      )

      const result = await new Promise<unknown>((resolve, reject) => {
        const timeout = setTimeout(() => {
          this.pending.delete(prepared.requestId)
          void cancelDataExchange(prepared.requestId, runtimeGrant).catch(
            () => {},
          )
          reject(new Error('Data provider response timed out'))
        }, PROVIDER_TIMEOUT_MS)
        this.pending.set(prepared.requestId, {
          requester,
          provider,
          requesterRuntimeGrant: runtimeGrant,
          access,
          resolve,
          reject,
          timeout,
        })
        provider.bridge.emit('dataExchange:invoke', {
          requestId: prepared.requestId,
          exportId: prepared.exportId,
          params: access.params,
          purpose: access.purpose,
        })
      })
      return result
    } finally {
      requester.activeRequests = Math.max(0, requester.activeRequests - 1)
    }
  }

  private async respond(
    provider: RuntimeRegistration,
    response: ProviderResponse,
  ): Promise<void> {
    if (!response || typeof response.requestId !== 'string') {
      throw new Error('Invalid provider response')
    }
    const invocation = this.pending.get(response.requestId)
    if (!invocation || invocation.provider !== provider) {
      throw new Error(
        'Data Exchange request is missing or belongs to another runtime',
      )
    }
    clearTimeout(invocation.timeout)
    this.pending.delete(response.requestId)

    const providerRuntimeGrant = await provider.bridge.getRuntimeGrant()
    if (!response.ok) {
      // Consume with a deliberately invalid response so provider failures also
      // exhaust the one-shot token. The original provider error is preserved.
      await consumeDataExchange(
        invocation.access.token,
        null,
        providerRuntimeGrant,
      ).catch(() => {})
      invocation.reject(
        new Error(response.error || 'Data provider failed to produce a result'),
      )
      return
    }

    try {
      const result = await consumeDataExchange(
        invocation.access.token,
        response.data,
        providerRuntimeGrant,
      )
      invocation.resolve(result)
    } catch (error) {
      invocation.reject(
        error instanceof Error ? error : new Error('Data response rejected'),
      )
      throw error
    }
  }
}

const broker = new DataExchangeBroker()

export function registerDataExchangeHandlers(
  bridge: TappBridge,
  instance: TappInstance,
): () => void {
  return broker.register(bridge, instance)
}

/** Trusted host adapter used by an authorized Agent Interaction intent. */
export function requestDataExchangeFromHost(
  bridge: TappBridge,
  request: DataExchangeRequest,
): Promise<unknown> {
  return broker.requestForHost(bridge, request)
}
