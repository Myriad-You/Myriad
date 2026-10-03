import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  leftoverNoteSources,
  noteDocKicker,
  noteEditorStatus,
  notesBoardIsEmpty,
  noteScheduleLabel,
  noteWallSearchSources,
  sourceLatestStory,
  visibleCloudNoteDocs,
} from './noteBoard.ts'

describe('visibleCloudNoteDocs', () => {
  it('已发布和空草稿都不上板', () => {
    const shown = visibleCloudNoteDocs([
      { status: 'draft', title: '', content_md: '' },
      { status: 'draft', title: '还没发', content_md: '' },
      { status: 'scheduled', title: '', content_md: '夜里发' },
      { status: 'published', title: '已见', content_md: '公开了' },
      { status: 'draft', title: '', content_md: '', has_body: true },
    ])
    assert.deepEqual(
      shown.map((doc) => doc.title || doc.content_md),
      ['还没发', '夜里发', ''],
    )
  })

  it('定时时间写成看得见的钟点', () => {
    const label = noteScheduleLabel(Date.UTC(2026, 2, 3, 8, 30), 'en-US')
    assert.match(label, /Mar/)
    assert.match(label, /3/)
  })
})

describe('leftoverNoteSources', () => {
  it('墙上已有笔记的源不再另占一张', () => {
    assert.deepEqual(
      leftoverNoteSources(
        [{ id: 1 }, { id: 2 }, { id: 3 }],
        [{ source_id: 2 }],
      ).map((source) => source.id),
      [1, 3],
    )
  })
})

describe('sourceLatestStory', () => {
  it('没有近文就空着', () => {
    assert.equal(sourceLatestStory({ id: 1, name: '我', icon: null }), null)
  })

  it('近文带上源名', () => {
    const story = sourceLatestStory({
      id: 4,
      name: '我',
      icon: '/me.png',
      recent_items: [
        {
          id: 8,
          title: '近文',
          summary: null,
          image: null,
          published_at: 1,
          is_read: false,
        },
      ],
    })
    assert.equal(story?.source_id, 4)
    assert.equal(story?.source_name, '我')
    assert.equal(story?.source_type, undefined)
    assert.equal(story?.title, '近文')
  })

  it('笔记近文带上 source_type，卡面用来源换成站点', () => {
    const story = sourceLatestStory({
      id: 5,
      name: '笔记',
      icon: '/n.png',
      source_type: 'note',
      recent_items: [
        {
          id: 9,
          title: '笔记近文',
          summary: null,
          image: null,
          published_at: 1,
          is_read: true,
        },
      ],
    })
    assert.equal(story?.source_type, 'note')
    assert.equal(story?.source_name, '笔记')
  })
})

describe('notesBoardIsEmpty', () => {
  it('只有云端稿也不算空', () => {
    assert.equal(
      notesBoardIsEmpty(0, 0, [
        { status: 'draft', title: '还没发', content_md: '' },
      ]),
      false,
    )
    assert.equal(notesBoardIsEmpty(0, 0, []), true)
  })
})

describe('noteDocKicker', () => {
  it('失败先说失败，再带上本来要发的钟点', () => {
    const kicker = noteDocKicker(
      {
        last_error: 'A title is required',
        status: 'draft',
        scheduled_at: Date.UTC(2026, 2, 3, 8, 30),
      },
      { failed: '失败', scheduled: '定时', draft: '草稿' },
      'en-US',
    )
    assert.match(kicker, /失败/)
    assert.match(kicker, /Mar/)
  })
})

describe('noteEditorStatus', () => {
  it('草稿保存后把状态和时间说清楚', () => {
    assert.equal(
      noteEditorStatus(
        { status: 'draft', savedHint: true },
        {
          failed: '失败',
          scheduled: '定时',
          published: '已发布',
          draft: '草稿',
          saved: '已保存',
        },
        'en-US',
      ),
      '草稿 · 已保存',
    )
  })
})

describe('noteWallSearchSources', () => {
  const blog = {
    id: 1,
    name: '明日が来ると',
    icon: null,
    source_type: 'rss',
    recent_items: [{ id: 23, title: '不虚此行 On the Journey', author: null } as never],
  }
  const notes = { id: 2, name: '笔记', icon: null, source_type: 'note', recent_items: [] }
  const other = { id: 3, name: '友链', icon: null, source_type: 'rss', recent_items: [{ id: 9, title: 'Journey', author: null } as never] }

  it('墙上只显示最新一篇的源，按那篇标题也能搜到', () => {
    const hit = noteWallSearchSources([], [blog, notes, other], [blog, notes], [], 'journey')
    assert.deepEqual(hit.map((source) => source.id), [1])
  })

  it('有笔记命中的源照旧补回；名称已命中的不重复', () => {
    const hit = noteWallSearchSources([blog], [blog, notes], [blog, notes], [{ source_id: 2 }], 'journey')
    assert.deepEqual(hit.map((source) => source.id), [1, 2])
  })

  it('空查询原样返回', () => {
    const named = [blog]
    assert.equal(noteWallSearchSources(named, [blog, notes], [blog, notes], [], ' '), named)
  })
})
