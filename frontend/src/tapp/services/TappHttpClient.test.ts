/* eslint-disable test/no-import-node-test -- node:test is the repository test runner */

import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { apiRequest } from './TappHttpClient.ts'

const originalFetch = globalThis.fetch

afterEach(() => {
  globalThis.fetch = originalFetch
})

describe('TappHttpClient response contract', () => {
  it('unwraps the standard success/data envelope', async () => {
    let credentials: RequestCredentials | undefined
    globalThis.fetch = async (_input, init) => {
      credentials = init?.credentials
      return Response.json({ success: true, data: { id: 'demo' } })
    }

    const result = await apiRequest<{ id: string }>('/api/tapps/demo')

    assert.deepEqual(result, { id: 'demo' })
    assert.equal(credentials, 'include')
  })

  it('preserves successful unwrapped response bodies', async () => {
    globalThis.fetch = async () => Response.json({ items: [1, 2, 3] })

    assert.deepEqual(await apiRequest('/api/tapps'), { items: [1, 2, 3] })
  })
})
