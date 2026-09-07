import { fileURLToPath } from 'node:url'
import { expect, test } from '@playwright/test'
import { createServer } from 'vite'
import astroConfig from '../../astro.config.mjs'

// The speech harness aliases transports. This separate check must load the
// installed SDKs using the site's actual dependency settings, without credentials
// or contacting Agora. Excluding their UMD entries used to pass the mocked tests
// while returning undefined createClient / RTM in the real browser.
test('real Agora SDKs expose RTC, audio PTS and RTM in the browser', async ({
  page,
}, testInfo) => {
  const server = await createServer({
    configFile: false,
    root: fileURLToPath(new URL('../..', import.meta.url)),
    cacheDir: testInfo.outputPath('vite-cache'),
    optimizeDeps: { ...astroConfig.vite.optimizeDeps, noDiscovery: true },
    server: { host: '127.0.0.1', port: 0 },
    plugins: [
      {
        name: 'real-agora-sdk-check',
        resolveId(id) {
          if (id === '/sdk-check.js') return '\0sdk-check'
        },
        load(id) {
          if (id !== '\0sdk-check') return
          return `export async function check() {
            const rtc = (await import('agora-rtc-sdk-ng')).default;
            const rtm = (await import('agora-rtm')).default;
            rtc.setParameter('ENABLE_AUDIO_PTS_METADATA', true);
            const client = rtc.createClient({ mode: 'rtc', codec: 'vp8' });
            const listener = () => {};
            client.on('audio-pts', listener);
            client.off('audio-pts', listener);
            return {
              rtc: typeof client.join,
              microphone: typeof rtc.createMicrophoneAudioTrack,
              rtm: typeof rtm.RTM,
            };
          }`
        },
      },
    ],
  })
  try {
    await server.listen()
    const origin = server.resolvedUrls!.local[0]!
    await page.route('**/*', (route) => {
      const url = new URL(route.request().url())
      if (url.origin !== new URL(origin).origin) return route.abort()
      if (url.pathname === '/') {
        return route.fulfill({
          contentType: 'text/html',
          body: '<!doctype html><title>Real SDK check</title>',
        })
      }
      return route.continue()
    })
    await page.goto(origin)
    const result = await page.evaluate(async () => {
      const entry = '/sdk-check.js'
      return (await import(/* @vite-ignore */ entry)).check()
    })
    expect(result).toEqual({
      rtc: 'function',
      microphone: 'function',
      rtm: 'function',
    })
  } finally {
    await server.close()
  }
})
