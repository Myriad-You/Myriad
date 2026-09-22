import assert from 'node:assert/strict'
import { afterEach, it, mock } from 'node:test'
import { byPlatform, getPlatform, listPlatform } from './TappReportCatalogApi'

afterEach(() => mock.restoreAll())

it('reads compact catalog entries and derives SDK fields from the single detail content', async () => {
  const content = { summary: 'Played', insights: ['one'], metadata: { games: 4 }, card_visuals: { image: 'data:image/png;base64,AA==' } }
  const detail = { id: 7, platform: 'steam', type: 'platform', content, createdAt: '2026-09-22' }
  mock.method(globalThis, 'fetch', async (input: string) => Response.json(
    String(input).endsWith('/report-catalog') ? { reports: [{ id: 7, platform: 'steam', type: 'platform', summary: 'Played', createdAt: detail.createdAt }] } : detail,
  ))
  const list = await listPlatform('grant')
  assert.equal(list.reports[0].summary, 'Played')
  assert.equal('content' in list.reports[0], false)
  const report = await getPlatform('7', 'grant')
  assert.deepEqual(report.content, content)
  assert.equal(report.summary, content.summary)
  const sdk = await byPlatform('steam', 'grant')
  assert.equal(sdk?.summary, content.summary)
  assert.deepEqual(sdk?.metadata, content.metadata)
  assert.deepEqual(sdk?.insights, content.insights)
  assert.deepEqual(sdk?.card_visuals, content.card_visuals)
  assert.equal(sdk?.cardVisuals, sdk?.card_visuals)
})

it('keeps the SDK empty summary when the stored report has no string summary', async () => {
  for (const content of [{}, { summary: null }]) {
    mock.method(globalThis, 'fetch', async () => Response.json({ id: 7, type: 'platform', content, createdAt: '2026-09-22' }))
    const report = await getPlatform('7', 'grant')
    assert.equal(report.summary, '')
    assert.deepEqual(report.content, content)
    mock.restoreAll()
  }
})
