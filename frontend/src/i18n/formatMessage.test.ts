import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { formatMessage } from './formatMessage.ts'

function catalog<T>(name: string): T {
  return JSON.parse(
    readFileSync(new URL(`./${name}.json`, import.meta.url), 'utf8'),
  ) as T
}

describe('formatMessage', () => {
  it('fills simple tokens', () => {
    assert.equal(
      formatMessage('en-US', 'Hello {name}', { name: 'Ada' }),
      'Hello Ada',
    )
  })

  it('selects English plural branches', () => {
    const tpl = '{n, plural, one {# new item} other {# new items}}'
    assert.equal(formatMessage('en-US', tpl, { n: 1 }), '1 new item')
    assert.equal(formatMessage('en-US', tpl, { n: 3 }), '3 new items')
  })

  it('keeps a single Chinese form', () => {
    assert.equal(
      formatMessage('zh-CN', '{count, plural, other {# 篇}}', { count: 1 }),
      '1 篇',
    )
    assert.equal(
      formatMessage('zh-TW', '{count, plural, other {# 篇}}', { count: 8 }),
      '8 篇',
    )
  })

  it('honors exact =0', () => {
    assert.equal(
      formatMessage('en-US', '{n, plural, =0 {none} other {# left}}', { n: 0 }),
      'none',
    )
  })

  it('pluralizes English countable nouns', () => {
    const minutes =
      '{minutes, plural, one {# minute ago} other {# minutes ago}}'
    assert.equal(formatMessage('en-US', minutes, { minutes: 1 }), '1 minute ago')
    assert.equal(
      formatMessage('en-US', minutes, { minutes: 3 }),
      '3 minutes ago',
    )
    const sites = '{count, plural, one {# site} other {# sites}}'
    assert.equal(formatMessage('en-US', sites, { count: 1 }), '1 site')
    assert.equal(formatMessage('en-US', sites, { count: 4 }), '4 sites')
  })

  it('fills remaining tokens after a plural branch', () => {
    const tpl =
      'Showing {shown, plural, one {# item} other {# items}} from {total}'
    assert.equal(
      formatMessage('en-US', tpl, { shown: 1, total: 9 }),
      'Showing 1 item from 9',
    )
  })

  it('pluralizes leftover English countable nouns in catalogs', () => {
    const errors = catalog<{
      agentQueueTimeout: string
      noticeFederationRevokedBody: string
    }>('errors.en-US')
    const brew = catalog<{
      noteTitleTooLong: string
      noteBodyTooLong: string
    }>('brew.en-US')
    const config = catalog<{
      runtimeDiagnosticsRecentFailures: string
    }>('config.en-US')

    assert.equal(
      formatMessage('en-US', errors.agentQueueTimeout, { sec: 1 }),
      'The system is busy. Waited more than 1 second. Try again later.',
    )
    assert.equal(
      formatMessage('en-US', errors.agentQueueTimeout, { sec: 8 }),
      'The system is busy. Waited more than 8 seconds. Try again later.',
    )
    assert.equal(
      formatMessage('en-US', errors.noticeFederationRevokedBody, {
        name: 'peer.example',
        count: 1,
      }),
      'peer.example could not be reached for a long time. Federation was unlinked and 1 queued item was cancelled.',
    )
    assert.equal(
      formatMessage('en-US', errors.noticeFederationRevokedBody, {
        name: 'peer.example',
        count: 4,
      }),
      'peer.example could not be reached for a long time. Federation was unlinked and 4 queued items were cancelled.',
    )
    assert.equal(
      formatMessage('en-US', brew.noteTitleTooLong, { max: 1, chars: 2 }),
      'Titles can be at most 1 character (this one is 2)',
    )
    assert.equal(
      formatMessage('en-US', brew.noteBodyTooLong, { max: 200, chars: 201 }),
      'Notes can be at most 200 characters (this one is 201)',
    )
    assert.equal(
      formatMessage('en-US', config.runtimeDiagnosticsRecentFailures, { n: 1 }),
      'Failures in the last 1 hour',
    )
    assert.equal(
      formatMessage('en-US', config.runtimeDiagnosticsRecentFailures, { n: 24 }),
      'Failures in the last 24 hours',
    )
    const core = catalog<{ auth: { rateLimitError: string } }>('en-US')
    assert.equal(
      formatMessage('en-US', core.auth.rateLimitError, { seconds: 1 }),
      'Too many login attempts, please retry in 1 second',
    )
    assert.equal(
      formatMessage('en-US', core.auth.rateLimitError, { seconds: 60 }),
      'Too many login attempts, please retry in 60 seconds',
    )
  })

  it('I18n format binds ICU to the loaded bundle locale', () => {
    const src = readFileSync(
      new URL('../contexts/I18nContext.tsx', import.meta.url),
      'utf8',
    )
    assert.match(src, /formatMessage\(bundle\?\.locale \?\? locale/)
  })
})
