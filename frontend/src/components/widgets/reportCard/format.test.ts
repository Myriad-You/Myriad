import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { localizeDiscordGuildTake } from './format.ts'

const labels = {
  discordRoleOwner: 'Owner',
  discordRoleAdmin: 'Admin',
  discordRoleMod: 'Mod',
  discordFeaturePartner: 'Partner',
  discordFeatureVerified: 'Verified',
  discordTakeOwnServer: 'Own server',
  discordTakeAdminSeat: 'Admin seat',
  discordTakeModSeat: 'Mod seat',
  discordTakeCommunityStay: 'Community regular',
  discordTakeMember: 'Member',
  discordTakeWithSize: '{role} · {size}',
  discordSizeHuge: '100k+',
  discordSizeLarge: '10k+',
  discordSizeMid: '1k+',
  discordSizeSmall: 'small',
}

describe('localizeDiscordGuildTake', () => {
  it('maps leftover Chinese owner/size templates', () => {
    assert.equal(localizeDiscordGuildTake('自建·万人级', labels), 'Owner · 10k+')
    assert.equal(localizeDiscordGuildTake('自建领地', labels), 'Own server')
    assert.equal(localizeDiscordGuildTake('常驻·千人圈', labels), 'Member · 1k+')
  })

  it('maps leftover English and Japanese templates', () => {
    assert.equal(localizeDiscordGuildTake('Owner · 10k+', labels), 'Owner · 10k+')
    assert.equal(localizeDiscordGuildTake('自作·万人級', labels), 'Owner · 10k+')
    assert.equal(localizeDiscordGuildTake('Community stay', labels), 'Community regular')
  })

  it('leaves AI-written takes alone', () => {
    assert.equal(localizeDiscordGuildTake('千人圈里摸鱼', labels), '千人圈里摸鱼')
  })
})
