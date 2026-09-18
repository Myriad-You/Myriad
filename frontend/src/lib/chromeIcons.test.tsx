import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import { LuSparkles, LuX } from './chromeStrokeIcons.tsx'
import { FaCog, FaSearch } from './faChromeIcons.tsx'

describe('chrome toolbar icons', () => {
  it('renders stroke and FA marks as svg', () => {
    for (const Icon of [LuX, LuSparkles, FaCog, FaSearch]) {
      const html = renderToStaticMarkup(createElement(Icon, { 'aria-hidden': true }))
      assert.match(html, /<svg/)
      assert.equal(html.includes('aria-hidden="true"'), true)
    }
  })
})
