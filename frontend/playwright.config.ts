import process from 'node:process'
import { defineConfig } from '@playwright/test'

const port = Number(process.env.MYRIAD_BROWSER_TEST_PORT || 4179)

export default defineConfig({
  testDir: './tests/browser',
  testMatch: '*.spec.ts',
  timeout: 25_000,
  workers: 1,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    trace: 'retain-on-failure',
    launchOptions: { args: ['--autoplay-policy=no-user-gesture-required'] },
  },
  webServer: {
    command: `node node_modules/vite/bin/vite.js --config tests/browser/vite.config.ts --port ${port}`,
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: false,
  },
})
