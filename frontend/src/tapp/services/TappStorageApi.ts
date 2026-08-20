/** Tapp host settings, install-level shared data, and subject-private storage. */

import { apiRequest } from './TappHttpClient'

/** Host settings editor. Only manifest-declared keys are accepted by backend. */
export async function getTappSettings(
  tappId: string,
): Promise<Record<string, unknown>> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/settings`)
}

export async function getTappSetting(
  tappId: string,
  key: string,
): Promise<unknown> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/settings/${encodeURIComponent(key)}`,
  )
}

export async function setTappSetting(
  tappId: string,
  key: string,
  value: unknown,
): Promise<void> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/settings/${encodeURIComponent(key)}`,
    { method: 'POST', body: JSON.stringify(value) },
  )
}

/** Install-owner namespace. Visitors can read; only owner/admin can write. */
export async function getShared(
  tappId: string,
  key: string,
): Promise<unknown> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/shared/${encodeURIComponent(key)}`,
  )
}

export async function setShared(
  tappId: string,
  key: string,
  value: unknown,
): Promise<void> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/shared/${encodeURIComponent(key)}`,
    { method: 'POST', body: JSON.stringify(value) },
  )
}

export async function removeShared(
  tappId: string,
  key: string,
): Promise<void> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/shared/${encodeURIComponent(key)}`,
    { method: 'DELETE' },
  )
}

export async function listSharedKeys(tappId: string): Promise<string[]> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/shared`)
}

export async function listSharedEntries(
  tappId: string,
): Promise<Record<string, unknown>> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/shared/entries`)
}

export async function clearShared(tappId: string): Promise<void> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/shared`, {
    method: 'DELETE',
  })
}

export async function getSharedUsage(
  tappId: string,
): Promise<{ used: number; quota: number }> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/shared/usage`)
}

export async function getStorage(
  tappId: string,
  key: string,
  runtimeGrant?: string,
): Promise<unknown> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/storage/${encodeURIComponent(key)}`,
    { runtimeGrant },
  )
}

export async function setStorage(
  tappId: string,
  key: string,
  value: unknown,
  runtimeGrant?: string,
): Promise<void> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/storage/${encodeURIComponent(key)}`,
    {
      method: 'POST',
      body: JSON.stringify(value),
      runtimeGrant,
    },
  )
}

export async function removeStorage(
  tappId: string,
  key: string,
  runtimeGrant?: string,
): Promise<void> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/storage/${encodeURIComponent(key)}`,
    {
      method: 'DELETE',
      runtimeGrant,
    },
  )
}

export async function listStorageKeys(
  tappId: string,
  runtimeGrant?: string,
): Promise<string[]> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/storage`, {
    runtimeGrant,
  })
}

export async function listStorageEntries(
  tappId: string,
  runtimeGrant?: string,
): Promise<Record<string, unknown>> {
  return apiRequest(
    `/api/tapps/${encodeURIComponent(tappId)}/storage/entries`,
    { runtimeGrant },
  )
}

export async function clearStorage(
  tappId: string,
  runtimeGrant?: string,
): Promise<void> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/storage`, {
    method: 'DELETE',
    runtimeGrant,
  })
}

export async function getStorageUsage(
  tappId: string,
  runtimeGrant?: string,
): Promise<{ used: number; quota: number }> {
  return apiRequest(`/api/tapps/${encodeURIComponent(tappId)}/storage/usage`, {
    runtimeGrant,
  })
}
