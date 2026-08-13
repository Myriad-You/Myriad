/**
 * Hello World Tapp
 * 官方入门示例，演示 Tapp 生命周期与常用 API
 * @version 1.0.0
 */

import type { ExampleTapp, TappCodeStructure } from './types'

// 页面 HTML 模板
const PAGE_HTML = `<!-- 背景层 -->
<div id="tapp-background">
  <div class="hw-bg-base"></div>
  <!-- 背景装饰光晕 -->
  <div class="hw-glow hw-glow-1"></div>
  <div class="hw-glow hw-glow-2"></div>
</div>

<!-- 内容层 -->
<div id="tapp-content">
  <div class="hw-page">
    <!-- 主内容区 -->
    <main class="hw-main">
      <!-- 标题卡片 -->
      <header class="hw-header glass">
        <div class="hw-emoji">👋</div>
        <div class="hw-title-row">
          <h1 id="hw-title" class="hw-title">Hello World</h1>
          <span class="hw-version">v1.0</span>
        </div>
        <p id="hw-subtitle" class="hw-subtitle">欢迎使用 Tapp 系统！探索下方卡片了解核心功能。</p>
      </header>
      
      <!-- 功能卡片网格 -->
      <div class="hw-grid">
        <!-- 生命周期 -->
        <article class="hw-card glass" style="--card-color: #8B5CF6; --delay: 0s;">
          <div class="hw-card-icon">🔄</div>
          <h3 id="hw-feat-lifecycle-title" class="hw-card-title">生命周期</h3>
          <p id="hw-feat-lifecycle-desc" class="hw-card-desc">onReady/onDestroy 完整生命周期管理</p>
        </article>
        
        <!-- 存储 API -->
        <article class="hw-card glass" style="--card-color: #F59E0B; --delay: 0.05s;">
          <div class="hw-card-icon">📦</div>
          <h3 id="hw-feat-storage-title" class="hw-card-title">存储 API</h3>
          <p id="hw-feat-storage-desc" class="hw-card-desc">持久化数据存储，跨会话保持</p>
        </article>
        
        <!-- 主题适配 -->
        <article class="hw-card glass" style="--card-color: #EC4899; --delay: 0.1s;">
          <div class="hw-card-icon">🎨</div>
          <h3 id="hw-feat-theme-title" class="hw-card-title">主题适配</h3>
          <p id="hw-feat-theme-desc" class="hw-card-desc">自动适应系统明暗主题</p>
        </article>
        
        <!-- 页面组件 -->
        <article class="hw-card glass" style="--card-color: #3B82F6; --delay: 0.15s;">
          <div class="hw-card-icon">📄</div>
          <h3 id="hw-feat-page-title" class="hw-card-title">页面组件</h3>
          <p id="hw-feat-page-desc" class="hw-card-desc">注册自定义页面，支持全屏模式</p>
        </article>
        
        <!-- DOM 安全 -->
        <article class="hw-card glass" style="--card-color: #EF4444; --delay: 0.2s;">
          <div class="hw-card-icon">🛡️</div>
          <h3 id="hw-feat-security-title" class="hw-card-title">DOM 安全</h3>
          <p id="hw-feat-security-desc" class="hw-card-desc">内置 XSS 防护的安全渲染</p>
        </article>
        
        <!-- 自适应尺寸 -->
        <article class="hw-card glass" style="--card-color: #14B8A6; --delay: 0.25s;">
          <div class="hw-card-icon">📐</div>
          <h3 id="hw-feat-responsive-title" class="hw-card-title">自适应尺寸</h3>
          <p id="hw-feat-responsive-desc" class="hw-card-desc">CSS 变量驱动的响应式设计</p>
        </article>
        
        <!-- 国际化 -->
        <article class="hw-card glass" style="--card-color: #6366F1; --delay: 0.3s;">
          <div class="hw-card-icon">🌐</div>
          <h3 id="hw-feat-i18n-title" class="hw-card-title">国际化</h3>
          <p id="hw-feat-i18n-desc" class="hw-card-desc">多语言支持，实时切换</p>
        </article>
        
        <!-- CSS 架构 -->
        <article class="hw-card glass hw-card-primary" style="--card-color: var(--hw-primary); --delay: 0.35s;">
          <div class="hw-card-icon">🎯</div>
          <h3 id="hw-feat-cssArch-title" class="hw-card-title">CSS 架构</h3>
          <p id="hw-feat-cssArch-desc" class="hw-card-desc">分离/统一模式，按需加载样式</p>
        </article>
      </div>
      
      <!-- 页脚 -->
      <footer class="hw-footer">
        <p id="hw-footer">由 Myriad Tapp 系统驱动</p>
      </footer>
    </main>
  </div>
</div>
`

// CSS 样式
const STYLES = `/* ========== CSS 变量 ========== */
:root {
  --hw-primary: var(--tapp-primary, #10B981);
  --hw-primary-rgb: 16, 185, 129;
  --hw-radius: 16px;
  --hw-radius-sm: 12px;
  --hw-transition: 0.3s cubic-bezier(0.4, 0, 0.2, 1);
  --hw-shadow: 0 8px 32px rgba(0, 0, 0, 0.12);
}

/* 基础重置 */
* { box-sizing: border-box; }
body { margin: 0; font-family: system-ui, -apple-system, sans-serif; }

/* Glass 效果 */
.glass {
  background: rgba(255, 255, 255, 0.8);
  backdrop-filter: blur(20px) saturate(180%);
  -webkit-backdrop-filter: blur(20px) saturate(180%);
  border: 1px solid rgba(0, 0, 0, 0.06);
}
.dark .glass {
  background: rgba(30, 30, 35, 0.85);
  border-color: rgba(255, 255, 255, 0.08);
}

/* 页面容器 */
.hw-page {
  position: relative;
  width: 100%;
  min-height: 100%;
  font-family: system-ui, -apple-system, sans-serif;
  color: var(--tapp-text, #1f1f1f);
}

/* 背景层（#tapp-background） */
#tapp-background {
  background: linear-gradient(135deg, #fafafa 0%, #f0f0f0 100%);
}
.dark #tapp-background {
  background: linear-gradient(135deg, #0a0a0a 0%, #141414 100%);
}

.hw-bg-base {
  position: absolute;
  inset: 0;
}

.hw-glow {
  position: absolute;
  border-radius: 50%;
  filter: blur(60px);
  animation: hw-glow-breathe 6s ease-in-out infinite;
}

.hw-glow-1 {
  right: -10%;
  top: -10%;
  width: 400px;
  height: 400px;
  background: radial-gradient(circle, rgba(var(--hw-primary-rgb), 0.15), transparent 70%);
}

.hw-glow-2 {
  left: -5%;
  bottom: -5%;
  width: 300px;
  height: 300px;
  background: radial-gradient(circle, rgba(var(--hw-primary-rgb), 0.1), transparent 70%);
  animation-delay: 3s;
}

@keyframes hw-glow-breathe {
  0%, 100% { opacity: 0.6; transform: scale(1); }
  50% { opacity: 1; transform: scale(1.1); }
}

/* 主内容区 */
.hw-main {
  position: relative;
  z-index: 1;
  max-width: 900px;
  margin: 0 auto;
  padding: 24px;
}

/* 标题卡片 */
.hw-header {
  border-radius: var(--hw-radius);
  padding: 32px;
  margin-bottom: 24px;
  position: relative;
  overflow: hidden;
  animation: hw-slide-up 0.5s ease-out;
}

.hw-emoji {
  font-size: 48px;
  margin-bottom: 12px;
  animation: hw-float 3s ease-in-out infinite;
}

@keyframes hw-float {
  0%, 100% { transform: translateY(0); }
  50% { transform: translateY(-6px); }
}

.hw-title-row {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
  margin-bottom: 8px;
}

.hw-title {
  font-size: 32px;
  font-weight: 800;
  margin: 0;
  color: var(--tapp-text, #1f1f1f);
}
.dark .hw-title { color: #f5f5f5; }

.hw-version {
  padding: 4px 12px;
  background: color-mix(in srgb, var(--hw-primary) 15%, transparent);
  color: var(--hw-primary);
  border-radius: 20px;
  font-size: 12px;
  font-weight: 600;
}

.hw-subtitle {
  font-size: 15px;
  color: var(--tapp-subtext, #666);
  margin: 0;
  line-height: 1.6;
}
.dark .hw-subtitle { color: #999; }

/* 功能卡片网格 */
.hw-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  gap: 16px;
  margin-bottom: 24px;
}

/* 功能卡片 */
.hw-card {
  border-radius: var(--hw-radius-sm);
  padding: 24px;
  cursor: default;
  transition: transform 0.25s ease, box-shadow 0.25s ease, border-color 0.25s ease;
  position: relative;
  overflow: hidden;
  animation: hw-slide-up 0.5s ease-out backwards;
  animation-delay: var(--delay, 0s);
  border: 1px solid transparent;
}

.hw-card:hover {
  transform: translateY(-4px);
  box-shadow: 0 12px 28px -8px color-mix(in srgb, var(--card-color) 30%, transparent);
  border-color: color-mix(in srgb, var(--card-color) 40%, transparent);
}

/* 卡片顶部装饰线 */
.hw-card::before {
  content: '';
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 3px;
  background: linear-gradient(90deg, transparent, var(--card-color), transparent);
  opacity: 0;
  transition: opacity 0.25s ease;
}

.hw-card:hover::before {
  opacity: 1;
}

.hw-card-icon {
  position: relative;
  z-index: 1;
  font-size: 32px;
  margin-bottom: 12px;
  display: inline-block;
  transition: transform 0.25s ease;
}

.hw-card:hover .hw-card-icon {
  transform: scale(1.1);
}

.hw-card-title {
  position: relative;
  z-index: 1;
  font-size: 16px;
  font-weight: 700;
  margin: 0 0 6px;
  color: var(--tapp-text, #1f1f1f);
}
.dark .hw-card-title { color: #f0f0f0; }

.hw-card-desc {
  position: relative;
  z-index: 1;
  font-size: 13px;
  color: var(--tapp-subtext, #666);
  margin: 0;
  line-height: 1.5;
}
.dark .hw-card-desc { color: #999; }

.hw-card-primary {
  border-color: color-mix(in srgb, var(--hw-primary) 30%, transparent);
}
.hw-card-primary:hover {
  border-color: color-mix(in srgb, var(--hw-primary) 60%, transparent);
}

/* 页脚 */
.hw-footer {
  text-align: center;
  padding: 16px 0;
}

.hw-footer p {
  margin: 0;
  font-size: 12px;
  color: var(--tapp-subtext, #666);
  opacity: 0.7;
}
.dark .hw-footer p { color: #888; }

/* 动画 */
@keyframes hw-slide-up {
  from {
    opacity: 0;
    transform: translateY(20px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}

/* 响应式 */
@media (max-width: 480px) {
  .hw-main { padding: 16px; }
  .hw-header { padding: 24px; }
  .hw-emoji { font-size: 40px; }
  .hw-title { font-size: 26px; }
  .hw-grid { grid-template-columns: 1fr; }
}

/* 减少动画 */
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: 0.01ms !important;
    transition-duration: 0.01ms !important;
  }
}
`

// 核心代码
const CORE_CODE = `// ========== i18n 翻译表 ==========
var i18n = {
  'zh-CN': {
    title: 'Hello World',
    subtitle: '欢迎使用 Tapp 系统！探索下方卡片了解核心功能。',
    features: {
      lifecycle: { title: '生命周期', desc: 'onReady/onDestroy 完整生命周期管理' },
      storage: { title: '存储 API', desc: '持久化数据存储，跨会话保持' },
      theme: { title: '主题适配', desc: '自动适应系统明暗主题' },
      page: { title: '页面组件', desc: '注册自定义页面，支持全屏模式' },
      security: { title: 'DOM 安全', desc: '内置 XSS 防护的安全渲染' },
      responsive: { title: '自适应尺寸', desc: 'CSS 变量驱动的响应式设计' },
      i18n: { title: '国际化', desc: '多语言支持，实时切换' },
      cssArch: { title: 'CSS 架构', desc: '支持统一/分离/混合三种模式' },
    },
    footer: '由 Myriad Tapp 系统驱动',
  },
  'en-US': {
    title: 'Hello World',
    subtitle: 'Welcome to Tapp System! Explore the cards below to discover core features.',
    features: {
      lifecycle: { title: 'Lifecycle', desc: 'onReady/onDestroy full lifecycle management' },
      storage: { title: 'Storage API', desc: 'Persistent data storage across sessions' },
      theme: { title: 'Theme Adapt', desc: 'Auto adapt to system light/dark theme' },
      page: { title: 'Page Component', desc: 'Register custom pages with fullscreen' },
      security: { title: 'DOM Security', desc: 'Built-in XSS-safe rendering' },
      responsive: { title: 'Responsive', desc: 'CSS variables driven responsive design' },
      i18n: { title: 'i18n', desc: 'Multi-language with live switch' },
      cssArch: { title: 'CSS Arch', desc: 'Unified/separated/hybrid modes' },
    },
    footer: 'Powered by Myriad Tapp System',
  },
  'ja-JP': {
    title: 'Hello World',
    subtitle: 'Tapp システムへようこそ！下のカードでコア機能をご覧ください。',
    features: {
      lifecycle: { title: 'ライフサイクル', desc: 'onReady/onDestroy 完全なライフサイクル管理' },
      storage: { title: 'ストレージ API', desc: 'セッション間で永続的なデータ保存' },
      theme: { title: 'テーマ適応', desc: 'システムテーマに自動対応' },
      page: { title: 'ページコンポーネント', desc: 'フルスクリーン対応のカスタムページ' },
      security: { title: 'DOM セキュリティ', desc: 'XSS 対策済みの安全なレンダリング' },
      responsive: { title: 'レスポンシブ', desc: 'CSS 変数駆動のレスポンシブデザイン' },
      i18n: { title: '国際化', desc: '多言語対応、リアルタイム切り替え' },
      cssArch: { title: 'CSS アーキテクチャ', desc: '統一/分離/ハイブリッドモード対応' },
    },
    footer: 'Myriad Tapp System で動作中',
  },
};

var currentLocale = 'zh-CN';

function normalizeLocale(locale) {
  if (!locale) return 'zh-CN';
  var l = locale.toLowerCase();
  if (l.startsWith('zh')) return 'zh-CN';
  if (l.startsWith('en')) return 'en-US';
  if (l.startsWith('ja')) return 'ja-JP';
  return 'zh-CN';
}

function t(key) {
  var keys = key.split('.');
  var value = i18n[currentLocale] || i18n['zh-CN'];
  for (var i = 0; i < keys.length; i++) {
    value = value[keys[i]];
    if (!value) return key;
  }
  return value;
}

// 更新页面文本（i18n）
function updateTexts() {
  var el;
  
  // 标题区域
  el = document.getElementById('hw-title');
  if (el) Tapp.dom.setText(el, t('title'));
  
  el = document.getElementById('hw-subtitle');
  if (el) Tapp.dom.setText(el, t('subtitle'));
  
  // 功能卡片
  var features = ['lifecycle', 'storage', 'theme', 'page', 'security', 'responsive', 'i18n', 'cssArch'];
  features.forEach(function(key) {
    el = document.getElementById('hw-feat-' + key + '-title');
    if (el) Tapp.dom.setText(el, t('features.' + key + '.title'));
    
    el = document.getElementById('hw-feat-' + key + '-desc');
    if (el) Tapp.dom.setText(el, t('features.' + key + '.desc'));
  });
  
  // 页脚
  el = document.getElementById('hw-footer');
  if (el) Tapp.dom.setText(el, t('footer'));
}
`

// 页面代码
const PAGE_CODE = `var isPaused = false;

Tapp.lifecycle.onReady(async function() {
  // 获取当前语言并更新文本
  var locale = await Tapp.ui.getLocale();
  currentLocale = normalizeLocale(locale);
  updateTexts();
  
  // 监听语言变化
  Tapp.ui.onLocaleChange(function(newLocale) {
    currentLocale = normalizeLocale(newLocale);
    updateTexts();
  });
});

// 页面不可见时暂停
Tapp.lifecycle.onPause(function() {
  isPaused = true;
});

// 页面恢复可见时恢复
Tapp.lifecycle.onResume(function() {
  isPaused = false;
});

Tapp.lifecycle.onDestroy(async function() {
  // 清理资源（如有需要）
});
`

// 导出 Tapp 定义
const codeStructure: TappCodeStructure = {
  core: CORE_CODE,
  page: PAGE_CODE,
  styles: STYLES,
  pageHtml: PAGE_HTML,
}

export const helloWorldTapp: ExampleTapp = {
  manifest: {
    id: 'com.myriad.hello-world',
    name: 'Hello World',
    version: '1.0.0',
    minSystemVersion: '0.2.1',
    description: '官方入门示例，演示 Tapp 生命周期与常用 API',
    locales: {
      'en-US': {
        description:
          'Official starter example demonstrating the Tapp lifecycle and common APIs',
      },
      'ja-JP': {
        description: '公式入門サンプル。Tapp のライフサイクルと主要 API を紹介',
      },
    },
    category: 'utility',
    main: 'index.js',
    author: {
      name: 'Myriad Team',
      email: 'tapp@myriad.app',
      url: 'https://github.com/Myriad-You',
    },
    permissions: ['storage:read', 'ui:theme:read'],
    iconSvg:
      "<svg viewBox='0 0 24 24' fill='none' xmlns='http://www.w3.org/2000/svg'><path d='M8 6L3 12l5 6M16 6l5 6-5 6' stroke='currentColor' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'/><path d='M14 4l-4 16' stroke='currentColor' stroke-width='2' stroke-linecap='round'/></svg>",
    themeColor: '#10B981',
    hasPage: true,
  },
  code: codeStructure,
  tags: ['official', 'beginner', 'lifecycle', 'storage', 'i18n'],
}
