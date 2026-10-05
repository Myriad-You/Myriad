import type { MeropeHerResponse } from '../../../services/agent/types'
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { formatMessage } from '../../../i18n/formatMessage'
import zh from '../../../i18n/zh-CN.json' with { type: 'json' }
import { herItems } from './herItems'

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

function items(filter: Parameters<typeof herItems>[0]['filter'], onPlay: (text: string) => void = () => {}) {
  return herItems({
    life,
    filter,
    copy: zh.agentPanel.her,
    format: (template, params) => formatMessage('zh-CN', template, params),
    ago: () => '3 小时前',
    onPlay,
  })
}

describe('her life as rows in agent settings', () => {
  it('lists her puzzles, wants and what she took in, in her words', () => {
    const rows = items('all')
    assert.deepEqual(
      rows.map((row) => row.title),
      [
        '暴雪夜，山下的人看到木屋的灯光忽明忽暗',
        '凌晨四点，面包店门口总站着同一个男人',
        '把 Reol 的歌都听一遍',
        '读了 《守夜人 第三章》 · 佚名',
        '查了：星轨是怎么拍出来的',
      ],
    )
    assert.equal(rows[0].meta, '1 人玩过，1 人猜中')
    assert.equal(rows[1].meta, '还没人玩过')
    assert.equal(rows[2].subtitle, '一首歌就把我抓住了。')
    assert.equal(rows[3].subtitle, '老周来回走动挡住灯光那段，写得太妙了。')
    assert.equal(rows[3].badge?.label, '被打动了')
    assert.equal(rows[3].meta, '3 小时前')
    assert.equal(rows[4].badge, undefined)
  })

  it('offers to play only what this person has not played', () => {
    const played: string[] = []
    const [open, yours] = items('puzzles', (text) => played.push(text))
    assert.equal(yours.actions, undefined)
    assert.equal(yours.badge?.label, '你玩过了')
    assert.deepEqual(open.actions?.map((action) => action.label), ['来一局'])
    open.actions![0].onClick()
    assert.deepEqual(played, ['来玩你出的这道汤：暴雪夜，山下的人看到木屋的灯光忽明忽暗'])
  })

  it('shows one kind at a time when filtered', () => {
    assert.equal(items('wants').length, 1)
    assert.equal(items('lately').length, 2)
  })
})
