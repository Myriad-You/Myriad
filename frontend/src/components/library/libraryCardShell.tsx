import type { ReactNode, SyntheticEvent } from 'react'
import type {
  WatchProgress,
  WatchProgressLabels,
} from '../../utils/libraryWatchProgress'
import type { Song } from '../../utils/musicPlayer'
import type { LibraryItem } from './libraryCanvasVisible'

import { FaBook, FaGamepad, FaMusic, FaVideo } from '@lib/icons'
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import {
  acquireCoverDecodeSlot,
  LIBRARY_CARD_COVER_SIZES,
  releaseCoverImageElement,
} from '../../utils/libraryCardMedia'
import {
  formatWatchProgressText,
  formatWatchStatusLabel,
  getWatchProgress,
} from '../../utils/libraryWatchProgress'
import {
  getNeteaseAudioUrlImmediate,
  isNeteaseVipFromMeta,
} from '../../utils/musicPlayer'
import { proxyImageUrlOr } from '../../utils/proxyImageUrl'
import { showInfo } from '../../utils/toastManager'
import { LIBRARY_LIVE_MS } from './libraryLiveMs'

function injectLibraryStyle(id: string, css: string) {
  if (typeof document === 'undefined') return
  let style = document.getElementById(id) as HTMLStyleElement | null
  if (!style) {
    style = document.createElement('style')
    style.id = id
    document.head.appendChild(style)
  }
  style.textContent = css
}
injectLibraryStyle(
  'library-card-shell-styles',
  `
        /* 入场：略缩短，避免与封面淡入叠成「先玻璃后整卡」 */
        @keyframes fadeInUp {
            from {
                opacity: 0;
                transform: translate3d(0, 10px, 0);
            }
            to {
                opacity: 1;
                transform: translate3d(0, 0, 0);
            }
        }

        .library-card-container {
            animation: fadeInUp 0.35s ease-out backwards;
        }
        .library-card-shell {
            position: relative;
            height: 100%;
            /* 整卡按圆角裁切（含子层 blur，避免四角发方） */
            overflow: hidden;
            border-radius: 0.75rem; /* rounded-xl 兜底，与 class 一致 */
        }

        .library-card-media {
            position: absolute;
            inset: 0;
            z-index: 0;
            background: #e8e8ed;
            overflow: hidden;
            border-radius: inherit;
            /* 点击交给上层链接/播放层，避免 img 抢事件导致「点了没反应」 */
            pointer-events: none;
        }

        html.dark .library-card-media {
            background: #1c1c20;
        }

        .library-card-media__img {
            width: 100%;
            height: 100%;
            object-fit: cover;
            opacity: 0;
            /* 就绪前微放大，淡入时缩入，比纯 opacity 更有「落稳」感 */
            transform: scale(1.045);
            transition:
                opacity 0.52s cubic-bezier(0.22, 1, 0.36, 1),
                transform 0.62s cubic-bezier(0.22, 1, 0.36, 1);
            will-change: opacity, transform;
        }

        .library-card-media__img.is-loaded {
            opacity: 1;
            transform: scale(1);
        }

        /* 默认 hover 封面放大；播放中/退场中禁用（.is-hover-locked） */
        .group:not(.is-hover-locked):hover .library-card-media__img.is-loaded {
            transform: scale(1.1);
        }

        /*
         * 播放中封面：
         * - 出场：轻弹放大再落稳
         * - 循环：轻微呼吸
         * - 退场：平滑收回 scale(1)，避免硬切
         * 播放/退场中不响应 hover（保留呼吸，不抢退场）
         */
        @keyframes library-cover-play-in {
            0% { transform: scale(1); }
            40% { transform: scale(1.04); }
            100% { transform: scale(1.012); }
        }

        /* 幅度更小、周期更长：约 1.2% 起伏 / 6.5s 一圈 */
        @keyframes library-cover-breath {
            0%, 100% { transform: scale(1.012); }
            50% { transform: scale(1.024); }
        }

        .library-card-media.is-breathing .library-card-media__img.is-loaded {
            animation:
                library-cover-play-in 0.4s cubic-bezier(0.22, 1, 0.36, 1) both,
                library-cover-breath 6.5s ease-in-out 0.4s infinite;
        }

        /*
         * 退场：不用固定起点的 keyframes（会从呼吸中途硬切到 1.02）。
         * JS 冻结当前 matrix 后只靠 transition 收到 scale(1)。
         */
        .library-card-media.is-breathing-out .library-card-media__img.is-loaded {
            animation: none;
            transition: transform 0.58s cubic-bezier(0.22, 1, 0.36, 1);
            transform: scale(1);
        }

        .group:not(.is-hover-locked):hover
            .library-card-media.is-breathing
            .library-card-media__img.is-loaded,
        .group:not(.is-hover-locked):hover
            .library-card-media.is-breathing-out
            .library-card-media__img.is-loaded {
            animation: none;
            transform: scale(1.1);
            transition: transform 0.5s cubic-bezier(0.22, 1, 0.36, 1);
        }

        @media (prefers-reduced-motion: reduce) {
            .library-card-media.is-breathing .library-card-media__img.is-loaded,
            .library-card-media.is-breathing-out .library-card-media__img.is-loaded {
                animation: none;
                transition: none;
                transform: none;
            }
        }

        .library-card-media__fallback {
            width: 100%;
            height: 100%;
            display: flex;
            align-items: center;
            justify-content: center;
        }

        .library-card-shell:not([data-media-ready='true']) .library-card-caption,
        .library-card-shell:not([data-media-ready='true']) .platform-icon-bg,
        .library-card-shell:not([data-media-ready='true']) .library-card-chrome {
            opacity: 0 !important;
            pointer-events: none;
            transition: none;
        }

        .library-card-shell[data-media-ready='true'] .library-card-caption,
        .library-card-shell[data-media-ready='true'] .platform-icon-bg {
            opacity: 1;
            transition: opacity 0.28s ease;
        }

        /* chrome 含 hover 层（自身 opacity-0），只恢复 transition，不强制 1 */
        .library-card-shell[data-media-ready='true'] .library-card-chrome {
            transition: opacity 0.28s ease;
        }
        /* 卡片容器样式 */
        .library-card-container {
            transition: left 0.4s ease-out, top 0.4s ease-out, width 0.4s ease-out, height 0.4s ease-out;
            /* 固定 GPU 合成层，避免卡片滚出/滚入视口时
               backdrop-filter 触发浏览器丢弃并重建绘制层（表现为瞬间透明再恢复） */
            transform: translateZ(0);
            -webkit-backface-visibility: hidden;
            backface-visibility: hidden;
        }

        html[data-perf-mode='exlight'] .library-card-container {
            transition: none;
        }

        html[data-perf-mode='exlight'] .rating-badge-shine,
        html[data-perf-mode='exlight'] .rating-badge-anim-max,
        html[data-perf-mode='exlight'] .animate-fade-in {
            animation: none;
        }
        /*
         * 右上角平台图标 — 轻量玻璃 + 品牌 tint
         * 已砍：噪点 / 多层渐变 / glow / 重阴影 / drop-shadow / saturate+brightness
         */
        .platform-icon-bg {
            --lib-plat-rgb: 255 255 255;
            --lib-plat-alpha: 62%;
            --lib-plat-brand: var(--platform-color, #6b7280);
            --lib-plat-border: color-mix(
                in srgb,
                rgb(255 255 255 / 50%),
                var(--lib-plat-brand) 26%
            );

            width: 1.75rem;
            height: 1.75rem;
            border-radius: 9999px;
            display: flex;
            align-items: center;
            justify-content: center;
            background: color-mix(
                in srgb,
                rgb(var(--lib-plat-rgb) / var(--lib-plat-alpha)),
                var(--lib-plat-brand) 18%
            );
            /* 单 blur，无 saturate/brightness —— 采样更省 */
            backdrop-filter: blur(4px);
            -webkit-backdrop-filter: blur(4px);
            border: 1px solid var(--lib-plat-border);
            box-shadow:
                inset 0 1px 0 rgb(255 255 255 / 45%),
                0 2px 6px -2px rgb(15 23 42 / 16%);
            transform: translateZ(0);
            transition: border-color 0.2s ease, box-shadow 0.2s ease;
        }

        html.dark .platform-icon-bg {
            --lib-plat-rgb: 14 14 18;
            --lib-plat-alpha: 52%;
            --lib-plat-border: color-mix(
                in srgb,
                rgb(255 255 255 / 14%),
                var(--lib-plat-brand) 32%
            );
            background: color-mix(
                in srgb,
                rgb(var(--lib-plat-rgb) / var(--lib-plat-alpha)),
                var(--lib-plat-brand) 22%
            );
            box-shadow:
                inset 0 1px 0 rgb(255 255 255 / 10%),
                0 2px 6px -2px rgb(0 0 0 / 35%);
        }

        :root[data-surface='liquid'] .platform-icon-bg {
            --lib-plat-alpha: 58%;
        }

        html.dark[data-surface='liquid'] .platform-icon-bg {
            --lib-plat-alpha: 48%;
        }

        .group\\/platform:hover .platform-icon-bg,
        .group:not(.is-hover-locked):hover .platform-icon-bg {
            --lib-plat-border: color-mix(
                in srgb,
                rgb(255 255 255 / 65%),
                var(--lib-plat-brand) 36%
            );
            box-shadow:
                inset 0 1px 0 rgb(255 255 255 / 50%),
                0 3px 8px -2px color-mix(in srgb, var(--lib-plat-brand) 22%, rgb(15 23 42 / 14%));
        }

        html.dark .group\\/platform:hover .platform-icon-bg,
        html.dark .group:not(.is-hover-locked):hover .platform-icon-bg {
            --lib-plat-border: color-mix(
                in srgb,
                rgb(255 255 255 / 20%),
                var(--lib-plat-brand) 42%
            );
        }

        .platform-icon-bg svg,
        .platform-icon-bg img {
            width: 0.875rem;
            height: 0.875rem;
            color: var(--lib-plat-brand);
        }
        /* 播放/退场：压掉 Tailwind group-hover 信息层 */
        .group.is-hover-locked:hover .library-card-hover-chrome {
            opacity: 0 !important;
        }

        /* 播放/退场：卡片不抬升/放大（只锁 transform，不硬改阴影） */
        .group.is-hover-locked .library-card-shell {
            transform: none !important;
        }
        /* 高分评分徽章 - 呼吸光晕 */
        @keyframes ratingGlow {
            0%, 100% {
                box-shadow: 0 2px 8px 0 color-mix(in srgb, #f59e0b 40%, transparent);
            }
            50% {
                box-shadow: 0 2px 18px 2px color-mix(in srgb, #f59e0b 75%, transparent);
            }
        }

        .rating-badge-anim {
            animation: ratingGlow 2.4s ease-in-out infinite;
        }

        /* 满分（10）更强更快 */
        .rating-badge-anim-max {
            animation: ratingGlow 1.8s ease-in-out infinite;
        }

        /* 高分评分徽章 - 流光扫过 */
        @keyframes ratingShine {
            0% { transform: translateX(-180%) skewX(-20deg); }
            16%, 100% { transform: translateX(320%) skewX(-20deg); }
        }

        .rating-badge-shine {
            position: absolute;
            top: 0;
            bottom: 0;
            width: 45%;
            background: linear-gradient(90deg, transparent, rgba(255,255,255,0.75), transparent);
            animation: ratingShine 8s ease-in-out infinite;
            pointer-events: none;
        }

        /*
         * 资料库封面卡标题板 — 轻量玻璃
         * 圆角语义对齐小组件（WidgetShell）：
         *   shell  外卡 rounded-xl (12px)
         *   nested 本标题板 rounded-lg (8px)
         *   micro  类型 chip rounded-md (6px)
         * 已砍：噪点 / 多层渐变 / 重阴影 / saturate+brightness
         */
        .library-card-caption {
            --lib-caption-rgb: 255 255 255;
            --lib-caption-alpha: 62%;
            --lib-caption-max-width: 280px;
            --lib-caption-border: rgb(255 255 255 / 42%);

            display: inline-flex;
            flex-direction: column;
            align-items: stretch;
            width: fit-content;
            max-width: min(100%, var(--lib-caption-max-width));
            min-width: 0;
            overflow: hidden;
            border-radius: 0.5rem; /* nested = lg，外卡 xl 的内嵌一档 */
            padding: 0.5rem 0.75rem;
            background: color-mix(
                in srgb,
                rgb(var(--lib-caption-rgb) / var(--lib-caption-alpha)),
                var(--platform-color, var(--color-primary, #3b82f6)) 4%
            );
            backdrop-filter: blur(5px);
            -webkit-backdrop-filter: blur(5px);
            border: 1px solid var(--lib-caption-border);
            box-shadow:
                inset 0 1px 0 rgb(255 255 255 / 40%),
                0 4px 12px -4px rgb(15 23 42 / 16%);
            transform: translateZ(0);
            transition: border-color 0.2s ease, box-shadow 0.2s ease;
        }

        html.dark .library-card-caption {
            --lib-caption-rgb: 12 12 16;
            --lib-caption-alpha: 54%;
            --lib-caption-border: rgb(255 255 255 / 12%);
            box-shadow:
                inset 0 1px 0 rgb(255 255 255 / 10%),
                0 4px 12px -4px rgb(0 0 0 / 40%);
        }

        :root[data-surface='liquid'] .library-card-caption {
            --lib-caption-alpha: 58%;
            --lib-caption-border: color-mix(
                in srgb,
                var(--surface-border, rgb(255 255 255 / 50%)) 75%,
                var(--platform-color, var(--color-primary, #3b82f6)) 25%
            );
        }

        html.dark[data-surface='liquid'] .library-card-caption {
            --lib-caption-alpha: 48%;
        }

        html[data-perf-mode='exlight'] .platform-icon-bg,
        html.dark[data-perf-mode='exlight'] .platform-icon-bg,
        html[data-perf-mode='exlight'][data-surface='liquid'] .platform-icon-bg,
        html.dark[data-perf-mode='exlight'][data-surface='liquid'] .platform-icon-bg,
        html[data-perf-mode='exlight'] .library-card-caption,
        html.dark[data-perf-mode='exlight'] .library-card-caption,
        html[data-perf-mode='exlight'][data-surface='liquid'] .library-card-caption,
        html.dark[data-perf-mode='exlight'][data-surface='liquid'] .library-card-caption {
            --lib-plat-alpha: 96%;
            --lib-caption-alpha: 96%;
        }

        .library-card-caption__row {
            display: flex;
            align-items: center;
            gap: 0.5rem;
            min-width: 0;
            max-width: 100%;
        }

        .group:not(.is-hover-locked):hover .library-card-caption {
            --lib-caption-border: color-mix(
                in srgb,
                rgb(255 255 255 / 60%),
                var(--platform-color, var(--color-primary, #3b82f6)) 16%
            );
            box-shadow:
                inset 0 1px 0 rgb(255 255 255 / 48%),
                0 6px 14px -4px rgb(15 23 42 / 18%);
        }

        html.dark .group:not(.is-hover-locked):hover .library-card-caption {
            --lib-caption-border: color-mix(
                in srgb,
                rgb(255 255 255 / 18%),
                var(--platform-color, var(--color-primary, #3b82f6)) 24%
            );
        }

        .library-card-caption__title {
            color: rgb(17 24 39);
            font-weight: 700;
            font-size: 0.875rem;
            line-height: 1.35;
            letter-spacing: -0.01em;
            min-width: 0;
            max-width: 100%;
            overflow: hidden;
        }

        /* 标题 + 类型 chip 同行：标题可收缩截断，chip 不挤出 */
        .library-card-caption__row .library-card-caption__title {
            flex: 1 1 auto;
        }

        html.dark .library-card-caption__title {
            color: rgb(243 244 246);
        }

        .library-card-caption__type {
            display: inline-flex;
            align-items: center;
            flex-shrink: 0;
            padding: 0.125rem 0.5rem;
            border-radius: 0.375rem;
            font-size: 0.6875rem;
            font-weight: 600;
            letter-spacing: 0.02em;
            white-space: nowrap;
            border: 1px solid transparent;
        }

        .library-card-caption__type--anime {
            background: color-mix(in srgb, #fce7f3 68%, transparent);
            color: #be185d;
            border-color: color-mix(in srgb, #f9a8d4 35%, transparent);
        }
        .library-card-caption__type--book {
            background: color-mix(in srgb, #fef3c7 68%, transparent);
            color: #b45309;
            border-color: color-mix(in srgb, #fcd34d 35%, transparent);
        }
        .library-card-caption__type--game {
            background: color-mix(in srgb, #d1fae5 68%, transparent);
            color: #047857;
            border-color: color-mix(in srgb, #6ee7b7 35%, transparent);
        }
        .library-card-caption__type--tv {
            background: color-mix(in srgb, #f3e8ff 68%, transparent);
            color: #7e22ce;
            border-color: color-mix(in srgb, #d8b4fe 35%, transparent);
        }

        html.dark .library-card-caption__type--anime {
            background: color-mix(in srgb, #be185d 22%, transparent);
            color: #fbcfe8;
            border-color: color-mix(in srgb, #f9a8d4 18%, transparent);
        }
        html.dark .library-card-caption__type--book {
            background: color-mix(in srgb, #b45309 22%, transparent);
            color: #fde68a;
            border-color: color-mix(in srgb, #fcd34d 18%, transparent);
        }
        html.dark .library-card-caption__type--game {
            background: color-mix(in srgb, #047857 22%, transparent);
            color: #a7f3d0;
            border-color: color-mix(in srgb, #6ee7b7 18%, transparent);
        }
        html.dark .library-card-caption__type--tv {
            background: color-mix(in srgb, #7e22ce 22%, transparent);
            color: #e9d5ff;
            border-color: color-mix(in srgb, #d8b4fe 18%, transparent);
        }

        .library-card-caption__meta {
            margin-top: 0.25rem;
            font-size: 0.75rem;
            line-height: 1.35;
            color: rgb(75 85 99);
            min-width: 0;
            max-width: 100%;
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
        }

        html.dark .library-card-caption__meta {
            color: rgb(209 213 219 / 80%);
        }

        /* 进度条等块级子项：与标题板同宽（内容宽） */
        .library-card-caption > :not(.library-card-caption__row):not(.library-card-caption__title) {
            max-width: 100%;
            min-width: 0;
        }

        /* —— 观看/阅读进度：流体细轨 + 类型色填充 —— */
        .library-progress {
            margin-top: 0.375rem;
            min-width: 0;
            display: flex;
            flex-direction: column;
            gap: 0.3125rem;
        }

        .library-progress__meta {
            display: flex;
            align-items: center;
            gap: 0.375rem;
            min-width: 0;
        }

        .library-progress__text {
            min-width: 0;
            font-size: 0.625rem;
            line-height: 1.3;
            font-variant-numeric: tabular-nums;
            letter-spacing: 0.01em;
            color: rgb(75 85 99);
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
        }

        html.dark .library-progress__text {
            color: rgb(209 213 219 / 88%);
        }

        .library-progress--dark .library-progress__text {
            color: rgb(255 255 255 / 82%);
        }

        .library-progress__chip {
            flex-shrink: 0;
            font-size: 0.5625rem;
            font-weight: 600;
            line-height: 1;
            letter-spacing: 0.02em;
            padding: 0.1875rem 0.375rem;
            border-radius: 9999px;
            border: 1px solid transparent;
        }

        .library-progress__chip--anime {
            background: color-mix(in srgb, #fce7f3 62%, transparent);
            color: #be185d;
            border-color: color-mix(in srgb, #f9a8d4 32%, transparent);
        }
        .library-progress__chip--book {
            background: color-mix(in srgb, #fef3c7 62%, transparent);
            color: #b45309;
            border-color: color-mix(in srgb, #fcd34d 32%, transparent);
        }
        .library-progress__chip--game {
            background: color-mix(in srgb, #d1fae5 62%, transparent);
            color: #047857;
            border-color: color-mix(in srgb, #6ee7b7 32%, transparent);
        }
        .library-progress__chip--tv {
            background: color-mix(in srgb, #f3e8ff 62%, transparent);
            color: #7e22ce;
            border-color: color-mix(in srgb, #d8b4fe 32%, transparent);
        }
        .library-progress__chip--default {
            background: color-mix(in srgb, #fce7f3 62%, transparent);
            color: #be185d;
            border-color: color-mix(in srgb, #f9a8d4 32%, transparent);
        }

        html.dark .library-progress__chip--anime,
        html.dark .library-progress__chip--default {
            background: color-mix(in srgb, #be185d 22%, transparent);
            color: #fbcfe8;
            border-color: color-mix(in srgb, #f9a8d4 18%, transparent);
        }
        html.dark .library-progress__chip--book {
            background: color-mix(in srgb, #b45309 22%, transparent);
            color: #fde68a;
            border-color: color-mix(in srgb, #fcd34d 18%, transparent);
        }
        html.dark .library-progress__chip--game {
            background: color-mix(in srgb, #047857 22%, transparent);
            color: #a7f3d0;
            border-color: color-mix(in srgb, #6ee7b7 18%, transparent);
        }
        html.dark .library-progress__chip--tv {
            background: color-mix(in srgb, #7e22ce 22%, transparent);
            color: #e9d5ff;
            border-color: color-mix(in srgb, #d8b4fe 18%, transparent);
        }

        .library-progress--dark .library-progress__chip {
            background: rgb(255 255 255 / 10%);
            color: rgb(255 255 255 / 90%);
            border-color: rgb(255 255 255 / 14%);
        }

        .library-progress__track {
            --lib-prog-fill: #f472b6;
            width: 100%;
            height: 4px;
            border-radius: 9999px;
            overflow: hidden;
            background: rgb(15 23 42 / 8%);
        }

        html.dark .library-progress__track {
            background: rgb(255 255 255 / 10%);
        }

        .library-progress--dark .library-progress__track {
            background: rgb(255 255 255 / 14%);
        }

        .library-progress__fill {
            height: 100%;
            border-radius: inherit;
            width: 0%;
            max-width: 100%;
            background: var(--lib-prog-fill);
            opacity: 0.9;
            transition: width 0.35s ease-out;
        }

        .library-progress__track--anime { --lib-prog-fill: #f472b6; }
        .library-progress__track--book { --lib-prog-fill: #fbbf24; }
        .library-progress__track--game { --lib-prog-fill: #34d399; }
        .library-progress__track--tv { --lib-prog-fill: #c084fc; }
        .library-progress__track--default { --lib-prog-fill: #f472b6; }
  `,
)

function coverFallbackUrl(title: string): string {
  return `https://ui-avatars.com/api/?name=${encodeURIComponent(title || '?')}&size=400&background=random`
}

function resolveLibraryItemUrl(item: {
  id: string
  platform: string
  item_type: string
  metadata: any
}): string | null {
  const m =
    item.metadata && typeof item.metadata === 'object' ? item.metadata : {}
  const asHttp = (v: unknown): string | null => {
    if (typeof v !== 'string') return null
    const s = v.trim()
    if (!s) return null
    if (/^https?:\/\//i.test(s)) return s
    if (s.startsWith('//')) return `https:${s}`
    return null
  }

  const direct =
    asHttp(m.url) ||
    asHttp(m.link) ||
    asHttp(m.web_url) ||
    asHttp(m.html_url) ||
    asHttp(m.short_link_v2) ||
    asHttp(m.short_link) ||
    asHttp(m?.subject?.url) ||
    asHttp(m?.node?.url) ||
    asHttp(m?.share_url)
  if (direct) return direct

  const platform = (item.platform || '').toLowerCase()
  const id = item.id || ''

  if (platform.includes('steam') || id.startsWith('steam_')) {
    const appid = m.appid ?? id.replaceAll(/^steam_game_/g, '')
    if (appid !== '' && appid != null) {
      return `https://store.steampowered.com/app/${appid}`
    }
  }

  if (
    platform.includes('bilibili') ||
    platform.includes('bili') ||
    id.startsWith('bilibili_')
  ) {
    if (m.season_id != null) {
      return `https://www.bilibili.com/bangumi/play/ss${m.season_id}`
    }
    const bvid = typeof m.bvid === 'string' ? m.bvid : null
    if (bvid) return `https://www.bilibili.com/video/${bvid}`
    const aid = m.aid ?? m.id
    if (aid != null && /^\d+$/.test(String(aid))) {
      return `https://www.bilibili.com/video/av${aid}`
    }
  }

  if (
    platform.includes('bangumi') ||
    platform.includes('bgm') ||
    id.startsWith('bangumi_')
  ) {
    const sid =
      m.subject_id ??
      m.subject?.id ??
      (id.startsWith('bangumi_subject_')
        ? id.slice('bangumi_subject_'.length)
        : null)
    if (sid != null && String(sid) !== '') {
      return `https://bgm.tv/subject/${sid}`
    }
  }

  if (
    platform.includes('mal') ||
    platform.includes('myanimelist') ||
    id.startsWith('mal_')
  ) {
    const mal = id.match(/^mal_(anime|manga)_(\d+)$/i)
    if (mal) return `https://myanimelist.net/${mal[1].toLowerCase()}/${mal[2]}`
    const kind =
      m.media_type === 'manga' || item.item_type === 'book' ? 'manga' : 'anime'
    const mid = m.id ?? m.node?.id
    if (mid != null) return `https://myanimelist.net/${kind}/${mid}`
  }

  if (platform.includes('github')) {
    if (typeof m.full_name === 'string' && m.full_name.includes('/')) {
      return `https://github.com/${m.full_name}`
    }
    if (typeof m.name === 'string' && typeof m.owner?.login === 'string') {
      return `https://github.com/${m.owner.login}/${m.name}`
    }
  }

  if (platform.includes('youtube') || platform.includes('yt')) {
    const vid = m.video_id ?? m.id
    if (typeof vid === 'string' && vid.length >= 6) {
      return `https://www.youtube.com/watch?v=${vid}`
    }
  }

  return null
}

export function openLibraryItemExternal(item: {
  id: string
  platform: string
  item_type: string
  title: string
  metadata: any
}): void {
  const url = resolveLibraryItemUrl(item)
  if (!url) return
  window.open(url, '_blank', 'noopener,noreferrer')
}

export function isBangumiPlatform(platform: string) {
  return platform.toLowerCase() === 'bangumi'
}

export function isMalPlatform(platform: string) {
  const key = platform.toLowerCase().replaceAll(/[\s_-]/g, '')
  return key === 'myanimelist' || key === 'mal'
}

export function hasUserRatingBadge(platform: string) {
  return isBangumiPlatform(platform) || isMalPlatform(platform)
}

export function getRatingBadgeStyle(rate: number) {
  if (rate >= 10) {
    return {
      box: 'w-10 h-10 text-xl bg-linear-to-br from-amber-300 via-yellow-400 to-orange-500 text-white ring-2 ring-amber-200/80 ring-offset-1 ring-offset-amber-500/30 shadow-amber-400/60',
      gloss: true,
    }
  }
  if (rate >= 9) {
    return {
      box: 'w-9 h-9 text-lg bg-linear-to-br from-amber-300 to-orange-500 text-white ring-2 ring-amber-200/70 shadow-amber-500/50',
      gloss: true,
    }
  }
  if (rate >= 8) {
    return {
      box: 'w-8 h-8 text-base bg-emerald-500 text-white ring-1 ring-emerald-300/50 shadow-emerald-500/40',
      gloss: false,
    }
  }
  if (rate >= 7) {
    return {
      box: 'w-8 h-8 text-base bg-green-500 text-white shadow-green-500/30',
      gloss: false,
    }
  }
  if (rate >= 6) {
    return {
      box: 'w-7 h-7 text-sm bg-lime-500 text-white',
      gloss: false,
    }
  }
  if (rate >= 5) {
    return {
      box: 'w-7 h-7 text-sm bg-amber-500 text-white',
      gloss: false,
    }
  }
  if (rate >= 3) {
    return {
      box: 'w-7 h-7 text-sm bg-orange-500 text-white',
      gloss: false,
    }
  }
  return {
    box: 'w-7 h-7 text-sm bg-rose-500 text-white',
    gloss: false,
  }
}

export function getItemGridSize(type: string, platform: string) {
  switch (type) {
    case 'game':
      return isBangumiPlatform(platform) ? { w: 1, h: 2 } : { w: 2, h: 1 }
    case 'video':
      return { w: 2, h: 1 }
    case 'anime':
    case 'tv_series':
    case 'book':
      return { w: 1, h: 2 }
    case 'music':
    default:
      return { w: 1, h: 1 }
  }
}

export const LibraryCardShell = memo(
  ({
    cover,
    title,
    className,
    imgClassName,
    placeholder,
    children,
    coverBreathing = false,
    priority = false,
    canvasEnterDelay = null,
  }: {
    cover: string | null
    title: string
    className?: string
    imgClassName?: string
    placeholder: ReactNode
    children: ReactNode
    coverBreathing?: boolean
    priority?: boolean
    canvasEnterDelay?: number | null
  }) => {
    const hasCover = Boolean(cover)
    const [mediaReady, setMediaReady] = useState(!hasCover)
    const [activeSrc, setActiveSrc] = useState<string | null>(null)
    // 退场播完再卸类，避免硬切。
    const [breathPhase, setBreathPhase] = useState<
      'off' | 'breathing' | 'exiting'
    >(coverBreathing ? 'breathing' : 'off')
    const [enterDelay] = useState(canvasEnterDelay)
    const [canvasEntering, setCanvasEntering] = useState(
      () => enterDelay != null,
    )
    const imgRef = useRef<HTMLImageElement>(null)
    const releaseSlotRef = useRef<(() => void) | null>(null)

    // reduced-motion / 收不到 animationend 时摘掉 enter，避免属性残留。
    useEffect(() => {
      if (!canvasEntering) return
      const delayMs = ((enterDelay ?? 0) + 0.62) * 1000 + 80
      const reduced =
        typeof window !== 'undefined' &&
        window.matchMedia?.('(prefers-reduced-motion: reduce)').matches
      if (reduced) {
        setCanvasEntering(false)
        return
      }
      const t = window.setTimeout(setCanvasEntering, delayMs, false)
      return () => window.clearTimeout(t)
    }, [canvasEntering, enterDelay])

    const clearCoverInline = useCallback(() => {
      const img = imgRef.current
      if (!img) return
      img.style.transition = ''
      img.style.transform = ''
      img.style.animation = ''
    }, [])

    useEffect(() => {
      if (coverBreathing) {
        clearCoverInline()
        setBreathPhase('breathing')
        return
      }
      setBreathPhase((prev) => {
        if (prev !== 'breathing') return prev === 'exiting' ? 'exiting' : 'off'
        return 'exiting'
      })
    }, [coverBreathing, clearCoverInline])

    // 退场：先冻呼吸 matrix，下一帧 transition 到 scale(1)，避免 keyframes 硬切。
    useEffect(() => {
      if (breathPhase !== 'exiting') return
      const img = imgRef.current
      const reduced =
        typeof window !== 'undefined' &&
        window.matchMedia?.('(prefers-reduced-motion: reduce)').matches

      if (!img || reduced) {
        clearCoverInline()
        const t = window.setTimeout(setBreathPhase, 40, 'off')
        return () => window.clearTimeout(t)
      }

      const matrix = window.getComputedStyle(img).transform
      img.style.animation = 'none'
      img.style.transition = 'none'
      img.style.transform =
        matrix && matrix !== 'none' ? matrix : 'scale(1.018)'
      void img.offsetWidth

      let raf2 = 0
      const raf1 = requestAnimationFrame(() => {
        raf2 = requestAnimationFrame(() => {
          img.style.transition =
            'transform 0.58s cubic-bezier(0.22, 1, 0.36, 1)'
          img.style.transform = 'scale(1)'
        })
      })

      const t = window.setTimeout(() => {
        clearCoverInline()
        setBreathPhase('off')
      }, LIBRARY_LIVE_MS.coverExit)
      return () => {
        cancelAnimationFrame(raf1)
        cancelAnimationFrame(raf2)
        window.clearTimeout(t)
      }
    }, [breathPhase, clearCoverInline])

    useEffect(() => {
      if (!cover) {
        setActiveSrc(null)
        setMediaReady(true)
        return
      }

      let cancelled = false
      setMediaReady(false)
      setActiveSrc(null)

      void acquireCoverDecodeSlot().then((release) => {
        if (cancelled) {
          release()
          return
        }
        releaseSlotRef.current = release
        setActiveSrc(cover)
      })

      return () => {
        cancelled = true
        releaseSlotRef.current?.()
        releaseSlotRef.current = null
        releaseCoverImageElement(imgRef.current)
        setActiveSrc(null)
      }
    }, [cover])

    const releaseDecodeSlot = useCallback(() => {
      releaseSlotRef.current?.()
      releaseSlotRef.current = null
    }, [])

    const markReady = useCallback(() => {
      releaseDecodeSlot()
      setMediaReady(true)
    }, [releaseDecodeSlot])

    const handleError = useCallback(
      (e: SyntheticEvent<HTMLImageElement>) => {
        const el = e.currentTarget
        if (el.dataset.fb === '1') {
          markReady()
          return
        }
        el.dataset.fb = '1'
        el.src = coverFallbackUrl(title)
      },
      [markReady, title],
    )

    const mediaBreathClass =
      breathPhase === 'breathing'
        ? ' is-breathing'
        : breathPhase === 'exiting'
          ? ' is-breathing-out'
          : ''

    const handleShellAnimationEnd = useCallback(
      (e: SyntheticEvent<HTMLDivElement>) => {
        if (e.target !== e.currentTarget) return
        if (e.currentTarget.dataset.canvasEnter !== '1') return
        setCanvasEntering(false)
        e.currentTarget.removeAttribute('data-canvas-enter')
        e.currentTarget.style.animationDelay = ''
      },
      [],
    )

    return (
      <div
        className={`library-card-shell rounded-xl ${className || ''}`}
        data-media-ready={mediaReady ? 'true' : 'false'}
        data-canvas-enter={canvasEntering ? '1' : undefined}
        style={
          canvasEntering && enterDelay != null
            ? { animationDelay: `${enterDelay}s` }
            : undefined
        }
        onAnimationEnd={canvasEntering ? handleShellAnimationEnd : undefined}
      >
        <div className={`library-card-media${mediaBreathClass}`}>
          {hasCover ? (
            <img
              ref={imgRef}
              src={activeSrc || undefined}
              alt={title}
              className={`library-card-media__img${mediaReady ? ' is-loaded' : ''}${imgClassName ? ` ${imgClassName}` : ''}`}
              loading={priority ? 'eager' : 'lazy'}
              decoding="async"
              sizes={LIBRARY_CARD_COVER_SIZES}
              draggable={false}
              onLoad={markReady}
              onError={handleError}
            />
          ) : (
            <div className="library-card-media__fallback">{placeholder}</div>
          )}
        </div>
        {children}
      </div>
    )
  },
)

export function getPlatformColor(platform: string) {
  switch (platform.toLowerCase()) {
    case 'bilibili':
      return '#00A1D6'
    case 'steam':
      return '#171a21'
    case 'netease music':
    case 'netease':
      return '#d33a31'
    case 'github':
      return '#24292e'
    case 'bangumi':
      return '#f09199'
    case 'mal':
    case 'myanimelist':
      return '#2E51A2'
    case 'x':
    case 'twitter':
      return '#000000'
    case 'discord':
      return '#5865F2'
    default:
      return '#6b7280'
  }
}

export function getTypeIcon(type: string) {
  switch (type) {
    case 'game':
      return <FaGamepad />
    case 'video':
    case 'anime':
    case 'tv_series':
      return <FaVideo />
    case 'music':
      return <FaMusic />
    case 'book':
      return <FaBook />
    default:
      return <FaBook />
  }
}

export function useLibraryCardActions() {
  const { t, format } = useI18n()

  const watchProgressLabels = useMemo<WatchProgressLabels>(
    () => ({
      progressEp: t.library.progressEp,
      progressEpOnly: t.library.progressEpOnly,
      progressCh: t.library.progressCh,
      progressChOnly: t.library.progressChOnly,
      progressVol: t.library.progressVol,
      progressVolOnly: t.library.progressVolOnly,
      progressJoin: t.library.progressJoin,
      statusDoing: t.library.statusDoing,
      statusDone: t.library.statusDone,
      statusWish: t.library.statusWish,
      statusOnHold: t.library.statusOnHold,
      statusDropped: t.library.statusDropped,
    }),
    [t],
  )

  const resolveWatchProgress = useCallback(
    (item: LibraryItem): WatchProgress | null => {
      return getWatchProgress(item.item_type, item.metadata)
    },
    [],
  )

  const formatItemWatchProgress = useCallback(
    (item: LibraryItem): string | null => {
      const progress = resolveWatchProgress(item)
      if (!progress) return null
      return formatWatchProgressText(progress, watchProgressLabels)
    },
    [resolveWatchProgress, watchProgressLabels],
  )

  const getExtraInfo = useCallback(
    (item: LibraryItem) => {
      if (item.item_type === 'game' && item.metadata.playtime_forever) {
        const hours = Math.round(item.metadata.playtime_forever / 60)
        return format(t.library.playedHours, { hours })
      }
      if (item.item_type === 'music') {
        if (
          isBangumiPlatform(item.platform) ||
          item.id.startsWith('bangumi_subject_')
        ) {
          const rate = Number(item.metadata.rate ?? item.metadata.score) || 0
          if (rate > 0) return `★ ${rate}`
          if (
            typeof item.metadata.artist === 'string' &&
            item.metadata.artist
          ) {
            return item.metadata.artist
          }
          return null
        }
        const artists = item.metadata.ar || item.metadata.artists || []
        if (Array.isArray(artists) && artists.length > 0) {
          return artists.map((a: any) => a.name || a).join(', ')
        }
        if (item.metadata.artist) {
          return item.metadata.artist
        }
      }
      if (
        item.item_type === 'video' ||
        item.item_type === 'anime' ||
        item.item_type === 'tv_series' ||
        item.item_type === 'book'
      ) {
        return formatItemWatchProgress(item)
      }
      return null
    },
    [t, formatItemWatchProgress],
  )

  const renderWatchProgressPanel = useCallback(
    (item: LibraryItem, opts?: { dark?: boolean }): React.ReactNode => {
      const progress = resolveWatchProgress(item)
      if (!progress) return null
      const text = formatWatchProgressText(progress, watchProgressLabels)
      const statusLabel = formatWatchStatusLabel(
        progress.status,
        watchProgressLabels,
        { onlyDoing: true },
      )
      const dark = opts?.dark === true
      const typeKey =
        item.item_type === 'book'
          ? 'book'
          : item.item_type === 'tv_series'
            ? 'tv'
            : item.item_type === 'game'
              ? 'game'
              : item.item_type === 'anime'
                ? 'anime'
                : 'default'
      const pct = Math.max(0, Math.min(100, progress.percent ?? 0))

      return (
        <div
          className={`library-progress${dark ? ' library-progress--dark' : ''}`}
        >
          <div className="library-progress__meta">
            <span className="library-progress__text" title={text}>
              {text}
            </span>
            {statusLabel && (
              <span
                className={`library-progress__chip library-progress__chip--${typeKey}`}
              >
                {statusLabel}
              </span>
            )}
          </div>
          {progress.percent != null && (
            <div
              className={`library-progress__track library-progress__track--${typeKey}`}
              role="progressbar"
              aria-valuenow={Math.round(pct)}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-label={text}
            >
              <div
                className="library-progress__fill"
                style={{ width: `${pct}%` }}
              />
            </div>
          )}
        </div>
      )
    },
    [resolveWatchProgress, watchProgressLabels],
  )

  const handlePlayMusic = useCallback(
    async (item: LibraryItem) => {
      if (
        isBangumiPlatform(item.platform) ||
        item.id.startsWith('bangumi_subject_')
      ) {
        const subjectId = item.id.startsWith('bangumi_subject_')
          ? item.id.slice('bangumi_subject_'.length)
          : String(item.metadata?.id || item.metadata?.subject_id || '')
        const url =
          (typeof item.metadata?.url === 'string' && item.metadata.url) ||
          (subjectId ? `https://bgm.tv/subject/${subjectId}` : '')
        if (url) {
          window.open(url, '_blank', 'noopener,noreferrer')
          showInfo(format(t.library.openExternal, { name: item.title || '' }))
        } else {
          showInfo(t.library.playbackNotSupported)
        }
        return
      }

      const platformKey = item.platform.toLowerCase()
      const isNetease =
        platformKey.includes('netease') ||
        platformKey.includes('网易') ||
        item.id.startsWith('netease_') ||
        item.id.startsWith('netease_song_')

      if (!isNetease) {
        const ext =
          typeof item.metadata?.url === 'string' ? item.metadata.url : ''
        if (ext) {
          window.open(ext, '_blank', 'noopener,noreferrer')
          showInfo(format(t.library.openExternal, { name: item.title || '' }))
        } else {
          showInfo(t.library.playbackNotSupported)
        }
        return
      }

      const songId = (
        item.metadata.id || item.id.replace('netease_song_', '')
      ).toString()
      const musicState = (window as any).__musicPlayerState
      if (
        musicState?.currentSong?.id != null &&
        String(musicState.currentSong.id) === songId
      ) {
        window.dispatchEvent(new CustomEvent('open-control-panel'))
        // 暂停中再点同一首应恢复，不要当成已在播放。
        if (!musicState.isPlaying) {
          window.dispatchEvent(new CustomEvent('toggle-play-pause'))
          showInfo(format(t.library.nowPlaying, { name: item.title || '' }))
        } else {
          showInfo(t.library.alreadyPlaying)
        }
        return
      }

      const isVip = isNeteaseVipFromMeta(item.metadata)
      if (isVip) {
        showInfo(t.library.vipSongWarning)
      }

      const name = item.metadata.name || item.title
      let artist = t.library.unknownArtist
      const artists = item.metadata.ar || item.metadata.artists || []
      if (Array.isArray(artists) && artists.length > 0) {
        artist = artists.map((a: any) => a.name || a).join(', ')
      } else if (item.metadata.artist) {
        artist = item.metadata.artist
      }

      let album = t.library.unknownAlbum
      let cover = item.cover || ''
      if (item.metadata.al) {
        album = item.metadata.al.name || album
        cover = item.metadata.al.picUrl || cover
      } else if (item.metadata.album) {
        album = item.metadata.album.name || item.metadata.album
        if (item.metadata.album.picUrl) {
          cover = item.metadata.album.picUrl
        }
      }

      const duration = item.metadata.dt
        ? Math.floor(item.metadata.dt / 1000)
        : item.metadata.duration
          ? item.metadata.duration
          : 0

      // 同步 URL 禁止 await geo，会把开播拖成数百 ms～数秒。
      const url = getNeteaseAudioUrlImmediate(songId)

      const song: Song = {
        id: songId.toString(),
        name,
        artist,
        album,
        // 裸 CDN 封面必须代理，否则取色 canvas CORS 失败。
        cover: proxyImageUrlOr(cover, cover || ''),
        url,
        duration,
        source: 'netease',
        isVip,
      }

      window.dispatchEvent(new CustomEvent('open-control-panel'))
      showInfo(format(t.library.nowPlaying, { name }))
      window.dispatchEvent(new CustomEvent('play-song', { detail: { song } }))
      void import('../../utils/analyticsEvents').then(
        ({ trackProductEvent, AnalyticsEvents }) => {
          trackProductEvent(AnalyticsEvents.MUSIC_LIBRARY_PLAY, {
            target: songId,
            throttleMs: 3000,
          })
        },
      )
    },
    [t],
  )

  return { getExtraInfo, renderWatchProgressPanel, handlePlayMusic }
}
