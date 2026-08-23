/**
 * Tripo 3D handlers. Generation is grant-gated; getUrl/getMetadata read
 * already-public persisted assets through the host (sandbox CSP blocks fetch).
 */

import type { TappBridge } from '../../TappBridge'
import { userFacingError } from '../../../../utils/userFacingError'
import * as TappApiService from '../../../services/TappApiService'

const ASSET_ID = /^[0-9a-f]{64}$/i

function argsOf(message: { payload?: unknown }): unknown[] {
  const payload = message.payload
  if (!payload || typeof payload !== 'object') return []
  const args = (payload as { args?: unknown }).args
  return Array.isArray(args) ? args : []
}

function fail(error: unknown, fallback: string) {
  return {
    success: false,
    error: userFacingError(error, fallback),
  }
}

async function readPublicAsset(
  assetId: string,
  metadata: boolean,
): Promise<{ base64: string; mimeType: string; size: number; assetId: string }> {
  if (!ASSET_ID.test(assetId)) {
    throw new Error('Invalid 3D asset id')
  }
  const suffix = metadata ? '/metadata' : ''
  const response = await fetch(
    `/api/digital-life/3d/assets/${encodeURIComponent(assetId)}${suffix}`,
  )
  if (!response.ok) {
    throw new Error(
      response.status === 404 ? '3D asset not found' : 'Could not read 3D asset',
    )
  }
  const buffer = await response.arrayBuffer()
  const bytes = new Uint8Array(buffer)
  let binary = ''
  const chunk = 0x8000
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk))
  }
  return {
    base64: btoa(binary),
    mimeType: metadata ? 'application/json' : 'model/gltf-binary',
    size: bytes.length,
    assetId,
  }
}

export function registerModel3dHandlers(bridge: TappBridge): void {
  bridge.registerHandler('model3d.status', async () => {
    try {
      const data = await TappApiService.getModel3dStatus(
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data }
    } catch (error) {
      return fail(error, '3D status failed')
    }
  })

  bridge.registerHandler('model3d.upload', async (message) => {
    const [request] = argsOf(message)
    if (!request || typeof request !== 'object') {
      return { success: false, error: 'Upload request required' }
    }
    try {
      const data = await TappApiService.uploadModel3dFile(
        request as {
          fileName: string
          contentType: string
          base64: string
        },
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data }
    } catch (error) {
      return fail(error, '3D upload failed')
    }
  })

  bridge.registerHandler('model3d.createTask', async (message) => {
    const [request] = argsOf(message)
    if (!request || typeof request !== 'object') {
      return { success: false, error: 'Task request required' }
    }
    try {
      const data = await TappApiService.createModel3dTask(
        request as { operation: string; payload?: unknown },
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data }
    } catch (error) {
      return fail(error, '3D task creation failed')
    }
  })

  bridge.registerHandler('model3d.getTask', async (message) => {
    const [taskId] = argsOf(message)
    if (typeof taskId !== 'string') {
      return { success: false, error: 'Task ID required' }
    }
    try {
      const data = await TappApiService.getModel3dTask(
        taskId,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data }
    } catch (error) {
      return fail(error, '3D task lookup failed')
    }
  })

  bridge.registerHandler('model3d.awaitTask', async (message) => {
    const [taskId] = argsOf(message)
    if (typeof taskId !== 'string') {
      return { success: false, error: 'Task ID required' }
    }
    try {
      const data = await TappApiService.awaitModel3dTask(
        taskId,
        await bridge.getRuntimeGrant(),
      )
      return { success: true, data }
    } catch (error) {
      return fail(error, '3D task await failed')
    }
  })

  bridge.registerHandler('model3d.getUrl', async (message) => {
    const [assetId] = argsOf(message)
    if (typeof assetId !== 'string') {
      return { success: false, error: 'Asset ID required' }
    }
    try {
      return { success: true, data: await readPublicAsset(assetId, false) }
    } catch (error) {
      return fail(error, '3D asset load failed')
    }
  })

  bridge.registerHandler('model3d.getMetadata', async (message) => {
    const [assetId] = argsOf(message)
    if (typeof assetId !== 'string') {
      return { success: false, error: 'Asset ID required' }
    }
    try {
      const asset = await readPublicAsset(assetId, true)
      const json = JSON.parse(atob(asset.base64)) as unknown
      return { success: true, data: json }
    } catch (error) {
      return fail(error, '3D metadata load failed')
    }
  })
}
