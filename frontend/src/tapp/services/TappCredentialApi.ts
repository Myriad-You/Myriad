/** Installation-manager API for write-only Tapp credentials. */

import { apiRequest } from './TappHttpClient'

export interface TappCredentialBindingSummary {
  api: string
  method: string
  endpoint: string
  access: string
  placement: string
  field: string
  signAlg?: string
  signOver?: string[]
}

export interface TappCredentialStatus {
  key: string
  configured: boolean
  needsReauthorization: boolean
  origins: string[]
  bindings?: TappCredentialBindingSummary[]
  updatedAt?: string
}

export async function getTappCredentialStatuses(
  tappId: string,
): Promise<TappCredentialStatus[]> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/credentials`)
}

export async function setTappCredential(
  tappId: string,
  key: string,
  value: string,
): Promise<void> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/credentials/${encodeURIComponent(key)}`,
    { method: 'POST', body: JSON.stringify({ value }) },
  )
}

export async function removeTappCredential(
  tappId: string,
  key: string,
): Promise<void> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/credentials/${encodeURIComponent(key)}`,
    { method: 'DELETE' },
  )
}
