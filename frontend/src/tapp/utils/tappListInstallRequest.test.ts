import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { resolveTappListInstallRequest } from './tappListInstallRequest.ts'

const TAPP_ID = 'com.example.app'
const CATALOG_URL =
  'https://raw.githubusercontent.com/Myriad-You/tapp-store/main/index.json'

describe('resolveTappListInstallRequest (shipped install shape)', () => {
  it('rejects bare numeric source without storeSource', () => {
    const r = resolveTappListInstallRequest({
      source: '1',
      tappId: TAPP_ID,
    })
    assert.equal(r.kind, 'error')
    if (r.kind === 'error') {
      assert.match(r.error, /Invalid source|storeSource/i)
    }
  })

  it('accepts source:store + storeSource id', () => {
    const r = resolveTappListInstallRequest({
      source: 'store',
      storeSource: '1',
      tappId: TAPP_ID,
      permissions: ['storage:read', 'storage:write'],
    })
    assert.equal(r.kind, 'store')
    if (r.kind === 'store') {
      assert.equal(r.catalogRef, '1')
      assert.equal(r.tappId, TAPP_ID)
      assert.deepEqual(r.permissions, ['storage:read', 'storage:write'])
    }
  })

  it('accepts HTTP source as catalog URL without storeSource', () => {
    const r = resolveTappListInstallRequest({
      source: CATALOG_URL,
      tappId: TAPP_ID,
    })
    assert.equal(r.kind, 'store')
    if (r.kind === 'store') {
      assert.equal(r.catalogRef, CATALOG_URL)
      assert.equal(r.tappId, TAPP_ID)
    }
  })

  it('prefers explicit storeSource over HTTP source for catalogRef', () => {
    const r = resolveTappListInstallRequest({
      source: CATALOG_URL,
      storeSource: '2',
      tappId: TAPP_ID,
    })
    assert.equal(r.kind, 'store')
    if (r.kind === 'store') {
      assert.equal(r.catalogRef, '2')
    }
  })

  it('rejects store mode without catalog (source:store alone)', () => {
    const r = resolveTappListInstallRequest({
      source: 'store',
      tappId: TAPP_ID,
    })
    assert.equal(r.kind, 'error')
    if (r.kind === 'error') {
      assert.match(r.error, /storeSource/i)
    }
  })

  it('rejects storeSource that is install-mode placeholder', () => {
    for (const bad of ['store', 'direct', 'STORE']) {
      const r = resolveTappListInstallRequest({
        source: 'store',
        storeSource: bad,
        tappId: TAPP_ID,
      })
      assert.equal(r.kind, 'error', `expected error for storeSource=${bad}`)
    }
  })

  it('requires tappId for store path', () => {
    const r = resolveTappListInstallRequest({
      source: 'store',
      storeSource: '1',
    })
    assert.equal(r.kind, 'error')
    if (r.kind === 'error') {
      assert.match(r.error, /tappId/i)
    }
  })

  it('accepts direct with manifest + modules', () => {
    const r = resolveTappListInstallRequest({
      source: 'direct',
      manifest: {
        id: TAPP_ID,
        name: 'App',
        version: '1.0.0',
        category: 'utility',
        core: { entry: 'core.js' },
        permissions: [],
      },
      modules: { 'core.js': 'console.log(1)' },
      permissions: ['storage:read', 'storage:write'],
    })
    assert.equal(r.kind, 'direct')
    if (r.kind === 'direct') {
      assert.equal(r.modules['core.js'], 'console.log(1)')
      assert.equal((r.manifest as { id: string }).id, TAPP_ID)
    }
  })

  it('rejects direct without manifest or modules', () => {
    assert.equal(
      resolveTappListInstallRequest({
        source: 'direct',
        modules: { 'core.js': 'x' },
      }).kind,
      'error',
    )
    assert.equal(
      resolveTappListInstallRequest({
        source: 'direct',
        manifest: { id: TAPP_ID },
      }).kind,
      'error',
    )
    // 空模块表等于没给源码
    assert.equal(
      resolveTappListInstallRequest({
        source: 'direct',
        manifest: { id: TAPP_ID },
        modules: {},
      }).kind,
      'error',
    )
  })
})
