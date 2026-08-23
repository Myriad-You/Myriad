import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { ApiError } from '../services/api'
import { handleErrorResponse } from './apiHelper.ts'

describe('handleErrorResponse', () => {
  it('throws ApiError with the backend code and message', async () => {
    const response = Response.json(
      {
        error: 'Setup already completed',
        message: 'Admin exists',
        code: 'setup_completed',
      },
      { status: 403 },
    )
    await assert.rejects(
      () => handleErrorResponse(response, '操作失败'),
      (error: unknown) => {
        assert.ok(error instanceof ApiError)
        assert.equal(error.status, 403)
        assert.equal(error.code, 'setup_completed')
        assert.equal(error.message, 'Admin exists')
        return true
      },
    )
  })

  it('does not dump raw JSON when the body is not JSON', async () => {
    const response = new Response('upstream exploded', {
      status: 502,
      headers: { 'content-type': 'text/plain' },
    })
    await assert.rejects(
      () => handleErrorResponse(response, '操作失败'),
      (error: unknown) => {
        assert.ok(error instanceof ApiError)
        assert.equal(error.status, 502)
        assert.equal(error.message, 'upstream exploded')
        return true
      },
    )
  })
})
