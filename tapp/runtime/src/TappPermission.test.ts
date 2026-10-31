import type { TappManifest, TappPermission } from './types'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { TappKernelError } from './kernelErrors.ts'
import { TappPermissionController } from './TappPermission.ts'

function manifest(permissions: string[]): TappManifest {
  return {
    id: 'com.example.permission-test',
    name: 'Permission Test',
    version: '1.0.0',
    core: { entry: 'core.js' },
    permissions: permissions as TappPermission[],
    category: 'utility',
  } as TappManifest
}

describe('manifest permission check', () => {
  it('reports refusals as codes, worded by the host', () => {
    const result = TappPermissionController.validateManifestPermissions(
      manifest(['storage:read', 'made:up']),
    )
    assert.equal(result.valid, false)
    assert.deepEqual(result.errors, [
      { code: 'unknownPermission', params: { permission: 'made:up' } },
    ])
  })

  it('warns on data write together with AI generation', () => {
    const result = TappPermissionController.validateManifestPermissions(
      manifest(['platform:write', 'ai:generate']),
    )
    assert.equal(result.valid, true)
    assert.deepEqual(result.warnings, [
      { code: 'sensitivePermissionCombo', params: {} },
    ])
  })
})

describe('kernel error', () => {
  it('keeps the code and params, with English text as its message', () => {
    const error = new TappKernelError('runtimeModuleHttp', { status: 502, id: 'three' })
    assert.equal(error.code, 'runtimeModuleHttp')
    assert.deepEqual(error.params, { status: 502, id: 'three' })
    assert.equal(error.message, 'HTTP 502 while loading runtime module three')
  })
})
