import type { Page, Route } from '@playwright/test'
import { Buffer } from 'node:buffer'
import { expect, test } from '@playwright/test'

function audio() {
  const sampleRate = 16_000
  const samples = sampleRate
  const wav = Buffer.alloc(44 + samples * 2)
  wav.write('RIFF', 0)
  wav.writeUInt32LE(wav.length - 8, 4)
  wav.write('WAVEfmt ', 8)
  wav.writeUInt32LE(16, 16)
  wav.writeUInt16LE(1, 20)
  wav.writeUInt16LE(1, 22)
  wav.writeUInt32LE(sampleRate, 24)
  wav.writeUInt32LE(sampleRate * 2, 28)
  wav.writeUInt16LE(2, 32)
  wav.writeUInt16LE(16, 34)
  wav.write('data', 36)
  wav.writeUInt32LE(samples * 2, 40)
  for (let i = 0; i < samples; i++) {
    wav.writeInt16LE(
      Math.round(Math.sin((i * Math.PI * 360) / sampleRate) * 8000),
      44 + i * 2,
    )
  }
  return wav.toString('base64')
}

test.beforeEach(async ({ page }) => {
  await page.route('**/api/**', (route) => {
    const path = new URL(route.request().url()).pathname
    if (path === '/api/speech/status') {
      return route.fulfill({
        json: {
          available: true,
          tts_enabled: true,
          asr_enabled: true,
          convo_enabled: false,
          persona_speech_enabled: true,
        },
      })
    }
    if (path === '/api/csrf-token')
      return route.fulfill({ json: { csrf_token: null } })
    return route.fulfill({
      status: 503,
      json: { error: 'Unmocked request in speech regression' },
    })
  })
})

async function say(page: Page) {
  await page.getByRole('button', { name: 'Voice on', exact: true }).click()
  await expect(page.getByTestId('input-active')).toHaveText('true')
  // Keep the synthetic vowel sounding long enough to qualify as an utterance.
  await page.waitForTimeout(200)
  await page.getByRole('button', { name: 'Voice off', exact: true }).click()
}

test('two real AudioWorklet utterances commit in spoken order despite ASR returning backwards', async ({
  page,
}) => {
  const requests: Route[] = []
  await page.route('**/api/speech/asr', (route) => {
    requests.push(route)
  })
  await page.goto('/')
  await page.getByRole('button', { name: 'Listen', exact: true }).click()
  await expect(page.getByTestId('recording')).toHaveText('true')
  await say(page)
  await expect.poll(() => requests.length).toBe(1)
  await say(page)
  await expect.poll(() => requests.length).toBe(2)
  const firstWav = Buffer.from(
    requests[0]!.request().postDataJSON().audio_data,
    'base64',
  )
  expect(firstWav.toString('ascii', 0, 4)).toBe('RIFF')
  expect(firstWav.length).toBeGreaterThan(16_000)
  await requests[1]!.fulfill({
    json: { success: true, text: 'second sentence' },
  })
  await expect(page.getByTestId('transcripts').locator('li')).toHaveCount(0)
  await expect(page.getByTestId('processing')).toHaveText('true')
  await requests[0]!.fulfill({
    json: { success: true, text: 'first sentence' },
  })
  await expect(page.getByTestId('transcripts').locator('li')).toHaveText([
    'first sentence',
    'second sentence',
  ])
  await page.getByRole('button', { name: 'Stop', exact: true }).click()
  await expect(page.getByTestId('tracks')).toHaveText('ended')
})

test('stopping listening aborts pending ASR and never submits late text', async ({
  page,
}) => {
  let held: Route | undefined
  await page.route('**/api/speech/asr', (route) => {
    held = route
  })
  await page.goto('/')
  await page.getByRole('button', { name: 'Listen', exact: true }).click()
  await expect(page.getByTestId('recording')).toHaveText('true')
  await say(page)
  await expect.poll(() => Boolean(held)).toBe(true)
  const aborted = page.waitForEvent('requestfailed', (request) =>
    request.url().endsWith('/api/speech/asr'),
  )
  await page.getByRole('button', { name: 'Stop', exact: true }).click()
  await aborted
  await held!
    .fulfill({ json: { success: true, text: 'late sentence' } })
    .catch(() => {})
  await expect(page.getByTestId('processing')).toHaveText('false')
  await expect(page.getByTestId('transcripts').locator('li')).toHaveCount(0)
  await expect(page.getByTestId('tracks')).toHaveText('ended')
})

test('a permission result arriving after unmount closes the newly returned track', async ({
  page,
}) => {
  await page.goto('/?late-permission')
  await page.getByRole('button', { name: 'Record', exact: true }).click()
  await page.getByRole('button', { name: 'Unmount', exact: true }).click()
  await page
    .getByRole('button', { name: 'Grant microphone', exact: true })
    .click()
  await expect(page.getByTestId('tracks')).toHaveText('ended')
})

test('an old permission result cannot clear a newly requested conversation', async ({
  page,
}) => {
  await page.goto('/?late-permission')
  await page.getByRole('button', { name: 'Listen', exact: true }).click()
  await page.getByRole('button', { name: 'Stop', exact: true }).click()
  await page.getByRole('button', { name: 'Listen', exact: true }).click()
  await page
    .getByRole('button', { name: 'Grant microphone', exact: true })
    .click()
  await expect(page.getByTestId('tracks')).toHaveText('ended')
  await expect(page.getByTestId('conversation')).toHaveText('true')
  await page
    .getByRole('button', { name: 'Grant microphone', exact: true })
    .click()
  await expect(page.getByTestId('recording')).toHaveText('true')
  await expect(page.getByTestId('conversation')).toHaveText('true')
  await page.getByRole('button', { name: 'Stop', exact: true }).click()
  await expect(page.getByTestId('tracks')).toHaveText('ended,ended')
})

test('stopping a pending RTC start leaves the late server session without opening a microphone', async ({
  page,
}) => {
  let start: Route | undefined
  const stopped: string[] = []
  await page.route('**/api/speech/status', (route) =>
    route.fulfill({
      json: {
        available: true,
        tts_enabled: true,
        asr_enabled: true,
        convo_enabled: true,
        persona_speech_enabled: true,
      },
    }),
  )
  await page.route('**/api/speech/convo/start', (route) => {
    start = route
  })
  await page.route('**/api/speech/convo/stop', (route) => {
    stopped.push(route.request().postDataJSON().agent_id)
    return route.fulfill({ json: { success: true } })
  })
  await page.goto('/')
  await page.getByRole('button', { name: 'Listen', exact: true }).click()
  await expect.poll(() => Boolean(start)).toBe(true)
  await page.getByRole('button', { name: 'Stop', exact: true }).click()
  await start!.fulfill({
    json: { success: true, token: 'test-token', agent_id: 'old-session' },
  })
  await expect.poll(() => stopped).toEqual(['old-session'])
  await expect(page.getByTestId('recording')).toHaveText('false')
  await expect(page.getByTestId('tracks')).toHaveText('')
})

test('realtime cloud audio uses the shared run identity and provider close tears down listening', async ({
  page,
}) => {
  let ttsRequests = 0
  const stopped: string[] = []
  await page.route('**/api/speech/status', (route) =>
    route.fulfill({
      json: {
        available: true,
        tts_enabled: true,
        asr_enabled: true,
        convo_enabled: true,
        persona_speech_enabled: true,
      },
    }),
  )
  await page.route('**/api/speech/convo/start', (route) =>
    route.fulfill({
      json: {
        success: true,
        app_id: 'fake-app',
        channel: 'fake-channel',
        uid: 1,
        agent_uid: 8888,
        token: 'fake-token',
        agent_id: 'fake-agent',
        session_id: 'fixture-chat',
      },
    }),
  )
  await page.route('**/api/speech/convo/stop', (route) => {
    stopped.push(route.request().postDataJSON().agent_id)
    return route.fulfill({ json: { success: true } })
  })
  await page.route('**/api/speech/tts', (route) => {
    ttsRequests += 1
    return route.fulfill({
      status: 500,
      json: { error: 'must not synthesize twice' },
    })
  })
  await page.goto('/')
  await page.getByRole('button', { name: 'Listen', exact: true }).click()
  await expect(page.getByTestId('recording')).toHaveText('true')
  await page.evaluate(() =>
    window.__emitVoiceRun({
      runId: 'voice-run',
      sessionId: 'fixture-chat',
      input: 'hello',
      providerTurnId: 7,
      sequence: 1,
    }),
  )
  await page.evaluate(() =>
    window.__fakeAgoraTranscript({
      publisher: '8888',
      channelName: 'fake-channel',
      message: JSON.stringify({
        object: 'assistant.transcription',
        turn_id: 7,
        language: 'en-US',
        words: [
          {
            word: 'hello',
            start_ms: 1_000,
            duration_ms: 2_000,
            stable: true,
          },
        ],
      }),
    }),
  )
  await page.evaluate(() => window.__fakeAgoraVoice(true))
  await expect(page.getByTestId('speech-events')).toContainText(
    'msg_rtc_voice-run:start',
  )
  await expect(page.getByTestId('mouth')).toHaveText('true')
  await expect(page.getByTestId('articulation')).not.toHaveText('')
  expect(ttsRequests).toBe(0)

  await page.evaluate(() => window.__closeVoiceRuns())
  await expect(page.getByTestId('recording')).toHaveText('false')
  await expect(page.getByTestId('conversation')).toHaveText('false')
  await expect(page.getByTestId('mouth')).toHaveText('false')
  await expect.poll(() => stopped).toEqual(['fake-agent'])
})

test('cancelled TTS cannot block the next real WebAudio playback or its body behavior', async ({
  page,
}) => {
  let old: Route | undefined
  await page.route('**/api/speech/tts', (route) => {
    if (route.request().postDataJSON().text.includes('old')) {
      old = route
      return
    }
    return route.fulfill({ json: { success: true, audio: audio() } })
  })
  await page.goto('/')
  await page.getByRole('button', { name: 'Old reply', exact: true }).click()
  await expect.poll(() => Boolean(old)).toBe(true)
  const aborted = page.waitForEvent('requestfailed', (request) =>
    request.url().endsWith('/api/speech/tts'),
  )
  await page.getByRole('button', { name: 'Cancel reply', exact: true }).click()
  await aborted
  await page.getByRole('button', { name: 'New reply', exact: true }).click()
  await expect(page.getByTestId('speech-events')).toContainText('new:prosody')
  await expect(page.getByTestId('mouth')).toHaveText('true')
  await expect
    .poll(async () => Number(await page.getByTestId('behaviors').textContent()))
    .toBeGreaterThan(0)
  await old!
    .fulfill({ json: { success: true, audio: audio() } })
    .catch(() => {})
  await expect(page.getByTestId('speech-events')).toContainText('new:end')
  await expect(page.getByTestId('speech-events')).not.toContainText('old:start')
  await expect(page.getByTestId('mouth')).toHaveText('false')
})

test('cancelling one playing reply preserves the next reply and its voice presence', async ({
  page,
}) => {
  await page.route('**/api/speech/tts', (route) =>
    route.fulfill({ json: { success: true, audio: audio() } }),
  )
  await page.goto('/')
  await page.getByRole('button', { name: 'Old reply', exact: true }).click()
  await expect(page.getByTestId('speech-events')).toContainText('old:prosody')
  await page.getByRole('button', { name: 'New reply', exact: true }).click()
  await page
    .getByRole('button', { name: 'Cancel old reply', exact: true })
    .click()
  await expect(page.getByTestId('speech-events')).toContainText('old:cancel')
  await expect(page.getByTestId('speech-events')).toContainText('new:prosody')
  await expect(page.getByTestId('tts-playing')).toHaveText('true')
  await expect(page.getByTestId('speech-events')).toContainText('new:end')
  await expect(page.getByTestId('tts-playing')).toHaveText('false')
})
