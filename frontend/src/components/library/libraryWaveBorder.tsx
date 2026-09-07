import type { CSSProperties } from 'react'
import { memo, useEffect, useRef, useState } from 'react'
import { audioManager } from '../../utils/musicPlayer'

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
  'library-wave-border-styles',
  `
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
  `,
)

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
export const LibraryPlayingWaveBorder = memo(
  ({
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
    const peak2TRef = useRef(
      (peak1TRef.current + 0.3 + Math.random() * 0.35) % 1,
    )
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
          if (live && !isPlaying && presence > 0.98 && settled) {
            return
          }
          if (
            reducedMotion &&
            live &&
            isPlaying &&
            presence > 0.98 &&
            settled
          ) {
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
              const onset = Math.min(
                1,
                onsetRaw + (isPlaying ? kick * 0.55 : 0),
              )

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
                hBias1Ref.current = ouStep(
                  hBias1Ref.current,
                  0,
                  0.006,
                  0.16,
                  dt,
                )
                hBias2Ref.current = ouStep(
                  hBias2Ref.current,
                  0,
                  0.0055,
                  0.18,
                  dt,
                )
                wBias1Ref.current = ouStep(
                  wBias1Ref.current,
                  0,
                  0.008,
                  0.12,
                  dt,
                )
                wBias2Ref.current = ouStep(
                  wBias2Ref.current,
                  0,
                  0.0075,
                  0.13,
                  dt,
                )
                hBias1Ref.current = Math.max(
                  -0.75,
                  Math.min(0.8, hBias1Ref.current),
                )
                hBias2Ref.current = Math.max(
                  -0.8,
                  Math.min(0.85, hBias2Ref.current),
                )
                wBias1Ref.current = Math.max(
                  -0.55,
                  Math.min(0.65, wBias1Ref.current),
                )
                wBias2Ref.current = Math.max(
                  -0.6,
                  Math.min(0.7, wBias2Ref.current),
                )

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
                  (timeAccRef.current >= nextReseedAtRef.current ||
                    onset > 0.72)
                ) {
                  anchor1Ref.current = Math.random()
                  anchor2Ref.current =
                    (anchor1Ref.current + 0.22 + Math.random() * 0.48) % 1
                  hBias1Ref.current +=
                    (Math.random() - 0.5) * (0.35 + onset * 0.4)
                  hBias2Ref.current +=
                    (Math.random() - 0.5) * (0.4 + onset * 0.45)
                  wBias1Ref.current += (Math.random() - 0.5) * 0.3
                  wBias2Ref.current += (Math.random() - 0.5) * 0.35
                  v1Ref.current += (Math.random() - 0.5) * 0.01
                  v2Ref.current += (Math.random() - 0.5) * 0.012
                  noiseSpeedRef.current = 0.02 + Math.random() * 0.06
                  nextReseedAtRef.current =
                    timeAccRef.current +
                    1.1 +
                    Math.random() * 2.4 +
                    (1 - onset) * 1.2
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
                  0.4 +
                    bass * 0.55 -
                    treble * 0.25 +
                    midF * 0.15 +
                    wBias1Ref.current,
                )
                const w2Target = Math.max(
                  0.28,
                  0.35 +
                    midF * 0.3 +
                    treble * 0.45 -
                    bass * 0.12 +
                    wBias2Ref.current,
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
                  (0.28 +
                    energy * 0.35 +
                    body * 0.4 +
                    onset * 0.15 +
                    kick * 0.38)
                : live
                  ? opacitySmoothRef.current
                  : opacityPresence * (0.28 + energy * 0.35 + body * 0.4)
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
        if (!(
          settled &&
          activeRef.current &&
          (!playingRef.current || reducedMotion)
        )) {
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
  },
)
