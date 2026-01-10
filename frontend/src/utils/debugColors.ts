/**
 * 壁纸颜色提取调试工具 v2.1
 *
 * 在浏览器控制台中使用：
 * - window.__debugColors.current()         查看当前应用的颜色
 * - window.__debugColors.wallpaperStatus() 查看壁纸一致性状态
 * - window.__debugColors.state()           查看完整的壁纸状态管理器信息
 * - window.__debugColors.cache()           查看缓存信息
 * - window.__debugColors.test(url)         测试颜色提取
 * - window.__debugColors.verify(url)       验证URL一致性
 * - window.__debugColors.clear()           清除所有缓存
 * - window.__debugColors.help()            显示帮助信息
 *
 * @module debugColors
 * @version 2.1
 */

import { applyColorPalette, clearColorCache, extractColorsFromImage } from './colorExtractor'
import { clearColorCache as clearWallpaperCache, getCacheInfo } from './wallpaperColorCache'
import { extractBackgroundUrl, wallpaperState } from './wallpaperState'

// ============================================================================
// 辅助函数
// ============================================================================

/** 截断URL用于显示 */
function truncateUrl(url: string | null | undefined, maxLen = 80): string | null {
  if (!url)
    return null
  return url.length > maxLen ? `${url.substring(0, maxLen)}...` : url
}

/** 格式化时间间隔 */
function formatTimeAgo(timestamp: number | null | undefined): string {
  if (!timestamp)
    return '未记录'
  const seconds = Math.round((Date.now() - timestamp) / 1000)
  if (seconds < 60)
    return `${seconds}秒前`
  if (seconds < 3600)
    return `${Math.round(seconds / 60)}分钟前`
  return `${Math.round(seconds / 3600)}小时前`
}

/** 获取当前CSS颜色变量 */
function getCurrentCSSColors(): Record<string, string> {
  const root = document.documentElement
  const style = getComputedStyle(root)
  return {
    primary: style.getPropertyValue('--color-primary').trim(),
    secondary: style.getPropertyValue('--color-secondary').trim(),
    accent: style.getPropertyValue('--color-accent').trim(),
    light: style.getPropertyValue('--color-light').trim(),
    dark: style.getPropertyValue('--color-dark').trim(),
  }
}

// ============================================================================
// 调试工具
// ============================================================================

export const debugColorTools = {
  /**
   * 获取当前应用的颜色
   */
  current() {
    const colors = getCurrentCSSColors()

    // 创建颜色预览
    const preview = Object.entries(colors)
      .map(([name, color]) => `%c ${name}: ${color} %c  `)
      .join('')

    const styles = Object.values(colors).flatMap(color => [
      `background: ${color}; color: white; padding: 2px 8px; border-radius: 3px;`,
      'background: transparent;',
    ])

    console.log('🎨 当前应用的颜色:')
    console.log(preview, ...styles)
    console.table(colors)

    return colors
  },

  /**
   * 获取壁纸状态（包括一致性验证）
   */
  wallpaperStatus() {
    const debugInfo = wallpaperState.getDebugInfo()
    // extractBackgroundUrl 接收元素ID（string），不是 HTMLElement
    const domUrl = extractBackgroundUrl('wallpaper')

    // 从 state 快照获取正确的属性
    const activeUrl = debugInfo.state?.activeUrl ?? null
    const appliedAt = debugInfo.state?.appliedAt ?? null

    const status = {
      activeUrl: truncateUrl(activeUrl),
      activeUrlFull: activeUrl,
      domUrl: truncateUrl(domUrl),
      domUrlFull: domUrl,
      isConsistent: debugInfo.isConsistent,
      isDOMConsistent: activeUrl === domUrl,
      lastApplied: formatTimeAgo(appliedAt),
      listenerCount: debugInfo.listenerCount,
    }

    if (status.isConsistent && status.isDOMConsistent) {
      console.log('✅ 壁纸状态一致')
    }
    else if (status.isConsistent) {
      console.log('⚠️ URL规范化后一致，原始URL略有不同（可能是缓存参数）')
    }
    else {
      console.warn('❌ 壁纸状态不一致')
    }

    console.table({
      '状态URL': status.activeUrl || '(无)',
      'DOM URL': status.domUrl || '(无)',
      '一致性': status.isConsistent ? '✓' : '✗',
      '最后应用': status.lastApplied,
      '监听器数量': status.listenerCount,
    })

    return status
  },

  /**
   * 获取完整的壁纸状态管理器信息
   */
  state() {
    const debugInfo = wallpaperState.getDebugInfo()
    console.log('📊 壁纸状态管理器信息:', debugInfo)
    return debugInfo
  },

  /**
   * 获取缓存信息
   */
  cache() {
    const info = getCacheInfo()

    console.log('💾 缓存信息:')
    console.table({
      缓存存在: info.exists ? '是' : '否',
      条目数量: info.count ?? 0,
      缓存大小: info.totalSize ? `${info.totalSize} bytes` : '(无)',
    })

    if (info.items && info.items.length > 0) {
      console.log('🎨 缓存条目:')
      console.table(info.items.map(item => ({
        URL: item.url,
        缓存年龄: `${item.age}秒`,
        访问次数: item.accessCount,
      })))
    }

    return info
  },

  /**
   * 测试颜色提取
   */
  async test(imageUrl: string) {
    console.log('🧪 开始测试颜色提取...')
    console.log('📍 URL:', truncateUrl(imageUrl, 100))

    const startTime = performance.now()

    try {
      const colors = await extractColorsFromImage(imageUrl, {
        forceRefresh: true,
        context: 'wallpaper',
      })

      const duration = Math.round(performance.now() - startTime)
      console.log(`✅ 提取成功 (${duration}ms):`)
      console.table(colors)

      // 颜色预览
      const preview = Object.entries(colors)
        .map(([name]) => `%c ${name} %c`)
        .join('')

      const styles = Object.values(colors).flatMap(color => [
        `background: ${color}; color: white; padding: 4px 12px; border-radius: 4px;`,
        'background: transparent;',
      ])

      console.log(preview, ...styles)

      // 询问是否应用
      if (confirm('是否应用这些颜色到页面？')) {
        applyColorPalette(colors)
        console.log('✅ 颜色已应用')
      }

      return colors
    }
    catch (error) {
      const duration = Math.round(performance.now() - startTime)
      console.error(`❌ 提取失败 (${duration}ms):`, error)
      throw error
    }
  },

  /**
   * 验证给定URL是否与当前壁纸一致
   */
  verify(url: string) {
    const isActive = wallpaperState.isUrlActive(url)
    const debugInfo = wallpaperState.getDebugInfo()
    const activeUrl = debugInfo.state?.activeUrl ?? null

    console.log('🔍 验证URL:', truncateUrl(url, 100))
    console.log('📍 当前活动URL:', truncateUrl(activeUrl, 100))

    if (isActive) {
      console.log('✅ URL与当前壁纸一致')
    }
    else {
      console.warn('⚠️ URL与当前壁纸不一致')
    }

    return isActive
  },

  /**
   * 清除所有缓存
   */
  clear() {
    clearColorCache()
    clearWallpaperCache()
    console.log('🗑️ 已清除:')
    console.log('  - 颜色提取器内存缓存')
    console.log('  - 壁纸颜色缓存 (localStorage)')
    console.log('💡 提示: 页面刷新后将重新提取颜色')
  },

  /**
   * 重置壁纸状态
   */
  reset() {
    // 使用正确的 API: clearState() 而不是 setActiveUrl()
    wallpaperState.clearState()
    clearColorCache()
    clearWallpaperCache()
    console.log('🔄 壁纸状态已重置')
    console.log('💡 提示: 刷新页面以重新加载壁纸')
  },

  /**
   * 显示帮助信息
   */
  help() {
    console.log(`
%c🎨 壁纸颜色提取调试工具 v2.1%c

%c状态检查%c
  current()         - 查看当前应用的CSS颜色变量
  wallpaperStatus() - 查看壁纸URL一致性状态
  state()           - 查看完整的壁纸状态管理器信息
  cache()           - 查看缓存详细信息

%c测试功能%c
  test(url)         - 测试从指定URL提取颜色
  verify(url)       - 验证URL是否与当前壁纸一致

%c清理功能%c
  clear()           - 清除所有缓存
  reset()           - 重置壁纸状态并清除缓存

%c示例%c
  __debugColors.test('https://example.com/image.jpg')
  __debugColors.wallpaperStatus()
  __debugColors.verify(document.body.style.backgroundImage)
`, 'font-size: 14px; font-weight: bold; color: #3b82f6;', '', 'font-weight: bold; color: #10b981;', '', 'font-weight: bold; color: #f59e0b;', '', 'font-weight: bold; color: #ef4444;', '', 'font-weight: bold; color: #8b5cf6;', '')
  },
}

// ============================================================================
// 挂载到 window 对象
// ============================================================================

if (import.meta.env.DEV) {
  (window as any).__debugColors = debugColorTools
  console.log(
    '%c🛠️ 颜色调试工具已加载%c 输入 %c__debugColors.help()%c 查看帮助',
    'color: #3b82f6; font-weight: bold;',
    '',
    'background: #f3f4f6; padding: 2px 6px; border-radius: 3px; font-family: monospace;',
    '',
  )
}
