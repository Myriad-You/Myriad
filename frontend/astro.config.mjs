import { readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import react from '@astrojs/react'
import tailwind from '@astrojs/tailwind'
import { defineConfig } from 'astro/config'
import { visualizer } from 'rollup-plugin-visualizer'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

// 读取 package.json 版本号
const pkg = JSON.parse(readFileSync(path.resolve(__dirname, 'package.json'), 'utf-8'))
const APP_VERSION = pkg.version || '0.1.0'

/**
 * 自定义 Vite 插件：SPA 路由回退
 * 将动态路由（如 /tapp/run/:id）在服务端重定向到 catch-all 页面
 * 但保留原始 URL，让 React Router 在客户端正确解析参数
 */
function spaFallbackPlugin() {
  return {
    name: 'spa-fallback',
    enforce: 'pre', // 确保在其他中间件之前执行
    configureServer(server) {
      // 直接添加中间件，不返回函数
      server.middlewares.use((req, res, next) => {
        const url = req.url || ''

        // 动态 Tapp 路由回退：/tapp/run/* 和 /tapp/detail/*
        // 服务端将这些路径重写为占位路径，但浏览器 URL 保持不变
        if (url.match(/^\/tapp\/run\/[^_/][^/]*/)) {
          req.url = '/tapp/run/_'
        }
        else if (url.match(/^\/tapp\/run(\?|$)/)) {
          // 多任务模式：/tapp/run 或 /tapp/run?multi=true
          req.url = '/tapp/run/_'
        }
        else if (url.match(/^\/tapp\/detail\/[^_/][^/]*/)) {
          req.url = '/tapp/detail/_'
        }

        next()
      })
    },
  }
}

// https://astro.build/config
export default defineConfig({
  integrations: [react(), tailwind()],
  // 使用 hybrid 模式：默认静态预渲染，但允许特定页面动态渲染
  // 这样可以支持 /tapp/run/:id 等动态路由
  output: 'static',
  server: {
    port: 4321,
    host: true,
  },
  build: {
    inlineStylesheets: 'auto',
  },
  // SPA 模式：所有路由都重定向到 index.html
  trailingSlash: 'never',
  vite: {
    define: {
      __APP_VERSION__: JSON.stringify(APP_VERSION),
    },
    plugins: [
      spaFallbackPlugin(), // 自定义 SPA 路由回退
      visualizer({
        filename: 'dist/stats.html',
        template: 'treemap',
        gzipSize: true,
        brotliSize: true,
      }),
    ],
    resolve: {
      alias: {
        '@': path.resolve(__dirname, './src'),
        '@components': path.resolve(__dirname, './src/components'),
        '@layouts': path.resolve(__dirname, './src/layouts'),
        '@lib': path.resolve(__dirname, './src/lib'),
        '@config': path.resolve(__dirname, './src/config.ts'),
      },
    },
    build: {
      cssCodeSplit: true,
      minify: 'terser',
      terserOptions: {
        compress: {
          drop_console: import.meta.env.PROD,
          drop_debugger: true,
          passes: 2,
        },
        mangle: {
          safari10: true,
        },
      },
      rollupOptions: {
        output: {
          manualChunks: (id) => {
            // React 核心 + React Router 合并到同一 chunk
            // 避免 React Router v7 在 React Context 初始化前加载导致 hydration 错误
            if (id.includes('node_modules/react/')
              || id.includes('node_modules/react-dom/')
              || id.includes('node_modules/react-router')
              || id.includes('node_modules/@remix-run')) {
              return 'react-vendor'
            }
            // Chart.js
            if (id.includes('node_modules/chart.js') || id.includes('node_modules/react-chartjs-2')) {
              return 'chart-vendor'
            }
            // Framer Motion
            if (id.includes('node_modules/framer-motion')) {
              return 'framer-motion'
            }
            // react-icons 各子包分开打包（仅动态导入时使用）
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
            // Axios
            if (id.includes('node_modules/axios')) {
              return 'axios'
            }
          },
          // 优化文件名用于长期缓存
          chunkFileNames: 'assets/[name]-[hash].js',
          entryFileNames: 'assets/[name]-[hash].js',
          assetFileNames: 'assets/[name]-[hash].[ext]',
        },
      },
      assetsInlineLimit: 4096,
      // 启用 gzip 和 brotli 压缩报告
      reportCompressedSize: true,
      chunkSizeWarningLimit: 1000,
    },
    server: {
      proxy: {
        '/api': {
          target: 'http://127.0.0.1:3000',
          changeOrigin: true,
        },
        '/health': {
          target: 'http://127.0.0.1:3000',
          changeOrigin: true,
        },
      },
    },
  },
})
