import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  isChannelPairingProvider,
  pairingCodeLive,
  telegramOpenHref,
} from './channelPairing'

describe('channelPairing', () => {
  it('treats qq and telegram as pairing providers, not OAuth', () => {
    assert.equal(isChannelPairingProvider('qq'), true)
    assert.equal(isChannelPairingProvider('Telegram'), true)
    assert.equal(isChannelPairingProvider('github'), false)
  })

  it('treats a future expiry as live', () => {
    assert.equal(
      pairingCodeLive(new Date(Date.now() + 60_000).toISOString()),
      true,
    )
    assert.equal(pairingCodeLive(new Date(Date.now() - 1).toISOString()), false)
    assert.equal(pairingCodeLive(null), false)
  })

  it('builds a public Telegram open link from the username', () => {
    assert.equal(telegramOpenHref('@site_bot'), 'https://t.me/site_bot')
    assert.equal(telegramOpenHref(''), null)
  })
})
