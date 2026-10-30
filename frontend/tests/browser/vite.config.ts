import process from 'node:process'
import { fileURLToPath } from 'node:url'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'

const port = Number(process.env.MYRIAD_BROWSER_TEST_PORT || 4179)

// Test-only entrypoints cannot become production pages. No backend proxy:
// every service request in this harness must be explicitly mocked by the test.
export default defineConfig({
  plugins: [tailwindcss()],
  root: fileURLToPath(new URL('./fixture', import.meta.url)),
  publicDir: fileURLToPath(new URL('../../public', import.meta.url)),
  // A concurrent Vite dev server must not replace this runner's optimized
  // React modules mid-test (two dispatcher instances cause invalid hook calls).
  cacheDir: fileURLToPath(new URL(`../../node_modules/.vite/browser-tests-${port}`, import.meta.url)),
  optimizeDeps: {
    include: ['react', 'react-dom/client', 'react/jsx-runtime', 'lucide-react'],
  },
  build: {
    target: 'es2025',
  },
  resolve: {
    dedupe: ['react', 'react-dom'],
    alias: {
      '@lib': fileURLToPath(new URL('../../src/lib', import.meta.url)),
      '@myriad/core': fileURLToPath(new URL('../../../core', import.meta.url)),
      'agora-rtc-sdk-ng': fileURLToPath(
        new URL('./fixture/fakeAgora.ts', import.meta.url),
      ),
      'agora-rtm': fileURLToPath(
        new URL('./fixture/fakeAgoraRtm.ts', import.meta.url),
      ),
    },
  },
  server: {
    // Each Playwright run starts a server. Editing another task must not
    // navigate a GPU replay away midway through its assertions.
    hmr: false,
    host: '127.0.0.1',
    port,
    strictPort: true,
    fs: {
      allow: [
        fileURLToPath(new URL('../..', import.meta.url)),
        fileURLToPath(new URL('../../../figure', import.meta.url)),
        fileURLToPath(new URL('../../../core/shared', import.meta.url)),
      ],
    },
  },
})
