import assert from 'node:assert/strict'
import test from 'node:test'
import { buildSearchableContent } from '../../../components/config/form/buildSearchableContent'
import { configSectionCatalog } from '../../../components/config/form/configSections'
import { DEFAULT_AUTO_FETCH_CONFIG } from '../../../components/config/form/defaults'
import configCopy from '../../../i18n/config.en-US.json'
import en from '../../../i18n/en-US.json'
import {
  PERSONA_DEFAULT_NAME,
  PERSONA_OFF_NAME,
  publicPersonaName,
  publicPersonaNameFromConfig,
  settingsAgentLabel,
} from './publicName'

test('public persona name matches the site face rule', () => {
  assert.equal(publicPersonaName(false, '瞳'), PERSONA_OFF_NAME)
  assert.equal(publicPersonaName(false, ''), PERSONA_OFF_NAME)
  assert.equal(publicPersonaName(true, '  瞳  '), '瞳')
  assert.equal(publicPersonaName(true, '   '), PERSONA_DEFAULT_NAME)
  assert.equal(publicPersonaName(true, null), PERSONA_DEFAULT_NAME)
})

test('public config name wins; otherwise fall back by meropeEnabled', () => {
  assert.equal(
    publicPersonaNameFromConfig({
      meropeEnabled: false,
      agentPersonaName: 'Agent',
    }),
    PERSONA_OFF_NAME,
  )
  assert.equal(
    publicPersonaNameFromConfig({
      meropeEnabled: true,
      agentPersonaName: '瞳',
    }),
    '瞳',
  )
  assert.equal(
    publicPersonaNameFromConfig({ meropeEnabled: false }),
    PERSONA_OFF_NAME,
  )
  assert.equal(
    publicPersonaNameFromConfig({ meropeEnabled: true }),
    PERSONA_DEFAULT_NAME,
  )
})

test('settings Agent label uses the character name once the public face has one', () => {
  assert.equal(settingsAgentLabel('Agent', PERSONA_OFF_NAME), 'Agent')
  assert.equal(settingsAgentLabel('Agent', '  Agent  '), 'Agent')
  assert.equal(settingsAgentLabel('Agent', ''), 'Agent')
  assert.equal(
    settingsAgentLabel('Agent', PERSONA_DEFAULT_NAME),
    PERSONA_DEFAULT_NAME,
  )
  assert.equal(settingsAgentLabel('Agent', '  瞳  '), '瞳')
})

test('settings nav and search follow the same public persona name', () => {
  const t = { ...en, config: configCopy }
  const agentTitle = settingsAgentLabel(t.config.agent, '瞳')
  const nav = configSectionCatalog(t, true, agentTitle)
  const search = buildSearchableContent(
    {
      platforms: [],
      auto_fetch: DEFAULT_AUTO_FETCH_CONFIG,
      ai_config: { config_fields: [] },
      tripo_config: { config_fields: [] },
      ui_config: { config_fields: [] },
    },
    t,
    'en-US',
    { isAdmin: true, agentTitle },
  )
  assert.equal(nav.find((section) => section.id === 'agent')?.title, '瞳')
  assert.equal(
    search.find((item) => item.type === 'section' && item.section === 'agent')
      ?.title,
    '瞳',
  )
  assert.ok(
    search.find((item) => item.section === 'agent')?.keywords.includes('瞳'),
  )
})
