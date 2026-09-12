import type { UpdateTrust } from '../../../services/updaterApi'
import type { U } from './helpers'
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import config from '../../../i18n/config.zh-CN.json'
import { TrustDetails } from './TrustDetails'

function render(trust?: UpdateTrust) {
  return renderToStaticMarkup(createElement(TrustDetails, { trust, u: config as U }))
}

test('a manifest with disabled or soft verification is never labeled verified', () => {
  for (const verification of ['off', 'soft_unverified'] as const) {
    const html = render({ trust_path: 'github_release', verification })
    assert.ok(html.includes('签名未验证'))
    assert.ok(!html.includes('签名已验证'))
  }
})

test('verified commit evidence includes full SHA and pulled digest', () => {
  const sha = 'a'.repeat(40)
  const digest = `sha256:${'b'.repeat(64)}`
  const html = render({ trust_path: 'signed_commit', verification: 'verified', commit_sha: sha,
    backend: { ref: 'docker.io/org/backend:dev-aaaaaaa', digest } })
  assert.ok(html.includes('签名已验证'))
  assert.ok(html.includes(sha))
  assert.ok(html.includes(digest))
})

test('an old job without evidence never gains a verified badge', () => {
  assert.equal(render(), '')
})
