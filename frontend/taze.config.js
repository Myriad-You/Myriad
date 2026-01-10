import { defineConfig } from 'taze'

export default defineConfig({
  force: true,
  write: true,
  install: true,
  ignorePaths: [
    '**/node_modules/**',
  ],
  packageMode: {
    'typescript': 'major',
    'eslint': 'ignore',
    'astro': 'minor',
    '@astrojs/': 'minor',
    'tailwindcss': 'minor',
    'react': 'minor',
    'react-dom': 'minor',
    'react-router-dom': 'minor',
  },
  depFields: {
    overrides: false,
  },
})
