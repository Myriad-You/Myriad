import { federationApi } from '../../../../services/federationApi'
import {
  areFederationPoliciesEqual,
  DEFAULT_FEDERATION_POLICY,
  federationPolicyFromApi,
  federationPolicyToUpdateRequest,
} from '../../federationPolicy'
import { useConfigDomain } from '../useConfigDomain'

export function useFederationDomain(
  isAdmin: boolean,
  federationEnabled = true,
) {
  const visible = isAdmin && federationEnabled
  return useConfigDomain({
    id: 'federation',
    sections: ['federation'],
    initial: DEFAULT_FEDERATION_POLICY,
    equal: areFederationPoliciesEqual,
    load: async () =>
      visible
        ? federationPolicyFromApi(await federationApi.getTrustPolicy())
        : DEFAULT_FEDERATION_POLICY,
    persist: async (draft) => {
      if (!visible) throw new Error('Administrator required')
      await federationApi.updateTrustPolicy(
        federationPolicyToUpdateRequest(draft),
      )
      return draft
    },
    reset: (_saved, scope) =>
      visible && (scope === 'federation' || scope === 'all')
        ? structuredClone(DEFAULT_FEDERATION_POLICY)
        : undefined,
  })
}
