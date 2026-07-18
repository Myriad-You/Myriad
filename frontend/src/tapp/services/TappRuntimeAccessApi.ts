/** Runtime grants, federation access and one-shot data exchange contracts. */

import type { TimelineResponse } from '../../types/federation'
import type { PermissionLevel } from '../types'
import { apiRequest } from './TappHttpClient'

export type RuntimeGrantKind = 'page' | 'widget' | 'headless'

export interface TappRuntimeGrantResponse {
  version: 2
  token: string
  runtimeId: string
  tappId: string
  ownerId: number
  subjectId: number
  instanceId: string
  kind: RuntimeGrantKind
  permissions: string[]
  expiresAt: string
}

export async function issueTappRuntimeGrant(
  tappId: string,
  instanceId: string,
  kind: RuntimeGrantKind,
): Promise<TappRuntimeGrantResponse> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/runtime-grants`, {
    method: 'POST',
    body: JSON.stringify({ instanceId, kind }),
  })
}

export async function revokeTappRuntimeGrant(
  tappId: string,
  runtimeId: string,
): Promise<void> {
  await apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/runtime-grants/${encodeURIComponent(runtimeId)}`,
    { method: 'DELETE' },
  )
}

export async function authorizeTappRuntimePermission(
  tappId: string,
  permission: string,
  runtimeGrant: string,
): Promise<void> {
  await apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/runtime-grants/authorize`,
    {
      method: 'POST',
      body: JSON.stringify({ permission }),
      runtimeGrant,
    },
  )
}

export interface FederationFeedResponse extends TimelineResponse {
  audience: 'public' | 'public+personal'
}

export async function getFederationFeed(
  runtimeGrant: string,
): Promise<FederationFeedResponse> {
  return apiRequest('/api/tapp/federation/feed', { runtimeGrant })
}

export interface PrepareDataExchangeRequest {
  targetTappId: string
  exportId: string
  params?: unknown
  purpose: string
}

export interface PreparedDataExchange {
  requestId: string
  requesterTappId: string
  requesterName: string
  providerTappId: string
  providerOwnerId: number
  providerName: string
  exportId: string
  exportDescription?: string
  params: unknown
  purpose: string
  maxBytes: number
  maxRecords?: number
  expiresAt: string
}

export interface OneShotDataAccessGrant {
  version: 1
  grantId: string
  token: string
  requestId: string
  providerTappId: string
  providerOwnerId: number
  exportId: string
  params: unknown
  purpose: string
  requestHash: string
  maxBytes: number
  maxRecords?: number
  expiresAt: string
}

export async function prepareDataExchange(
  request: PrepareDataExchangeRequest,
  runtimeGrant: string,
): Promise<PreparedDataExchange> {
  return apiRequest('/api/tapp/data-exchange/requests', {
    method: 'POST',
    body: JSON.stringify(request),
    runtimeGrant,
  })
}

export async function authorizeDataExchange(
  requestId: string,
  runtimeGrant: string,
): Promise<OneShotDataAccessGrant> {
  return apiRequest(
    `/api/tapp/data-exchange/requests/${encodeURIComponent(requestId)}/authorize`,
    { method: 'POST', runtimeGrant },
  )
}

export async function cancelDataExchange(
  requestId: string,
  runtimeGrant: string,
): Promise<void> {
  await apiRequest(
    `/api/tapp/data-exchange/requests/${encodeURIComponent(requestId)}`,
    { method: 'DELETE', runtimeGrant },
  )
}

export async function consumeDataExchange(
  grantToken: string,
  response: unknown,
  providerRuntimeGrant: string,
): Promise<unknown> {
  return apiRequest('/api/tapp/data-exchange/consume', {
    method: 'POST',
    body: JSON.stringify({ grantToken, response }),
    runtimeGrant: providerRuntimeGrant,
  })
}

export async function getAllowedPermissionLevels(): Promise<PermissionLevel[]> {
  const response = await apiRequest<{ allowed_levels: PermissionLevel[] }>(
    '/api/config/permissions',
  )
  return response.allowed_levels
}
