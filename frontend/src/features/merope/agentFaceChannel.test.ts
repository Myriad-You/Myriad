import type { AgentFaceSink } from './agentFaceChannel'
import type {
  MeropeSpeechEventDetail,
  SpeechUtteranceInput,
} from './speechEvents'
import assert from 'node:assert/strict'
import test from 'node:test'
import { AgentFaceChannel } from './agentFaceChannel'

class RecordingSink implements AgentFaceSink {
  readonly speechEvents: MeropeSpeechEventDetail[] = []
  readonly utterances: SpeechUtteranceInput[] = []
  readonly performances: unknown[] = []
  readonly states: unknown[] = []
  /** 按发生顺序记录，用来断言表演与说话的先后。 */
  readonly order: string[] = []

  speech = (detail: MeropeSpeechEventDetail): void => {
    this.speechEvents.push(detail)
    this.order.push(`speech:${detail.phase}`)
  }

  utterance = (input: SpeechUtteranceInput): void => {
    this.utterances.push(input)
    this.order.push('utterance')
  }

  performance = (detail: unknown): void => {
    this.performances.push(detail)
    this.order.push('performance')
  }

  state = (detail: unknown): void => {
    this.states.push(detail)
    this.order.push('state')
  }
}

test('a streamed reply opens on the first non-blank token', () => {
  const sink = new RecordingSink()
  const reply = new AgentFaceChannel(sink).openReply('msg-1', 'zh-CN')

  reply.chunk('  ')
  assert.deepEqual(sink.speechEvents, [])

  reply.chunk('你好')
  reply.chunk('，')
  reply.end()

  assert.deepEqual(sink.speechEvents, [
    {
      phase: 'start',
      messageId: 'msg-1',
      source: 'reply',
      utteranceId: 'stream-1-msg-1',
      locale: 'zh-CN',
    },
    {
      phase: 'chunk',
      messageId: 'msg-1',
      source: 'reply',
      utteranceId: 'stream-1-msg-1',
      text: '你好',
      locale: 'zh-CN',
    },
    {
      phase: 'chunk',
      messageId: 'msg-1',
      source: 'reply',
      utteranceId: 'stream-1-msg-1',
      text: '，',
      locale: 'zh-CN',
    },
    {
      phase: 'end',
      messageId: 'msg-1',
      source: 'reply',
      utteranceId: 'stream-1-msg-1',
      locale: 'zh-CN',
    },
  ])
})

test('end and cancel are idempotent, and the next token reopens with a new id', () => {
  const sink = new RecordingSink()
  const reply = new AgentFaceChannel(sink).openReply('msg-1')

  reply.end()
  reply.cancel()
  assert.deepEqual(sink.speechEvents, [])

  reply.chunk('先说这句')
  reply.end()
  reply.end()
  reply.chunk('再说这句')
  reply.end()

  assert.deepEqual(
    sink.speechEvents.map((event) => [event.phase, event.utteranceId]),
    [
      ['start', 'stream-1-msg-1'],
      ['chunk', 'stream-1-msg-1'],
      ['end', 'stream-1-msg-1'],
      ['start', 'stream-2-msg-1'],
      ['chunk', 'stream-2-msg-1'],
      ['end', 'stream-2-msg-1'],
    ],
  )
})

test('a finished stream keeps the whole-line fallback from repeating it', () => {
  const sink = new RecordingSink()
  const channel = new AgentFaceChannel(sink)
  const reply = channel.openReply('msg-1', 'zh-CN')

  reply.chunk('东京 25°C')
  reply.end()

  channel.deliver({
    messageId: 'msg-1',
    text: '  东京   25°C  ',
    locale: 'zh-CN',
  })
  assert.deepEqual(sink.utterances, [])

  channel.deliver({ messageId: 'msg-1', text: '换了一句', locale: 'zh-CN' })
  assert.deepEqual(sink.utterances, [
    {
      messageId: 'msg-1',
      source: 'reply',
      text: '换了一句',
      utteranceId: 'reply-2-msg-1',
      locale: 'zh-CN',
    },
  ])
})

test('an interrupted stream still allows the whole-line fallback', () => {
  const sink = new RecordingSink()
  const channel = new AgentFaceChannel(sink)
  const reply = channel.openReply('msg-1')

  reply.chunk('说到一半')
  reply.cancel()
  channel.deliver({ messageId: 'msg-1', text: '说到一半' })

  assert.deepEqual(
    sink.utterances.map((item) => item.text),
    ['说到一半'],
  )
})

test('message-wide cancellation carries no utterance id', () => {
  const sink = new RecordingSink()
  new AgentFaceChannel(sink).cancel('msg-1')

  assert.deepEqual(sink.speechEvents, [
    { phase: 'cancel', messageId: 'msg-1', source: 'reply' },
  ])
})

test('the spoken ledger stays bounded', () => {
  const sink = new RecordingSink()
  const channel = new AgentFaceChannel(sink)
  for (let index = 0; index < 220; index += 1) {
    channel.deliver({ messageId: `msg-${index}`, text: `第 ${index} 句` })
  }
  // 最早的记录已被丢弃，同一句可以再说一次；最近的仍然去重。
  channel.deliver({ messageId: 'msg-0', text: '第 0 句' })
  channel.deliver({ messageId: 'msg-219', text: '第 219 句' })
  assert.equal(sink.utterances.length, 221)
  assert.equal(sink.utterances[220].messageId, 'msg-0')
})

test('proactive lines share the ledger without colliding with replies', () => {
  const sink = new RecordingSink()
  const channel = new AgentFaceChannel(sink)

  // 通知与回复是两套 id 空间：同样的正文互不遮挡。
  channel.deliver({
    messageId: 'notif-1',
    text: '报告生成好了',
    source: 'proactive',
  })
  channel.deliver({ messageId: 'msg-1', text: '报告生成好了' })
  assert.deepEqual(
    sink.utterances.map((item) => [item.source, item.utteranceId]),
    [
      ['proactive', 'proactive-1-notif-1'],
      ['reply', 'reply-2-msg-1'],
    ],
  )

  // 同一条通知重复投递不再说第二遍。
  channel.deliver({
    messageId: 'notif-1',
    text: '报告生成好了',
    source: 'proactive',
  })
  assert.equal(sink.utterances.length, 2)
})

test('a delivered line stages the performance before the words', () => {
  const sink = new RecordingSink()
  const performance = {
    phase: 'delivery' as const,
    moodRevision: 7,
    motionStyle: 'even' as const,
    plan: { cues: [] },
  }
  new AgentFaceChannel(sink).deliver({
    messageId: 'msg-1',
    text: '好的',
    locale: 'zh-CN',
    performance,
  })

  assert.deepEqual(sink.order, ['performance', 'utterance'])
  const staged = sink.performances[0] as {
    text: string
    source: string
    messageId: string
    performance: typeof performance
    motionIntentId?: string
  }
  assert.equal(staged.text, '好的')
  assert.equal(staged.source, 'reply')
  assert.equal(staged.messageId, 'msg-1')
  assert.deepEqual(staged.performance, performance)
  assert.equal(typeof staged.motionIntentId, 'string')
  assert.deepEqual(sink.utterances, [
    {
      messageId: 'msg-1',
      source: 'reply',
      text: '好的',
      utteranceId: 'reply-1-msg-1',
      locale: 'zh-CN',
    },
  ])
})

test('a line without words still stages, and words without a plan still speak', () => {
  const sink = new RecordingSink()
  const channel = new AgentFaceChannel(sink)
  const performance = {
    phase: 'reaction' as const,
    moodRevision: 1,
    motionStyle: 'even' as const,
    plan: { cues: [] },
  }

  channel.deliver({ messageId: 'msg-1', performance })
  channel.deliver({ messageId: 'msg-2', text: '在的' })
  channel.deliver({ messageId: 'msg-3', text: '   ' })

  assert.deepEqual(sink.order, ['performance', 'performance', 'utterance'])
  const first = sink.performances[0] as {
    text: string
    messageId: string
    motionIntentId?: string
    performance: typeof performance
  }
  const second = sink.performances[1] as {
    text: string
    messageId: string
    motionIntentId?: string
  }
  assert.equal(first.text, '')
  assert.equal(first.messageId, 'msg-1')
  assert.deepEqual(first.performance, performance)
  assert.equal(typeof first.motionIntentId, 'string')
  assert.equal(second.text, '在的')
  assert.equal(second.messageId, 'msg-2')
  assert.equal(second.motionIntentId, undefined)
  assert.deepEqual(
    sink.utterances.map((item) => item.messageId),
    ['msg-2'],
  )
})

test('state changes go straight through, unattached to any line', () => {
  const sink = new RecordingSink()
  new AgentFaceChannel(sink).updateState({ mood: 'anything', activity: 'idle' })

  assert.deepEqual(sink.states, [{ mood: 'anything', activity: 'idle' }])
  assert.deepEqual(sink.order, ['state'])
})
