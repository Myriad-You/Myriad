import process from 'node:process'
import { defineConfig } from '@playwright/test'

const baseURL = process.env.MYRIAD_SMOKE_BASE_URL || 'http://127.0.0.1:18103'

// API integration smoke. Not a production-page UI end-to-end suite.
export default defineConfig({
  testDir: './tests/smoke',
  testMatch: '*.spec.ts',
  timeout: 90_000,
  workers: 1,
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  use: {
    baseURL,
    extraHTTPHeaders: { Origin: 'http://localhost:1102' },
  },
})
