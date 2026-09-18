import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import { BangumiIcon, FaGithub, SiBilibili } from './platformBrandIcons.tsx'

function renderIcon(
  Icon: (props: Record<string, unknown>) => unknown,
  props: Record<string, unknown> = {},
): string {
  return renderToStaticMarkup(createElement(Icon as never, props))
}

describe('platformBrandIcons a11y', () => {
  it('plain brand marks stay decorative', () => {
    for (const [name, Icon] of [
      ['SiBilibili', SiBilibili],
      ['FaGithub', FaGithub],
      ['BangumiIcon', BangumiIcon],
    ] as const) {
      const html = renderIcon(Icon)
      assert.equal(html.includes('role="img"'), false, name)
      assert.equal(html.includes('aria-hidden="true"'), true, name)
    }
  })
})
