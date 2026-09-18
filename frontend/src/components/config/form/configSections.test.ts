import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import configCopy from '../../../i18n/config.en-US.json'
import en from '../../../i18n/en-US.json'
import { buildSearchableContent } from './buildSearchableContent'
import { CONFIG_NAV_SECTIONS } from './configNavPersistence'
import { configSectionCatalog } from './configSections'
import { DEFAULT_AUTO_FETCH_CONFIG } from './defaults'

const t = { ...en, config: configCopy }
const config = {
  platforms: [],
  auto_fetch: DEFAULT_AUTO_FETCH_CONFIG,
  ai_config: { config_fields: [] },
  tripo_config: { config_fields: [] },
  report_config: { config_fields: [] },
  ui_config: { config_fields: [] },
}

describe('settings page catalog', () => {
  it('each navigation page has the same name and description in search', () => {
    const pages = configSectionCatalog(t, true, 'Persona')
    const search = buildSearchableContent(config, t, 'en-US', {
      isAdmin: true,
      agentTitle: 'Persona',
    })
    const ids = pages.map((page) => page.id)
    assert.ok(ids.includes('agent'))
    assert.equal(pages.find((page) => page.id === 'agent')?.href, '/agent/settings')
    assert.deepEqual(
      ids.filter((id) => id !== 'agent'),
      [...CONFIG_NAV_SECTIONS],
    )
    assert.equal(ids[ids.indexOf('ai') + 1], 'agent')
    for (const page of pages) {
      assert.ok(
        search.some(
          (item) =>
            item.type === 'section' &&
            item.section === page.id &&
            item.title === page.title &&
            item.description === page.description,
        ),
        page.id,
      )
    }
  })
  it('hidden federation pages are absent from both section and guide search results', () => {
    assert.ok(
      configSectionCatalog(t, false).every((page) => page.id !== 'federation'),
    )
    assert.ok(
      buildSearchableContent(config, t, 'en-US', { isAdmin: false }).every(
        (item) => item.section !== 'federation',
      ),
    )
  })
  it('hides federation when the egress-location gate is closed', () => {
    assert.ok(
      configSectionCatalog(t, true, 'Persona', false).every(
        (page) => page.id !== 'federation',
      ),
    )
    assert.ok(
      buildSearchableContent(config, t, 'en-US', {
        isAdmin: true,
        federationEnabled: false,
      }).every((item) => item.section !== 'federation'),
    )
  })
  it('keeps lab as the UI section while Tripo remains the guide path namespace', () => {
    const results = buildSearchableContent(config, t, 'en-US')
    const guides = results.filter((item) =>
      item.guidePath?.startsWith('tripo.'),
    )
    assert.ok(guides.length > 0)
    assert.ok(guides.every((item) => item.section === 'lab'))
  })
})
