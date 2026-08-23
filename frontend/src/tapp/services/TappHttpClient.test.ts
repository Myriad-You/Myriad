import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'
import { apiRequest, TappHttpError } from './TappHttpClient.ts'

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

  it('throws a readable TappHttpError with the backend code', async () => {
    globalThis.fetch = async () =>
      Response.json(
        {
          error: 'Installation is read-only',
          code: 'GUEST_LAYOUT_READONLY',
        },
        { status: 403 },
      )

    await assert.rejects(
      () => apiRequest('/api/tapps/demo'),
      (error: unknown) => {
        assert.ok(error instanceof TappHttpError)
        assert.equal(error.status, 403)
        assert.equal(error.code, 'GUEST_LAYOUT_READONLY')
        assert.equal(error.message, 'Installation is read-only')
        assert.equal(error.message.includes('API Error:'), false)
        return true
      },
    )
  })
})
