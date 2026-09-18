import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const dir = dirname(fileURLToPath(import.meta.url))

describe('FriendLinksWidget source load', () => {
  it('asks for catalog friend-link sources instead of the full overlay', () => {
    const src = readFileSync(join(dir, 'FriendLinksWidget.tsx'), 'utf8')
    assert.match(src, /fetchFriendLinkSources\(\s*'widget'/)
    assert.doesNotMatch(src, /recent_items|pulses/)
  })

  it('does not fetch until the card intersects, and never polls', () => {
    const src = readFileSync(join(dir, 'FriendLinksWidget.tsx'), 'utf8')
    assert.match(src, /new IntersectionObserver/)
    assert.match(src, /if \(!shown\) return/)
    assert.match(src, /void loadFriendLinks\(\)/)
    assert.doesNotMatch(src, /useLocation/)
    assert.doesNotMatch(src, /REFRESH_INTERVAL/)
    assert.doesNotMatch(
      src,
      /useHomeVisibilityInterval\(\s*loadFriendLinks/,
    )
  })
})
