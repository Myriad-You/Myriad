import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { resolveWorkbenchPane } from './board.ts'
import {
  collectWorkbenchNoteAuthors,
  collectWorkbenchNoteTopics,
  filterWorkbenchComments,
  filterWorkbenchNotes,
  filterWorkbenchReviews,
  formatWorkbenchBytes,
  workbenchMediaFormatKey,
  workbenchMediaFormatLabel,
  workbenchMediaRefLabel,
  workbenchNoteAuthorName,
  workbenchNoteCover,
  workbenchNoteExcerpt,
  workbenchNoteOpen,
  workbenchNoteStatusKey,
  workbenchNoteWhen,
} from './workbench.ts'

describe('filterWorkbenchReviews', () => {
  it('按状态、站名和申请人筛', () => {
    const rows = [
      {
        site_name: '甲站',
        site_url: 'https://a.example',
        feed_url: 'https://a.example/rss',
        applicant_name: 'Ada',
        status: 'pending',
      },
      {
        site_name: '乙站',
        site_url: 'https://b.example',
        message: '请收录',
        applicant_email: 'bob@example.com',
        status: 'approved',
      },
    ]
    assert.equal(filterWorkbenchReviews(rows, '', 'pending').length, 1)
    assert.deepEqual(
      filterWorkbenchReviews(rows, '甲', 'all').map((row) => row.site_name),
      ['甲站'],
    )
    assert.deepEqual(
      filterWorkbenchReviews(rows, 'bob', 'all').map((row) => row.site_name),
      ['乙站'],
    )
  })
})

describe('filterWorkbenchComments', () => {
  it('按正文、摘录、文章和作者筛', () => {
    const rows = [
      {
        comment: '好看',
        selected_text: '第一段',
        item_title: '笔记甲',
        source_name: '本地',
        user_display_name: '站长',
      },
      {
        comment: '另一条',
        selected_text: '第二段',
        item_title: '订阅乙',
        source_name: '外站',
        user_name: 'guest',
      },
    ]
    assert.equal(filterWorkbenchComments(rows, '').length, 2)
    assert.deepEqual(
      filterWorkbenchComments(rows, '笔记').map((row) => row.comment),
      ['好看'],
    )
    assert.deepEqual(
      filterWorkbenchComments(rows, 'guest').map((row) => row.comment),
      ['另一条'],
    )
  })
})

describe('workbenchNoteOpen', () => {
  it('已发布走文章 id，草稿走文档 id', () => {
    assert.deepEqual(workbenchNoteOpen({ id: 3, item_id: 9 }), {
      kind: 'item',
      id: 9,
    })
    assert.deepEqual(workbenchNoteOpen({ id: 3, item_id: null }), {
      kind: 'doc',
      id: 3,
    })
  })
})

describe('workbenchNoteStatusKey', () => {
  it('只认定时和已发布，失败仍跟原状态', () => {
    assert.equal(
      workbenchNoteStatusKey({ status: 'scheduled' }),
      'noteStatusScheduled',
    )
    assert.equal(
      workbenchNoteStatusKey({ status: 'published' }),
      'noteStatusPublished',
    )
    assert.equal(workbenchNoteStatusKey({ status: 'draft' }), 'noteStatusDraft')
  })
})

describe('workbenchNoteWhen', () => {
  it('定时和失败用预约时间，已发布用发布时间', () => {
    assert.equal(
      workbenchNoteWhen({
        status: 'scheduled',
        scheduled_at: 20,
        updated_at: 10,
      }),
      20,
    )
    assert.equal(
      workbenchNoteWhen({
        status: 'draft',
        last_error: 'boom',
        scheduled_at: 20,
        updated_at: 10,
      }),
      20,
    )
    assert.equal(
      workbenchNoteWhen({
        status: 'published',
        published_at: 30,
        updated_at: 10,
      }),
      30,
    )
    assert.equal(workbenchNoteWhen({ status: 'draft', updated_at: 10 }), 10)
  })
})

describe('media format labels', () => {
  it('uses filename fallback and known labels', () => {
    assert.equal(
      workbenchMediaFormatKey({
        mime: 'application/octet-stream',
        name: 'a.webp',
      }),
      'webp',
    )
    assert.equal(workbenchMediaFormatLabel('webp', '其他'), 'WebP')
    assert.equal(workbenchMediaFormatLabel('other', '其他'), '其他')
  })
})

describe('formatWorkbenchBytes / workbenchMediaRefLabel', () => {
  it('空尺寸不写，引用按已知种类翻译', () => {
    assert.equal(formatWorkbenchBytes(0), '')
    assert.equal(formatWorkbenchBytes(512), '512 B')
    assert.equal(formatWorkbenchBytes(2048), '2.0 KB')
    assert.equal(
      workbenchMediaRefLabel(['notes', 'site', 'other'], {
        notes: '笔记',
        articles: '文章',
        site: '站点',
      }),
      '笔记 · 站点',
    )
  })
})

describe('filterWorkbenchNotes', () => {
  const docs = [
    {
      id: 1,
      title: '春天的草稿',
      excerpt: '樱花开了',
      topic: '生活',
      status: 'draft',
    },
    {
      id: 2,
      title: '夜里发',
      content_md: '定时稿',
      topic: '工作',
      status: 'scheduled',
    },
    {
      id: 3,
      title: '已见',
      content_md: '公开了',
      topic: null,
      status: 'published',
    },
    {
      id: 4,
      title: '失败的一篇',
      content_md: '没发出去',
      topic: '工作',
      status: 'scheduled',
      last_error: 'boom',
    },
  ]

  it('按状态、分类和标题正文搜', () => {
    assert.deepEqual(
      filterWorkbenchNotes(docs, { status: 'draft', query: '' }).map(
        (d) => d.id,
      ),
      [1],
    )
    assert.deepEqual(
      filterWorkbenchNotes(docs, { status: 'scheduled', query: '' }).map(
        (d) => d.id,
      ),
      [2, 4],
    )
    assert.deepEqual(
      filterWorkbenchNotes(docs, {
        status: 'all',
        query: '',
        topic: '工作',
      }).map((d) => d.id),
      [2, 4],
    )
    assert.deepEqual(
      filterWorkbenchNotes(docs, { status: 'all', query: '', topic: '' }).map(
        (d) => d.id,
      ),
      [3],
    )
    assert.deepEqual(
      filterWorkbenchNotes(docs, { status: 'all', query: '樱花' }).map(
        (d) => d.id,
      ),
      [1],
    )
    assert.deepEqual(collectWorkbenchNoteTopics(docs), ['工作', '生活'])
  })

  it('分类按段匹配，叠了两个都能筛到', () => {
    const stacked = [
      {
        id: 9,
        title: '叠分类',
        content_md: '两档',
        topic: '工作, 生活',
        status: 'draft' as const,
      },
    ]
    assert.deepEqual(
      filterWorkbenchNotes(stacked, {
        status: 'all',
        query: '',
        topic: '工作',
      }).map((d) => d.id),
      [9],
    )
    assert.deepEqual(
      filterWorkbenchNotes(stacked, {
        status: 'all',
        query: '',
        topic: '生活',
      }).map((d) => d.id),
      [9],
    )
    assert.deepEqual(
      filterWorkbenchNotes(stacked, { status: 'all', query: '', topic: '' }).map(
        (d) => d.id,
      ),
      [],
    )
    assert.deepEqual(collectWorkbenchNoteTopics(stacked), ['工作', '生活'])
  })

  it('按作者筛，搜索也能打到名字', () => {
    const authored = [
      {
        id: 1,
        title: '站长稿',
        content_md: '甲',
        topic: null,
        status: 'draft',
        user_id: 1,
        user_display_name: '站长',
      },
      {
        id: 2,
        title: '客人稿',
        content_md: '乙',
        topic: null,
        status: 'draft',
        user_id: 2,
        user_name: 'ada',
      },
    ]
    assert.deepEqual(
      filterWorkbenchNotes(authored, {
        status: 'all',
        query: '',
        author: '1',
      }).map((d) => d.id),
      [1],
    )
    assert.deepEqual(
      filterWorkbenchNotes(authored, { status: 'all', query: 'ada' }).map(
        (d) => d.id,
      ),
      [2],
    )
    assert.deepEqual(collectWorkbenchNoteAuthors(authored, '匿名'), [
      { key: '1', label: '站长' },
      { key: '2', label: 'ada' },
    ])
    const jointly = [
      ...authored,
      {
        id: 3,
        title: '联名稿',
        content_md: '丙',
        topic: null,
        status: 'draft',
        user_id: 1,
        user_display_name: '站长',
        authors: [
          {
            user_id: 1,
            user_display_name: '站长',
            role: 'owner',
          },
          {
            user_id: 3,
            user_name: 'bee',
            role: 'author',
          },
        ],
      },
    ]
    assert.deepEqual(
      filterWorkbenchNotes(jointly, {
        status: 'all',
        query: '',
        author: '3',
      }).map((d) => d.id),
      [3],
    )
    assert.deepEqual(
      filterWorkbenchNotes(jointly, {
        status: 'all',
        query: '',
        author: '1',
      }).map((d) => d.id),
      [1, 3],
    )
    assert.deepEqual(
      filterWorkbenchNotes(jointly, { status: 'all', query: 'bee' }).map(
        (d) => d.id,
      ),
      [3],
    )
    assert.equal(
      workbenchNoteAuthorName(jointly[2], '匿名'),
      '站长 · bee',
    )
    assert.deepEqual(collectWorkbenchNoteAuthors(jointly, '匿名'), [
      { key: '1', label: '站长' },
      { key: '2', label: 'ada' },
      { key: '3', label: 'bee' },
    ])
  })
})

describe('workbenchNoteCover', () => {
  it('指定封面优先，否则用正文第一张图', () => {
    assert.equal(
      workbenchNoteCover({
        image: 'https://img.example/cover.jpg',
        content_md: '![内文](https://img.example/body.jpg)\n开头',
      }),
      'https://img.example/cover.jpg',
    )
    assert.equal(
      workbenchNoteCover({
        image: '  ',
        content_md: '前文\n![内文](https://img.example/body.jpg)',
      }),
      'https://img.example/body.jpg',
    )
    assert.equal(
      workbenchNoteCover({ image: null, content_md: '没有图' }),
      null,
    )
    assert.equal(
      workbenchNoteCover({
        image: null,
        content_md: '![封面][cover]\n\n[cover]: https://img.example/ref.jpg',
      }),
      'https://img.example/ref.jpg',
    )
  })
})

describe('workbenchNoteExcerpt', () => {
  it('去掉标记，留下开头正文', () => {
    assert.equal(workbenchNoteExcerpt(''), '')
    assert.equal(
      workbenchNoteExcerpt(
        '![封面](https://img.example/a.jpg)\n# 标题\n这是**开头**，还有[链接](https://ex.am)。',
      ),
      '标题 这是开头，还有链接。',
    )
    assert.equal(
      workbenchNoteExcerpt(':::widget weather 2x2\n\n正文从这里起。'),
      '正文从这里起。',
    )
    assert.equal(workbenchNoteExcerpt('一二三四五', 3), '一二三…')
  })
})

describe('resolveWorkbenchPane', () => {
  it('认 home / notes / media / sources / add / rsshub / 两页分类 / 两页导入导出；旧深链并进，其余落概览', () => {
    assert.equal(resolveWorkbenchPane('home'), 'home')
    assert.equal(resolveWorkbenchPane('media'), 'media')
    assert.equal(resolveWorkbenchPane('sources'), 'sources')
    assert.equal(resolveWorkbenchPane('add'), 'add')
    assert.equal(resolveWorkbenchPane('topics'), 'topics')
    assert.equal(resolveWorkbenchPane('rsshub'), 'rsshub')
    assert.equal(resolveWorkbenchPane('notesIo'), 'notesIo')
    assert.equal(resolveWorkbenchPane('feedsIo'), 'feedsIo')
    assert.equal(resolveWorkbenchPane('pipack'), 'feedsIo')
    assert.equal(resolveWorkbenchPane('opml'), 'feedsIo')
    assert.equal(resolveWorkbenchPane('wordpress'), 'notesIo')
    assert.equal(resolveWorkbenchPane('halo'), 'notesIo')
    assert.equal(resolveWorkbenchPane('typecho'), 'notesIo')
    assert.equal(resolveWorkbenchPane('markdown'), 'notesIo')
    assert.equal(resolveWorkbenchPane('list'), 'sources')
    assert.equal(resolveWorkbenchPane('notes'), 'notes')
    assert.equal(resolveWorkbenchPane('comments'), 'comments')
    assert.equal(resolveWorkbenchPane('reviews'), 'reviews')
    assert.equal(resolveWorkbenchPane('noteCategories'), 'noteCategories')
    assert.equal(resolveWorkbenchPane('sourceCategories'), 'sourceCategories')
    assert.equal(resolveWorkbenchPane('categories'), 'noteCategories')
    assert.equal(resolveWorkbenchPane('category'), 'noteCategories')
    assert.equal(resolveWorkbenchPane('sourceCategory'), 'sourceCategories')
    assert.equal(resolveWorkbenchPane(null), 'home')
    assert.equal(resolveWorkbenchPane('settings'), 'home')
  })
})
