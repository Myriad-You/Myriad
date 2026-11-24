import assert from 'node:assert/strict'
import { afterEach, it, mock } from 'node:test'
import { ApiError, apiService } from '../../services/api'
import { listSiteExpressions, MeropeApiError, meropeError, startSiteExpression } from './api'
import { listSiteArmVariants, startSiteArmVariant } from './armVariantsApi'

afterEach(() => mock.restoreAll())

const rejected = {
  error: 'The gesture could not be drawn without changing the design',
  code: 'arm_variant_rejected',
  differences: ['Wrong arm raised'],
}

it('asking for a gesture arm resolves once the server has taken it up, on the picture it is for', async () => {
  const post = mock.method(apiService, 'post', async () => ({ key: 'wave-left', drawing: true }))
  assert.equal(await startSiteArmVariant('wave-left'), undefined)
  await startSiteArmVariant('chin-right', 'w1')
  assert.deepEqual(post.mock.calls.map((call) => call.arguments[0]), [
    '/merope/rig/arm-variants/wave-left',
    '/merope/rig/full-body/w1/arm-variants/chin-right',
  ])
  // Answered at once: no wait of minutes is asked of the request any more.
  assert.equal(post.mock.calls[0].arguments[2], undefined)
})

it('a gesture arm the server will not take up fails with its answer', async () => {
  const busy = { error: 'This gesture is already being drawn', code: 'arm_variant_generation_in_progress' }
  mock.method(apiService, 'post', async () => {
    throw new ApiError(busy.error, 409, busy.code, undefined, undefined, busy)
  })
  await assert.rejects(startSiteArmVariant('wave-left'), (reason: unknown) => {
    assert.ok(reason instanceof MeropeApiError)
    assert.deepEqual([reason.message, reason.status, reason.code], [busy.error, 409, busy.code])
    return true
  })
})

it('the list names the gesture arms being drawn and why the ones not kept were not', async () => {
  mock.method(apiService, 'get', async () => ({
    variants: { 'wave-left': { url: '/media/a.png', hand: [[1, 2]] }, 'wave-right': null },
    drawing: ['point-left', 'juggle-left'],
    failed: { 'wave-right': { ...rejected, status: 422 }, 'chin-left': 'broken', 'juggle-left': { status: 500 } },
  }))
  const listed = await listSiteArmVariants()
  assert.deepEqual(listed.variants.map((variant) => `${variant.gesture}-${variant.side}`), ['wave-left'])
  assert.deepEqual(listed.drawing, ['point-left'])
  assert.deepEqual(Object.keys(listed.failed), ['wave-right'])
  // The same error the request itself threw when it still waited for the drawing.
  const thrown = meropeError(new ApiError(rejected.error, 422, rejected.code, undefined, undefined, rejected), 'fallback')
  const kept = listed.failed['wave-right']
  assert.ok(kept instanceof MeropeApiError)
  assert.deepEqual([kept.message, kept.status, kept.code], [thrown.message, thrown.status, thrown.code])
})

it('a list from before failures were listed has none', async () => {
  mock.method(apiService, 'get', async () => ({ variants: {}, drawing: [] }))
  assert.deepEqual((await listSiteArmVariants('w1')).failed, {})
  mock.restoreAll()
  mock.method(apiService, 'get', async () => ({ expressions: {}, drawing: [] }))
  assert.deepEqual((await listSiteExpressions()).failed, {})
})

it('expression redraws are asked for and listed the same way', async () => {
  const post = mock.method(apiService, 'post', async () => ({ kind: 'cry', drawing: true }))
  assert.equal(await startSiteExpression('cry'), undefined)
  assert.equal(post.mock.calls[0].arguments[0], '/merope/rig/expressions/cry')
  mock.method(apiService, 'get', async () => ({
    expressions: { cry: null, squeeze: '/media/s.png', close: null },
    drawing: ['close'],
    failed: { cry: { error: 'provider said no', code: 'image_provider_rejected', status: 400 } },
  }))
  const listed = await listSiteExpressions()
  assert.deepEqual(listed.urls, { squeeze: '/media/s.png' })
  assert.deepEqual(listed.drawing, ['close'])
  const kept = listed.failed.cry
  assert.ok(kept instanceof MeropeApiError)
  assert.deepEqual([kept.message, kept.status, kept.code], ['provider said no', 400, 'image_provider_rejected'])
})
