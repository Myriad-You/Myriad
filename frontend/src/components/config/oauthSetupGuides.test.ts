import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { getOAuthSetupGuide, resolveOAuthPresetId } from './oauthSetupGuides'

const t = {
  platformSetupOptional: '可选',
  platformSetupOpen: '打开',
  oauthSetupTitle: '配置步骤',
  oauthSetupCopyCallback: '复制回调',
  oauthSetupFillTitle: '回到本页填写',
  oauthSetupFillDesc: '填凭证。',
  oauthSetupGithub1Title: '打开 GitHub',
  oauthSetupGithub1Desc: 'OAuth Apps。',
  oauthSetupGithub2Title: '登记回调',
  oauthSetupGithub2Desc: '粘贴回调。',
  oauthSetupGithub3Title: '复制凭证',
  oauthSetupGithub3Desc: '复制 Secret。',
  oauthSetupGoogle1Title: '',
  oauthSetupGoogle1Desc: '',
  oauthSetupGoogle2Title: '',
  oauthSetupGoogle2Desc: '',
  oauthSetupGoogle3Title: '',
  oauthSetupGoogle3Desc: '',
  oauthSetupMicrosoft1Title: '',
  oauthSetupMicrosoft1Desc: '',
  oauthSetupMicrosoft2Title: '',
  oauthSetupMicrosoft2Desc: '',
  oauthSetupMicrosoft3Title: '',
  oauthSetupMicrosoft3Desc: '',
  oauthSetupGitlab1Title: '',
  oauthSetupGitlab1Desc: '',
  oauthSetupGitlab2Title: '',
  oauthSetupGitlab2Desc: '',
  oauthSetupGitlab3Title: '',
  oauthSetupGitlab3Desc: '',
  oauthSetupDiscord1Title: '',
  oauthSetupDiscord1Desc: '',
  oauthSetupDiscord2Title: '',
  oauthSetupDiscord2Desc: '',
  oauthSetupDiscord3Title: '',
  oauthSetupDiscord3Desc: '',
  oauthSetupAuthentik1Title: '',
  oauthSetupAuthentik1Desc: '',
  oauthSetupAuthentik2Title: '',
  oauthSetupAuthentik2Desc: '',
  oauthSetupAuthentik3Title: '',
  oauthSetupAuthentik3Desc: '',
  oauthSetupKeycloak1Title: '',
  oauthSetupKeycloak1Desc: '',
  oauthSetupKeycloak2Title: '',
  oauthSetupKeycloak2Desc: '',
  oauthSetupKeycloak3Title: '',
  oauthSetupKeycloak3Desc: '',
  oauthSetupAuth0Step1Title: '',
  oauthSetupAuth0Step1Desc: '',
  oauthSetupAuth0Step2Title: '',
  oauthSetupAuth0Step2Desc: '',
  oauthSetupAuth0Step3Title: '',
  oauthSetupAuth0Step3Desc: '',
  oauthSetupCustom1Title: '打开 IdP',
  oauthSetupCustom1Desc: '创建应用。',
  oauthSetupCustom2Title: '登记回调',
  oauthSetupCustom2Desc: '粘贴回调。',
  oauthSetupCustom3Title: '复制凭证',
  oauthSetupCustom3Desc: '复制 Secret。',
}

describe('OAuth setup guides', () => {
  it('opens GitHub then offers to copy the callback', () => {
    const guide = getOAuthSetupGuide('github', t, {
      hasCallback: true,
      copyCallback: () => {},
    })
    assert.equal(guide.title, '配置步骤')
    assert.equal(
      guide.steps[0].href,
      'https://github.com/settings/developers',
    )
    assert.equal(guide.steps[1].actionLabel, '复制回调')
    assert.equal(typeof guide.steps[1].onAction, 'function')
    assert.equal(guide.steps[3].title, '回到本页填写')
  })

  it('resolves a second copy by slug suffix', () => {
    assert.equal(
      resolveOAuthPresetId({
        slug: 'google-2',
        kind: 'oidc',
        display_name: 'Google 2',
        enabled: true,
        client_id: '',
        client_secret: '',
        scopes: [],
      }),
      'google',
    )
  })
})
