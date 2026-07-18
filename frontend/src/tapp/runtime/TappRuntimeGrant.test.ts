/* eslint-disable test/no-import-node-test -- node:test is the repository test runner */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { TappRuntimeGrant } from './TappRuntimeGrant.ts'

describe('TappRuntimeGrant subject reset', () => {
  it('destroys every unissued grant before an auth subject changes', async () => {
    const page = new TappRuntimeGrant('com.example.page', 'page-1', 'page')
    const headless = new TappRuntimeGrant(
      'com.example.headless',
      'headless-1',
      'headless',
    )

    TappRuntimeGrant.destroyAll()

    await assert.rejects(page.getToken(), /already stopped/)
    await assert.rejects(headless.getToken(), /already stopped/)
  })
})
