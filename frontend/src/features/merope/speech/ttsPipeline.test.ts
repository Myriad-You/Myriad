import type { SpeechSegment } from './speechSegmenter'
import assert from 'node:assert/strict'
import test from 'node:test'
import { TtsPipeline } from './ttsPipeline'

function segment(
  sequence: number,
  text: string,
  messageId = 'msg-1',
): SpeechSegment {
  return {
    segmentId: `${messageId}:${sequence}`,
    sequence,
    text,
    messageId,
    generation: 1,
    interrupt: 'queue',
  }
}

function buffer(tag: string): ArrayBuffer {
  return new TextEncoder().encode(tag).buffer as ArrayBuffer
}

function deferred<T>(): {
  promise: Promise<T>
  resolve: (value: T) => void
} {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((next) => {
    resolve = next
  })
  return { promise, resolve }
}

test('synthesizes at most two segments and plays in sequence order', async () => {
  const played: string[] = []
  const synth = new Map<string, ReturnType<typeof deferred<ArrayBuffer | null>>>()
  const ends: Array<() => void> = []
  let inflight = 0
  let maxInflight = 0
  const pipeline = new TtsPipeline({
    synthesize: (item) => {
      inflight += 1
      maxInflight = Math.max(maxInflight, inflight)
      const wait = deferred<ArrayBuffer | null>()
      synth.set(item.text, wait)
      return wait.promise.finally(() => {
        inflight -= 1
      })
    },
    play: (_audio, item, onEnded) => {
      played.push(item.text)
      ends.push(onEnded)
      return { stop: () => undefined }
    },
  })
  pipeline.enqueue([
    segment(1, 'one'),
    segment(2, 'two'),
    segment(3, 'three'),
  ])
  assert.equal(synth.size, 2)
  assert.ok(maxInflight <= 2)
  synth.get('one')?.resolve(buffer('one'))
  synth.get('two')?.resolve(buffer('two'))
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.deepEqual(played, ['one'])
  ends[0]!()
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.deepEqual(played, ['one', 'two'])
  synth.get('three')?.resolve(buffer('three'))
  ends[1]!()
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.deepEqual(played, ['one', 'two', 'three'])
})

test('interrupt stops playback, clears the queue, and ignores late synth', async () => {
  const played: string[] = []
  const cancelled: string[] = []
  const first = deferred<ArrayBuffer | null>()
  const pipeline = new TtsPipeline({
    synthesize: (item) =>
      item.text === '旧' ? first.promise : Promise.resolve(buffer(item.text)),
    play: (_audio, item, _onEnded) => {
      played.push(item.text)
      return { stop: () => undefined }
    },
    onCancel: (messageId) => cancelled.push(messageId),
  })
  pipeline.enqueue([segment(1, '旧')])
  first.resolve(buffer('旧'))
  await Promise.resolve()
  await Promise.resolve()
  assert.deepEqual(played, ['旧'])
  pipeline.enqueue([segment(1, '新')], 'interrupt')
  await Promise.resolve()
  await Promise.resolve()
  assert.deepEqual(cancelled, ['msg-1'])
  assert.deepEqual(played, ['旧', '新'])
})

test('a later utterance plays after the previous one even when sequences restart', async () => {
  const played: string[] = []
  const ends: Array<() => void> = []
  const pipeline = new TtsPipeline({
    synthesize: async (item) => buffer(item.text),
    play: (_audio, item, onEnded) => {
      played.push(item.text)
      ends.push(onEnded)
      return { stop: () => undefined }
    },
  })
  pipeline.enqueue([
    segment(1, 'A1', 'msg-a'),
    segment(2, 'A2', 'msg-a'),
  ])
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.deepEqual(played, ['A1'])
  ends[0]!()
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.deepEqual(played, ['A1', 'A2'])
  ends[1]!()
  await new Promise((resolve) => setTimeout(resolve, 0))
  pipeline.enqueue([
    segment(1, 'B1', 'msg-b'),
    segment(2, 'B2', 'msg-b'),
  ])
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.deepEqual(played, ['A1', 'A2', 'B1'])
  ends[2]!()
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.deepEqual(played, ['A1', 'A2', 'B1', 'B2'])
})

test('stopping A does not let a stale onended start the next B segment twice', async () => {
  const played: string[] = []
  const pipeline = new TtsPipeline({
    synthesize: async (item) => buffer(item.text),
    play: (_audio, item, onEnded) => {
      played.push(item.text)
      return {
        stop: () => {
          queueMicrotask(onEnded)
        },
      }
    },
  })
  pipeline.enqueue([segment(1, 'A:1', 'A')])
  await new Promise((resolve) => setTimeout(resolve, 0))
  pipeline.enqueue([segment(1, 'B:1', 'B'), segment(2, 'B:2', 'B')])
  assert.equal(pipeline.cancel('A'), true)
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.deepEqual(played, ['A:1', 'B:1'])
})

test('cancelling the playing message leaves a later message in the queue', async () => {
  const played: string[] = []
  const ends: Array<() => void> = []
  const pipeline = new TtsPipeline({
    synthesize: async (item) => buffer(item.text),
    play: (_audio, item, onEnded) => {
      played.push(item.text)
      ends.push(onEnded)
      return { stop: () => played.push(`stop:${item.text}`) }
    },
  })
  pipeline.enqueue([segment(1, 'A', 'msg-a')])
  await new Promise((resolve) => setTimeout(resolve, 0))
  pipeline.enqueue([segment(1, 'B', 'msg-b')])
  assert.equal(pipeline.cancel('msg-a'), true)
  assert.deepEqual(played, ['A', 'stop:A'])
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.deepEqual(played, ['A', 'stop:A', 'B'])
  assert.equal(pipeline.isBusyWith('msg-a'), false)
  assert.equal(pipeline.isBusyWith('msg-b'), true)
})

test('cancelling a message that is not playing does not drop a later one', async () => {
  const first = deferred<ArrayBuffer | null>()
  const second = deferred<ArrayBuffer | null>()
  const played: string[] = []
  const pipeline = new TtsPipeline({
    synthesize: (item) =>
      item.text === 'A' ? first.promise : second.promise,
    play: (_audio, item) => {
      played.push(item.text)
      return { stop: () => undefined }
    },
  })
  pipeline.enqueue([segment(1, 'A', 'msg-a')])
  pipeline.enqueue([segment(1, 'B', 'msg-b')])
  assert.equal(pipeline.cancel('msg-a'), false)
  first.resolve(buffer('A'))
  second.resolve(buffer('B'))
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.deepEqual(played, ['B'])
})

test('cancel of another message does not stop the playing utterance', async () => {
  const played: string[] = []
  const pipeline = new TtsPipeline({
    synthesize: async (item) => buffer(item.text),
    play: (_audio, item, _onEnded) => {
      played.push(item.text)
      return { stop: () => played.push(`stop:${item.text}`) }
    },
  })
  pipeline.enqueue([segment(1, 'chat', 'chat-1')])
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.equal(pipeline.isBusyWith('chat-1'), true)
  assert.equal(pipeline.cancel('work-1'), false)
  assert.deepEqual(played, ['chat'])
  assert.equal(pipeline.playing, true)
})

test('failed synthesis skips the segment and keeps later audio', async () => {
  const played: string[] = []
  const pipeline = new TtsPipeline({
    synthesize: async (item) => (item.text === '坏' ? null : buffer(item.text)),
    play: (_audio, item, onEnded) => {
      played.push(item.text)
      queueMicrotask(onEnded)
      return { stop: () => undefined }
    },
  })
  pipeline.enqueue([segment(1, '坏'), segment(2, '好')])
  await Promise.resolve()
  await Promise.resolve()
  await new Promise((resolve) => setTimeout(resolve, 0))
  assert.deepEqual(played, ['好'])
})
