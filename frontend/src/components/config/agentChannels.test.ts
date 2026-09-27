import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import {
  AGENT_CHANNEL_IDS,
  channelIsStored,
  clearAgentChannelValues,
  fieldHasStoredValue,
  visibleAgentChannels,
} from './agentChannels.ts'

describe('agentChannels', () => {
  it('treats whitespace as empty', () => {
    assert.equal(fieldHasStoredValue(''), false)
    assert.equal(fieldHasStoredValue('   '), false)
    assert.equal(fieldHasStoredValue('••••••••'), true)
    assert.equal(fieldHasStoredValue('1024'), true)
  })

  it('shows a channel when enabled or any credential is present', () => {
    const values: Record<string, string> = {
      qq_bot_enabled: 'false',
      qq_bot_app_id: '',
      qq_bot_app_secret: '',
      telegram_bot_enabled: 'true',
      telegram_bot_token: '',
      discord_bot_enabled: 'false',
      discord_bot_token: '••••••••',
      feishu_bot_enabled: 'false',
      feishu_bot_app_id: '',
      feishu_bot_app_secret: '',
    }
    const get = (key: string) => values[key] ?? ''
    assert.equal(channelIsStored('qq', get), false)
    assert.equal(channelIsStored('telegram', get), true)
    assert.equal(channelIsStored('discord', get), true)
    assert.equal(channelIsStored('feishu', get), false)
    assert.deepEqual(visibleAgentChannels(get, ['qq']), ['qq', 'telegram', 'discord'])
  })

  it('keeps catalog order and ignores unknown revealed ids', () => {
    const get = () => ''
    assert.deepEqual(visibleAgentChannels(get, ['feishu', 'qq']), ['qq', 'feishu'])
    assert.deepEqual(AGENT_CHANNEL_IDS, ['qq', 'telegram', 'discord', 'feishu', 'onebot'])
  })

  it('clears enable and credential fields on remove', () => {
    const written: Record<string, string> = {}
    clearAgentChannelValues('qq', (key, value) => {
      written[key] = value
    })
    assert.deepEqual(written, {
      qq_bot_enabled: 'false',
      qq_bot_app_id: '',
      qq_bot_app_secret: '',
    })
  })
})

describe('Agent page channel add wiring', () => {
  it('leaves the Agent page back to site settings', () => {
    const page = readFileSync(
      new URL('./AgentConfigSection.tsx', import.meta.url),
      'utf8',
    )
    const form = readFileSync(
      new URL('../agent/settings/AgentSettingsForm.tsx', import.meta.url),
      'utf8',
    )
    assert.match(page, /onLeave/)
    assert.match(page, /leaveLabel/)
    assert.match(form, /navigate\('\/config'\)/)
    assert.match(form, /leaveLabel=\{t\.nav\.config\}/)
  })

  it('puts add on the page header and does not mount all bots by default', () => {
    const page = readFileSync(
      new URL('./AgentConfigSection.tsx', import.meta.url),
      'utf8',
    )
    assert.match(page, /headerBetweenPinned/)
    assert.match(page, /<AgentChannelAddTrigger/)
    assert.match(page, /agentChannelsTitle/)
    assert.match(page, /<AgentChannelSources/)
    assert.doesNotMatch(page, /qqBotTitle/)
    assert.doesNotMatch(page, /telegramBotTitle/)
  })

  it('renders bots as vendor-style cards, not LLM tier rows', () => {
    const source = readFileSync(
      new URL('./AgentChannelSources.tsx', import.meta.url),
      'utf8',
    )
    assert.match(source, /oidc-provider-card ai-vendor-card/)
    assert.match(source, /useAddedCardOpen/)
    assert.doesNotMatch(source, /AgentNestedSection/)
    assert.doesNotMatch(source, /ai-llm-tier/)
  })
})
