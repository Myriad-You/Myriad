import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  compactCredentialEndpoint,
  summarizeCredentialBindings,
  uniqueNonEmpty,
} from './credentialBindingDisplay.ts'

const PATREON_CAMPAIGNS =
  'https://www.patreon.com/api/oauth2/v2/campaigns?fields%5Bcampaign%5D=currency'
const PATREON_MEMBERS_FIRST =
  'https://www.patreon.com/api/oauth2/v2/campaigns/{{params.campaignId}}/members?fields%5Bmember%5D=full_name,patron_status,last_charge_date,lifetime_support_cents,currently_entitled_amount_cents&page%5Bcount%5D=1000'
const PATREON_MEMBERS_NEXT =
  'https://www.patreon.com/api/oauth2/v2/campaigns/{{params.campaignId}}/members?fields%5Bmember%5D=full_name,patron_status,last_charge_date,lifetime_support_cents,currently_entitled_amount_cents&page%5Bcount%5D=1000&page%5Bcursor%5D={{params.cursor}}'

describe('compactCredentialEndpoint', () => {
  it('keeps only the path of an absolute URL', () => {
    assert.equal(
      compactCredentialEndpoint(PATREON_CAMPAIGNS),
      '/api/oauth2/v2/campaigns',
    )
  })

  it('drops query strings from templated member URLs', () => {
    assert.equal(
      compactCredentialEndpoint(PATREON_MEMBERS_FIRST),
      '/api/oauth2/v2/campaigns/{{params.campaignId}}/members',
    )
    assert.equal(
      compactCredentialEndpoint(PATREON_MEMBERS_NEXT),
      '/api/oauth2/v2/campaigns/{{params.campaignId}}/members',
    )
  })

  it('strips query from relative inbound paths', () => {
    assert.equal(compactCredentialEndpoint('/sponsors?limit=20'), '/sponsors')
  })

  it('returns empty for blank input', () => {
    assert.equal(compactCredentialEndpoint('   '), '')
  })
})

describe('uniqueNonEmpty', () => {
  it('preserves first-seen order and drops blanks', () => {
    assert.deepEqual(uniqueNonEmpty(['GET', '', 'GET', 'POST', 'GET']), [
      'GET',
      'POST',
    ])
  })
})

describe('summarizeCredentialBindings', () => {
  it('compacts the Patreon token case to three short rows', () => {
    const summary = summarizeCredentialBindings([
      {
        api: 'patreonCampaigns',
        method: 'GET',
        endpoint: PATREON_CAMPAIGNS,
        access: 'manager',
      },
      {
        api: 'patreonMembersFirst',
        method: 'GET',
        endpoint: PATREON_MEMBERS_FIRST,
        access: 'manager',
      },
      {
        api: 'patreonMembersNext',
        method: 'GET',
        endpoint: PATREON_MEMBERS_NEXT,
        access: 'manager',
      },
    ])

    assert.equal(summary.count, 3)
    assert.deepEqual(summary.methods, ['GET'])
    assert.deepEqual(summary.accesses, ['manager'])
    assert.deepEqual(
      summary.rows.map((row) => row.path),
      [
        '/api/oauth2/v2/campaigns',
        '/api/oauth2/v2/campaigns/{{params.campaignId}}/members',
        '/api/oauth2/v2/campaigns/{{params.campaignId}}/members',
      ],
    )
    assert.ok(
      summary.rows.every((row) => row.path.length < row.endpoint.length),
    )
  })
})
