import assert from 'node:assert/strict'
import test from 'node:test'
import { speakableText } from './speakableText'
import { SpeechSegmenter } from './speechSegmenter'

test('strips markdown, code, and URLs so TTS does not read markup', () => {
  assert.equal(speakableText('你好 **世界**'), '你好 世界')
  assert.equal(speakableText('见 https://example.com/a 吧'), '见 吧')
  assert.equal(
    speakableText('前文\n```js\nconsole.log(1)\n```\n后文'),
    '前文\n后文',
  )
  assert.equal(speakableText('用 `code` 标记'), '用 标记')
  assert.equal(speakableText('[点这里](https://x.test) 继续'), '继续')
  assert.equal(speakableText('# 标题\n- 一项'), '标题\n一项')
  assert.equal(
    speakableText('第一步准备好素材\n第二步导入到工作台'),
    '第一步准备好素材\n第二步导入到工作台',
  )
})

test('emits a segment at a stable sentence end, not on every token', () => {
  const splitter = new SpeechSegmenter('msg-1', 2)
  assert.deepEqual(splitter.push('你好'), [])
  assert.deepEqual(splitter.push('，今'), [])
  const ready = splitter.push('天好。下')
  assert.equal(ready.length, 1)
  assert.equal(ready[0]?.text, '你好，今天好。')
  assert.equal(ready[0]?.sequence, 1)
  assert.equal(ready[0]?.generation, 2)
  const rest = splitter.end()
  assert.equal(rest.length, 1)
  assert.equal(rest[0]?.text, '下')
})

test('a short opening sentence merges with the next instead of blocking cuts', () => {
  const splitter = new SpeechSegmenter('msg-3')
  const tokens = [
    '嗯。',
    '我想想，',
    '这个问题其实挺有意思的。',
    '你要不要再说细一点？',
  ]
  const segments: string[] = []
  for (const token of tokens) {
    for (const item of splitter.push(token)) segments.push(item.text)
  }
  for (const item of splitter.end()) segments.push(item.text)
  assert.equal(segments.length, 2)
  assert.equal(segments[0], '嗯。我想想，这个问题其实挺有意思的。')
  assert.equal(segments[1], '你要不要再说细一点？')
})

test('English periods cut streaming TTS without splitting decimals', () => {
  const splitter = new SpeechSegmenter('msg-en')
  const tokens = [
    'Okay. ',
    'Let me check that for you. ',
    "Here's what I found. ",
    'It looks fine.',
  ]
  const segments: string[] = []
  for (const token of tokens) {
    for (const item of splitter.push(token)) segments.push(item.text)
  }
  for (const item of splitter.end()) segments.push(item.text)
  assert.deepEqual(segments, [
    'Okay.',
    'Let me check that for you.',
    "Here's what I found.",
    'It looks fine.',
  ])

  const number = new SpeechSegmenter('msg-num')
  assert.deepEqual(number.push('It is 3.5'), [])
  const ready = number.push(' degrees now, almost ready.')
  assert.equal(ready.length, 1)
  assert.equal(ready[0]?.text, 'It is 3.5 degrees now, almost ready.')
})

test('list newlines become streaming cuts after markers are stripped', () => {
  const splitter = new SpeechSegmenter('msg-list')
  const ready = splitter.push(
    '第一步准备好素材\n第二步导入到工作台\n第三步检查一下结果对不对',
  )
  assert.deepEqual(
    ready.map((item) => item.text),
    ['第一步准备好素材', '第二步导入到工作台'],
  )
  assert.deepEqual(
    splitter.end().map((item) => item.text),
    ['第三步检查一下结果对不对'],
  )

  const bullets = new SpeechSegmenter('msg-bullets')
  const tokens = ['- 准备素材\n', '- 导入工作台\n', '- 检查结果\n']
  const spoken: string[] = []
  for (const token of tokens) {
    for (const item of bullets.push(token)) spoken.push(item.text)
  }
  for (const item of bullets.end()) spoken.push(item.text)
  assert.deepEqual(spoken, ['准备素材', '导入工作台', '检查结果'])
  for (const line of spoken) {
    assert.doesNotMatch(line, /[-*#]/)
  }
})

test('English abbreviations do not cut a sentence in the middle', () => {
  const lines = [
    'Ask Mr. Tanaka.',
    'Use e.g. this one.',
    'Sales etc. matter.',
    'It is 10 a.m. now.',
    'See Fig. 3 below.',
  ]
  for (const line of lines) {
    const splitter = new SpeechSegmenter('msg-abbr')
    const segments = [
      ...splitter.push(line),
      ...splitter.end(),
    ].map((item) => item.text)
    assert.deepEqual(segments, [line], line)
  }
  const ok = new SpeechSegmenter('msg-ok')
  const parts: string[] = []
  for (const token of ['Okay. ', 'Let me check.']) {
    for (const item of ok.push(token)) parts.push(item.text)
  }
  for (const item of ok.end()) parts.push(item.text)
  assert.deepEqual(parts, ['Okay.', 'Let me check.'])
})

test('empty speakable leftovers do not emit a segment', () => {
  const splitter = new SpeechSegmenter('msg-2')
  for (const token of ['```js\n', 'foo()\n', '```']) {
    assert.deepEqual(splitter.push(token), [], token)
  }
  assert.deepEqual(splitter.end(), [])
})

function dense(text: string): string {
  return text.replaceAll(/\s+/g, '')
}

function stream(tokens: readonly string[]): string[] {
  const splitter = new SpeechSegmenter('msg-stream')
  const spoken: string[] = []
  for (const token of tokens) {
    for (const item of splitter.push(token)) spoken.push(item.text)
  }
  for (const item of splitter.end()) spoken.push(item.text)
  return spoken
}

test('a code fence arriving token by token is never spoken', () => {
  const tokens = [
    '好的，看这段：\n',
    '```python\n',
    'print(1)\n',
    'sum([1, 2])\n',
    '```\n',
    '就是这样，我把要点再说一遍。',
  ]
  const spoken = stream(tokens)
  for (const line of spoken) assert.doesNotMatch(line, /`|print|sum/, line)
  assert.deepEqual(spoken, ['好的，看这段：', '就是这样，我把要点再说一遍。'])
  assert.equal(dense(spoken.join('')), dense(speakableText(tokens.join(''))))
})

test('prose before a fence streams instead of waiting for the block to close', () => {
  const splitter = new SpeechSegmenter('msg-lead')
  assert.deepEqual(splitter.push('好的，看这段：\n'), [])
  assert.deepEqual(
    splitter.push('```python\n').map((item) => item.text),
    ['好的，看这段：'],
  )
})

test('an unterminated fence stays unspoken even at the end of the message', () => {
  assert.deepEqual(stream(['给你代码。\n', '```js\n', 'let a = 1\n']), ['给你代码。'])
})

test('inline markup does not split a sentence or lose the text around it', () => {
  const cases: ReadonlyArray<readonly string[]> = [
    ['先执行 ', '`npm run ', 'build`', ' 然后看结果。'],
    ['请看这个。', '[标题', '](https://x.test)', ' 然后继续。'],
    ['访问 www.', 'example.com 看看。'],
    ['这里有个 <', 'b>重点</b> 要记住。'],
  ]
  for (const tokens of cases) {
    const spoken = stream(tokens)
    assert.equal(
      dense(spoken.join('')),
      dense(speakableText(tokens.join(''))),
      tokens.join(''),
    )
  }
  assert.deepEqual(stream(['先执行 ', '`npm run ', 'build`', ' 然后看结果。']), [
    '先执行 然后看结果。',
  ])
})

test('brackets and comparisons that are not markup keep streaming', () => {
  assert.deepEqual(
    stream(['数组 [1, 2, 3] 就是这样。', '然后我们继续下一步。']),
    ['数组 [1, 2, 3] 就是这样。', '然后我们继续下一步。'],
  )
  assert.deepEqual(stream(['如果 a < b 就成立。', '再看下一条。']), [
    '如果 a < b 就成立。',
    '再看下一条。',
  ])
})

test('the speakable string never shrinks while a message streams', () => {
  const tokens = [
    '第一句已经说完了。\n',
    '```python\n',
    'print(1)\n',
    '```\n',
    '接着说第二句。',
    '还有 `x` 和 ',
    '[链接](https://x.test)。',
    '最后一句收尾。',
  ]
  const splitter = new SpeechSegmenter('msg-mono')
  let spoken = ''
  for (const token of tokens) {
    const before = spoken
    for (const item of splitter.push(token)) spoken += item.text
    assert.ok(spoken.startsWith(before), token)
  }
  const before = spoken
  for (const item of splitter.end()) spoken += item.text
  assert.ok(spoken.startsWith(before))
  assert.equal(dense(spoken), dense(speakableText(tokens.join(''))))
})

test('segments carry the language they were written in', () => {
  const japanese = new SpeechSegmenter('m1', 0, 'ja-JP')
  const [segment] = [...japanese.push('日本語です。'), ...japanese.end()]
  assert.equal(segment?.locale, 'ja-JP')

  const chinese = new SpeechSegmenter('m2', 0, 'zh-CN')
  const [other] = [...chinese.push('这是中文。'), ...chinese.end()]
  assert.equal(other?.locale, 'zh-CN')
})
