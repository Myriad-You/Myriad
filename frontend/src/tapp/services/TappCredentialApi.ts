/** 安装管理员 API；凭据只写。 */

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

export interface TappInboundBlock {
  fingerprint: string
  source: string
  scope: string
}

export interface TappInboundGuardStatus {
  paused: boolean
  blocks: TappInboundBlock[]
}

export async function getTappInboundGuard(
  tappId: string,
): Promise<TappInboundGuardStatus> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/inbound-guard`)
}

export async function pauseTappInbound(tappId: string): Promise<void> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/inbound-guard/pause`,
    { method: 'POST', body: JSON.stringify({}) },
  )
}

export async function resumeTappInbound(tappId: string): Promise<void> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/inbound-guard/pause`,
    { method: 'DELETE' },
  )
}

export async function unblockTappInbound(
  tappId: string,
  fingerprint: string,
): Promise<void> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/inbound-guard/blocks/${encodeURIComponent(fingerprint)}`,
    { method: 'DELETE' },
  )
}
