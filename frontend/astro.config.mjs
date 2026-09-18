import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import react from '@astrojs/react'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig, fontProviders } from 'astro/config'
import { backendDevProxyPlugin } from './scripts/astro/backendDevProxy.mjs'
import { BACKEND_TARGET } from './scripts/astro/constants.mjs'
import { deferNonCriticalCssIntegration } from './scripts/astro/deferNonCriticalCss.mjs'
import { reloadOnOutdatedOptimizeDepPlugin } from './scripts/astro/reloadOnOutdatedOptimizeDep.mjs'
import { siteBrandingStampPlugin } from './scripts/astro/siteBrandingStampPlugin.mjs'
import { spaFallbackPlugin } from './scripts/astro/spaFallback.mjs'
import { stripDevSourcemapsPlugin } from './scripts/astro/stripDevSourcemaps.mjs'
import { SITE_FONTS } from './src/siteFonts.mjs'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

const pkg = JSON.parse(
  readFileSync(path.resolve(__dirname, 'package.json'), 'utf-8'),
)
const APP_VERSION = pkg.version || '0.5.1'

export default defineConfig({
  integrations: [
    react(),
    deferNonCriticalCssIntegration(),
    {
      name: 'isolate-vite-command-cache',
      hooks: {
        'astro:config:setup': ({ command, updateConfig }) => {
          // A build must not overwrite a running dev server's React prebundle:
          // the production jsx-dev-runtime exports jsxDEV as undefined.
          updateConfig({
            vite: {
              cacheDir: path.resolve(__dirname, 'node_modules/.vite', command),
            },
          })
        },
      },
    },
  ],
  output: 'static',
  server: {
    port: 1102,
    host: true,
  },
  build: {
    inlineStylesheets: 'auto',
    // Pair with trailingSlash: 'never' so we emit dist/setup.html, not
    // dist/setup/index.html. Directory output makes tower-http ServeDir 307
    // slashless directory requests to /setup/, which fights the no-trailing-slash
    // URL convention and loops /setup ↔ /setup/. file format has no directory, no 307.
    format: 'file',
  },
  // Astro Fonts API self-hosts these faces; no runtime Google Fonts CDN.
  fonts: SITE_FONTS.map((font) => ({
    provider: fontProviders.google(),
    name: font.name,
    cssVariable: font.cssVariable,
    weights: font.weights,
    styles: font.styles,
    fallbacks: font.fallbacks,
  })),
  trailingSlash: 'never',
  // Highlighting is Prism on the React side, not Astro Markdown / Shiki.
  // Disable the default highlighter so non-overlay paths do not import('shiki/wasm').
  markdown: {
    syntaxHighlight: false,
  },
  vite: {
    define: {
      __APP_VERSION__: JSON.stringify(APP_VERSION),
    },
    css: {
      // Inline CSS maps are not needed for dest HMR and inflate the transform cache.
      devSourcemap: false,
    },
    optimizeDeps: {
      // Don't block first-paint on the full crawl. Default true waits for every
      // discovered dep; Agora ESM's ua-parser-js default-import then held
      // react.js forever and the PageLoader never dismissed.
      holdUntilCrawlEnd: false,
      // Mid-session discovery rewrites the dep browserHash. Vite then 504s
      // the old hash, and Astro's island retry of App.tsx dies with
      // "Outdated Optimize Dep" instead of a clean full reload.
      // Rolldown can also rename `client-*.js` without changing `?v=`;
      // reloadOnOutdatedOptimizeDepPlugin turns that 504 into a full reload.
      noDiscovery: true,
      include: [
        'axios',
        'isomorphic-dompurify',
        'jszip',
        'prismjs',
        'prismjs/components/prism-json',
        'react',
        'react-dom',
        'react-dom/client',
        'react-router-dom',
        // Not on the first-paint graph; still prebundle so mid-session discovery does not 504.
        'ag-psd',
        'motion/react',
        'pinyin-pro',
        // The default SDK entries are self-contained UMD, not browser ESM.
        // Prebundle them to expose exports; excluding them yields undefined
        // createClient / RTM in the browser. Do not use the optional ESM tree.
        'agora-rtc-sdk-ng',
        'agora-rtm',
      ],
      // Icon barrels are one file per pack (si ≈ 5MB). Prebundling them plus
      // an inline source map was a 13MB script on every page that imports
      // `@lib/icons`. Dest ESM + noDiscovery keeps them out of the shared
      // prebundle hash (no mid-session 504).
      // Agora's optional ESM tree must not enter the Vite prebundle.
      // Stay on the self-contained UMD entries; a failed ESM crawl holds every
      // optimized dep — the PageLoader never dismisses.
      exclude: [
        'lucide-react',
        'react-icons/bs',
        'react-icons/fa',
        'react-icons/fa6',
        'react-icons/lu',
        'react-icons/si',
        '@agora-js/shared',
        '@agora-js/media',
        '@agora-js/report',
        '@agora-js/protocol',
        'ua-parser-js',
        // Shiki's bundle-full does import('shiki/wasm'). Prebundling it lets
        // Vite's module-runner rewrite that specifier to a file path that Node
        // then cannot load under pnpm's isolated layout.
        'shiki',
      ],
    },
    ssr: {
      external: [
        'agora-rtc-sdk-ng',
        'agora-rtm',
        '@agora-js/shared',
        '@agora-js/media',
        '@agora-js/report',
        '@agora-js/protocol',
        // Keep the highlighter on Node's native ESM so import('shiki/wasm')
        // resolves from inside the shiki package, not the project root.
        'shiki',
      ],
    },
    plugins: [
      tailwindcss(),
      reloadOnOutdatedOptimizeDepPlugin(),
      siteBrandingStampPlugin({ backendTarget: BACKEND_TARGET, frontendRoot: __dirname }),
      backendDevProxyPlugin(),
      spaFallbackPlugin(),
      stripDevSourcemapsPlugin(),
    ],
    resolve: {
      alias: {
        '@': path.resolve(__dirname, './src'),
        '@components': path.resolve(__dirname, './src/components'),
        '@layouts': path.resolve(__dirname, './src/layouts'),
        '@lib': path.resolve(__dirname, './src/lib'),
        '@config': path.resolve(__dirname, './src/config.ts'),
        '@shared': path.resolve(__dirname, '../shared'),
      },
    },
    environments: {
      client: {
        build: {
          target: 'es2025',
          rollupOptions: {
            output: {
              codeSplitting: {
                groups: [
                  {
                    // Claim React before other groups recursively capture their
                    // dependencies. A manualChunks name alone lets Motion take
                    // jsx-runtime and forces every JSX entry to load Motion.
                    name: 'react-vendor',
                    priority: 100,
                    test: (id) =>
                      id.includes('node_modules/react/') ||
                      id.includes('node_modules/react-dom/') ||
                      id.includes('node_modules/react-router') ||
                      id.includes('node_modules/@remix-run') ||
                      id.includes('jsx-runtime'),
                  },
                  {
                    name: (id) => {
                      if (
                        id.includes('node_modules/chart.js') ||
                        id.includes('node_modules/react-chartjs-2')
                      ) {
                        return 'chart-vendor'
                      }
                      // motion-dom / motion-utils are separate packages whose paths
                      // also contain node_modules/motion. Merging them into `motion`
                      // would make shared helpers a static dep of the 124K chunk and
                      // break lazyMotion's dynamic load.
                      if (id.includes('node_modules/motion-utils')) {
                        return 'motion-utils'
                      }
                      if (id.includes('node_modules/motion-dom')) {
                        return 'motion-dom'
                      }
                      if (id.includes('node_modules/motion')) {
                        return 'motion-vendor'
                      }
                      if (id.includes('node_modules/react-icons/fa6/')) {
                        return 'icons-fa6'
                      }
                      if (id.includes('node_modules/react-icons/fa/')) {
                        return 'icons-fa'
                      }
                      if (id.includes('node_modules/react-icons/si/')) {
                        return 'icons-si'
                      }
                      if (id.includes('node_modules/react-icons')) {
                        return 'icons-base'
                      }
                      if (id.includes('node_modules/axios')) {
                        return 'axios'
                      }
                    },
                  },
                ],
              },
              chunkFileNames: 'assets/[name]-[hash].js',
              entryFileNames: 'assets/[name]-[hash].js',
              assetFileNames: 'assets/[name]-[hash].[ext]',
            },
          },
        },
      },
    },
    build: {
      target: 'es2025',
      cssCodeSplit: true,
      minify: 'terser',
      terserOptions: {
        compress: {
          // eslint-disable-next-line node/prefer-global/process
          drop_console: process.env.NODE_ENV === 'production',
          drop_debugger: true,
          passes: 2,
        },
      },
      assetsInlineLimit: 4096,
      reportCompressedSize: true,
      chunkSizeWarningLimit: 1000,
    },
  },
})
