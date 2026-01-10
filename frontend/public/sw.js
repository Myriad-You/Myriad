// Service Worker v2.2 for Myriad
// 性能优化版 - 缓存策略 + 安全过滤 + 206 响应处理
// v2.2: 移除壁纸图片缓存，避免跨域问题

const CACHE_VERSION = 'myriad-v2.2'
const STATIC_CACHE = `${CACHE_VERSION}-static`
const DYNAMIC_CACHE = `${CACHE_VERSION}-dynamic`
const IMAGE_CACHE = `${CACHE_VERSION}-images`

// 需要预缓存的静态资源
const STATIC_ASSETS = [
  '/',
  '/favicon.svg',
]

// 缓存配置
const MAX_DYNAMIC_CACHE_SIZE = 50
const MAX_IMAGE_CACHE_SIZE = 50 // 增加图片缓存上限
const CACHE_MAX_AGE = {
  static: 30 * 24 * 60 * 60 * 1000, // 30天
  images: 7 * 24 * 60 * 60 * 1000, // 7天
  wallpaper: 24 * 60 * 60 * 1000, // 壁纸缓存1天（可能是动态API）
  api: 5 * 60 * 1000, // 5分钟
}

// 壁纸 CDN 域名列表（优先缓存）
const WALLPAPER_CDN_DOMAINS = [
  'nmxc.ltd',
  's.nmxc.ltd',
  'picsum.photos',
  'unsplash.com',
  'source.unsplash.com',
]

// 安装 Service Worker
self.addEventListener('install', (event) => {
  console.log('[SW] Installing Service Worker...')

  event.waitUntil(
    caches.open(STATIC_CACHE)
      .then((cache) => {
        console.log('[SW] Precaching static assets')
        return cache.addAll(STATIC_ASSETS)
      })
      .catch((err) => {
        console.error('[SW] Precache failed:', err)
      }),
  )

  // 强制激活新的 Service Worker
  self.skipWaiting()
})

// 激活 Service Worker 并清理旧缓存
self.addEventListener('activate', (event) => {
  console.log('[SW] Activating Service Worker...')

  event.waitUntil(
    caches.keys().then((keys) => {
      return Promise.all(
        keys
          .filter(key => key.startsWith('myriad-') && key !== STATIC_CACHE && key !== DYNAMIC_CACHE && key !== IMAGE_CACHE)
          .map((key) => {
            console.log('[SW] Removing old cache:', key)
            return caches.delete(key)
          }),
      )
    }),
  )

  // 立即控制所有客户端
  return self.clients.claim()
})

// 限制缓存大小
async function limitCacheSize(cacheName, maxSize) {
  const cache = await caches.open(cacheName)
  const keys = await cache.keys()

  if (keys.length > maxSize) {
    const keysToDelete = keys.slice(0, keys.length - maxSize)
    await Promise.all(keysToDelete.map(key => cache.delete(key)))
  }
}

// 检查缓存是否过期
function isCacheExpired(response, maxAge) {
  const cachedDate = response.headers.get('sw-cached-date')
  if (!cachedDate)
    return false

  const cacheTime = new Date(cachedDate).getTime()
  const now = Date.now()
  return (now - cacheTime) > maxAge
}

// 添加缓存时间戳
async function cacheWithTimestamp(cacheName, request, response) {
  // ✅ 跳过 206 Partial Content 响应（Cache API 不支持）
  if (response.status === 206) {
    console.log('[SW] Skipping cache for 206 response:', request.url)
    return
  }

  const cache = await caches.open(cacheName)
  const headers = new Headers(response.headers)
  headers.set('sw-cached-date', new Date().toISOString())

  const blob = await response.blob()
  const cachedResponse = new Response(blob, {
    status: response.status,
    statusText: response.statusText,
    headers,
  })

  await cache.put(request, cachedResponse)
}

// 拦截请求
self.addEventListener('fetch', (event) => {
  const { request } = event
  const url = new URL(request.url)

  // 跳过非 HTTP(S) 请求
  if (!url.protocol.startsWith('http')) {
    return
  }

  // API 请求 - 网络优先策略
  if (url.pathname.startsWith('/api/')) {
    // ✅ 安全修复 P0: 排除敏感API，防止XSS通过Cache API读取认证数据
    const isSensitiveAPI
      = url.pathname.includes('/auth/') // 认证相关
        || url.pathname.includes('/config') // 配置信息
        || url.pathname.includes('/profile/report') // 个人报告
        || url.pathname.includes('/csrf-token') // CSRF Token
        || url.pathname.includes('/setup/') // 设置接口

    event.respondWith(
      fetch(request)
        .then((response) => {
          // ✅ 安全修复 P0: 只缓存非敏感的成功 GET 请求（排除 206 响应）
          if (request.method === 'GET' && response.ok && response.status !== 206 && !isSensitiveAPI) {
            const responseClone = response.clone()
            caches.open(DYNAMIC_CACHE).then((cache) => {
              cache.put(request, responseClone)
              limitCacheSize(DYNAMIC_CACHE, MAX_DYNAMIC_CACHE_SIZE)
            })
          }
          return response
        })
        .catch(() => {
          // ✅ 安全修复 P0: 敏感API失败时不从缓存读取
          if (isSensitiveAPI) {
            return new Response(JSON.stringify({ error: 'Network error' }), {
              status: 503,
              headers: { 'Content-Type': 'application/json' },
            })
          }
          // 网络失败时尝试从缓存获取（仅非敏感API）
          return caches.match(request)
        }),
    )
    return
  }

  // 图片请求 - 缓存优先策略(带过期检查)
  // ⚠️ 壁纸图片不缓存，避免跨域问题
  if (request.destination === 'image' || /\.(jpg|jpeg|png|gif|webp|svg|avif)$/i.test(url.pathname)) {
    // 检查是否为壁纸 CDN 图片
    const isWallpaperCDN = WALLPAPER_CDN_DOMAINS.some(domain => url.hostname.includes(domain))

    // 壁纸图片不走缓存，直接网络请求（避免跨域缓存问题）
    if (isWallpaperCDN) {
      event.respondWith(
        fetch(request, { mode: 'cors', credentials: 'omit' })
          .catch((error) => {
            console.warn('[SW] Wallpaper fetch failed:', request.url.slice(0, 80), error)
            // 返回透明占位
            return new Response(
              new Blob([]),
              {
                status: 200,
                headers: { 'Content-Type': 'image/svg+xml' },
              },
            )
          }),
      )
      return
    }

    const cacheMaxAge = CACHE_MAX_AGE.images

    event.respondWith(
      caches.match(request)
        .then(async (cachedResponse) => {
          // 检查缓存是否过期
          if (cachedResponse && !isCacheExpired(cachedResponse, cacheMaxAge)) {
            return cachedResponse
          }

          try {
            const response = await fetch(request, { mode: 'cors', credentials: 'omit' })

            if (response.ok && response.status !== 206) {
              const responseClone = response.clone()
              await cacheWithTimestamp(IMAGE_CACHE, request, responseClone)
              limitCacheSize(IMAGE_CACHE, MAX_IMAGE_CACHE_SIZE)
            }
            return response
          }
          catch (error) {
            // 网络失败时返回过期缓存
            if (cachedResponse) {
              console.log('[SW] Using cached image after network error:', request.url)
              return cachedResponse
            }
            console.warn('[SW] Image fetch failed:', request.url, error)
            return new Response(
              new Blob([]),
              {
                status: 200,
                headers: { 'Content-Type': 'image/svg+xml' },
              },
            )
          }
        }),
    )
    return
  }

  // CSS/JS静态资源 - 缓存优先(带过期检查)
  if (/\.(css|js|woff2?)$/i.test(url.pathname)) {
    event.respondWith(
      caches.match(request)
        .then(async (cachedResponse) => {
          if (cachedResponse && !isCacheExpired(cachedResponse, CACHE_MAX_AGE.static)) {
            return cachedResponse
          }

          try {
            const response = await fetch(request)
            if (response.ok) {
              const responseClone = response.clone()
              await cacheWithTimestamp(STATIC_CACHE, request, responseClone)
            }
            return response
          }
          catch (error) {
            if (cachedResponse) {
              return cachedResponse
            }
            throw error
          }
        }),
    )
    return
  }

  // 其他请求 - 网络优先,缓存回退
  event.respondWith(
    fetch(request)
      .then((response) => {
        // ✅ 只缓存成功的 GET 请求（排除 206 响应）
        if (request.method === 'GET' && response.ok && response.status !== 206) {
          const responseClone = response.clone()
          caches.open(DYNAMIC_CACHE).then((cache) => {
            cache.put(request, responseClone)
            limitCacheSize(DYNAMIC_CACHE, MAX_DYNAMIC_CACHE_SIZE)
          })
        }
        return response
      })
      .catch(() => {
        return caches.match(request).then((cachedResponse) => {
          if (cachedResponse) {
            return cachedResponse
          }
          // 导航请求失败返回首页
          if (request.mode === 'navigate') {
            return caches.match('/')
          }
          throw new Error('Network failed and no cache available')
        })
      }),
  )
})

// 消息处理
self.addEventListener('message', (event) => {
  if (event.data && event.data.type === 'SKIP_WAITING') {
    self.skipWaiting()
  }

  if (event.data && event.data.type === 'CLEAR_CACHE') {
    event.waitUntil(
      caches.keys().then((keys) => {
        return Promise.all(keys.map(key => caches.delete(key)))
      }).then(() => {
        event.ports[0].postMessage({ success: true })
      }),
    )
  }
})
