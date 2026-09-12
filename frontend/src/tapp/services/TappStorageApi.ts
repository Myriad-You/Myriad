import { apiRequest } from './TappHttpClient'

function tappPath(tappId: string, scope: string, suffix = ''): string {
  return `/api/tapps/${encodeURIComponent(tappId)}/${scope}${suffix}`
}

function itemPath(tappId: string, scope: string, key: string): string {
  return tappPath(tappId, scope, `/${encodeURIComponent(key)}`)
}

/** 仅接受 Manifest 声明的键。 */
export async function getTappSettings(
  tappId: string,
): Promise<Record<string, unknown>> {
  return apiRequest(tappPath(tappId, 'settings'))
}

export async function getTappSetting(
  tappId: string,
  key: string,
): Promise<unknown> {
  return apiRequest(itemPath(tappId, 'settings', key))
}

export async function setTappSetting(
  tappId: string,
  key: string,
  value: unknown,
): Promise<void> {
  return apiRequest(itemPath(tappId, 'settings', key), {
    method: 'POST',
    body: JSON.stringify(value),
  })
}

type InstallKvScope = 'shared' | 'private'

function installKv(scope: InstallKvScope) {
  return {
    get(tappId: string, key: string): Promise<unknown> {
      return apiRequest(itemPath(tappId, scope, key))
    },
    set(tappId: string, key: string, value: unknown): Promise<void> {
      return apiRequest(itemPath(tappId, scope, key), {
        method: 'POST',
        body: JSON.stringify(value),
      })
    },
    remove(tappId: string, key: string): Promise<void> {
      return apiRequest(itemPath(tappId, scope, key), { method: 'DELETE' })
    },
    keys(tappId: string): Promise<string[]> {
      return apiRequest(tappPath(tappId, scope))
    },
    getAll(tappId: string): Promise<Record<string, unknown>> {
      return apiRequest(tappPath(tappId, scope, '/entries'))
    },
    clear(tappId: string): Promise<void> {
      return apiRequest(tappPath(tappId, scope), { method: 'DELETE' })
    },
    usage(tappId: string): Promise<{ used: number; quota: number }> {
      return apiRequest(tappPath(tappId, scope, '/usage'))
    },
  }
}

const sharedKv = installKv('shared')
const privateKv = installKv('private')

/** 安装 owner 命名空间。访客可读；仅 owner/管理员可写。 */
export const getShared = sharedKv.get
export const setShared = sharedKv.set
export const removeShared = sharedKv.remove
export const listSharedKeys = sharedKv.keys
export const listSharedEntries = sharedKv.getAll
export const clearShared = sharedKv.clear
export const getSharedUsage = sharedKv.usage

/** 安装 owner 命名空间。仅 owner/管理员；会话身份，无 Runtime Grant。 */
export const getPrivate = privateKv.get
export const setPrivate = privateKv.set
export const removePrivate = privateKv.remove
export const listPrivateKeys = privateKv.keys
export const listPrivateEntries = privateKv.getAll
export const clearPrivate = privateKv.clear
export const getPrivateUsage = privateKv.usage

export async function getStorage(
  tappId: string,
  key: string,
  runtimeGrant?: string,
): Promise<unknown> {
  return apiRequest(itemPath(tappId, 'storage', key), { runtimeGrant })
}

export async function setStorage(
  tappId: string,
  key: string,
  value: unknown,
  runtimeGrant?: string,
): Promise<void> {
  return apiRequest(itemPath(tappId, 'storage', key), {
    method: 'POST',
    body: JSON.stringify(value),
    runtimeGrant,
  })
}

export async function removeStorage(
  tappId: string,
  key: string,
  runtimeGrant?: string,
): Promise<void> {
  return apiRequest(itemPath(tappId, 'storage', key), {
    method: 'DELETE',
    runtimeGrant,
  })
}

export async function listStorageKeys(
  tappId: string,
  runtimeGrant?: string,
): Promise<string[]> {
  return apiRequest(tappPath(tappId, 'storage'), { runtimeGrant })
}

export async function listStorageEntries(
  tappId: string,
  runtimeGrant?: string,
): Promise<Record<string, unknown>> {
  return apiRequest(tappPath(tappId, 'storage', '/entries'), { runtimeGrant })
}

export async function clearStorage(
  tappId: string,
  runtimeGrant?: string,
): Promise<void> {
  return apiRequest(tappPath(tappId, 'storage'), {
    method: 'DELETE',
    runtimeGrant,
  })
}

export async function getStorageUsage(
  tappId: string,
  runtimeGrant?: string,
): Promise<{ used: number; quota: number }> {
  return apiRequest(tappPath(tappId, 'storage', '/usage'), { runtimeGrant })
}
