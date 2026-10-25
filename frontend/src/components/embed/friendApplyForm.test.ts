import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  displaySiteUrl,
  EMPTY_FRIEND_APPLY,
  friendApplyPayload,
  friendApplyProblems,
  normalizeSiteUrl,
} from './friendApplyForm'

describe('friendApplyForm', () => {
  it('和后端一样补 https、只收 http(s)、不许带账号、去掉 #', () => {
    assert.equal(normalizeSiteUrl('Example.COM/blog#top')?.href, 'https://example.com/blog')
    assert.equal(normalizeSiteUrl(' http://a.dev ')?.href, 'http://a.dev/')
    assert.equal(normalizeSiteUrl('javascript:alert(1)'), null)
    assert.equal(normalizeSiteUrl('localhost:3000'), null)
    assert.equal(normalizeSiteUrl('https://user:pass@example.com'), null)
    assert.equal(normalizeSiteUrl('ftp://example.com'), null)
    assert.equal(normalizeSiteUrl('   '), null)
  })

  it('按栏说出问题，空的可选栏不算错', () => {
    assert.deepEqual(friendApplyProblems(EMPTY_FRIEND_APPLY), { siteName: 'required', siteUrl: 'required' })
    assert.deepEqual(
      friendApplyProblems({ ...EMPTY_FRIEND_APPLY, siteName: '某站', siteUrl: 'example.com' }),
      {},
    )
    assert.deepEqual(
      friendApplyProblems({
        ...EMPTY_FRIEND_APPLY,
        siteName: '某站',
        siteUrl: 'ftp://x',
        feedUrl: 'not a url://',
        applicantEmail: 'me@localhost',
        description: '字'.repeat(501),
      }),
      { siteUrl: 'url', feedUrl: 'url', applicantEmail: 'email', description: 'tooLong' },
    )
  })

  it('交给接口时补全网址、去掉空白、空栏不发', () => {
    assert.deepEqual(
      friendApplyPayload({ ...EMPTY_FRIEND_APPLY, siteName: ' 某站 ', siteUrl: 'example.com', message: '  ' }),
      {
        site_name: '某站',
        site_url: 'https://example.com/',
        feed_url: undefined,
        description: undefined,
        message: undefined,
        applicant_name: undefined,
        applicant_email: undefined,
      },
    )
  })

  it('预览上的地址不带协议、www 和末尾斜杠', () => {
    assert.equal(displaySiteUrl(new URL('https://www.example.com/')), 'example.com')
    assert.equal(displaySiteUrl(new URL('https://example.com/blog/')), 'example.com/blog')
  })
})
