import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  aiDailyToTrendPoints,
  aiModelsToRankRows,
  aiSourceDisplayName,
  aiUserDisplayName,
  aiUsersToRankRows,
} from './aiUsageMap'

describe('aiUsageMap', () => {
  it('maps daily calls/tokens onto TrendPoint views/visitors', () => {
    const points = aiDailyToTrendPoints([
      { day: '2026-07-01', calls: 3, tokens: 1200 },
      { day: '2026-07-02', calls: 0, tokens: 0 },
    ])
    assert.equal(points[0]!.views, 3)
    assert.equal(points[0]!.visitors, 1200)
    assert.equal(points[1]!.views, 0)
  })

  it('labels anonymous and named users', () => {
    assert.equal(
      aiUserDisplayName({ subject_id: 0 }, 'Anonymous / guest'),
      'Anonymous / guest',
    )
    assert.equal(
      aiUserDisplayName(
        { subject_id: 2, display_name: 'Ada', username: 'ada' },
        'Anonymous / guest',
      ),
      'Ada',
    )
    assert.equal(
      aiUserDisplayName(
        { subject_id: 9, username: null, display_name: null },
        'Anon',
      ),
      '#9',
    )
  })

  it('builds rank rows by user and model with tokens as primary', () => {
    const users = aiUsersToRankRows(
      [{ subject_id: 1, username: 'bob', display_name: 'Bob', calls: 2, tokens: 99 }],
      { anonymousLabel: 'Anon', callsLabel: (n) => `${n}x` },
    )
    assert.equal(users[0]!.value, 99)
    assert.equal(users[0]!.secondary, '2x')
    assert.equal(users[0]!.name, 'Bob')

    const models = aiModelsToRankRows(
      [{ model: 'gpt-test', provider: 'openai', calls: 5, tokens: 400 }],
      { callsLabel: (n) => `${n} calls` },
    )
    assert.equal(models[0]!.name, 'gpt-test')
    assert.equal(models[0]!.meta, 'openai')
    assert.equal(models[0]!.value, 400)
  })

  it('labels ledger sources, including the new site-wide paths', () => {
    const labels = {
      scheduler: 'Scheduled jobs',
      agent: 'Agent',
      reports: 'Reports',
      runtime: 'Tapp / runtime',
      merope: 'Agent persona',
      playground: 'Playground',
      speech: 'Speech',
      brewlia: 'Brewlia',
      prompt: 'Prompt',
      seo: 'SEO',
      internal: 'Internal',
      other: 'Other',
    }
    assert.equal(aiSourceDisplayName('merope', labels), 'Agent persona')
    assert.equal(aiSourceDisplayName('playground', labels), 'Playground')
    assert.equal(aiSourceDisplayName('speech', labels), 'Speech')
    assert.equal(aiSourceDisplayName('internal', labels), 'Internal')
    assert.equal(aiSourceDisplayName('internal:scheduler', labels), 'Scheduled jobs')
    assert.equal(aiSourceDisplayName('runtime', labels), 'Tapp / runtime')
    assert.equal(aiSourceDisplayName('', labels), 'Other')
  })
})
