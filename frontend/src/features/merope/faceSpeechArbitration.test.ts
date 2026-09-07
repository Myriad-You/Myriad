import type { AgentFaceSink } from './agentFaceChannel'
import type { BodyAdapter, BodyIntent } from './body/types'
import type {
  MeropeSpeechEventDetail,
  SpeechUtteranceInput,
} from './speechEvents'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { AgentFaceChannel } from './agentFaceChannel'
import {
  arbitrateFaceSpeech,
  cancelGatedSpeech,
  deliverGatedLine,
  deliverProactiveFace,
  deliverWorkNotificationFace,
  FaceSpeechGate,
  notificationCarriesMeropeSpeech,
  openGatedReply,
  setLiveBody,
} from './faceSpeechArbitration'
import { setLiveFaceVisible } from './faceVisible'
import { getSpeechPipeline } from './speech/speechPipelineHost'

function enablePersonaSpeech(tts = false): void {
  getSpeechPipeline().applyStatus({
    available: true,
    tts_enabled: tts,
    persona_speech_enabled: true,
  })
}

function disablePersonaSpeech(): void {
  getSpeechPipeline().applyStatus({
    available: true,
    tts_enabled: true,
    persona_speech_enabled: false,
  })
}

class RecordingSink implements AgentFaceSink {
  readonly speechEvents: MeropeSpeechEventDetail[] = []
  readonly utterances: SpeechUtteranceInput[] = []
  readonly performances: unknown[] = []
  readonly states: unknown[] = []
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

test('visible Chat speaks; background Work is recorded without speech', () => {
  assert.equal(
    arbitrateFaceSpeech({
      visibleMode: 'chat',
      incomingMode: 'chat',
      chatUtteranceActive: false,
    }),
    'speak',
  )
  assert.equal(
    arbitrateFaceSpeech({
      visibleMode: 'chat',
      incomingMode: 'work',
      chatUtteranceActive: false,
    }),
    'record-without-speech',
  )
})

test('an in-progress Chat utterance blocks Work speech even if Work is visible', () => {
  assert.equal(
    arbitrateFaceSpeech({
      visibleMode: 'work',
      incomingMode: 'work',
      chatUtteranceActive: true,
    }),
    'record-without-speech',
  )
  assert.equal(
    arbitrateFaceSpeech({
      visibleMode: 'work',
      incomingMode: 'work',
      chatUtteranceActive: false,
    }),
    'speak',
  )
})

test('Chat mid-utterance continues while a background Work completion is recorded, not spoken', () => {
  const sink = new RecordingSink()
  const channel = new AgentFaceChannel(sink)
  const gate = new FaceSpeechGate(() => 'chat')
  const recorded: FaceDeliveryRecord[] = []

  const chat = openGatedReply(channel, gate, 'chat', 'chat-msg', 'zh-CN')
  chat.chunk('我正在说')

  const work = deliverGatedLine(channel, gate, 'work', {
    messageId: 'work-msg',
    text: '报告已经写好了',
    locale: 'zh-CN',
  })
  recorded.push(work)

  chat.chunk('这句话')
  chat.end()

  assert.equal(work.surface, 'record')
  assert.equal(work.messageId, 'work-msg')
  assert.equal(work.text, '报告已经写好了')
  assert.deepEqual(
    recorded.map((item) => item.surface),
    ['record'],
  )

  assert.deepEqual(
    sink.speechEvents.map((event) => [
      event.phase,
      event.messageId,
      'text' in event ? event.text : '',
    ]),
    [
      ['start', 'chat-msg', ''],
      ['chunk', 'chat-msg', '我正在说'],
      ['chunk', 'chat-msg', '这句话'],
      ['end', 'chat-msg', ''],
    ],
  )
  assert.equal(sink.utterances.length, 0)
  assert.equal(
    sink.speechEvents.some((event) => event.messageId === 'work-msg'),
    false,
  )
})

interface FaceDeliveryRecord {
  surface: 'speech' | 'record'
  messageId: string
  text?: string
}

test('notification-center Work completion is recorded, not spoken, while Chat is mid-utterance', () => {
  const sink = new RecordingSink()
  const channel = new AgentFaceChannel(sink)
  const gate = new FaceSpeechGate(() => 'chat')
  const chat = openGatedReply(channel, gate, 'chat', 'chat-msg')
  chat.chunk('还在说')

  const notice = deliverWorkNotificationFace(channel, gate, {
    id: 'notif-work-1',
    body: '任务完成了',
    performance: {
      phase: 'delivery',
      moodRevision: 3,
      motionStyle: 'even',
      plan: { cues: [] },
    },
    meropeState: { mood: 'calm', activity: 'idle' },
  })

  chat.chunk('完')
  chat.end()

  assert.equal(notice.surface, 'record')
  assert.equal(notice.messageId, 'notif-work-1')
  assert.equal(notice.text, '任务完成了')
  assert.deepEqual(sink.states, [{ mood: 'calm', activity: 'idle' }])
  assert.equal(sink.utterances.length, 0)
  assert.equal(sink.performances.length, 0)
  assert.equal(
    sink.speechEvents.some((event) => event.messageId === 'notif-work-1'),
    false,
  )
  assert.deepEqual(
    sink.speechEvents.map((event) => event.messageId),
    ['chat-msg', 'chat-msg', 'chat-msg', 'chat-msg'],
  )
})

test('Chat start then engine cancel releases occupancy so visible Work may speak', () => {
  enablePersonaSpeech()
  setLiveFaceVisible(true)
  const sink = new RecordingSink()
  const channel = new AgentFaceChannel(sink)
  let visible: 'work' | 'chat' = 'chat'
  const gate = new FaceSpeechGate(() => visible)
  const chat = openGatedReply(channel, gate, 'chat', 'chat-msg')
  chat.chunk('说到一半')
  // Engine cancel path — not the ReplyUtterance wrapper's cancel().
  cancelGatedSpeech(channel, gate, 'chat-msg')
  visible = 'work'

  const work = deliverGatedLine(channel, gate, 'work', {
    messageId: 'work-msg',
    text: '报告已经写好了',
  })
  assert.equal(work.surface, 'speech')
  assert.deepEqual(
    sink.utterances.map((item) => [item.messageId, item.text]),
    [['work-msg', '报告已经写好了']],
  )
})

test('notification center and engine cancel go through the gated Work/Chat helpers', () => {
  const panel = readFileSync(
    new URL('../../components/GlobalControlPanel.tsx', import.meta.url),
    'utf8',
  )
  const engine = readFileSync(
    new URL('../../components/agent-panel/AgentEngine.tsx', import.meta.url),
    'utf8',
  )
  assert.match(panel, /deliverProactiveFace\(/)
  assert.doesNotMatch(panel, /deliverWorkNotificationFace\(/)
  assert.doesNotMatch(panel, /notificationCarriesMeropeSpeech/)
  assert.doesNotMatch(panel, /n\.metadata\?\.event_key/)
  assert.doesNotMatch(panel, /agentFace\.deliver\(/)
  // The engine cancels through the facade and never names the channel itself.
  assert.match(engine, /stopTurnSpeech\(/)
  assert.doesNotMatch(engine, /agentFace\./)
  const face = readFileSync(new URL('./engineFace.ts', import.meta.url), 'utf8')
  assert.match(face, /cancelGatedSpeech\(/)
  assert.doesNotMatch(face, /agentFace\.cancel\(/)
  const arbitration = readFileSync(
    new URL('./faceSpeechArbitration.ts', import.meta.url),
    'utf8',
  )
  assert.match(arbitration, /speakUnmountedLine/)
  assert.doesNotMatch(arbitration, /body\/host|getProductionBody|runtimeHost/)
  assert.doesNotMatch(engine, /speakLine/)
  assert.doesNotMatch(panel, /speakLine/)
})

test('generic producer event keys are not persona speech', () => {
  assert.equal(
    notificationCarriesMeropeSpeech({ event_key: 'brew.source_error' }),
    false,
  )
  assert.equal(
    notificationCarriesMeropeSpeech({ event_key: 'agent.task_completed' }),
    false,
  )
  assert.equal(
    notificationCarriesMeropeSpeech({
      event_key: 'agent.merope.platform_activity',
    }),
    true,
  )
  assert.equal(notificationCarriesMeropeSpeech({ performance: {} }), true)
})

test('hidden face records a line instead of pretending it was spoken', () => {
  setLiveFaceVisible(false)
  const sink = new RecordingSink()
  const channel = new AgentFaceChannel(sink)
  const gate = new FaceSpeechGate(() => 'chat')
  const result = deliverGatedLine(channel, gate, 'chat', {
    messageId: 'proactive-1',
    text: '想跟你说一声',
    source: 'proactive',
  })
  assert.equal(result.surface, 'record')
  assert.equal(sink.utterances.length, 0)
  setLiveFaceVisible(true)
})

test('a live body is the app-layer outlet for a finished line', () => {
  enablePersonaSpeech()
  setLiveFaceVisible(true)
  const intended: BodyIntent[] = []
  const body: BodyAdapter = {
    capabilities: () => ({ semantic: [] }),
    state: () => ({
      expression: 'steady',
      posture: 'neutral',
      acting: null,
      speaking: false,
      faceVisible: true,
      capabilities: [],
    }),
    intend: (intent) => {
      intended.push(intent)
    },
  }
  setLiveBody(body)
  try {
    const sink = new RecordingSink()
    const channel = new AgentFaceChannel(sink)
    const gate = new FaceSpeechGate(() => 'chat')
    const result = deliverGatedLine(channel, gate, 'chat', {
      messageId: 'chat-msg',
      text: '想跟你说一声',
      performance: {
        phase: 'delivery',
        moodRevision: 1,
        motionStyle: 'even',
        plan: {
          baseline: {
            expression: 'warm',
            posture: 'neutral',
            motionEnergy: 0.8,
            attention: 0.9,
          },
          cues: [],
        },
      },
    })
    assert.equal(result.surface, 'speech')
    assert.equal(intended.length, 1)
    assert.equal(intended[0]?.speechText, '想跟你说一声')
    assert.ok(intended[0]?.performance)
    assert.equal(
      (sink.performances[0] as { performance?: unknown } | undefined)
        ?.performance,
      undefined,
    )
    assert.deepEqual(
      sink.utterances.map((item) => [item.messageId, item.text]),
      [['chat-msg', '想跟你说一声']],
    )

    const chat = openGatedReply(channel, gate, 'chat', 'chat-busy')
    chat.chunk('还在说')
    intended.length = 0
    const blocked = deliverGatedLine(channel, gate, 'work', {
      messageId: 'work-msg',
      text: '报告已经写好了',
      performance: {
        phase: 'delivery',
        moodRevision: 2,
        motionStyle: 'even',
        plan: { cues: [] },
      },
    })
    assert.equal(blocked.surface, 'record')
    assert.equal(intended.length, 0)
    chat.end()
  } finally {
    setLiveBody(null)
  }
})

test('proactive face records while Chat currently holds the mouth', () => {
  const sink = new RecordingSink()
  const channel = new AgentFaceChannel(sink)
  const gate = new FaceSpeechGate(() => 'chat')
  gate.chatUtteranceActive = true
  const result = deliverProactiveFace(channel, gate, {
    id: 'live-1',
    body: '今天又见到你了。',
  })
  assert.equal(result.surface, 'record')
  assert.equal(sink.utterances.length, 0)
})

test('Work completion still speaks when Chat is not talking and Work is visible', () => {
  enablePersonaSpeech()
  setLiveFaceVisible(true)
  const sink = new RecordingSink()
  const channel = new AgentFaceChannel(sink)
  const gate = new FaceSpeechGate(() => 'work')

  const work = deliverGatedLine(channel, gate, 'work', {
    messageId: 'work-msg',
    text: '报告已经写好了',
  })

  assert.equal(work.surface, 'speech')
  assert.deepEqual(
    sink.utterances.map((item) => [item.messageId, item.text]),
    [['work-msg', '报告已经写好了']],
  )
})

test('persona speech switch off still mouths the line without TTS', () => {
  disablePersonaSpeech()
  assert.equal(getSpeechPipeline().available, false)
  setLiveFaceVisible(true)
  const intended: BodyIntent[] = []
  const body: BodyAdapter = {
    capabilities: () => ({ semantic: [] }),
    state: () => ({
      expression: 'steady',
      posture: 'neutral',
      acting: null,
      speaking: false,
      faceVisible: true,
      capabilities: [],
    }),
    intend: (intent) => {
      intended.push(intent)
    },
  }
  setLiveBody(body)
  try {
    const sink = new RecordingSink()
    const channel = new AgentFaceChannel(sink)
    const gate = new FaceSpeechGate(() => 'chat')
    const spoken = deliverGatedLine(channel, gate, 'chat', {
      messageId: 'chat-msg',
      text: '这句只动嘴',
    })
    assert.equal(spoken.surface, 'speech')
    assert.equal(intended[0]?.speechText, '这句只动嘴')
    assert.deepEqual(
      sink.utterances.map((item) => [item.messageId, item.text]),
      [['chat-msg', '这句只动嘴']],
    )
  } finally {
    setLiveBody(null)
  }
})
