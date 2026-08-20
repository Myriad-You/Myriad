/**
 * Reset defaults must match the product default delegation:
 * `component:theme` and `shortcut:register` are default-delegated to ordinary
 * users; guests and every other elevated capability stay closed.
 *
 * This drives the permissions-page reset request (granted-permission
 * delegation only — declared and approved permissions are unaffected).
 */
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { DEFAULT_PERMISSION_CONFIG } from './defaults.ts'

describe('DEFAULT_PERMISSION_CONFIG', () => {
  it('default-delegates component:theme and shortcut:register to ordinary users', () => {
    assert.equal(DEFAULT_PERMISSION_CONFIG.user_perm_component_theme, true)
    assert.equal(DEFAULT_PERMISSION_CONFIG.user_perm_shortcut_register, true)
  })

  it('keeps the corresponding guest delegations closed', () => {
    assert.equal(DEFAULT_PERMISSION_CONFIG.guest_perm_component_theme, false)
    assert.equal(DEFAULT_PERMISSION_CONFIG.guest_perm_shortcut_register, false)
  })

  it('keeps every other elevated delegation closed', () => {
    const closed = [
      DEFAULT_PERMISSION_CONFIG.user_perm_ai_generate,
      DEFAULT_PERMISSION_CONFIG.user_perm_ai_analyze,
      DEFAULT_PERMISSION_CONFIG.user_perm_ai_chat,
      DEFAULT_PERMISSION_CONFIG.user_perm_report_write,
      DEFAULT_PERMISSION_CONFIG.user_perm_network_fetch,
      DEFAULT_PERMISSION_CONFIG.user_perm_event_publish,
      DEFAULT_PERMISSION_CONFIG.user_perm_ai_image,
      DEFAULT_PERMISSION_CONFIG.user_perm_scheduler_register,
      DEFAULT_PERMISSION_CONFIG.user_perm_speech_tts,
      DEFAULT_PERMISSION_CONFIG.user_perm_speech_asr,
      DEFAULT_PERMISSION_CONFIG.user_perm_storage_write,
      DEFAULT_PERMISSION_CONFIG.user_perm_federation_post,
      DEFAULT_PERMISSION_CONFIG.user_perm_federation_channel,
      DEFAULT_PERMISSION_CONFIG.user_perm_federation_room,
      DEFAULT_PERMISSION_CONFIG.user_perm_brew_comment_write,
      DEFAULT_PERMISSION_CONFIG.guest_perm_ai_generate,
      DEFAULT_PERMISSION_CONFIG.guest_perm_ai_analyze,
      DEFAULT_PERMISSION_CONFIG.guest_perm_ai_chat,
      DEFAULT_PERMISSION_CONFIG.guest_perm_report_write,
      DEFAULT_PERMISSION_CONFIG.guest_perm_network_fetch,
      DEFAULT_PERMISSION_CONFIG.guest_perm_event_publish,
      DEFAULT_PERMISSION_CONFIG.guest_perm_ai_image,
      DEFAULT_PERMISSION_CONFIG.guest_perm_scheduler_register,
      DEFAULT_PERMISSION_CONFIG.guest_perm_speech_tts,
      DEFAULT_PERMISSION_CONFIG.guest_perm_speech_asr,
      DEFAULT_PERMISSION_CONFIG.guest_perm_storage_write,
      DEFAULT_PERMISSION_CONFIG.guest_perm_federation_post,
      DEFAULT_PERMISSION_CONFIG.guest_perm_federation_channel,
      DEFAULT_PERMISSION_CONFIG.guest_perm_federation_room,
      DEFAULT_PERMISSION_CONFIG.guest_perm_brew_comment_write,
    ]
    assert.ok(
      closed.every((value) => value === false),
      'all other elevated defaults must stay closed',
    )
  })
})
