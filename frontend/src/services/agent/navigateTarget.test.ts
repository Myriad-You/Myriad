import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { isInSitePath } from './navigateTarget'

describe('isInSitePath', () => {
  it('accepts in-site paths with query and hash', () => {
    for (const path of [
      '/',
      '/library',
      '/tapp?id=3',
      '/agent/settings#persona',
    ]) {
      assert.ok(isInSitePath(path), path)
    }
  })

  it('rejects anything that could leave the site', () => {
    for (const path of [
      'https://evil.example/',
      '//evil.example/x',
      '/\\evil.example/x',
      'javascript:alert(1)',
      'library',
      '',
    ]) {
      assert.ok(!isInSitePath(path), path)
    }
  })
})
