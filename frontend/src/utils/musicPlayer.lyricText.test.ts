import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { unescapeQQLyricText } from './musicPlayer.ts'

describe('unescapeQQLyricText', () => {
  it('实体只解一层', () => {
    assert.equal(unescapeQQLyricText('&lt;3 &amp; &#39;hi&#39;&#10;x'), "<3 & 'hi'\nx")
    assert.equal(unescapeQQLyricText('&amp;lt;3 &amp;#10;'), '&lt;3 &#10;')
  })
})
