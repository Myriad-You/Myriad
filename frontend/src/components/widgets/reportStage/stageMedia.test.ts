import assert from 'node:assert/strict'
import test from 'node:test'
import { mediaMentionedIn, readStageMedia } from './stageMedia'
import { buildStageReport } from './stageScript'

const visuals = {
  library_items: [
    { title: '君という神話', artist: '麻枝准', cover: '/api/proxy/image?url=a', type: 'music' },
    { title: '约定的梦幻岛 第二季', cover: '/api/proxy/image?url=b', type: 'anime' },
    { title: 'STEINS;GATE', cover: '/api/proxy/image?url=c', type: 'anime' },
    { title: 'No cover' },
    { title: 'Evil', cover: '//evil.example/x.png' },
    { title: '十三机兵防卫圈', cover: '/api/proxy/image?url=a', type: 'game' },
  ],
  top_titles: [{ name: 'ELDEN RING', image: 'https://cdn.example/elden.jpg', type: 'game' }],
}

test('covers come from the card lists, with a safe url, once per image', () => {
  const media = readStageMedia(visuals, 'bilibili')
  assert.deepEqual(media.map((item) => [item.title, item.shape]), [
    ['君という神話', 'square'],
    ['约定的梦幻岛 第二季', 'poster'],
    ['STEINS;GATE', 'poster'],
    ['ELDEN RING', 'banner'],
  ])
  assert.equal(media[0].by, '麻枝准')
  assert.deepEqual(readStageMedia(null, 'steam'), [])
})

test('a line picks the covers it names: title, title without its season, or the artist', () => {
  const media = readStageMedia(visuals, 'bilibili')
  const names = (text: string) => mediaMentionedIn(text, media).map((item) => item.title)
  assert.deepEqual(names('追完约定的梦幻岛，又去听麻枝准'), ['约定的梦幻岛 第二季', '君という神話'])
  assert.deepEqual(names('Steins;Gate 世界线收束'), ['STEINS;GATE'], 'case and punctuation do not matter')
  assert.deepEqual(names('最近什么都没看'), [])
  assert.deepEqual(names('ring 和 elden 分开说不算'), [])
})

test('beats carry their covers, and the report keeps the shelf', () => {
  const report = buildStageReport({
    platform: 'netease',
    card_visuals: visuals,
    facts: [{ id: 'top_artist', label: 'Top artist', value: 'Aimer', media: '/api/proxy/image?url=z' }],
    script: { chapters: [{ key: 'taste', beats: [
      { text: '循环最多的是麻枝准。' },
      { text: 'Aimer 也常听。', fact: 'top_artist' },
      { text: '其余随缘。' },
    ] }] },
  }, { echo: 'Echo', insight: 'Insight' })!
  const beats = report.chapters[0].beats
  assert.deepEqual(beats[0].media?.map((item) => item.title), ['君という神話'])
  assert.equal(beats[1].media, undefined, 'a fact image is not a cover: only works the line names')
  assert.equal(beats[2].media, undefined)
  assert.equal(report.media.length, 4)
})

test('a cover links to its page when the card data carries the id', () => {
  const [show, subject, song] = [
    ...readStageMedia({ library_items: [{ title: '夏日重现', cover: '/a.jpg', season_id: 42 }] }, 'bilibili'),
    ...readStageMedia({ top_subjects: [{ title: 'CLANNAD', cover: '/b.jpg', subject_id: 51 }] }, 'bangumi'),
    ...readStageMedia({ library_items: [{ title: '魚', cover: '/c.jpg', artist: 'あたらよ' }] }, 'netease'),
  ]
  assert.equal(show.href, 'https://www.bilibili.com/bangumi/play/ss42')
  assert.equal(subject.href, 'https://bgm.tv/subject/51')
  assert.equal(song.href, undefined, 'a song needs its id from the library first')
})
