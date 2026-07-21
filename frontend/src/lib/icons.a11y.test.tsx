/**
 *   pnpm exec tsx --test src/lib/icons.a11y.test.tsx
 *
 * Ensures Simple Icons re-exports from @lib/icons do not expose nameless
 * role="img" (Lighthouse svg-img-alt), while still supporting accessible names.
 */
/* eslint-disable test/no-import-node-test -- node:test */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'

import {
  BangumiIcon,
  MyriadStoreIcon,
  SiCloudflare,
  SiGithub,
  SiOpenai,
} from './icons.tsx'

function renderIcon(
  Icon: (props: Record<string, unknown>) => unknown,
  props: Record<string, unknown> = {},
): string {
  return renderToStaticMarkup(createElement(Icon as never, props))
}

describe('icon a11y defaults (Si* / custom brand icons)', () => {
  it('SiGithub plain: decorative — no role=img, aria-hidden', () => {
    const html = renderIcon(SiGithub)
    assert.equal(html.includes('role="img"'), false, html)
    assert.equal(html.includes('aria-hidden="true"'), true, html)
  })

  it('SiGithub with title: keeps role=img and accessible name', () => {
    const html = renderIcon(SiGithub, { title: 'GitHub' })
    assert.equal(html.includes('role="img"'), true, html)
    assert.equal(html.includes('<title>GitHub</title>'), true, html)
    assert.equal(html.includes('aria-hidden="true"'), false, html)
  })

  it('SiGithub with aria-label: keeps role=img', () => {
    const html = renderIcon(SiGithub, { 'aria-label': 'GitHub' })
    assert.equal(html.includes('role="img"'), true, html)
    assert.equal(html.includes('aria-label="GitHub"'), true, html)
    assert.equal(html.includes('aria-hidden="true"'), false, html)
  })

  it('SiGithub respects explicit role override', () => {
    const html = renderIcon(SiGithub, { role: 'presentation' })
    assert.equal(html.includes('role="presentation"'), true, html)
  })

  it('SiCloudflare plain: decorative (footer sponsor icon)', () => {
    const html = renderIcon(SiCloudflare)
    assert.equal(html.includes('role="img"'), false, html)
    assert.equal(html.includes('aria-hidden="true"'), true, html)
  })

  it('custom brand icons plain: decorative', () => {
    for (const [name, Icon] of [
      ['SiOpenai', SiOpenai],
      ['BangumiIcon', BangumiIcon],
      ['MyriadStoreIcon', MyriadStoreIcon],
    ] as const) {
      const html = renderIcon(Icon)
      assert.equal(
        html.includes('role="img"'),
        false,
        `${name} should not have role=img: ${html}`,
      )
      assert.equal(
        html.includes('aria-hidden="true"'),
        true,
        `${name} should be aria-hidden: ${html}`,
      )
    }
  })

  it('custom brand icons with title: named img', () => {
    const html = renderIcon(SiOpenai, { title: 'OpenAI' })
    assert.equal(html.includes('role="img"'), true, html)
    assert.equal(html.includes('<title>OpenAI</title>'), true, html)
  })
})
