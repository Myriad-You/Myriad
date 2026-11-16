import { TappKernel } from '@myriad/tapp-runtime'
import { userFacingError } from '../utils/userFacingError'
import {
  federationLiveLimits,
  federationMessageEnvelopeBytes,
  refreshFederationLimits,
} from './runtime/federationLimits'
import { getQuotaManager } from './services/QuotaManager'
import { getTappResources } from './services/TappPackageResourceApi'
import {
  authorizeTappRuntimePermission,
  issueTappRuntimeGrant,
  revokeTappRuntimeGrant,
} from './services/TappRuntimeAccessApi'

/** 站点这一页唯一的 Tapp 内核：宿主相关的一切（接口、源站、nonce、文案）都在这里交给它。 */
export const tappKernel = new TappKernel({
  grants: {
    issue: (tappId, instanceId, kind) =>
      issueTappRuntimeGrant(tappId, instanceId, kind),
    revoke: (tappId, runtimeId) => revokeTappRuntimeGrant(tappId, runtimeId),
    authorize: (tappId, permission, token) =>
      authorizeTappRuntimePermission(tappId, permission, token),
  },
  fetchResources: (tappId, options) => getTappResources(tappId, options),
  fetchRuntimeModule: (path) => fetch(path, { credentials: 'same-origin' }),
  quota: {
    checkQuota: (tappId, action) => getQuotaManager().checkQuota(tappId, action),
    recordUsage: (tappId, action) => getQuotaManager().recordUsage(tappId, action),
  },
  federation: {
    limits: federationLiveLimits,
    envelopeBytes: federationMessageEnvelopeBytes,
    refresh: () => {
      void refreshFederationLimits()
    },
  },
  describeError: userFacingError,
  assetOrigin: () => (typeof window === 'undefined' ? '' : window.location.origin),
  cspNonce: () =>
    typeof document === 'undefined'
      ? undefined
      : document.querySelector<HTMLMetaElement>('meta[property=csp-nonce]')?.nonce,
  messageTarget: typeof window === 'undefined' ? null : window,
})
