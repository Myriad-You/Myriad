import type { CSSProperties, ReactNode, SyntheticEvent } from 'react'
import type {
  LibraryCanvasLayout,
  LibraryCanvasTransform,
} from '../utils/libraryCanvas'
import type {
  LibraryLayoutMode,
  LibraryPreferencesUpdatedDetail,
} from '../utils/libraryPreferences'
import type {
  WatchProgress,
  WatchProgressLabels,
} from '../utils/libraryWatchProgress'
import type { Song } from '../utils/musicPlayer'

import { FaBook, FaGamepad, FaMusic, FaVideo } from '@lib/icons'
import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useI18n } from '../contexts/I18nContext'
import { userFacingError } from '../utils/userFacingError'
import { useMusicLyricsSlice } from '../contexts/MusicPlayerContext'
import { useLibraryIntersectionObserver } from '../hooks/animation'
import { softLockWallpaperForLibraryCanvas } from '../hooks/useEvocativeWallpaper'
import { useLibraryCanvasControls } from '../hooks/useLibraryCanvasControls'
import { usePerformanceProfile } from '../hooks/usePerformanceProfile'
import { useSharedResize } from '../hooks/useSharedEventListener'
import {
  buildCenterOutCanvasLayout,
  getLibraryCanvasFocusScale,
  getLibraryCanvasViewportBinKey,
  getLibraryCanvasViewportBounds,
  LIBRARY_CANVAS_STRIDE,
  libraryCanvasLayoutIntersects,
} from '../utils/libraryCanvas'
import {
  acquireCoverDecodeSlot,
  LIBRARY_CARD_COVER_SIZES,
  releaseCoverImageElement,
} from '../utils/libraryCardMedia'
import { slimLibraryItems } from '../utils/libraryItemSlim'
import {
  LIBRARY_PREFERENCES_UPDATED_EVENT,
  resolveLibraryLayoutMode,
} from '../utils/libraryPreferences'
import {
  formatWatchProgressText,
  formatWatchStatusLabel,
  getWatchProgress,
} from '../utils/libraryWatchProgress'
import {
  audioManager,
  getNeteaseAudioUrlImmediate,
  isNeteaseVipFromMeta,
} from '../utils/musicPlayer'
import { isMobileNavLayout } from '../utils/navLayout'
import { proxyImageUrlOr } from '../utils/proxyImageUrl'
import { getLibraryDataPageDeduped } from '../utils/requestDedup'
import { showInfo } from '../utils/toastManager'
import { LibraryCanvasChrome } from './library/LibraryCanvasChrome'
import PlatformIcon from './PlatformIcon'
import { LyricWaveScroll } from './shared/LyricWaveScroll'
import { QuickTransition } from './SkeletonTransition'
import { Spinner } from './Spinner'

/**
 * 资料库「正在播 / 换歌退场」动效时长（ms）— 一处改、全局对齐。
 * LEAVE_HOLD ≥ 歌词退场 ≥ CSS is-leaving；COVER_EXIT 对齐呼吸收回 transition。
 */
const LIBRARY_LIVE_MS = {
  /** 换歌后旧卡保留挂载，盖住歌词+光带+封面退场 */
  leaveHold: 560,
  /** LibraryCardLyrics 卸 DOM（对齐 .is-leaving ~0.52s） */
  lyricsUnmount: 520,
  /** 封面呼吸收回后卸 phase */
  coverExit: 600,
} as const

/**
 * Session-scoped: canvas cards that already played (or silently claimed) their
 * first-reveal enter. Prevents virtualization remounts from replaying motion.
 */
const canvasCardRevealedIds = new Set<string>()

/**
 * Claim first-reveal for a canvas card id.
 * Returns enter delay seconds when the shell should animate; null to mount quiet
 * (already seen, or surface is mid-drag).
 */
function claimCanvasCardEnter(
  itemId: string,
  itemIndex: number,
  surfaceDragging: boolean,
): number | null {
  if (canvasCardRevealedIds.has(itemId)) return null
  canvasCardRevealedIds.add(itemId)
  if (surfaceDragging) return null
  return Math.min((itemIndex % 12) * 0.055, 0.6)
}

// 注入 / 热更新资料库网格样式（HMR 时覆写 textContent）
if (typeof document !== 'undefined') {
  let style = document.getElementById(
    'library-grid-styles',
  ) as HTMLStyleElement | null
  if (!style) {
    style = document.createElement('style')
    style.id = 'library-grid-styles'
    document.head.appendChild(style)
  }
  style.textContent = `
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

        /* 画布虚拟化会按视口装卸卡片；外层禁用 fadeInUp（焦点 scale 占用 transform）。 */
        .library-canvas-world .library-card-container {
            animation: none;
            transition: transform 120ms ease-out;
            /* 静止不占合成层；拖拽时再开 will-change，避免几十张卡常驻 GPU 内存 */
        }

        /*
         * 首次揭示入场挂在内层 shell（不碰外层 focus scale）。
         * 回扫已见 id / 拖拽中静默挂载，避免虚拟化重播与平移时弹入。
         */
        @keyframes library-canvas-shell-enter {
            from {
                opacity: 0;
                transform: scale(0.96);
            }
            to {
                opacity: 1;
                transform: scale(1);
            }
        }

        .library-canvas-world .library-card-shell[data-canvas-enter='1'] {
            animation: library-canvas-shell-enter 0.62s cubic-bezier(0.22, 1, 0.36, 1) both;
        }

        @media (prefers-reduced-motion: reduce) {
            .library-canvas-world .library-card-shell[data-canvas-enter='1'] {
                animation: none;
                opacity: 1;
                transform: none;
            }
        }

        [data-library-canvas-surface='true'][data-dragging='true'] {
            cursor: grabbing;
        }

        [data-library-canvas-surface='true'][data-dragging='true'] .library-canvas-world {
            will-change: transform;
        }

        [data-library-canvas-surface='true'][data-dragging='true'] .library-card-container {
            will-change: transform;
            transition: none;
        }

        [data-library-canvas-surface='true'],
        [data-library-canvas-surface='true'] .library-card-container {
            -webkit-user-select: none;
            user-select: none;
        }

        [data-library-canvas-surface='true'] .library-card-container img {
            -webkit-user-drag: none;
            user-select: none;
        }

        /*
         * 无限画布本身已响应指针；背景强制静止，避免双重位移与点击涟漪干扰。
         * transform 过渡由 useEvocativeWallpaper soft-lock 写入（需能先 transition:none
         * 缓存 from 帧再缓入 identity；此处勿 !important，否则 from 帧插值被掐断）。
         * 退出画布后 soft-restore 缓回 scale。
         */
        html[data-library-canvas='active'] #wallpaper {
            animation: none !important;
        }

        html[data-library-canvas='active'] #wallpaper-ripple-canvas {
            /* 长时画布会话用 display 省合成；恢复时 canvas 会重建并从 opacity 0 淡入 */
            display: none !important;
        }

        html[data-library-canvas='active'] #wallpaper-awaiting-fx,
        html[data-library-canvas='active'] #wallpaper-awaiting-fx::after {
            animation: none !important;
        }

        /*
         * 封面就绪门闩：lazy 图未完成时先不画标题玻璃 / 平台标，
         * 否则空白底上 backdrop 会先闪出来再变成完整卡。
         */
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

        .animate-fade-in {
            animation: fadeIn 0.5s ease-out forwards;
        }

        @keyframes fadeIn {
            from {
                opacity: 0;
            }
            to {
                opacity: 1;
            }
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
            backdrop-filter: blur(8px);
            -webkit-backdrop-filter: blur(8px);
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

        /* 加载按钮样式 */
        .load-more-btn {
            padding: 0.625rem 1.5rem;
            border-radius: 0.5rem;
            transition: all 0.2s;
            font-size: 0.875rem;
            font-weight: 500;
            box-shadow: 0 1px 2px 0 rgba(0, 0, 0, 0.05);
        }

        .load-more-btn:hover {
            box-shadow: 0 4px 6px -1px rgba(0, 0, 0, 0.1);
        }

        /* 加载按钮主题色 */
        .load-more-btn.primary-load-btn {
            background-color: color-mix(in srgb, var(--color-primary, #3b82f6) 10%, transparent);
            color: var(--color-primary, #3b82f6);
            border: 1px solid color-mix(in srgb, var(--color-primary, #3b82f6) 20%, transparent);
        }

        .load-more-btn.primary-load-btn:hover {
            background-color: color-mix(in srgb, var(--color-primary, #3b82f6) 15%, transparent);
        }

        /*
         * 播放中：真实频谱驱动的连续四边水波「光带」
         * 路径贴边；宽 stroke + 强 blur 同时晕向卡内与卡外
         */
        .library-playing-wave {
            --music-color: #ef4444;
            --wave-r: 0.75rem;
            /*
             * 宽 stroke + 强 blur 需要较大外扩，否则光晕面积会被 clip 裁成细线感。
             * 仍被平行圆角裁齐；谷区始终有外侧底光
             */
            position: absolute;
            inset: -28px;
            z-index: 3;
            pointer-events: none;
            overflow: hidden;
            border-radius: calc(var(--wave-r) + 28px);
            clip-path: inset(0 round calc(var(--wave-r) + 28px));
            -webkit-clip-path: inset(0 round calc(var(--wave-r) + 28px));
            isolation: isolate;
        }

        .library-playing-wave svg {
            position: absolute;
            inset: 0;
            width: 100%;
            height: 100%;
            overflow: hidden;
        }

        .library-playing-wave__band {
            fill: none;
            stroke: var(--music-color);
            stroke-linejoin: round;
            stroke-linecap: round;
            /* 透明度由 rAF 逐帧插值，避免 CSS transition 与 JS 抢控制导致闪切 */
        }

        /*
         * 面积优先：两层都是宽 stroke + 强 blur 的柔光带
         * 峰靠路径外鼓成「光团」，不要细线/硬核
         */
        .library-playing-wave__band--soft {
            stroke-width: 42;
            opacity: 0;
            filter: blur(16px);
        }

        .library-playing-wave__band--mid {
            stroke-width: 26;
            opacity: 0;
            filter: blur(9px);
        }

        /*
         * 资料库卡片歌词外壳（进出场 + 遮罩）
         * 行级波浪引擎见 shared/LyricWaveScroll
         */
        .library-card-lyrics {
            --music-color: #ef4444;
            position: absolute;
            inset: 0;
            z-index: 2;
            pointer-events: none;
            display: flex;
            align-items: flex-end;
            justify-content: center;
            padding: 0 0.5rem 0.45rem;
            opacity: 0;
            transform: translateY(6px) scale(0.985);
            filter: blur(0);
            transition:
                opacity 0.42s cubic-bezier(0.22, 1, 0.36, 1),
                transform 0.48s cubic-bezier(0.22, 1, 0.36, 1),
                filter 0.4s ease;
        }

        .library-card-lyrics.is-on {
            opacity: 1;
            transform: translateY(0) scale(1);
            filter: blur(0);
        }

        /* 播完/换歌退场：略下沉 + 微缩 + 轻糊，比硬淡出更顺 */
        .library-card-lyrics.is-leaving {
            opacity: 0;
            transform: translateY(8px) scale(0.97);
            filter: blur(1.2px);
            transition:
                opacity 0.48s cubic-bezier(0.33, 1, 0.68, 1),
                transform 0.52s cubic-bezier(0.33, 1, 0.68, 1),
                filter 0.42s cubic-bezier(0.4, 0, 0.2, 1);
        }

        /* 播放/退场中不藏歌词、不抢退场 */
        .group:not(.is-hover-locked):hover .library-card-lyrics.is-on {
            opacity: 0;
            transform: translateY(4px) scale(0.99);
            filter: blur(0.4px);
            transition-duration: 0.22s;
        }

        /* 播放/退场：压掉 Tailwind group-hover 信息层 */
        .group.is-hover-locked:hover .library-card-hover-chrome {
            opacity: 0 !important;
        }

        /* 播放/退场：卡片不抬升/放大（只锁 transform，不硬改阴影） */
        .group.is-hover-locked .library-card-shell {
            transform: none !important;
        }

        .library-card-lyrics__mask {
            position: absolute;
            inset: 0;
            border-radius: inherit;
            background:
                linear-gradient(
                    to top,
                    color-mix(
                        in srgb,
                        var(--music-color) 55%,
                        rgb(0 0 0 / 90%)
                    ) 0%,
                    color-mix(
                        in srgb,
                        var(--music-color) 38%,
                        rgb(0 0 0 / 72%)
                    ) 28%,
                    color-mix(
                        in srgb,
                        var(--music-color) 16%,
                        transparent
                    ) 55%,
                    transparent 78%
                );
            opacity: 0.95;
            transition:
                opacity 0.4s ease,
                background 0.45s ease;
        }

        .library-card-lyrics.is-leaving .library-card-lyrics__mask {
            opacity: 0;
            transition-duration: 0.45s;
        }

        @media (prefers-reduced-motion: reduce) {
            .library-card-lyrics,
            .library-card-lyrics.is-on,
            .library-card-lyrics.is-leaving {
                transition: opacity 0.15s ease;
                transform: none;
                filter: none;
            }
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
            backdrop-filter: blur(10px);
            -webkit-backdrop-filter: blur(10px);
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
    `
}

interface LibraryItem {
  id: string
  item_type: 'game' | 'video' | 'music' | 'anime' | 'tv_series' | 'book'
  title: string
  cover: string | null
  platform: string
  metadata: any
}

function coverFallbackUrl(title: string): string {
  return `https://ui-avatars.com/api/?name=${encodeURIComponent(title || '?')}&size=400&background=random`
}

/** 从 metadata / id 解析可打开的外链；无有效 http(s) 则返回 null */
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

  // Steam
  if (platform.includes('steam') || id.startsWith('steam_')) {
    const appid = m.appid ?? id.replace(/^steam_game_/, '')
    if (appid !== '' && appid != null) {
      return `https://store.steampowered.com/app/${appid}`
    }
  }

  // Bilibili
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

  // Bangumi
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

  // MyAnimeList
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

  // GitHub
  if (platform.includes('github')) {
    if (typeof m.full_name === 'string' && m.full_name.includes('/')) {
      return `https://github.com/${m.full_name}`
    }
    if (typeof m.name === 'string' && typeof m.owner?.login === 'string') {
      return `https://github.com/${m.owner.login}/${m.name}`
    }
  }

  // YouTube
  if (platform.includes('youtube') || platform.includes('yt')) {
    const vid = m.video_id ?? m.id
    if (typeof vid === 'string' && vid.length >= 6) {
      return `https://www.youtube.com/watch?v=${vid}`
    }
  }

  return null
}

function openLibraryItemExternal(item: {
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

function preferReducedMotion(): boolean {
  if (typeof window === 'undefined' || !window.matchMedia) return false
  return window.matchMedia('(prefers-reduced-motion: reduce)').matches
}

function readCardCornerRadius(el: HTMLElement): { rx: number; ry: number } {
  const cs = getComputedStyle(el)
  const parsePair = (raw: string): [number, number] => {
    const parts = raw
      .trim()
      .split(/\s+/)
      .map((p) => Number.parseFloat(p) || 0)
    if (parts.length >= 2) return [parts[0], parts[1]]
    return [parts[0] || 12, parts[0] || 12]
  }
  const [tlx, tly] = parsePair(cs.borderTopLeftRadius)
  const [trx, try_] = parsePair(cs.borderTopRightRadius)
  const [brx, bry] = parsePair(cs.borderBottomRightRadius)
  const [blx, bly] = parsePair(cs.borderBottomLeftRadius)
  const rx = (tlx + trx + brx + blx) / 4
  const ry = (tly + try_ + bry + bly) / 4
  return {
    rx: rx > 0 ? rx : 12,
    ry: ry > 0 ? ry : 12,
  }
}

/** 圆角矩形周长 t∈[0,1] → 点 + 内法线（像素）；角用椭圆参数方程保证连续 */
function pointOnRoundedRect(
  w: number,
  h: number,
  rx: number,
  ry: number,
  t: number,
): { x: number; y: number; nx: number; ny: number } {
  const ax = Math.max(0.5, Math.min(rx, w / 2 - 0.01))
  const ay = Math.max(0.5, Math.min(ry, h / 2 - 0.01))
  const sw = Math.max(0, w - 2 * ax)
  const sh = Math.max(0, h - 2 * ay)
  // 四分椭圆弧长（Ramanujan 近似）
  const arc =
    (Math.PI * (3 * (ax + ay) - Math.sqrt((3 * ax + ay) * (ax + 3 * ay)))) / 8
  const segs = [sw, arc, sh, arc, sw, arc, sh, arc]
  const total = segs.reduce((a, b) => a + b, 0) || 1
  let dist = (((t % 1) + 1) % 1) * total

  const onArc = (cx: number, cy: number, a0: number, a1: number, u: number) => {
    const a = a0 + (a1 - a0) * u
    const cos = Math.cos(a)
    const sin = Math.sin(a)
    // 椭圆外法线 ∝ (cos/rx, sin/ry)，取反为内
    const nx = cos / ax
    const ny = sin / ay
    const len = Math.hypot(nx, ny) || 1
    return {
      x: cx + ax * cos,
      y: cy + ay * sin,
      nx: -nx / len,
      ny: -ny / len,
    }
  }

  if (dist <= segs[0]) {
    const u = segs[0] > 0 ? dist / segs[0] : 0
    return { x: ax + u * sw, y: 0, nx: 0, ny: 1 }
  }
  dist -= segs[0]
  if (dist <= segs[1]) {
    return onArc(w - ax, ay, -Math.PI / 2, 0, dist / Math.max(segs[1], 1e-6))
  }
  dist -= segs[1]
  if (dist <= segs[2]) {
    const u = segs[2] > 0 ? dist / segs[2] : 0
    return { x: w, y: ay + u * sh, nx: -1, ny: 0 }
  }
  dist -= segs[2]
  if (dist <= segs[3]) {
    return onArc(w - ax, h - ay, 0, Math.PI / 2, dist / Math.max(segs[3], 1e-6))
  }
  dist -= segs[3]
  if (dist <= segs[4]) {
    const u = segs[4] > 0 ? dist / segs[4] : 0
    return { x: w - ax - u * sw, y: h, nx: 0, ny: -1 }
  }
  dist -= segs[4]
  if (dist <= segs[5]) {
    return onArc(
      ax,
      h - ay,
      Math.PI / 2,
      Math.PI,
      dist / Math.max(segs[5], 1e-6),
    )
  }
  dist -= segs[5]
  if (dist <= segs[6]) {
    const u = segs[6] > 0 ? dist / segs[6] : 0
    return { x: 0, y: h - ay - u * sh, nx: 1, ny: 0 }
  }
  dist -= segs[6]
  return onArc(
    ax,
    ay,
    Math.PI,
    (Math.PI * 3) / 2,
    dist / Math.max(segs[7], 1e-6),
  )
}

function sampleBand(bands: number[], t: number): number {
  if (!bands.length) return 0
  const x = (((t % 1) + 1) % 1) * bands.length
  const i0 = Math.floor(x) % bands.length
  const i1 = (i0 + 1) % bands.length
  const f = x - Math.floor(x)
  return bands[i0] * (1 - f) + bands[i1] * f
}

/** 圆周距离 [0, 0.5] */
function circDist(a: number, b: number): number {
  const d = Math.abs((((a - b) % 1) + 1) % 1)
  return d > 0.5 ? 1 - d : d
}

/**
 * 光晕即波浪（面积靠宽 stroke + blur）：
 * - 整圈厚度/起伏跟 8 段频谱 + 相位流动
 * - 最多 2 个高峰鼓包（高度/宽/位置可随机 + 频谱）
 * - presence 只负责淡入淡出，不抹平动态范围
 */
function buildSpectrumWavePath(
  w: number,
  h: number,
  rx: number,
  ry: number,
  bands: number[],
  opts: {
    energy: number
    onset: number
    treble: number
    bass: number
    mid: number
    flux: number
    /** 频谱绕边流动相位 */
    wavePhase: number
    /** 次级随机相位 */
    noisePhase: number
    peak1T: number
    peak2T: number
    peak1H: number
    peak2H: number
    peak1W: number
    peak2W: number
    presence: number
  },
  ox = 0,
  oy = 0,
  maxOutPx = 26,
): string {
  const {
    energy,
    onset,
    treble,
    bass,
    mid,
    flux,
    wavePhase,
    noisePhase,
    peak1T,
    peak2T,
    peak1H,
    peak2H,
    peak1W,
    peak2W,
    presence,
  } = opts
  const p = Math.max(0, Math.min(1, presence))
  if (p < 0.004) return ''

  const peakCap = Math.max(12, maxOutPx - 1)

  // 底环：安静薄、响乐厚（始终外侧有光，但不锁死固定厚度）
  const basePx = (2.2 + energy * 5.5 + bass * 4.2 + mid * 1.6) * p
  // 频谱沿边起伏幅度（主动态来源）
  const flowAmp = (3.5 + energy * 7 + treble * 6 + flux * 4 + onset * 3.5) * p
  // 高峰额外鼓出
  const peakAmp = (5 + energy * 6 + treble * 10 + onset * 8 + bass * 2) * p

  // 峰宽：跟 peakW + 频谱（低音宽、高频尖）
  const sig1 = Math.max(
    0.032,
    Math.min(0.12, (0.042 + bass * 0.04 - treble * 0.015) * peak1W),
  )
  const sig2 = Math.max(
    0.028,
    Math.min(0.11, (0.038 + mid * 0.03 + treble * 0.012) * peak2W),
  )
  const sharp1 = 1.9 + treble * 0.9 + (1 - Math.min(1, peak1W)) * 0.6
  const sharp2 = 2.0 + treble * 1.1 + (1 - Math.min(1, peak2W)) * 0.7

  // 频谱绕边滚动：相位把 8 段「转」起来 → 能感到流动且跟音乐
  const bandSpin = wavePhase * 0.09
  const bandSpin2 = wavePhase * 0.055 + noisePhase * 0.03

  const samples = Math.min(240, Math.max(130, Math.round((w + h) * 0.5)))
  const pts: { x: number; y: number }[] = []

  for (let i = 0; i < samples; i++) {
    const t = i / samples
    const { x, y, nx, ny } = pointOnRoundedRect(w, h, rx, ry, t)

    // 局部频谱（滚动采样）— 这是「不固定」的核心
    const local = sampleBand(bands, t + bandSpin)
    const localB = sampleBand(bands, t * 1.7 + bandSpin2)
    const localC = sampleBand(bands, t * 0.55 - bandSpin * 0.6)

    // 双峰超高斯：高度完全由 peakH * 局部频谱调制，可落到很低
    const d1 = circDist(t, peak1T) / sig1
    const d2 = circDist(t, peak2T) / sig2
    const e1 =
      Math.exp(-(d1 ** sharp1)) *
      Math.max(0.05, peak1H) *
      (0.35 + local * 0.9 + bass * 0.35 + onset * 0.4)
    const e2 =
      Math.exp(-(d2 ** sharp2)) *
      Math.max(0.05, peak2H) *
      (0.3 + local * 1.0 + treble * 0.5 + onset * 0.45)
    const peakBlob = Math.max(e1, e2)

    // 沿边频谱起伏（无峰处也有高低，避免「死环」）
    const flow =
      local * 0.55 +
      localB * 0.28 +
      localC * 0.17 +
      // 弱谐波：用频谱能量缩放，不是固定正弦波
      Math.sin(wavePhase * 0.9 + t * Math.PI * 2 * (1.4 + mid * 0.8)) *
        (0.08 + treble * 0.18 + energy * 0.1) *
        (0.25 + local) +
      Math.sin(noisePhase + t * Math.PI * 2 * (2.3 + treble)) *
        (0.05 + flux * 0.12) *
        (0.2 + localB)

    // 像素外扩：底 + 频谱流 + 高峰
    let outPx =
      basePx * (0.75 + energy * 0.35) +
      Math.max(0, flow) * flowAmp +
      peakBlob * peakAmp

    // 谷区仍保持外侧底光，但不锁成固定环
    const floor = basePx * (0.55 + bass * 0.2)
    outPx = Math.max(floor, Math.min(peakCap, outPx))

    const cornerEase = cornerWeight(t, w, h, rx, ry)
    const a = -outPx * (0.82 + 0.18 * cornerEase)
    pts.push({ x: ox + x + nx * a, y: oy + y + ny * a })
  }

  if (pts.length < 3) return ''
  let d = `M ${pts[0].x.toFixed(2)} ${pts[0].y.toFixed(2)}`
  for (let i = 0; i < pts.length; i++) {
    const p0 = pts[i]
    const p1pt = pts[(i + 1) % pts.length]
    const midX = (p0.x + p1pt.x) / 2
    const midY = (p0.y + p1pt.y) / 2
    d += ` Q ${p0.x.toFixed(2)} ${p0.y.toFixed(2)} ${midX.toFixed(2)} ${midY.toFixed(2)}`
  }
  d += ' Z'
  return d
}

/** 直边≈1、圆角中心≈0：角上少起伏，贴圆角更稳 */
function cornerWeight(
  t: number,
  w: number,
  h: number,
  rx: number,
  ry: number,
): number {
  const ax = Math.max(0.5, Math.min(rx, w / 2))
  const ay = Math.max(0.5, Math.min(ry, h / 2))
  const sw = Math.max(0, w - 2 * ax)
  const sh = Math.max(0, h - 2 * ay)
  const arc =
    (Math.PI * (3 * (ax + ay) - Math.sqrt((3 * ax + ay) * (ax + 3 * ay)))) / 8
  const segs = [sw, arc, sh, arc, sw, arc, sh, arc]
  const total = segs.reduce((a, b) => a + b, 0) || 1
  let dist = (((t % 1) + 1) % 1) * total
  for (let i = 0; i < 8; i++) {
    if (dist <= segs[i]) {
      // 奇数段是角
      if (i % 2 === 1) {
        const u = segs[i] > 0 ? dist / segs[i] : 0
        // 角中心最贴边（weight 低），两端过渡到直边
        return 0.25 + 0.75 * Math.sin(u * Math.PI)
      }
      return 1
    }
    dist -= segs[i]
  }
  return 1
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t
}

/** 圆周上最短弧插值 → 0..1 */
function circLerp(a: number, b: number, t: number): number {
  const d = ((b - a + 1.5) % 1) - 0.5
  return (a + d * t + 1) % 1
}

/** smoothstep：淡入淡出更柔和 */
function smoothstep01(x: number): number {
  const t = Math.max(0, Math.min(1, x))
  return t * t * (3 - 2 * t)
}

/** 出场：前段加速到位（弹起感） */
function easeOutCubic(t: number): number {
  const x = Math.max(0, Math.min(1, t))
  return 1 - (1 - x) ** 3
}

/** 退场：先慢后快收束，避免突然塌缩 */
function easeInCubic(t: number): number {
  const x = Math.max(0, Math.min(1, t))
  return x * x * x
}

/**
 * 有机随机游走（Ornstein–Uhlenbeck 近似）
 * 均值回归 + 噪声，不会瞬跳
 */
function ouStep(
  value: number,
  mean: number,
  reversion: number,
  noise: number,
  dt: number,
): number {
  const n =
    (Math.random() * 2 - 1) * noise * Math.sqrt(Math.max(0.001, dt) * 30)
  return value + (mean - value) * reversion * dt * 30 + n
}

/**
 * 真实频谱驱动的连续四边柔光带
 * 频谱快响应 + 可见随机漂移；
 * 出场弹起 / 退场频谱残留收束，避免硬切与塌成细环
 *
 * active  = 当前曲（含暂停）→ 光晕保持显示
 * playing = 真正在播 → 频谱动画；暂停只冻结末帧，不隐藏
 * !active = 换歌离场 → 残留收束后卸载
 */
const LibraryPlayingWaveBorder = memo(({
  musicColor,
  active,
  playing = false,
}: {
  musicColor: string
  /** true=当前曲（含暂停）；false=换歌离场 */
  active: boolean
  /** true=正在播放（驱动频谱）；false=暂停冻结 */
  playing?: boolean
}) => {
  const wrapRef = useRef<HTMLDivElement>(null)
  const svgRef = useRef<SVGSVGElement>(null)
  const softRef = useRef<SVGPathElement>(null)
  const midRef = useRef<SVGPathElement>(null)
  const prevBandsRef = useRef<number[]>([0, 0, 0, 0, 0, 0, 0, 0])
  const smoothBandsRef = useRef<number[]>([0, 0, 0, 0, 0, 0, 0, 0])
  /** 暂停冻结 / 退场残留频谱 */
  const residualBandsRef = useRef<number[]>([0, 0, 0, 0, 0, 0, 0, 0])
  const energyHistRef = useRef<number[]>([])
  const wavePhaseRef = useRef(Math.random() * Math.PI * 2)
  const noisePhaseRef = useRef(Math.random() * Math.PI * 2)
  const peak1TRef = useRef(Math.random())
  const peak2TRef = useRef((peak1TRef.current + 0.3 + Math.random() * 0.35) % 1)
  const peak1HRef = useRef(0.7 + Math.random() * 0.5)
  const peak2HRef = useRef(0.65 + Math.random() * 0.55)
  const peak1WRef = useRef(0.55 + Math.random() * 0.5)
  const peak2WRef = useRef(0.5 + Math.random() * 0.55)
  const hBias1Ref = useRef((Math.random() - 0.5) * 0.5)
  const hBias2Ref = useRef((Math.random() - 0.5) * 0.55)
  const wBias1Ref = useRef((Math.random() - 0.5) * 0.4)
  const wBias2Ref = useRef((Math.random() - 0.5) * 0.45)
  const v1Ref = useRef((Math.random() - 0.5) * 0.006)
  const v2Ref = useRef((Math.random() - 0.5) * 0.006)
  const anchor1Ref = useRef(peak1TRef.current)
  const anchor2Ref = useRef(peak2TRef.current)
  const nextReseedAtRef = useRef(1.2 + Math.random() * 1.5)
  const timeAccRef = useRef(0)
  const introRef = useRef(0)
  const bodySmoothRef = useRef(0)
  const opacitySmoothRef = useRef(0)
  /** 出场瞬间高亮 kick（0→1 后衰减） */
  const enterKickRef = useRef(0)
  const speedSmoothRef = useRef(0.06)
  const noiseSpeedRef = useRef(0.03 + Math.random() * 0.04)
  const sizeRef = useRef({ w: 0, h: 0, rx: 12, ry: 12 })
  const activeRef = useRef(active)
  const playingRef = useRef(playing)
  const prevPlayingRef = useRef(playing)
  activeRef.current = active
  playingRef.current = playing
  const [mounted, setMounted] = useState(active)

  useEffect(() => {
    if (active) setMounted(true)
  }, [active])

  useEffect(() => {
    if (!mounted) return

    // 刚切入当前曲：重置形态并打一记出场 kick（暂停再播不走这里）
    if (active && introRef.current < 0.08) {
      const t1 = Math.random()
      const t2 = (t1 + 0.28 + Math.random() * 0.4) % 1
      peak1TRef.current = t1
      peak2TRef.current = t2
      anchor1Ref.current = t1
      anchor2Ref.current = t2
      peak1HRef.current = 0.55 + Math.random() * 0.65
      peak2HRef.current = 0.5 + Math.random() * 0.7
      peak1WRef.current = 0.4 + Math.random() * 0.7
      peak2WRef.current = 0.35 + Math.random() * 0.75
      hBias1Ref.current = (Math.random() - 0.5) * 0.6
      hBias2Ref.current = (Math.random() - 0.5) * 0.65
      wBias1Ref.current = (Math.random() - 0.5) * 0.5
      wBias2Ref.current = (Math.random() - 0.5) * 0.55
      v1Ref.current = (Math.random() - 0.5) * 0.008
      v2Ref.current = (Math.random() - 0.5) * 0.008
      noiseSpeedRef.current = 0.025 + Math.random() * 0.05
      wavePhaseRef.current = Math.random() * Math.PI * 2
      noisePhaseRef.current = Math.random() * Math.PI * 2
      nextReseedAtRef.current = 1.4 + Math.random() * 2
      timeAccRef.current = 0
      opacitySmoothRef.current = 0
      bodySmoothRef.current = 0
      enterKickRef.current = 1
      // 给一点初始环，避免首帧全空
      residualBandsRef.current = residualBandsRef.current.map(
        () => 0.18 + Math.random() * 0.12,
      )
    }

    const audio = audioManager.getCurrentAudio()
    if (audio && playing) {
      audioManager.connectAudioToAnalyser(audio)
      void audioManager.resumeAudioContext()
    }

    const wrap = wrapRef.current
    // 点击层无圆角；尺寸/圆角以 .library-card-shell 为准（缺省再退回 parent）
    const shell =
      (wrap?.closest('.library-card-shell') as HTMLElement | null) ||
      (wrap?.parentElement as HTMLElement | null)
    const PAD = 28

    const syncGeometry = () => {
      if (!wrap || !shell) return
      const w = shell.clientWidth
      const h = shell.clientHeight
      if (w < 2 || h < 2) return
      let { rx, ry } = readCardCornerRadius(shell)
      if (!(rx > 0)) rx = 12
      if (!(ry > 0)) ry = 12
      const r = Math.min(rx, ry, w / 2, h / 2)
      sizeRef.current = { w, h, rx: r, ry: r }
      const rCss = `${r}px`
      const clipR = `${r + PAD}px`
      wrap.style.setProperty('--wave-r', rCss)
      wrap.style.borderRadius = clipR
      wrap.style.clipPath = `inset(0 round ${clipR})`
      ;(
        wrap.style as CSSStyleDeclaration & { webkitClipPath?: string }
      ).webkitClipPath = `inset(0 round ${clipR})`
      const svg = svgRef.current
      if (svg) {
        svg.setAttribute('viewBox', `0 0 ${w + PAD * 2} ${h + PAD * 2}`)
        svg.setAttribute('width', '100%')
        svg.setAttribute('height', '100%')
      }
    }

    syncGeometry()
    const ro =
      typeof ResizeObserver !== 'undefined'
        ? new ResizeObserver(() => syncGeometry())
        : null
    if (shell && ro) ro.observe(shell)

    const reducedMotion = preferReducedMotion()
    let raf = 0
    let last = 0
    /** reduced-motion / 暂停冻结：到位后停 rAF，playing/active 变化会重跑 effect */
    let settled = false

    const tick = (now: number) => {
      const dt = last ? Math.min(0.05, (now - last) / 1000) : 0.032
      // ~45fps：频谱更跟得上；减动效略降采样
      const frameMs = reducedMotion ? 48 : 22
      if (now - last >= frameMs) {
        last = now
        const live = activeRef.current
        const isPlaying = playingRef.current

        // 从暂停恢复播放：补 kick，不重置整圈形态
        if (isPlaying && !prevPlayingRef.current) {
          enterKickRef.current = Math.max(enterKickRef.current, 0.85)
          settled = false
          const audioNow = audioManager.getCurrentAudio()
          if (audioNow) {
            audioManager.connectAudioToAnalyser(audioNow)
            void audioManager.resumeAudioContext()
          }
        }
        prevPlayingRef.current = isPlaying

        // 出场/保持由 live 决定；暂停仍 full presence，只有换歌才退场
        const introTarget = live ? 1 : 0
        const introRate = live ? 2.55 : 0.72
        introRef.current = lerp(
          introRef.current,
          introTarget,
          1 - Math.exp(-introRate * dt * 30),
        )
        const intro = Math.max(0, Math.min(1, introRef.current))
        // 几何 presence：出场 easeOut 弹开，退场 easeIn 先稳后收
        const presence = live
          ? easeOutCubic(smoothstep01(intro))
          : easeInCubic(smoothstep01(intro))
        // 透明度：出场略滞后；退场略快于几何收缩，避免「空壳还亮」
        const opacityPresence = live
          ? easeOutCubic(smoothstep01(Math.max(0, intro * 1.08 - 0.05)))
          : easeInCubic(smoothstep01(Math.min(1, intro * 1.25)))

        // 出场 kick 衰减（~0.45s）
        enterKickRef.current = lerp(
          enterKickRef.current,
          0,
          1 - Math.exp(-(isPlaying ? 3.2 : 5.5) * dt),
        )
        const kick = reducedMotion ? 0 : enterKickRef.current

        // 仅换歌离场后卸载；暂停不卸
        if (
          !live &&
          introRef.current < 0.012 &&
          opacitySmoothRef.current < 0.02
        ) {
          introRef.current = 0
          opacitySmoothRef.current = 0
          bodySmoothRef.current = 0
          enterKickRef.current = 0
          residualBandsRef.current = [0, 0, 0, 0, 0, 0, 0, 0]
          softRef.current?.setAttribute('d', '')
          midRef.current?.setAttribute('d', '')
          if (softRef.current) softRef.current.style.opacity = '0'
          if (midRef.current) midRef.current.style.opacity = '0'
          setMounted(false)
          return
        }

        // 暂停冻结 / reduced 静环：到位后停 rAF，保留末帧
        if (
          live &&
          !isPlaying &&
          presence > 0.98 &&
          settled
        ) {
          return
        }
        if (reducedMotion && live && isPlaying && presence > 0.98 && settled) {
          return
        }

        const { w, h, rx, ry } = sizeRef.current
        if (w > 0 && h > 0) {
          // 暂停且已到位：完全不改 path/opacity，末帧定格
          if (live && !isPlaying && presence > 0.98) {
            settled = true
          } else {
          let raw: number[]
          if (reducedMotion) {
            // 静态柔环，不读频谱、不流动
            const level = live ? 0.32 : 0.12 * presence
            raw = [
              level,
              level,
              level * 0.95,
              level * 0.9,
              level * 0.9,
              level * 0.85,
              level * 0.85,
              level * 0.8,
            ]
          } else if (isPlaying) {
            raw = audioManager.getSpectrumBands()
            // 缓存末帧，供暂停冻结 / 退场残留
            for (let i = 0; i < 8; i++) {
              residualBandsRef.current[i] =
                raw[i] ?? residualBandsRef.current[i]
            }
          } else if (live) {
            // 暂停：沿用残留频谱，不衰减、不流动
            raw = residualBandsRef.current
          } else {
            // 换歌退场：残留频谱缓衰减，保持环形态再收
            const decay = Math.exp(-2.8 * dt)
            for (let i = 0; i < 8; i++) {
              residualBandsRef.current[i] *= decay
            }
            raw = residualBandsRef.current
          }
          const prev = prevBandsRef.current
          const smooth = smoothBandsRef.current

          let energy = 0
          let flux = 0
          // 播放轻平滑；暂停冻结用粘滞；退场更黏
          const bandLag = isPlaying ? 0.55 : live ? 0.92 : 0.82
          for (let i = 0; i < 8; i++) {
            const v = raw[i] ?? 0
            smooth[i] = smooth[i] * bandLag + v * (1 - bandLag)
            energy += smooth[i]
            flux += Math.max(0, v - (prev[i] ?? 0))
            prev[i] = v
          }
          energy /= 8
          flux = Math.min(1.2, flux * 0.65)
          // 出场 kick 补一点假能量，频谱还没上来时也有光
          if (isPlaying && kick > 0.02) {
            energy = Math.min(1.15, energy + kick * 0.42)
            flux = Math.min(1.2, flux + kick * 0.25)
          }

          const bass = (smooth[0] + smooth[1]) * 0.5
          const midF = (smooth[2] + smooth[3] + smooth[4]) / 3
          const treble = (smooth[5] + smooth[6] + smooth[7]) / 3

          const hist = energyHistRef.current
          hist.push(energy)
          if (hist.length > 14) hist.shift()
          const avgE =
            hist.reduce((a, b) => a + b, 0) / Math.max(1, hist.length)
          const onsetRaw = Math.max(0, (energy - avgE * 1.05) * 2.8)
          const onset = Math.min(1, onsetRaw + (isPlaying ? kick * 0.55 : 0))

          // 仅播放时推进相位 / 峰漂移；暂停完全冻结形态
          const motion = isPlaying ? presence : 0
          if (isPlaying) {
            timeAccRef.current += dt
          }

          // 相位速度：跟能量/高频/flux 强绑定
          const speedTarget =
            0.035 +
            bass * 0.04 +
            energy * 0.09 +
            treble * 0.12 +
            flux * 0.08 +
            onset * 0.06 +
            noiseSpeedRef.current * 0.4 +
            kick * 0.05
          speedSmoothRef.current = lerp(
            speedSmoothRef.current,
            isPlaying ? speedTarget : 0,
            isPlaying ? 0.18 : 0.35,
          )
          wavePhaseRef.current += speedSmoothRef.current * motion
          noisePhaseRef.current +=
            (noiseSpeedRef.current + treble * 0.05 + flux * 0.04) * motion

          // 随机偏置：仅播放时游走
          if (isPlaying) {
            hBias1Ref.current = ouStep(hBias1Ref.current, 0, 0.006, 0.16, dt)
            hBias2Ref.current = ouStep(hBias2Ref.current, 0, 0.0055, 0.18, dt)
            wBias1Ref.current = ouStep(wBias1Ref.current, 0, 0.008, 0.12, dt)
            wBias2Ref.current = ouStep(wBias2Ref.current, 0, 0.0075, 0.13, dt)
            hBias1Ref.current = Math.max(-0.75, Math.min(0.8, hBias1Ref.current))
            hBias2Ref.current = Math.max(-0.8, Math.min(0.85, hBias2Ref.current))
            wBias1Ref.current = Math.max(-0.55, Math.min(0.65, wBias1Ref.current))
            wBias2Ref.current = Math.max(-0.6, Math.min(0.7, wBias2Ref.current))

            // 峰速：频谱推 + 随机游走（可见漂移）
            v1Ref.current = ouStep(
              v1Ref.current,
              (smooth[1] - smooth[4]) * 0.004,
              0.03,
              0.0028,
              dt,
            )
            v2Ref.current = ouStep(
              v2Ref.current,
              (smooth[6] - smooth[2]) * 0.005,
              0.028,
              0.0032,
              dt,
            )
          }

          const s1 =
            (0.003 +
              bass * 0.006 +
              midF * 0.004 +
              flux * 0.005 +
              onset * 0.004) *
            motion
          const s2 =
            (0.0025 +
              treble * 0.01 +
              midF * 0.003 +
              flux * 0.006 +
              onset * 0.005) *
            motion

          // 频谱重心吸引：峰1偏低频能量位置，峰2偏高频
          if (isPlaying) {
            const bassFocus =
              (0 * smooth[0] +
                0.12 * smooth[1] +
                0.25 * smooth[2] +
                0.4 * smooth[3]) /
              Math.max(0.08, smooth[0] + smooth[1] + smooth[2] + smooth[3])
            const trebFocus =
              (0.55 * smooth[4] +
                0.7 * smooth[5] +
                0.85 * smooth[6] +
                1.0 * smooth[7]) /
              Math.max(0.08, smooth[4] + smooth[5] + smooth[6] + smooth[7])
            // 映射到周长，并加相位，避免钉死在固定边
            const spin = (wavePhaseRef.current * 0.02) % 1
            anchor1Ref.current =
              (bassFocus * 0.35 + spin + hBias1Ref.current * 0.08 + 1) % 1
            anchor2Ref.current =
              (trebFocus * 0.35 +
                0.5 +
                spin * 1.3 +
                hBias2Ref.current * 0.08 +
                1) %
              1

            peak1TRef.current = circLerp(
              peak1TRef.current,
              anchor1Ref.current,
              0.04 * motion,
            )
            peak2TRef.current = circLerp(
              peak2TRef.current,
              anchor2Ref.current,
              0.035 * motion,
            )
            peak1TRef.current =
              (peak1TRef.current + s1 + v1Ref.current * motion + 1) % 1
            peak2TRef.current =
              (peak2TRef.current + s2 + v2Ref.current * motion + 1) % 1

            const gap = circDist(peak1TRef.current, peak2TRef.current)
            if (gap < 0.18) {
              const push = (0.18 - gap) * 0.12
              peak2TRef.current = (peak2TRef.current + push + 1) % 1
            }

            // 强 onset / 定时：猛推随机态（仍平滑到目标）
            if (
              presence > 0.45 &&
              (timeAccRef.current >= nextReseedAtRef.current || onset > 0.72)
            ) {
              anchor1Ref.current = Math.random()
              anchor2Ref.current =
                (anchor1Ref.current + 0.22 + Math.random() * 0.48) % 1
              hBias1Ref.current += (Math.random() - 0.5) * (0.35 + onset * 0.4)
              hBias2Ref.current += (Math.random() - 0.5) * (0.4 + onset * 0.45)
              wBias1Ref.current += (Math.random() - 0.5) * 0.3
              wBias2Ref.current += (Math.random() - 0.5) * 0.35
              v1Ref.current += (Math.random() - 0.5) * 0.01
              v2Ref.current += (Math.random() - 0.5) * 0.012
              noiseSpeedRef.current = 0.02 + Math.random() * 0.06
              nextReseedAtRef.current =
                timeAccRef.current + 1.1 + Math.random() * 2.4 + (1 - onset) * 1.2
            }

            // 峰高：可很低可很高 — 频谱主导 + 大随机偏置
            const h1Target = Math.max(
              0.08,
              0.2 +
                bass * 0.9 +
                onset * 0.85 +
                smooth[0] * 0.7 +
                smooth[1] * 0.45 +
                hBias1Ref.current +
                kick * 0.35,
            )
            const h2Target = Math.max(
              0.08,
              0.15 +
                treble * 1.25 +
                onset * 0.95 +
                smooth[6] * 0.85 +
                smooth[7] * 0.7 +
                hBias2Ref.current +
                kick * 0.4,
            )
            // 较快追上频谱
            peak1HRef.current = lerp(peak1HRef.current, h1Target, 0.14)
            peak2HRef.current = lerp(peak2HRef.current, h2Target, 0.15)
            const w1Target = Math.max(
              0.3,
              0.4 + bass * 0.55 - treble * 0.25 + midF * 0.15 + wBias1Ref.current,
            )
            const w2Target = Math.max(
              0.28,
              0.35 + midF * 0.3 + treble * 0.45 - bass * 0.12 + wBias2Ref.current,
            )
            peak1WRef.current = lerp(peak1WRef.current, w1Target, 0.1)
            peak2WRef.current = lerp(peak2WRef.current, w2Target, 0.11)
          }

          const d = buildSpectrumWavePath(
            w,
            h,
            rx,
            ry,
            smooth,
            {
              energy: energy * presence,
              onset: onset * presence,
              treble: treble * presence,
              bass: bass * presence,
              mid: midF * presence,
              flux: flux * presence,
              wavePhase: wavePhaseRef.current,
              noisePhase: noisePhaseRef.current,
              peak1T: peak1TRef.current,
              peak2T: peak2TRef.current,
              peak1H: peak1HRef.current,
              peak2H: peak2HRef.current,
              peak1W: Math.max(0.3, Math.min(1.4, peak1WRef.current)),
              peak2W: Math.max(0.28, Math.min(1.35, peak2WRef.current)),
              presence,
            },
            PAD,
            PAD,
            PAD - 2,
          )

          softRef.current?.setAttribute('d', d)
          midRef.current?.setAttribute('d', d)

          // 线宽/透明度：出场 kick 更亮更厚；退场跟 opacityPresence 先灭
          // 暂停：保持当前 body/opacity，不向 0 收敛
          const bodyTarget = isPlaying
            ? (energy * 0.5 +
                bass * 0.25 +
                treble * 0.35 +
                onset * 0.4 +
                flux * 0.2 +
                kick * 0.55) *
              presence
            : live
              ? bodySmoothRef.current
              : (energy * 0.5 + bass * 0.25 + treble * 0.35) * presence
          bodySmoothRef.current = lerp(
            bodySmoothRef.current,
            bodyTarget,
            isPlaying ? 0.22 : live ? 0 : 0.14,
          )
          const body = bodySmoothRef.current
          const opTarget = isPlaying
            ? opacityPresence *
              (0.28 + energy * 0.35 + body * 0.4 + onset * 0.15 + kick * 0.38)
            : live
              ? opacitySmoothRef.current
              : opacityPresence *
                (0.28 + energy * 0.35 + body * 0.4)
          opacitySmoothRef.current = lerp(
            opacitySmoothRef.current,
            opTarget,
            isPlaying ? 0.2 : live ? 0 : 0.28,
          )
          const op = opacitySmoothRef.current
          if (softRef.current) {
            softRef.current.style.opacity = String(
              Math.min(1, Math.max(0, op * 0.95)),
            )
            softRef.current.style.strokeWidth = String(
              22 +
                energy * 18 +
                bass * 12 +
                body * 20 +
                presence * 8 +
                kick * 14,
            )
          }
          if (midRef.current) {
            midRef.current.style.opacity = String(
              Math.min(1, Math.max(0, op * 0.9)),
            )
            midRef.current.style.strokeWidth = String(
              14 +
                energy * 12 +
                treble * 10 +
                body * 14 +
                onset * 6 +
                kick * 10,
            )
          }

          if (reducedMotion && live && isPlaying && presence > 0.98) {
            settled = true
          }
          if (live && !isPlaying && presence > 0.98) {
            settled = true
          }
          } // end non-frozen draw
        }
      }
      // 冻结 / reduced 静环已静定则停环；否则续帧
      if (
        !(
          settled &&
          activeRef.current &&
          (!playingRef.current || reducedMotion)
        )
      ) {
        raf = requestAnimationFrame(tick)
      }
    }
    raf = requestAnimationFrame(tick)

    return () => {
      cancelAnimationFrame(raf)
      ro?.disconnect()
    }
  }, [mounted, active, playing])

  if (!mounted) return null

  return (
    <div
      ref={wrapRef}
      className="library-playing-wave library-card-chrome"
      style={{ '--music-color': musicColor } as CSSProperties}
      aria-hidden
    >
      <svg ref={svgRef} preserveAspectRatio="none">
        <path
          ref={softRef}
          className="library-playing-wave__band library-playing-wave__band--soft"
        />
        <path
          ref={midRef}
          className="library-playing-wave__band library-playing-wave__band--mid"
        />
      </svg>
    </div>
  )
})

/**
 * 父级轻量订阅：仅 songId / isPlaying / musicColor
 * 切句不触发 LibraryGrid 重渲染
 */
interface LibraryMusicIdentity {
  songId: string | null
  isPlaying: boolean
  musicColor: string
}

function readLibraryMusicIdentity(): LibraryMusicIdentity {
  if (typeof window === 'undefined') {
    return { songId: null, isPlaying: false, musicColor: '#ef4444' }
  }
  const g = (window as any).__musicPlayerState
  return {
    songId: g?.currentSong?.id != null ? String(g.currentSong.id) : null,
    isPlaying: Boolean(g?.isPlaying),
    musicColor: String(g?.musicColor || '#ef4444'),
  }
}

function useLibraryMusicIdentity(): LibraryMusicIdentity {
  const [snap, setSnap] = useState(readLibraryMusicIdentity)
  useEffect(() => {
    // 事件只作通知：一律读 __musicPlayerState（宿主完整合并后的真相）。
    // 禁止从 detail 重建——embed 等路径会发 partial（仅 currentSong），
    // 缺字段会被当成 null/false/默认红，卡片「正在播」状态会假掉。
    const applyFromGlobal = () => {
      const next = readLibraryMusicIdentity()
      setSnap((prev) =>
        prev.songId === next.songId &&
        prev.isPlaying === next.isPlaying &&
        prev.musicColor === next.musicColor
          ? prev
          : next,
      )
    }
    window.addEventListener('music-player-state-change', applyFromGlobal)
    applyFromGlobal()
    return () =>
      window.removeEventListener('music-player-state-change', applyFromGlobal)
  }, [])
  return snap
}

/**
 * 资料库卡片歌词外壳：进出场 + 封面色遮罩；
 * 切换引擎见共享 LyricWaveScroll（与控制面板同一套）
 *
 * 退场时冻结歌词快照：换歌会 resetLyrics，不能靠 live hasLyrics 决定是否卸载，
 * 否则 is-leaving 会被短路硬切。
 */
const LibraryCardLyrics = memo(({
  active,
  musicColor,
}: {
  /** true=当前曲（含暂停）；false=换歌离场 */
  active: boolean
  musicColor: string
}) => {
  const { lyrics: liveLyrics, currentLyricIndex: liveIndex } =
    useMusicLyricsSlice()
  const hasLiveLyrics = useMemo(
    () => liveLyrics.some((l) => (l.text || '').trim()),
    [liveLyrics],
  )

  const [mounted, setMounted] = useState(false)
  const [visible, setVisible] = useState(false)
  /** 展示用（active 时跟随 live；leave 期间冻结） */
  const [displayLyrics, setDisplayLyrics] = useState(liveLyrics)
  const [displayIndex, setDisplayIndex] = useState(liveIndex)
  const leaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  /** 是否曾成功进场；从未进场则不走 leave 计时（避免首帧无词误卸） */
  const everShownRef = useRef(false)

  // active 且有词：同步展示；leave 时不写，保留上一曲快照
  useEffect(() => {
    if (active && hasLiveLyrics) {
      setDisplayLyrics(liveLyrics)
      setDisplayIndex(liveIndex)
    }
  }, [active, hasLiveLyrics, liveLyrics, liveIndex])

  useEffect(() => {
    if (leaveTimerRef.current) {
      clearTimeout(leaveTimerRef.current)
      leaveTimerRef.current = null
    }
    if (active && hasLiveLyrics) {
      everShownRef.current = true
      setMounted(true)
      const raf = requestAnimationFrame(() => setVisible(true))
      return () => cancelAnimationFrame(raf)
    }
    // 仍是当前曲但歌词暂空（切歌 reset / 二次加载中）：只等词，绝不 leave
    // 否则 everShown 时会误走退场，把已挂载歌词卸掉，且二次加载失败时永久空白
    if (active) {
      if (!everShownRef.current) {
        setMounted(false)
        setVisible(false)
      }
      return
    }
    // 从未进场：保持未挂载
    if (!everShownRef.current) {
      setMounted(false)
      setVisible(false)
      return
    }
    // 非当前曲退场：冻结 display 快照；时长对齐 CSS is-leaving
    setVisible(false)
    leaveTimerRef.current = setTimeout(() => {
      setMounted(false)
      everShownRef.current = false
      leaveTimerRef.current = null
    }, LIBRARY_LIVE_MS.lyricsUnmount)
    return () => {
      if (leaveTimerRef.current) {
        clearTimeout(leaveTimerRef.current)
        leaveTimerRef.current = null
      }
    }
  }, [active, hasLiveLyrics])

  const hasDisplayLyrics = displayLyrics.some((l) => (l.text || '').trim())
  if (!mounted || !hasDisplayLyrics) return null

  return (
    <div
      className={`library-card-lyrics${visible ? ' is-on' : ' is-leaving'}`}
      style={{ '--music-color': musicColor } as CSSProperties}
      aria-hidden
    >
      <div className="library-card-lyrics__mask" />
      <LyricWaveScroll
        variant="card"
        lyrics={displayLyrics}
        currentLyricIndex={displayIndex}
        musicColor={musicColor}
      />
    </div>
  )
})

/**
 * 卡片外壳：封面占位 + 加载完再显示玻璃 chrome，避免滚动时标题/平台标先闪。
 */
const LibraryCardShell = memo(({
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
  /** 额外图片类（如 hover scale） */
  imgClassName?: string
  placeholder: ReactNode
  children: ReactNode
  /** 播放中封面呼吸动效 */
  coverBreathing?: boolean
  /** Canvas visible cards: eager fetch (decode slot still caps concurrency). */
  priority?: boolean
  /** First-reveal shell enter delay (seconds); null = no enter animation. */
  canvasEnterDelay?: number | null
}) => {
  const hasCover = Boolean(cover)
  const [mediaReady, setMediaReady] = useState(!hasCover)
  /** Gate real src through a decode budget so pan-in doesn't decode 30 covers at once. */
  const [activeSrc, setActiveSrc] = useState<string | null>(null)
  /** off | breathing | exiting — 退场播完再卸类，避免硬切 */
  const [breathPhase, setBreathPhase] = useState<
    'off' | 'breathing' | 'exiting'
  >(coverBreathing ? 'breathing' : 'off')
  // Freeze mount-time enter delay — parent re-renders pass null after the id is claimed.
  const [enterDelay] = useState(canvasEnterDelay)
  const [canvasEntering, setCanvasEntering] = useState(() => enterDelay != null)
  const imgRef = useRef<HTMLImageElement>(null)
  const releaseSlotRef = useRef<(() => void) | null>(null)

  // reduced-motion / missed animationend: drop enter flag so attributes don't linger
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
      // 重新进场：清掉上次退出时写死的 inline transform
      clearCoverInline()
      setBreathPhase('breathing')
      return
    }
    setBreathPhase((prev) => {
      if (prev !== 'breathing') return prev === 'exiting' ? 'exiting' : 'off'
      return 'exiting'
    })
  }, [coverBreathing, clearCoverInline])

  // 退场：冻结当前呼吸 matrix → 下一帧 transition 到 scale(1)（避免 keyframes 硬切）
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
    img.style.transform = matrix && matrix !== 'none' ? matrix : 'scale(1.018)'
    // 强制提交 frozen 帧
    void img.offsetWidth

    let raf2 = 0
    const raf1 = requestAnimationFrame(() => {
      raf2 = requestAnimationFrame(() => {
        img.style.transition = 'transform 0.58s cubic-bezier(0.22, 1, 0.36, 1)'
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

  // Decode budget + unload on unmount (virtualized cards leave the viewport).
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
      // Drop decoded bitmap when the card is culled from the canvas viewport.
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
})

interface LibraryResponse {
  success: boolean
  items: LibraryItem[]
  total: number
  has_more?: boolean
  next_offset?: number | null
  preferences?: {
    layout?: 'list' | 'canvas'
  }
}

type CardLayout = LibraryCanvasLayout

interface LibraryGridProps {
  filter: 'all' | 'game' | 'video' | 'music' | 'anime' | 'tv_series' | 'book'
}

// 判断是否为 Bangumi 平台
function isBangumiPlatform(platform: string) {
  return platform.toLowerCase() === 'bangumi'
}

// 判断是否为 MyAnimeList 平台
function isMalPlatform(platform: string) {
  const key = platform.toLowerCase().replace(/[\s_-]/g, '')
  return key === 'myanimelist' || key === 'mal'
}

// 是否展示用户评分徽章（Bangumi / MyAnimeList 等 1–10 分制）
function hasUserRatingBadge(platform: string) {
  return isBangumiPlatform(platform) || isMalPlatform(platform)
}

// Bangumi 用户评分徽章样式（仿 Metacritic 分色标记）
// 分数越高越推荐 —— 色彩越暖、尺寸越大、越醒目
function getRatingBadgeStyle(rate: number) {
  // 满分（10）：金色渐变，最大最亮，双环 + 光晕，独享的稀有感
  if (rate >= 10) {
    return {
      box: 'w-10 h-10 text-xl bg-linear-to-br from-amber-300 via-yellow-400 to-orange-500 text-white ring-2 ring-amber-200/80 ring-offset-1 ring-offset-amber-500/30 shadow-amber-400/60',
      gloss: true,
    }
  }
  // 神作（9）：金色渐变 + 光晕
  if (rate >= 9) {
    return {
      box: 'w-9 h-9 text-lg bg-linear-to-br from-amber-300 to-orange-500 text-white ring-2 ring-amber-200/70 shadow-amber-500/50',
      gloss: true,
    }
  }
  // 力荐（8）
  if (rate >= 8) {
    return {
      box: 'w-8 h-8 text-base bg-emerald-500 text-white ring-1 ring-emerald-300/50 shadow-emerald-500/40',
      gloss: false,
    }
  }
  // 推荐（7）
  if (rate >= 7) {
    return {
      box: 'w-8 h-8 text-base bg-green-500 text-white shadow-green-500/30',
      gloss: false,
    }
  }
  // 还行（6）
  if (rate >= 6) {
    return {
      box: 'w-7 h-7 text-sm bg-lime-500 text-white',
      gloss: false,
    }
  }
  // 不过不失（5）
  if (rate >= 5) {
    return {
      box: 'w-7 h-7 text-sm bg-amber-500 text-white',
      gloss: false,
    }
  }
  // 较差（3-4）
  if (rate >= 3) {
    return {
      box: 'w-7 h-7 text-sm bg-orange-500 text-white',
      gloss: false,
    }
  }
  // 差评（1-2）
  return {
    box: 'w-7 h-7 text-sm bg-rose-500 text-white',
    gloss: false,
  }
}

// 获取项目在网格中的尺寸 (w, h)
function getItemGridSize(type: string, platform: string) {
  switch (type) {
    case 'game':
      // Bangumi 游戏使用竖版封面，其余（如 Steam）保持横版
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

const CANVAS_STRIDE = LIBRARY_CANVAS_STRIDE
/** Desktop default zoom; mobile uses a tighter fit so cards aren't huge on first open. */
const CANVAS_DEFAULT_SCALE_DESKTOP = 0.75
const CANVAS_DEFAULT_SCALE_MOBILE = 0.5
const CANVAS_MIN_SCALE = 0.45
const CANVAS_MAX_SCALE = 1.6
const CANVAS_SPATIAL_BIN_SIZE = CANVAS_STRIDE * 4
const LIBRARY_PAGE_SIZE = 120

/** Visible-card query used by both React virtualization and live paint. */
function queryCanvasVisibleItems(
  transform: LibraryCanvasTransform,
  viewport: { width: number; height: number },
  layouts: Map<string, CardLayout>,
  spatialIndex: {
    bins: Map<string, LibraryItem[]>
    order: Map<string, number>
  },
  laidOutItems: LibraryItem[],
): LibraryItem[] {
  if (layouts.size === 0) return []
  if (viewport.width === 0 || viewport.height === 0) {
    return laidOutItems.slice(0, 30)
  }
  const { minX, maxX, minY, maxY } = getLibraryCanvasViewportBounds(
    transform,
    viewport,
  )
  const minBinX = Math.floor(minX / CANVAS_SPATIAL_BIN_SIZE)
  const maxBinX = Math.floor(maxX / CANVAS_SPATIAL_BIN_SIZE)
  const minBinY = Math.floor(minY / CANVAS_SPATIAL_BIN_SIZE)
  const maxBinY = Math.floor(maxY / CANVAS_SPATIAL_BIN_SIZE)
  const candidates: LibraryItem[] = []
  const seen = new Set<string>()

  for (let binX = minBinX; binX <= maxBinX; binX++) {
    for (let binY = minBinY; binY <= maxBinY; binY++) {
      const bin = spatialIndex.bins.get(`${binX},${binY}`)
      if (!bin) continue
      bin.forEach((item) => {
        if (seen.has(item.id)) return
        seen.add(item.id)
        const layout = layouts.get(item.id)
        if (
          layout &&
          libraryCanvasLayoutIntersects(layout, { minX, maxX, minY, maxY })
        ) {
          candidates.push(item)
        }
      })
    }
  }

  return candidates.sort(
    (a, b) =>
      (spatialIndex.order.get(a.id) ?? 0) - (spatialIndex.order.get(b.id) ?? 0),
  )
}

function readCanvasDefaultScale(): number {
  // Align with nav island: touch tablets in 768–1023 use compact chrome too.
  if (typeof window === 'undefined') return CANVAS_DEFAULT_SCALE_DESKTOP
  try {
    return isMobileNavLayout()
      ? CANVAS_DEFAULT_SCALE_MOBILE
      : CANVAS_DEFAULT_SCALE_DESKTOP
  } catch {
    return CANVAS_DEFAULT_SCALE_DESKTOP
  }
}

function balancedShuffleLibraryItems(items: LibraryItem[]): LibraryItem[] {
  const groups: Record<string, LibraryItem[]> = {
    game: [],
    video: [],
    music: [],
    anime: [],
    tv_series: [],
    book: [],
  }
  items.forEach((item) => groups[item.item_type]?.push(item))
  Object.values(groups).forEach((group) =>
    group.sort(() => Math.random() - 0.5),
  )

  const result: LibraryItem[] = []
  const maxLength = Math.max(
    0,
    ...Object.values(groups).map((group) => group.length),
  )
  for (let index = 0; index < maxLength; index++) {
    Object.keys(groups)
      .sort(() => Math.random() - 0.5)
      .forEach((type) => {
        const item = groups[type][index]
        if (item) result.push(item)
      })
  }
  return result
}

export default function LibraryGrid({ filter }: LibraryGridProps) {
  const [allItems, setAllItems] = useState<LibraryItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [prevFilter, setPrevFilter] = useState<
    'all' | 'game' | 'video' | 'music' | 'anime' | 'tv_series' | 'book'
  >('all')
  const [isTransitioning, setIsTransitioning] = useState(false)
  const transitionTimersRef = useRef<ReturnType<typeof setTimeout>[]>([])

  // 布局状态：preferred 来自用户设置；低端设备强制列表（见 resolveLibraryLayoutMode）
  const [layouts, setLayouts] = useState<Map<string, CardLayout>>(new Map())
  const [preferredLayout, setPreferredLayout] =
    useState<LibraryLayoutMode>('list')
  const { highHardware } = usePerformanceProfile()
  const layoutMode = resolveLibraryLayoutMode(preferredLayout, highHardware)
  const [visibleCount, setVisibleCount] = useState(20) // 初始显示数量
  const [libraryHasMore, setLibraryHasMore] = useState(false)
  const nextLibraryOffsetRef = useRef<number | null>(null)
  const libraryPageLoadingRef = useRef(false)
  const libraryFetchGenerationRef = useRef(0)
  const loadNextLibraryPageRef = useRef<() => Promise<void>>(async () => {})
  const containerRef = useRef<HTMLDivElement>(null)
  // Mount-time only: avoid flipping scale when rotating/resizing mid-session.
  const canvasDefaultScaleRef = useRef(readCanvasDefaultScale())
  const worldRef = useRef<HTMLDivElement | null>(null)
  const canvasViewportRef = useRef({ width: 0, height: 0 })
  const libraryHasMoreRef = useRef(false)
  const canvasLoadedRadiusRef = useRef(0)
  const loadNextLibraryPageLiveRef = useRef<() => void>(() => {})
  const layoutsRef = useRef(new Map<string, CardLayout>())
  const laidOutItemsRef = useRef<LibraryItem[]>([])
  const canvasSpatialIndexRef = useRef<{
    bins: Map<string, LibraryItem[]>
    order: Map<string, number>
  }>({ bins: new Map(), order: new Map() })
  const lastCanvasVisibleSigRef = useRef('')
  /** Tracks prior canvas effective mode for one-shot paint cleanup on leave. */
  const wasCanvasLayoutRef = useRef(false)
  const [canvasLiveVisibleItems, setCanvasLiveVisibleItems] = useState<
    LibraryItem[]
  >([])

  const paintCanvasTransform = useCallback(
    (t: LibraryCanvasTransform) => {
      const surface = containerRef.current
      const world = worldRef.current
      if (world) {
        world.style.transform = `translate3d(${t.x}px, ${t.y}px, 0) scale(${t.scale})`
      }
      if (surface) {
        surface.style.backgroundSize = `${28 * t.scale}px ${28 * t.scale}px`
        surface.style.backgroundPosition = `calc(50% + ${t.x}px) calc(50% + ${t.y}px)`
      }

      const viewport = canvasViewportRef.current
      if (world && viewport.width > 0 && viewport.height > 0) {
        const cards = world.querySelectorAll<HTMLElement>('[data-canvas-card]')
        for (let i = 0; i < cards.length; i++) {
          const el = cards[i]
          const left = Number(el.dataset.layoutLeft)
          const top = Number(el.dataset.layoutTop)
          const width = Number(el.dataset.layoutWidth)
          const height = Number(el.dataset.layoutHeight)
          if (
            !Number.isFinite(left) ||
            !Number.isFinite(top) ||
            !Number.isFinite(width) ||
            !Number.isFinite(height)
          ) {
            continue
          }
          const focus = getLibraryCanvasFocusScale(
            {
              left,
              top,
              width,
              height,
              gridX: 0,
              gridY: 0,
              gridW: 1,
              gridH: 1,
            },
            t,
            viewport,
          )
          el.style.transform = `scale(${focus})`
          el.style.zIndex = String(Math.round(focus * 100))
        }
      }

      // Live virtualization: mount/unmount only when membership changes.
      const nextVisible = queryCanvasVisibleItems(
        t,
        viewport,
        layoutsRef.current,
        canvasSpatialIndexRef.current,
        laidOutItemsRef.current,
      )
      let sig = ''
      for (let i = 0; i < nextVisible.length; i++) {
        if (i) sig += '\n'
        sig += nextVisible[i].id
      }
      if (sig !== lastCanvasVisibleSigRef.current) {
        lastCanvasVisibleSigRef.current = sig
        setCanvasLiveVisibleItems(nextVisible)
      }

      // Chrome lives in a portal — update labels/disabled without React.
      const pct = Math.round(t.scale * 100)
      const zoomLabel = document.querySelector(
        '[data-library-canvas-zoom-percent]',
      )
      if (zoomLabel) zoomLabel.textContent = `${pct}%`
      const zoomOut = document.querySelector(
        '[data-library-canvas-zoom-out]',
      ) as HTMLButtonElement | null
      const zoomIn = document.querySelector(
        '[data-library-canvas-zoom-in]',
      ) as HTMLButtonElement | null
      const resetBtn = document.querySelector(
        '[data-library-canvas-reset]',
      ) as HTMLButtonElement | null
      if (zoomOut) zoomOut.disabled = t.scale <= CANVAS_MIN_SCALE + 0.001
      if (zoomIn) zoomIn.disabled = t.scale >= CANVAS_MAX_SCALE - 0.001
      if (resetBtn) {
        const isDefault =
          Math.abs(t.x) < 0.5 &&
          Math.abs(t.y) < 0.5 &&
          Math.abs(t.scale - canvasDefaultScaleRef.current) < 0.001
        resetBtn.disabled = isDefault
      }

      // Edge preload without waiting for a React transform commit.
      if (
        libraryHasMoreRef.current &&
        canvasLoadedRadiusRef.current > 0 &&
        viewport.width > 0 &&
        viewport.height > 0
      ) {
        const worldCenterX = -t.x / t.scale
        const worldCenterY = -t.y / t.scale
        const viewportRadius =
          Math.hypot(viewport.width, viewport.height) / 2 / t.scale
        const preloadBoundary = Math.max(
          0,
          canvasLoadedRadiusRef.current - viewportRadius * 1.25,
        )
        if (Math.hypot(worldCenterX, worldCenterY) >= preloadBoundary) {
          loadNextLibraryPageLiveRef.current()
        }
      }
    },
    [],
  )

  // React transform commits when spatial bins change (chrome props) or on force flush.
  const shouldCommitCanvasTransform = useCallback(
    (next: LibraryCanvasTransform, committed: LibraryCanvasTransform) => {
      const viewport = canvasViewportRef.current
      if (viewport.width <= 0 || viewport.height <= 0) return true
      return (
        getLibraryCanvasViewportBinKey(
          next,
          viewport,
          CANVAS_SPATIAL_BIN_SIZE,
        ) !==
        getLibraryCanvasViewportBinKey(
          committed,
          viewport,
          CANVAS_SPATIAL_BIN_SIZE,
        )
      )
    },
    [],
  )

  const {
    atMaxZoom: canvasAtMaxZoom,
    atMinZoom: canvasAtMinZoom,
    finishPointer: finishCanvasPointer,
    handleBlur: handleCanvasBlur,
    handleClickCapture: handleCanvasClickCapture,
    handleKeyDown: handleCanvasKeyDown,
    handleKeyUp: handleCanvasKeyUp,
    handlePointerDown: handleCanvasPointerDown,
    handlePointerMove: handleCanvasPointerMove,
    isDefault: canvasViewIsDefault,
    reset: resetCanvasView,
    transform: canvasTransform,
    transformRef: canvasTransformRef,
    zoom: zoomCanvas,
    zoomPercent: canvasZoomPercent,
  } = useLibraryCanvasControls({
    active: layoutMode === 'canvas',
    defaultScale: canvasDefaultScaleRef.current,
    maxScale: CANVAS_MAX_SCALE,
    minScale: CANVAS_MIN_SCALE,
    surfaceRef: containerRef,
    onPaint: paintCanvasTransform,
    shouldCommit: shouldCommitCanvasTransform,
  })
  const [canvasViewport, setCanvasViewport] = useState({
    width: 0,
    height: 0,
  })
  canvasViewportRef.current = canvasViewport

  // Layout phase: mark canvas + cache wallpaper from-frame before paint so
  // entering /library from another route eases parallax out instead of hard-cutting.
  useLayoutEffect(() => {
    const root = document.documentElement
    const active = layoutMode === 'canvas'
    if (active) {
      root.dataset.libraryCanvas = 'active'
      softLockWallpaperForLibraryCanvas()
    } else {
      delete root.dataset.libraryCanvas
    }
    window.dispatchEvent(new Event('libraryCanvasModeChanged'))

    return () => {
      if (active && root.dataset.libraryCanvas === 'active') {
        delete root.dataset.libraryCanvas
        window.dispatchEvent(new Event('libraryCanvasModeChanged'))
      }
    }
  }, [layoutMode])

  const containerWidthRef = useRef<number>(0) // 缓存容器宽度，避免重复读取
  // 父级只跟 songId / isPlaying / musicColor，切句不重渲染整表
  const {
    songId: liveSongId,
    isPlaying: globalIsPlaying,
    musicColor,
  } = useLibraryMusicIdentity()
  const { t } = useI18n()

  // 不经 Context 的 playSong，避免订阅歌词态
  const playSongRef = useRef((song: Song) => {
    window.dispatchEvent(new CustomEvent('play-song', { detail: { song } }))
  })

  // 换歌/播完切走：旧卡保留离场窗口（见 LIBRARY_LIVE_MS.leaveHold）
  const prevLiveSongIdRef = useRef<string | null>(liveSongId)
  const [leavingSongId, setLeavingSongId] = useState<string | null>(null)
  useEffect(() => {
    const prev = prevLiveSongIdRef.current
    prevLiveSongIdRef.current = liveSongId
    if (prev && prev !== liveSongId) {
      // 注意：此处 return 后不会执行下面的 liveSongId===null 清理，
      // 否则会立刻清掉 leavingSongId，退场动画被掐断。
      setLeavingSongId(prev)
      const t = window.setTimeout(setLeavingSongId, LIBRARY_LIVE_MS.leaveHold, null)
      return () => window.clearTimeout(t)
    }
    // 仅「本来就没有曲 / 清空」时卸离场标记（非换歌路径）
    if (!liveSongId) setLeavingSongId(null)
  }, [liveSongId])

  // 筛选后的所有项目
  const filteredAllItems = useMemo(() => {
    return filter === 'all'
      ? allItems
      : allItems.filter((item) => item.item_type === filter)
  }, [filter, allItems])

  // 核心布局算法：完全避免空隙
  const computeLayout = useCallback(() => {
    if (!containerRef.current || filteredAllItems.length === 0) return

    if (layoutMode === 'canvas') {
      const width = containerRef.current.offsetWidth
      const height = containerRef.current.offsetHeight
      setCanvasViewport((current) =>
        current.width === width && current.height === height
          ? current
          : { width, height },
      )
      setLayouts(
        buildCenterOutCanvasLayout(filteredAllItems, (item) =>
          getItemGridSize(item.item_type, item.platform),
        ),
      )
      return
    }

    // 使用缓存的容器宽度，避免强制重排
    // 只有缓存无效时才读取
    if (containerWidthRef.current === 0) {
      containerWidthRef.current = containerRef.current.offsetWidth
    }
    const containerWidth = containerWidthRef.current
    const gap = 16
    let columns = 5

    // 响应式列数
    if (containerWidth < 640) columns = 2
    else if (containerWidth < 768) columns = 3
    else if (containerWidth < 1024) columns = 4
    else if (containerWidth < 1536) columns = 5
    else columns = 6

    const baseWidth = (containerWidth - gap * (columns + 1)) / columns
    const uniformHeight = baseWidth

    // 居中逻辑修正：针对纯宽卡片（2x1）在奇数列数下的居中处理
    let startOffset = 0
    let layoutColumns = columns

    // 如果当前筛选下只有宽2的卡片（如纯 Steam 游戏或视频分类），且列数是奇数
    // 那么最后一列无法被填满（因为没有宽1的卡片），导致整体偏左
    // 需要计算偏移量使内容居中
    // 注意：Bangumi 游戏为竖版（宽1），与 Steam 游戏混排时不应触发此居中
    const allWideCards =
      filteredAllItems.length > 0 &&
      filteredAllItems.every(
        (item) => getItemGridSize(item.item_type, item.platform).w === 2,
      )
    if (allWideCards && columns % 2 !== 0 && columns > 1) {
      layoutColumns = columns - 1
      // 剩余空间 = 1个列宽 + 1个间隙
      // 偏移量 = 剩余空间 / 2
      startOffset = (baseWidth + gap) / 2
    }

    // 1. 准备队列：按尺寸分类，保持原始相对顺序
    const queues: Record<
      string,
      { item: LibraryItem; originalIndex: number }[]
    > = {
      '1x1': [],
      '1x2': [],
      '2x1': [],
      // '2x2': [] // 暂无2x2类型
    }

    filteredAllItems.forEach((item, index) => {
      const size = getItemGridSize(item.item_type, item.platform)
      const key = `${size.w}x${size.h}`
      if (queues[key]) {
        queues[key].push({ item, originalIndex: index })
      } else {
        // 默认归为 1x1
        queues['1x1'].push({ item, originalIndex: index })
      }
    })

    // 2. 网格状态追踪
    // 使用 Map 记录被占用的格子 "x,y" -> true
    const occupied = new Set<string>()
    const isOccupied = (x: number, y: number) => occupied.has(`${x},${y}`)
    const markOccupied = (x: number, y: number, w: number, h: number) => {
      for (let i = 0; i < w; i++) {
        for (let j = 0; j < h; j++) {
          occupied.add(`${x + i},${y + j}`)
        }
      }
    }

    const newLayouts = new Map<string, CardLayout>()
    let maxY = 0
    let placedCount = 0
    const totalItems = filteredAllItems.length

    // 3. 遍历网格填充
    // y 从 0 开始无限增长，x 从 0 到 columns-1
    let y = 0
    while (placedCount < totalItems) {
      for (let x = 0; x < layoutColumns; x++) {
        if (isOccupied(x, y)) continue

        // 发现空位 (x, y)
        // 尝试寻找最佳匹配项
        // 优先级：
        // 1. 检查是否能放入 2x1 (需要 x+1 空闲)
        // 2. 检查是否能放入 1x2 (需要 y+1 空闲 - 总是假设 y+1 空闲，除非有预占，但这里我们是逐行扫描，y+1通常未处理)
        // 注意：如果之前有 1x2 占据了 (x, y+1)，则 isOccupied(x, y+1) 会为 true。
        // 3. 放入 1x1

        // 为了保持"平均开始排布"，我们在所有能放入的候选中，选择 originalIndex 最小的那个

        const candidates: {
          type: string
          index: number
          item: LibraryItem
          w: number
          h: number
        }[] = []

        // 检查 1x1
        if (queues['1x1'].length > 0) {
          const qItem = queues['1x1'][0]
          candidates.push({
            item: qItem.item,
            index: qItem.originalIndex,
            type: '1x1',
            w: 1,
            h: 1,
          })
        }

        // 检查 2x1
        const canFit2x1 = x + 1 < layoutColumns && !isOccupied(x + 1, y)
        if (canFit2x1 && queues['2x1'].length > 0) {
          const qItem = queues['2x1'][0]
          candidates.push({
            item: qItem.item,
            index: qItem.originalIndex,
            type: '2x1',
            w: 2,
            h: 1,
          })
        }

        // 检查 1x2
        // 垂直方向通常是无限的，但要检查是否被上方的某些长条物体阻挡？
        // 我们是按 y 递增扫描，所以 (x, y+1) 只有可能被之前的操作占据（不太可能，除非有复杂形状）
        // 但为了严谨，检查一下
        const canFit1x2 = !isOccupied(x, y + 1)
        if (canFit1x2 && queues['1x2'].length > 0) {
          const qItem = queues['1x2'][0]
          candidates.push({
            item: qItem.item,
            index: qItem.originalIndex,
            type: '1x2',
            w: 1,
            h: 2,
          })
        }

        if (candidates.length === 0) {
          // 没有剩余物品能放入此格
          // 只能留空 (虽然用户说避免空白，但如果没有物品了就没办法)
          // 或者：如果只有 2x1 且当前只有 1格宽，那必须留空
          // 标记此格为"跳过/虚拟占用"以继续循环?
          // 不，直接 continue，外层循环会处理下一个 x
          // 但如果不标记，下次循环回来还是空的。
          // 所以必须标记为"废弃"
          // 但如果后续还有物品，只是当前放不下（比如只有2x1但这里只有1格），那这个格子就真的废了
          // 除非我们能从后面拉一个 1x1 过来。但如果 1x1 队列空了，那就真没办法。
          // 标记为占用，但不放置物品
          // occupied.add(`${x},${y}`); // 实际上不需要显式add，只要不处理就行，但为了算法推进，视为已处理
          continue
        }

        // 选择 originalIndex 最小的候选者
        candidates.sort((a, b) => a.index - b.index)
        const best = candidates[0]

        // 放置物品
        const queue = queues[best.type as keyof typeof queues]
        queue.shift() // 移除已使用的

        // 计算像素位置
        const left = gap + x * (baseWidth + gap) + startOffset
        const top = gap + y * (uniformHeight + gap)
        const width = best.w * baseWidth + (best.w - 1) * gap
        const height = best.h * uniformHeight + (best.h - 1) * gap

        newLayouts.set(best.item.id, {
          left,
          top,
          width,
          height,
          gridX: x,
          gridY: y,
          gridW: best.w,
          gridH: best.h,
        })

        markOccupied(x, y, best.w, best.h)
        placedCount++

        // 更新最大高度
        const itemBottom = top + height
        if (itemBottom > maxY) maxY = itemBottom
      }

      // 检查当前行是否还有未处理的空位（被跳过的）
      // 如果所有列都处理过（占用或尝试过），进入下一行
      y++

      // 安全阀：防止死循环 (如果数据异常)
      if (y > totalItems * 2) break
    }

    setLayouts(newLayouts)
  }, [filteredAllItems, filter, layoutMode])

  // 使用共享的 resize 监听器
  useSharedResize(
    () => {
      // resize 时刷新容器宽度缓存
      if (containerRef.current) {
        containerWidthRef.current = containerRef.current.offsetWidth
        if (layoutMode === 'canvas') {
          setCanvasViewport({
            width: containerRef.current.offsetWidth,
            height: containerRef.current.offsetHeight,
          })
        }
      }
      computeLayout()
    },
    { debounce: 150 },
  )

  // 初始计算布局
  useEffect(() => {
    computeLayout()
  }, [computeLayout])

  // 滚动加载更多 -  添加节流防止过快触发
  const loadMoreRef = useRef<number | null>(null)
  const loadMore = useCallback(() => {
    if (loadMoreRef.current) return // 防止重复触发
    loadMoreRef.current = requestAnimationFrame(() => {
      if (visibleCount < filteredAllItems.length) {
        setVisibleCount((prev) => Math.min(prev + 20, filteredAllItems.length))
      } else if (libraryHasMore) {
        void loadNextLibraryPageRef.current().then(() => {
          setVisibleCount((prev) => prev + 20)
        })
      }
      loadMoreRef.current = null
    })
  }, [filteredAllItems.length, libraryHasMore, visibleCount])

  // 清理 RAF
  useEffect(() => {
    return () => {
      if (loadMoreRef.current) {
        cancelAnimationFrame(loadMoreRef.current)
      }
    }
  }, [])

  const hasMore =
    layoutMode === 'list' &&
    (visibleCount < filteredAllItems.length || libraryHasMore)

  // 🆕 使用资料库原子化 IntersectionObserver
  const { observeLibraryIntersection, unobserveLibraryIntersection } =
    useLibraryIntersectionObserver()
  const observerTarget = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (layoutMode !== 'list') return
    const target = observerTarget.current
    if (!target) return

    observeLibraryIntersection(target, (entry) => {
      if (entry.isIntersecting && hasMore) {
        loadMore()
      }
    })

    return () => unobserveLibraryIntersection(target)
  }, [
    hasMore,
    layoutMode,
    loadMore,
    observeLibraryIntersection,
    unobserveLibraryIntersection,
  ])

  const laidOutItems = useMemo(
    () => filteredAllItems.filter((item) => layouts.has(item.id)),
    [filteredAllItems, layouts],
  )

  // 画布空间索引只在数据或布局变化时重建；拖拽时仅查询视口附近的分桶。
  const canvasSpatialIndex = useMemo(() => {
    const bins = new Map<string, LibraryItem[]>()
    const order = new Map<string, number>()
    if (layoutMode !== 'canvas') return { bins, order }

    laidOutItems.forEach((item, index) => {
      const layout = layouts.get(item.id)
      if (!layout) return
      order.set(item.id, index)
      const minBinX = Math.floor(layout.left / CANVAS_SPATIAL_BIN_SIZE)
      const maxBinX = Math.floor(
        (layout.left + layout.width) / CANVAS_SPATIAL_BIN_SIZE,
      )
      const minBinY = Math.floor(layout.top / CANVAS_SPATIAL_BIN_SIZE)
      const maxBinY = Math.floor(
        (layout.top + layout.height) / CANVAS_SPATIAL_BIN_SIZE,
      )

      for (let binX = minBinX; binX <= maxBinX; binX++) {
        for (let binY = minBinY; binY <= maxBinY; binY++) {
          const key = `${binX},${binY}`
          const bin = bins.get(key)
          if (bin) bin.push(item)
          else bins.set(key, [item])
        }
      }
    })

    return { bins, order }
  }, [laidOutItems, layoutMode, layouts])

  layoutsRef.current = layouts
  laidOutItemsRef.current = laidOutItems
  canvasSpatialIndexRef.current = canvasSpatialIndex

  // 排序后的可见项目
  const visibleItems = useMemo(() => {
    if (layouts.size === 0) return []

    if (layoutMode === 'canvas') {
      // Live set is owned by paintCanvasTransform (absolute-follow pose).
      // Fall back while viewport is measuring or before the first paint.
      if (canvasLiveVisibleItems.length > 0) return canvasLiveVisibleItems
      return queryCanvasVisibleItems(
        canvasTransform,
        canvasViewport,
        layouts,
        canvasSpatialIndex,
        laidOutItems,
      )
    }

    // 按布局位置排序 (top, then left) - 实际上布局算法已经大致按顺序了，但为了确保渲染顺序
    const sortedItems = [...laidOutItems].sort((a, b) => {
      const layoutA = layouts.get(a.id)!
      const layoutB = layouts.get(b.id)!
      if (Math.abs(layoutA.top - layoutB.top) > 10)
        return layoutA.top - layoutB.top
      return layoutA.left - layoutB.left
    })

    return sortedItems.slice(0, visibleCount)
  }, [
    canvasLiveVisibleItems,
    canvasSpatialIndex,
    canvasTransform,
    canvasViewport,
    laidOutItems,
    layoutMode,
    layouts,
    visibleCount,
  ])

  // 动态计算容器高度
  const containerHeight = useMemo(() => {
    if (layoutMode === 'canvas') return 0
    if (visibleItems.length === 0) return 400
    let maxBottom = 0
    visibleItems.forEach((item) => {
      const layout = layouts.get(item.id)
      if (layout) {
        const bottom = layout.top + layout.height
        if (bottom > maxBottom) maxBottom = bottom
      }
    })
    return maxBottom + 20
  }, [layoutMode, visibleItems, layouts])

  const fetchLibraryData = useCallback(async () => {
    const generation = ++libraryFetchGenerationRef.current
    try {
      setLoading(true)
      setError(null)
      setAllItems([])
      nextLibraryOffsetRef.current = null
      libraryPageLoadingRef.current = false
      setLibraryHasMore(false)
      // 首屏只取一批；无限画布在接近已加载边界时继续扩展。
      const data: LibraryResponse = await getLibraryDataPageDeduped(
        0,
        LIBRARY_PAGE_SIZE,
        filter,
      )
      if (generation !== libraryFetchGenerationRef.current) return

      if (data.success) {
        const balanced = balancedShuffleLibraryItems(
          slimLibraryItems(data.items as LibraryItem[]),
        )
        setPreferredLayout(
          data.preferences?.layout === 'canvas' ? 'canvas' : 'list',
        )
        setAllItems(balanced)
        nextLibraryOffsetRef.current = data.next_offset ?? null
        setLibraryHasMore(Boolean(data.has_more && data.next_offset != null))
        setLoading(false)
      } else {
        throw new Error('No library data available')
      }
    } catch (err) {
      if (generation !== libraryFetchGenerationRef.current) return
      setError(userFacingError(err, t.library.emptyLibrary))
      setLoading(false)
    }
  }, [filter, t.library.emptyLibrary])

  useEffect(() => {
    void fetchLibraryData()
  }, [fetchLibraryData])

  useEffect(() => {
    const handlePreferencesUpdated = (event: Event) => {
      const detail = (event as CustomEvent<LibraryPreferencesUpdatedDetail>)
        .detail
      if (detail?.layout) setPreferredLayout(detail.layout)
      void fetchLibraryData()
    }
    window.addEventListener(
      LIBRARY_PREFERENCES_UPDATED_EVENT,
      handlePreferencesUpdated,
    )
    return () =>
      window.removeEventListener(
        LIBRARY_PREFERENCES_UPDATED_EVENT,
        handlePreferencesUpdated,
      )
  }, [fetchLibraryData])

  const loadNextLibraryPage = useCallback(async () => {
    const offset = nextLibraryOffsetRef.current
    if (offset == null || libraryPageLoadingRef.current) return

    const generation = libraryFetchGenerationRef.current
    libraryPageLoadingRef.current = true
    try {
      const data: LibraryResponse = await getLibraryDataPageDeduped(
        offset,
        LIBRARY_PAGE_SIZE,
        filter,
      )
      if (generation !== libraryFetchGenerationRef.current) return
      if (!data.success) throw new Error('No library data available')

      const incoming = balancedShuffleLibraryItems(
        slimLibraryItems(data.items as LibraryItem[]),
      )
      setAllItems((current) => {
        if (incoming.length === 0) return current
        const known = new Set(current.map((item) => item.id))
        const unique = incoming.filter((item) => !known.has(item.id))
        return unique.length > 0 ? [...current, ...unique] : current
      })
      nextLibraryOffsetRef.current = data.next_offset ?? null
      setLibraryHasMore(Boolean(data.has_more && data.next_offset != null))
    } catch (err) {
      if (err instanceof Error && err.name !== 'AbortError') {
        console.error('Failed to load the next library page:', err)
      }
    } finally {
      if (generation === libraryFetchGenerationRef.current) {
        libraryPageLoadingRef.current = false
      }
    }
  }, [filter])

  loadNextLibraryPageRef.current = loadNextLibraryPage

  const canvasLoadedRadius = useMemo(() => {
    let radius = 0
    layouts.forEach((layout) => {
      const centerX = layout.left + layout.width / 2
      const centerY = layout.top + layout.height / 2
      const cardRadius = Math.hypot(layout.width, layout.height) / 2
      radius = Math.max(radius, Math.hypot(centerX, centerY) + cardRadius)
    })
    return radius
  }, [layouts])

  libraryHasMoreRef.current = libraryHasMore
  canvasLoadedRadiusRef.current = canvasLoadedRadius
  loadNextLibraryPageLiveRef.current = () => {
    void loadNextLibraryPage()
  }

  // After every React commit while canvas is active, re-paint the live pose.
  // Covers: virtualized card mount, music/live re-renders stomping chrome
  // disabled attrs, and first layout after viewport measure.
  // Leaving canvas: one-shot strip of absolute-follow surface paint (cards clear
  // via React list styles: transform none / zIndex auto).
  useLayoutEffect(() => {
    if (layoutMode === 'canvas') {
      wasCanvasLayoutRef.current = true
      paintCanvasTransform(canvasTransformRef.current)
      return
    }
    if (!wasCanvasLayoutRef.current) return
    wasCanvasLayoutRef.current = false
    const surface = containerRef.current
    if (!surface) return
    surface.style.removeProperty('background-size')
    surface.style.removeProperty('background-position')
  })

  useEffect(() => {
    if (layoutMode !== 'canvas') return
    lastCanvasVisibleSigRef.current = ''
    resetCanvasView()
  }, [filter, layoutMode, resetCanvasView])

  // 空状态图标
  const emptyIcon = useMemo(
    () => (
      <svg
        className="w-5.5 h-5.5"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
      >
        <path
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth={2}
          d="M3.75 6A2.25 2.25 0 016 3.75h2.25A2.25 2.25 0 0110.5 6v2.25a2.25 2.25 0 01-2.25 2.25H6a2.25 2.25 0 01-2.25-2.25V6zM3.75 15.75A2.25 2.25 0 016 13.5h2.25a2.25 2.25 0 012.25 2.25V18a2.25 2.25 0 01-2.25 2.25H6A2.25 2.25 0 013.75 18v-2.25zM13.5 6a2.25 2.25 0 012.25-2.25H18A2.25 2.25 0 0120.25 6v2.25A2.25 2.25 0 0118 10.5h-2.25a2.25 2.25 0 01-2.25-2.25V6zM13.5 15.75a2.25 2.25 0 012.25-2.25H18a2.25 2.25 0 012.25 2.25V18A2.25 2.25 0 0118 20.25h-2.25A2.25 2.25 0 0113.5 18v-2.25z"
        />
      </svg>
    ),
    [],
  )

  const emptyTitle = error ? t.library.emptyLibrary : t.library.emptyCategory
  const showEmpty = !loading && filteredAllItems.length === 0

  const getPlatformColor = useCallback((platform: string) => {
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
  }, [])

  const getTypeIcon = useCallback((type: string) => {
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
  }, [])

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
        return t.library.playedHours.replace('{hours}', hours.toString())
      }
      if (item.item_type === 'music') {
        // Bangumi music (subject_type=3): no Netease ar/artists; show rating / open-external hint
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
        // Netease (and other streamable) music metadata
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

  /** Progress row + thin bar for anime/book vertical title plates. */
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
      // Bangumi subject_type=3 → music, but id is bangumi_subject_* — not Netease
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
          showInfo(t.library.openExternal.replace('{name}', item.title || ''))
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
          showInfo(t.library.openExternal.replace('{name}', item.title || ''))
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
        // 暂停中：再点同一首应恢复播放，而不是误报「已在播放」
        if (!musicState.isPlaying) {
          window.dispatchEvent(new CustomEvent('toggle-play-pause'))
          showInfo(t.library.nowPlaying.replace('{name}', item.title || ''))
        } else {
          showInfo(t.library.alreadyPlaying)
        }
        return
      }

      // 正确标记 VIP；临时播放在 useMusicPlayer 内放行 excludeVipSongs
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

      // 同步 URL：禁止 await geo（会把「点击→开播」拖成数百 ms～数秒）
      // 海外若 play-url 失败，播放器方案 C 会自动降级全量代理
      const url = getNeteaseAudioUrlImmediate(songId)

      const song: Song = {
        id: songId.toString(),
        name,
        artist,
        album,
        // 临时播放入口：裸 CDN 封面必须代理，否则播放器取色 canvas CORS 失败
        cover: proxyImageUrlOr(cover, cover || ''),
        url,
        duration,
        source: 'netease',
        isVip,
      }

      // 先开面板 + toast，再播：体感即时
      window.dispatchEvent(new CustomEvent('open-control-panel'))
      showInfo(t.library.nowPlaying.replace('{name}', name))
      playSongRef.current(song)
      void import('../utils/analyticsEvents').then(
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

  const needsTransition = (from: string, to: string) => {
    return from !== 'all' && to !== 'all' && from !== to
  }

  useEffect(() => {
    if (filter === prevFilter) return

    transitionTimersRef.current.forEach(clearTimeout)
    transitionTimersRef.current = []

    if (needsTransition(prevFilter, filter)) {
      setIsTransitioning(true)
      const swapTimer = setTimeout(() => {
        setPrevFilter(filter)
      }, 200)
      const finishTimer = setTimeout(setIsTransitioning, 350, false)
      transitionTimersRef.current = [swapTimer, finishTimer]
    } else {
      setPrevFilter(filter)
      setIsTransitioning(false)
    }
    // 切换分类时重置显示数量
    setVisibleCount(20)
  }, [filter, prevFilter])

  useEffect(() => {
    return () => {
      transitionTimersRef.current.forEach(clearTimeout)
      transitionTimersRef.current = []
    }
  }, [])

  // 首屏加载：单一 Spinner，垂直居中（扣除顶/底安全区，与 Brew 观感一致）
  if (loading && allItems.length === 0) {
    return (
      <div
        className="flex w-full items-center justify-center min-h-[calc(100dvh-12rem)] sm:min-h-[calc(100dvh-11rem)] md:min-h-[calc(100dvh-8rem)]"
        role="status"
      >
        <Spinner size="lg" color="primary" />
      </div>
    )
  }

  return (
    <div className="animate-in fade-in slide-in-from-bottom-4 duration-700 ease-out">
      {error || showEmpty ? (
        <div className="flex flex-col items-start py-8">
          <div className="rounded-2xl glass-surface glass-90 border border-gray-200/50 dark:border-neutral-700/50 shadow-lg shadow-black/10 flex items-center gap-3 px-5 py-3">
            <div className="w-9 h-9 rounded-xl bg-gray-100/80 dark:bg-white/5 flex items-center justify-center text-gray-400 dark:text-gray-500 shrink-0">
              {emptyIcon}
            </div>
            <div className="min-w-0">
              <p className="text-sm font-medium text-gray-600 dark:text-gray-300">
                {emptyTitle}
              </p>
            </div>
          </div>
        </div>
      ) : (
        <div className={layoutMode === 'canvas' ? '' : 'space-y-8'}>
          <QuickTransition transitioning={isTransitioning}>
            <div
              ref={containerRef}
              className={
                layoutMode === 'canvas'
                  ? // inset-0 铺满 fixed 视口；勿再写死 h-dvh（iOS 地址栏伸缩时会短一截）
                    'fixed inset-0 z-0 min-h-lvh w-full overflow-hidden touch-none cursor-grab bg-white/5 dark:bg-black/5'
                  : 'relative w-full'
              }
              style={
                layoutMode === 'canvas'
                  ? {
                      // Size/position painted via paintCanvasTransform for absolute follow.
                      backgroundImage:
                        'radial-gradient(circle, color-mix(in srgb, var(--text-color, currentColor) 18%, transparent) 1px, transparent 1.2px)',
                    }
                  : {
                      height: `${containerHeight}px`,
                      minHeight: '400px',
                      // Dragging the custom scrollbar locks page-height math.
                      // Don't ease the grid taller mid-drag or the thumb slips.
                      transition:
                        typeof document !== 'undefined' &&
                        document.documentElement.dataset.scrollbarDragging ===
                          'true'
                          ? 'none'
                          : 'height 0.4s ease-out',
                    }
              }
              onPointerDown={
                layoutMode === 'canvas' ? handleCanvasPointerDown : undefined
              }
              onPointerMove={
                layoutMode === 'canvas' ? handleCanvasPointerMove : undefined
              }
              onPointerUp={
                layoutMode === 'canvas' ? finishCanvasPointer : undefined
              }
              onPointerCancel={
                layoutMode === 'canvas' ? finishCanvasPointer : undefined
              }
              onClickCapture={
                layoutMode === 'canvas' ? handleCanvasClickCapture : undefined
              }
              onKeyDown={
                layoutMode === 'canvas' ? handleCanvasKeyDown : undefined
              }
              onKeyUp={layoutMode === 'canvas' ? handleCanvasKeyUp : undefined}
              onBlur={layoutMode === 'canvas' ? handleCanvasBlur : undefined}
              tabIndex={layoutMode === 'canvas' ? 0 : undefined}
              data-library-canvas-surface={
                layoutMode === 'canvas' ? 'true' : undefined
              }
              aria-label={
                layoutMode === 'canvas' ? t.library.canvasAriaLabel : undefined
              }
            >
              {layoutMode === 'canvas' && (
                <LibraryCanvasChrome
                  ariaLabel={t.library.canvasAriaLabel}
                  atMaxZoom={canvasAtMaxZoom}
                  atMinZoom={canvasAtMinZoom}
                  dismissHintLabel={t.library.canvasDismissHint}
                  hint={t.library.canvasPanHint}
                  mobileHint={t.library.canvasPanHintMobile}
                  isDefault={canvasViewIsDefault}
                  onReset={resetCanvasView}
                  onZoom={zoomCanvas}
                  resetLabel={t.library.canvasResetView}
                  zoomInLabel={t.library.canvasZoomIn}
                  zoomOutLabel={t.library.canvasZoomOut}
                  zoomPercent={canvasZoomPercent}
                />
              )}
              <div
                ref={layoutMode === 'canvas' ? worldRef : undefined}
                className={
                  layoutMode === 'canvas'
                    ? 'library-canvas-world absolute left-1/2 top-1/2'
                    : 'contents'
                }
                style={
                  layoutMode === 'canvas'
                    ? {
                        // Transform painted via paintCanvasTransform (absolute follow).
                        transformOrigin: '0 0',
                      }
                    : undefined
                }
              >
                {visibleItems.map((item, itemIndex) => {
                  const layout = layouts.get(item.id)
                  if (!layout) return null

                  const platformColor = getPlatformColor(item.platform)
                  // VIP badge is Netease-only (fee/isVip); Bangumi music has no fee model
                  const isNeteaseMusic =
                    item.item_type === 'music' &&
                    !isBangumiPlatform(item.platform) &&
                    (item.platform.toLowerCase().includes('netease') ||
                      item.platform.toLowerCase().includes('网易') ||
                      item.id.startsWith('netease_'))
                  const isVip =
                    isNeteaseMusic && isNeteaseVipFromMeta(item.metadata)
                  const currentSongId = (
                    item.metadata.id || item.id.replace('netease_song_', '')
                  ).toString()

                  // 轻量身份：切句不刷整表；换歌离场保留短窗口
                  const isCurrentSong = liveSongId === currentSongId
                  const isLeavingSong = leavingSongId === currentSongId
                  const showMusicLive = isCurrentSong || isLeavingSong
                  const isPlaying = Boolean(isCurrentSong && globalIsPlaying)
                  // 播放中 + 退场窗口：锁 hover，避免中途放大/藏词打断动画
                  const hoverLocked = isPlaying || isLeavingSong

                  const rowIndex = Math.floor(layout.top / 300)
                  const listAnimationDelay = rowIndex * 0.05
                  const surfaceDragging =
                    layoutMode === 'canvas' &&
                    containerRef.current?.dataset.dragging === 'true'
                  const canvasEnterDelay =
                    layoutMode === 'canvas'
                      ? claimCanvasCardEnter(
                          item.id,
                          itemIndex,
                          Boolean(surfaceDragging),
                        )
                      : null
                  const canvasPriority = layoutMode === 'canvas'

                  // Bangumi / MAL 用户评分（0 表示未评分），显示在卡片左上角
                  const isBangumi = isBangumiPlatform(item.platform)
                  const userRate = hasUserRatingBadge(item.platform)
                    ? Number(
                        item.metadata.rate ?? item.metadata?.list_status?.score,
                      ) || 0
                    : 0
                  // Bangumi 游戏使用竖版，渲染为封面卡片
                  const isBangumiGame = isBangumi && item.item_type === 'game'

                  const ratingBadge =
                    userRate > 0
                      ? (() => {
                          const rs = getRatingBadgeStyle(userRate)
                          const animClass = rs.gloss
                            ? userRate >= 10
                              ? 'rating-badge-anim-max'
                              : 'rating-badge-anim'
                            : ''
                          return (
                            <div
                              className={`library-card-chrome absolute top-2.5 left-2.5 z-20 flex items-center justify-center overflow-hidden rounded-lg font-extrabold leading-none shadow-lg pointer-events-none ${rs.box} ${animClass}`}
                            >
                              {rs.gloss && (
                                <>
                                  <span className="absolute inset-x-0 top-0 h-1/2 bg-linear-to-b from-white/45 to-transparent" />
                                  <span className="rating-badge-shine" />
                                </>
                              )}
                              <span className="relative">{userRate}</span>
                            </div>
                          )
                        })()
                      : null

                  const platformCorner = (
                    <div className="absolute top-3 right-3 z-10 group/platform">
                      <div className="platform-icon-bg">
                        <PlatformIcon
                          platform={item.platform}
                          className="w-3.5 h-3.5"
                        />
                      </div>
                      <div className="absolute top-full right-0 mt-2 bg-black/90 text-white text-xs px-2.5 py-1 rounded-md opacity-0 group-hover/platform:opacity-100 transition-opacity duration-200 whitespace-nowrap pointer-events-none">
                        {item.platform}
                      </div>
                    </div>
                  )

                  return (
                    <div
                      key={item.id}
                      className={`absolute group library-card-container${hoverLocked ? ' is-hover-locked' : ''}`}
                      data-canvas-card={
                        layoutMode === 'canvas' ? '' : undefined
                      }
                      data-layout-left={
                        layoutMode === 'canvas' ? layout.left : undefined
                      }
                      data-layout-top={
                        layoutMode === 'canvas' ? layout.top : undefined
                      }
                      data-layout-width={
                        layoutMode === 'canvas' ? layout.width : undefined
                      }
                      data-layout-height={
                        layoutMode === 'canvas' ? layout.height : undefined
                      }
                      style={
                        {
                          left: `${layout.left}px`,
                          top: `${layout.top}px`,
                          width: `${layout.width}px`,
                          height: `${layout.height}px`,
                          '--platform-color': platformColor,
                          // List fadeInUp stagger only; canvas enter delay lives on the shell.
                          ...(layoutMode === 'list'
                            ? { animationDelay: `${listAnimationDelay}s` }
                            : {}),
                          // Canvas focus scale is painted each frame (absolute follow).
                          // List must set transform/zIndex so React clears leftover paint.
                          transformOrigin:
                            layoutMode === 'canvas' ? 'center center' : undefined,
                          transform:
                            layoutMode === 'canvas' ? undefined : 'none',
                          zIndex: layoutMode === 'canvas' ? undefined : 'auto',
                        } as CSSProperties
                      }
                      // 入场动画播放一次后移除，避免卡片滚出/滚入视口时
                      // 浏览器重建绘制层导致 fadeInUp 重播（表现为瞬间透明再恢复）
                      onAnimationEnd={(e) => {
                        if (e.target === e.currentTarget) {
                          ;(e.currentTarget as HTMLElement).style.animation =
                            'none'
                        }
                      }}
                    >
                      {item.item_type === 'music' ? (
                        <LibraryCardShell
                          cover={item.cover}
                          title={item.title}
                          coverBreathing={Boolean(isPlaying)}
                          priority={canvasPriority}
                          canvasEnterDelay={canvasEnterDelay}
                          className={
                            hoverLocked
                              ? 'bg-white rounded-xl shadow-md transition-shadow duration-300'
                              : 'bg-white rounded-xl shadow-md hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1 hover:scale-[1.02]'
                          }
                          placeholder={
                            <div className="w-full h-full flex items-center justify-center bg-linear-to-br from-pink-400 to-pink-500">
                              <span className="text-6xl">
                                {getTypeIcon(item.item_type)}
                              </span>
                            </div>
                          }
                        >
                          <div
                            className="absolute inset-0 z-[1] cursor-pointer"
                            data-canvas-card-action
                            onClick={(e) => {
                              e.preventDefault()
                              e.stopPropagation()
                              handlePlayMusic(item)
                            }}
                          >
                            {showMusicLive && (
                              <>
                                {/* active=当前曲（含暂停，光晕冻结保留）；playing=频谱动画 */}
                                <LibraryPlayingWaveBorder
                                  musicColor={musicColor}
                                  active={isCurrentSong}
                                  playing={isPlaying}
                                />
                                {/* active=当前曲（含暂停）；换歌时 false 走退场 */}
                                <LibraryCardLyrics
                                  active={isCurrentSong}
                                  musicColor={musicColor}
                                />
                              </>
                            )}

                            <div className="library-card-chrome library-card-hover-chrome absolute inset-0 bg-linear-to-t from-black/95 via-black/60 to-transparent opacity-0 group-hover:opacity-100 transition-all duration-300 flex flex-col justify-end p-3">
                              <div>
                                <div className="flex items-start gap-1">
                                  <h3 className="font-bold text-white text-xs leading-tight line-clamp-2 mb-1 flex-1">
                                    {item.title}
                                  </h3>
                                  {isVip && (
                                    <span className="inline-flex items-center px-1.5 py-0.5 rounded-md bg-linear-to-r from-yellow-500 to-amber-600 text-[10px] font-semibold text-white shadow-md select-none">
                                      VIP
                                    </span>
                                  )}
                                </div>
                                {renderWatchProgressPanel(item, {
                                  dark: true,
                                }) ??
                                  (getExtraInfo(item) && (
                                    <p className="text-[10px] text-white/75 line-clamp-1">
                                      {getExtraInfo(item)}
                                    </p>
                                  ))}
                              </div>
                            </div>
                          </div>

                          {platformCorner}
                          {ratingBadge}
                        </LibraryCardShell>
                      ) : item.item_type === 'anime' ||
                        item.item_type === 'tv_series' ||
                        item.item_type === 'book' ||
                        isBangumiGame ? (
                        <LibraryCardShell
                          cover={item.cover}
                          title={item.title}
                          priority={canvasPriority}
                          canvasEnterDelay={canvasEnterDelay}
                          className="bg-white rounded-xl shadow-lg hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1"
                          placeholder={
                            <div className="w-full h-full flex items-center justify-center bg-linear-to-br from-pink-400 to-purple-500">
                              <span className="text-6xl">
                                <FaVideo />
                              </span>
                            </div>
                          }
                        >
                          <button
                            type="button"
                            className="absolute inset-0 z-[1] cursor-pointer text-left bg-transparent border-0 p-0"
                            data-canvas-card-action
                            aria-label={item.title}
                            onClick={(e) => {
                              e.preventDefault()
                              e.stopPropagation()
                              openLibraryItemExternal(item)
                            }}
                          >
                            <div className="absolute bottom-3 left-3 right-3 z-[1] flex justify-start pointer-events-none">
                              <div className="library-card-caption">
                                <div className="library-card-caption__row">
                                  <h3 className="library-card-caption__title line-clamp-1">
                                    {item.title}
                                  </h3>
                                  <span
                                    className={`library-card-caption__type ${
                                      item.item_type === 'anime'
                                        ? 'library-card-caption__type--anime'
                                        : item.item_type === 'book'
                                          ? 'library-card-caption__type--book'
                                          : item.item_type === 'game'
                                            ? 'library-card-caption__type--game'
                                            : 'library-card-caption__type--tv'
                                    }`}
                                  >
                                    {item.item_type === 'anime'
                                      ? t.library.anime
                                      : item.item_type === 'book'
                                        ? t.library.book
                                        : item.item_type === 'game'
                                          ? t.library.game
                                          : t.library.tvSeries}
                                  </span>
                                </div>
                                {renderWatchProgressPanel(item)}
                              </div>
                            </div>
                          </button>
                          {platformCorner}
                          {ratingBadge}
                        </LibraryCardShell>
                      ) : item.item_type === 'video' ? (
                        <LibraryCardShell
                          cover={item.cover}
                          title={item.title}
                          priority={canvasPriority}
                          canvasEnterDelay={canvasEnterDelay}
                          className="bg-white rounded-xl shadow-lg hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1"
                          placeholder={
                            <div className="w-full h-full flex items-center justify-center bg-linear-to-br from-blue-400 to-blue-500">
                              <span className="text-6xl">
                                {getTypeIcon(item.item_type)}
                              </span>
                            </div>
                          }
                        >
                          <button
                            type="button"
                            className="absolute inset-0 z-[1] cursor-pointer text-left bg-transparent border-0 p-0"
                            data-canvas-card-action
                            aria-label={item.title}
                            onClick={(e) => {
                              e.preventDefault()
                              e.stopPropagation()
                              openLibraryItemExternal(item)
                            }}
                          >
                            <div className="absolute bottom-3 left-3 right-3 z-[1] flex justify-start pointer-events-none">
                              <div className="library-card-caption">
                                <h3 className="library-card-caption__title line-clamp-2">
                                  {item.title}
                                </h3>
                                {renderWatchProgressPanel(item) ??
                                  (getExtraInfo(item) && (
                                    <p className="library-card-caption__meta">
                                      {getExtraInfo(item)}
                                    </p>
                                  ))}
                              </div>
                            </div>
                          </button>
                          {platformCorner}
                          {ratingBadge}
                        </LibraryCardShell>
                      ) : (
                        <LibraryCardShell
                          cover={item.cover}
                          title={item.title}
                          priority={canvasPriority}
                          canvasEnterDelay={canvasEnterDelay}
                          className="bg-white rounded-xl shadow-lg hover:shadow-2xl transition-all duration-300 transform hover:-translate-y-1"
                          placeholder={
                            <div className="w-full h-full flex items-center justify-center bg-linear-to-br from-purple-400 to-pink-500">
                              <span className="text-6xl">
                                {getTypeIcon(item.item_type)}
                              </span>
                            </div>
                          }
                        >
                          <button
                            type="button"
                            className="absolute inset-0 z-[1] cursor-pointer bg-transparent border-0 p-0"
                            data-canvas-card-action
                            aria-label={item.title}
                            onClick={(e) => {
                              e.preventDefault()
                              e.stopPropagation()
                              openLibraryItemExternal(item)
                            }}
                          />
                          <div className="library-card-chrome absolute inset-0 z-[1] bg-linear-to-t from-black/90 via-black/40 to-transparent opacity-0 group-hover:opacity-100 transition-opacity duration-300 flex flex-col justify-end p-4 pointer-events-none">
                            <h3 className="font-bold text-white text-base line-clamp-2 leading-snug mb-1">
                              {item.title}
                            </h3>
                            {renderWatchProgressPanel(item, { dark: true }) ??
                              (getExtraInfo(item) && (
                                <p className="text-sm text-white/80">
                                  {getExtraInfo(item)}
                                </p>
                              ))}
                          </div>
                          {platformCorner}
                          {ratingBadge}
                        </LibraryCardShell>
                      )}
                    </div>
                  )
                })}
              </div>
            </div>
          </QuickTransition>

          {/* 无限滚动哨兵：不可见，避免底部常驻 Spinner 造成「卡住/双重加载」 */}
          {hasMore && (
            <div
              ref={observerTarget}
              className="h-px w-full"
              aria-hidden="true"
            />
          )}
        </div>
      )}
    </div>
  )
}
