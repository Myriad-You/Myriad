import type { TappKernelHost } from './kernel'
import type { TappFederationLimits, TappMessageTarget } from './TappBridge'
import { TappKernel } from './kernel'

export const TEST_FEDERATION_LIMITS: TappFederationLimits = {
  messagePayloadBytes: 4 * 1024 * 1024,
  noteImageBytes: 32 * 1024 * 1024,
  noteVideoBytes: 256 * 1024 * 1024,
}

export interface TestTappKernel {
  kernel: TappKernel
  /** Deliver a message as the host window would. */
  dispatch: (event: MessageEvent) => void
  /** Live federation limits the bridges read; tests may change them. */
  federationLimits: TappFederationLimits
}

/** A kernel on a stub host: grants always issue, quota always allows, nothing reaches the network. */
export function createTestTappKernel(
  overrides: Partial<TappKernelHost> = {},
): TestTappKernel {
  const listeners = new Set<(event: MessageEvent) => void>()
  const messageTarget: TappMessageTarget = {
    addEventListener: (_type, listener) => listeners.add(listener),
    removeEventListener: (_type, listener) => listeners.delete(listener),
  }
  const federationLimits = { ...TEST_FEDERATION_LIMITS }
  let issued = 0
  const kernel = new TappKernel({
    grants: {
      issue: async (tappId, instanceId, kind) => {
        issued += 1
        return {
          version: 2,
          token: `grant-${issued}`,
          runtimeId: `runtime-${issued}`,
          tappId,
          ownerId: 1,
          subjectId: 1,
          instanceId,
          kind,
          permissions: [],
          expiresAt: new Date(Date.now() + 3_600_000).toISOString(),
        }
      },
      revoke: async () => {},
      authorize: async () => {},
    },
    fetchResources: async () => {
      throw new Error('no package resources in tests')
    },
    fetchRuntimeModule: async () => ({ ok: false, status: 404, text: async () => '' }),
    quota: {
      checkQuota: () => ({ allowed: true, remaining: Number.POSITIVE_INFINITY }),
      recordUsage: () => {},
    },
    federation: {
      limits: () => federationLimits,
      envelopeBytes: () => federationLimits.messagePayloadBytes + 64 * 1024,
      refresh: () => {},
    },
    describeError: String,
    assetOrigin: () => '',
    cspNonce: () => undefined,
    messageTarget,
    ...overrides,
  })
  return {
    kernel,
    dispatch: (event) => {
      for (const listener of listeners) listener(event)
    },
    federationLimits,
  }
}
