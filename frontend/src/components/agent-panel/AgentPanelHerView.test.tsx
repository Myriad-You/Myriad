import type { MeropeHerResponse } from '../../services/agent/types'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { formatMessage } from '../../i18n/formatMessage'
import zh from '../../i18n/zh-CN.json' with { type: 'json' }
import { HerLifeView } from './AgentPanelHerView'

const life: MeropeHerResponse = {
  lately: [
    {
      at: '2026-10-01T09:00:00Z',
      kind: 'chapter',
      title: '守夜人 第三章',
      by: '佚名',
      reaction: 'moved',
      said: '老周来回走动挡住灯光那段，写得太妙了。',
    },
    {
      at: '2026-10-01T06:00:00Z',
      kind: 'inquiry',
      title: '星轨是怎么拍出来的',
      by: null,
      reaction: null,
      said: '原来要盖镜头防过曝。',
    },
  ],
  wants: [{ want: '把 Reol 的歌都听一遍', why: '一首歌就把我抓住了。', since: '2026-09-28T00:00:00Z' }],
  puzzles: [
    { surface: '暴雪夜，山下的人看到木屋的灯光忽明忽暗', played: 1, solved: 1, yours: false },
    { surface: '凌晨四点，面包店门口总站着同一个男人', played: 0, solved: 0, yours: true },
  ],
}

function render(onPlay: (text: string) => void = () => {}): string {
  return renderToStaticMarkup(
    createElement(HerLifeView, {
      life,
      copy: zh.agentPanel.her,
      format: (template, params) => formatMessage('zh-CN', template, params),
      ago: () => '3 小时前',
      onPlay,
    }),
  )
}

describe('her life as shown in the panel', () => {
  it('shows her puzzles, wants and what she took in, in her words', () => {
    const html = render()
    for (const shown of [
      '她出的汤',
      '暴雪夜，山下的人看到木屋的灯光忽明忽暗',
      '1 人玩过，1 人猜中',
      '还没人玩过',
      '她想要的',
      '把 Reol 的歌都听一遍',
      '最近在做的',
      '读了 《守夜人 第三章》 · 佚名',
      '被打动了 · 3 小时前',
      '查了：星轨是怎么拍出来的',
      '老周来回走动挡住灯光那段，写得太妙了。',
    ]) {
      assert.ok(html.includes(shown), shown)
    }
  })

  it('offers to play only what this person has not played', () => {
    const html = render()
    assert.equal(html.split('来一局').length - 1, 1)
    assert.ok(html.includes('你玩过了'))
  })
})
