import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { islandPanelTabForClick } from './islandClick.ts'

describe('islandPanelTabForClick', () => {
  it('opens control from the island body even if the carousel shows a notification', () => {
    assert.equal(
      islandPanelTabForClick({
        affordance: 'control',
        carouselType: 'notification',
        viewport: 'mobile',
      }),
      'control',
    )
    assert.equal(
      islandPanelTabForClick({
        affordance: 'control',
        carouselType: 'notification',
        viewport: 'desktop',
      }),
      'control',
    )
  })

  it('opens notifications only from the notification affordance', () => {
    assert.equal(
      islandPanelTabForClick({
        affordance: 'notification',
        carouselType: 'greeting',
        viewport: 'mobile',
      }),
      'notifications',
    )
    assert.equal(
      islandPanelTabForClick({
        affordance: 'notification',
        viewport: 'desktop',
      }),
      'notifications',
    )
  })

  it('does not let viewport swap a control click onto notifications', () => {
    for (const viewport of ['mobile', 'desktop', 'narrow', null]) {
      assert.equal(
        islandPanelTabForClick({ affordance: 'control', viewport }),
        'control',
        `viewport=${String(viewport)}`,
      )
    }
  })
})
