import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { firstSiteResult, LIBRARY_PATH } from './AgentPanelSearchResults'
import { hasSiteResults } from './useSiteSearch'

const post = { title: 'A', href: '/blog/posts/a/', excerpt: '', published_at: '', category: null }
const item = { title: 'B', item_type: 'game', platform: 'Steam', cover: null }

test('an empty search shows nothing', () => {
  assert.equal(hasSiteResults(null), false)
  assert.equal(hasSiteResults({ posts: [], library: [] }), false)
  assert.equal(hasSiteResults({ posts: [], library: [item] }), true)
})

test('enter opens the first post, else the library', () => {
  assert.equal(firstSiteResult({ posts: [post], library: [item] }), '/blog/posts/a/')
  assert.equal(firstSiteResult({ posts: [], library: [item] }), LIBRARY_PATH)
  assert.equal(firstSiteResult({ posts: [], library: [] }), null)
})

test('a visitor cannot ask: every way of sending stops before it', () => {
  const composer = readFileSync(new URL('./AgentPanelComposer.tsx', import.meta.url), 'utf8')
  const submit = composer.split('const submit = useCallback(')[1]!.split('[canAsk, onSubmit]')[0]!
  assert.match(submit, /if \(!canAsk\) return/)
  assert.match(composer, /onSubmit=\{canAsk \? onSubmit : IGNORE_SUBMIT\}/)
  assert.match(composer, /\{canAsk \? <ComposerAction/)
  assert.match(composer, /const shortcuts = !resident && isAuthenticated/)
  const engine = readFileSync(new URL('./AgentEngine.tsx', import.meta.url), 'utf8')
  assert.match(engine, /if \(!isAuthenticated\) return\n/)
})
