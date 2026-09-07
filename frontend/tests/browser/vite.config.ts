import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'

// Test-only entrypoints cannot become Astro production pages. No backend proxy:
// every service request in this harness must be explicitly mocked by the test.
export default defineConfig({
  root: fileURLToPath(new URL('./fixture', import.meta.url)),
  resolve: {
    alias: {
      'agora-rtc-sdk-ng': fileURLToPath(
        new URL('./fixture/fakeAgora.ts', import.meta.url),
      ),
      'agora-rtm': fileURLToPath(
        new URL('./fixture/fakeAgoraRtm.ts', import.meta.url),
      ),
    },
  },
  server: {
    host: '127.0.0.1',
    port: 4179,
    strictPort: true,
    fs: { allow: [fileURLToPath(new URL('../..', import.meta.url))] },
  },
})
