import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { composerActionKind } from './composerAction'

describe('composerActionKind', () => {
  it('defaults to voice when the field is empty and speech is available', () => {
    assert.equal(
      composerActionKind({
        hasText: false,
        busy: false,
        speechAvailable: true,
        voiceLocked: false,
      }),
      'voice',
    )
  })

  it('hides the button when speech is not configured and the field is empty', () => {
    assert.equal(
      composerActionKind({
        hasText: false,
        busy: false,
        speechAvailable: false,
        voiceLocked: false,
      }),
      null,
    )
  })

  it('still shows send when speech is missing but the field has text', () => {
    assert.equal(
      composerActionKind({
        hasText: true,
        busy: false,
        speechAvailable: false,
        voiceLocked: false,
      }),
      'send',
    )
  })

  it('becomes send once the field has text', () => {
    assert.equal(
      composerActionKind({
        hasText: true,
        busy: false,
        speechAvailable: true,
        voiceLocked: false,
      }),
      'send',
    )
  })

  it('becomes stop while a run is in flight and the field is empty', () => {
    assert.equal(
      composerActionKind({
        hasText: false,
        busy: true,
        speechAvailable: true,
        voiceLocked: false,
      }),
      'stop',
    )
  })

  it('still shows stop when speech is not configured', () => {
    assert.equal(
      composerActionKind({
        hasText: false,
        busy: true,
        speechAvailable: false,
        voiceLocked: false,
      }),
      'stop',
    )
  })

  it('stays send if the user types during a run', () => {
    assert.equal(
      composerActionKind({
        hasText: true,
        busy: true,
        speechAvailable: true,
        voiceLocked: false,
      }),
      'send',
    )
  })

  it('does not show voice while thinking, even if the mic is open', () => {
    assert.equal(
      composerActionKind({
        hasText: false,
        busy: true,
        speechAvailable: true,
        voiceLocked: true,
      }),
      'stop',
    )
  })

  it('keeps the mic during a live conversation even if a run is in flight', () => {
    assert.equal(
      composerActionKind({
        hasText: false,
        busy: true,
        speechAvailable: true,
        voiceLocked: true,
        conversation: true,
      }),
      'voice',
    )
  })

  it('becomes send when the field is empty but files are attached', () => {
    assert.equal(
      composerActionKind({
        hasText: false,
        hasAttachments: true,
        busy: false,
        speechAvailable: true,
        voiceLocked: false,
      }),
      'send',
    )
  })
})
