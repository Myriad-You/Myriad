import { memo, useEffect, useMemo, useRef } from 'react'
import { isPageVisible, onVisibility } from '../../hooks/animation'
import { audioManager } from '../../utils/musicPlayer'

export interface PlayingSpectrumProps {
  themeColor?: string
  scale?: number
  isPlaying?: boolean
  useSpectrum?: boolean
  variant?: 'bottom' | 'center'
  className?: string
}

const PlayingSpectrum = memo(({
  themeColor = 'var(--music-base-primary, #ec4899)',
  scale = 1,
  isPlaying = false,
  useSpectrum = true,
  variant = 'bottom',
  className,
}: PlayingSpectrumProps) => {
  const bar1Ref = useRef<HTMLDivElement>(null)
  const bar2Ref = useRef<HTMLDivElement>(null)
  const bar3Ref = useRef<HTMLDivElement>(null)
  const bar4Ref = useRef<HTMLDivElement>(null)
  const animationRef = useRef<number | null>(null)
  const connectedRef = useRef(false)
  const pageVisibleRef = useRef(isPageVisible())

  const live = Boolean(isPlaying && useSpectrum)
  const centered = variant === 'center'

  useEffect(() => {
    const applyHeights = (h1: string, h2: string, h3: string, h4: string) => {
      if (bar1Ref.current) bar1Ref.current.style.height = h1
      if (bar2Ref.current) bar2Ref.current.style.height = h2
      if (bar3Ref.current) bar3Ref.current.style.height = h3
      if (bar4Ref.current) bar4Ref.current.style.height = h4
    }

    if (!live) {
      if (animationRef.current != null) {
        cancelAnimationFrame(animationRef.current)
        animationRef.current = null
      }
      connectedRef.current = false
      if (isPlaying) {
        applyHeights('35%', '70%', '55%', '40%')
      } else {
        applyHeights('28%', '48%', '40%', '32%')
      }
      return
    }

    let cancelled = false
    let lastUpdateTime = 0
    const UPDATE_INTERVAL = 60

    const ensureConnected = () => {
      if (connectedRef.current) return true
      const audio = audioManager.getCurrentAudio()
      if (!audio) return false
      const ok = audioManager.connectAudioToAnalyser(audio)
      if (ok) connectedRef.current = true
      return ok
    }

    const stopLoop = () => {
      if (animationRef.current != null) {
        cancelAnimationFrame(animationRef.current)
        animationRef.current = null
      }
    }

    const updateSpectrum = (timestamp: number) => {
      if (cancelled) return
      if (!pageVisibleRef.current || !isPlaying) {
        animationRef.current = null
        return
      }

      if (timestamp - lastUpdateTime >= UPDATE_INTERVAL) {
        ensureConnected()
        const data = audioManager.getSpectrumData()
        const base = centered ? 22 : 30
        const amp = centered ? 78 : 70
        applyHeights(
          `${base + data[0] ** 2.0 * amp}%`,
          `${base + data[1] ** 2.0 * amp}%`,
          `${base + data[2] ** 2.0 * amp}%`,
          `${base + data[3] ** 2.0 * amp}%`,
        )
        lastUpdateTime = timestamp
      }
      animationRef.current = requestAnimationFrame(updateSpectrum)
    }

    const startLoop = () => {
      if (cancelled || animationRef.current != null) return
      if (!pageVisibleRef.current || !isPlaying) return
      animationRef.current = requestAnimationFrame(updateSpectrum)
    }

    pageVisibleRef.current = isPageVisible()
    ensureConnected()

    // 回到前台必须重启循环，只改 ref 不够。
    const unsubscribe = onVisibility((visible) => {
      pageVisibleRef.current = visible
      if (visible) startLoop()
      else stopLoop()
    })

    startLoop()

    return () => {
      cancelled = true
      unsubscribe()
      stopLoop()
    }
  }, [live, isPlaying, centered])

  const containerStyle = useMemo(
    () => ({
      gap: `${Math.max(1.25, 1.75 * scale)}px`,
      height: `${(centered ? 16 : 16) * scale}px`,
      display: 'flex' as const,
      alignItems: (centered ? 'center' : 'flex-end') as
        | 'center'
        | 'flex-end',
    }),
    [scale, centered],
  )

  const barStyle = useMemo(
    () => ({
      background: themeColor,
      width: `${2 * scale}px`,
      transition: live ? 'height 0.06s linear' : undefined,
      alignSelf: centered ? ('center' as const) : undefined,
    }),
    [themeColor, scale, live, centered],
  )

  const initialHeights = live
    ? (['30%', '50%', '40%', '35%'] as const)
    : isPlaying
      ? (['35%', '70%', '55%', '40%'] as const)
      : (['28%', '48%', '40%', '32%'] as const)

  return (
    <div
      className={`playing-spectrum playing-spectrum--${variant}${className ? ` ${className}` : ''}${isPlaying ? ' is-playing' : ''}`}
      style={containerStyle}
      aria-hidden
    >
      <div
        ref={bar1Ref}
        className="playing-spectrum__bar"
        style={{
          ...barStyle,
          height: initialHeights[0],
          borderRadius: 999,
          minHeight: 2,
        }}
      />
      <div
        ref={bar2Ref}
        className="playing-spectrum__bar"
        style={{
          ...barStyle,
          height: initialHeights[1],
          borderRadius: 999,
          minHeight: 2,
        }}
      />
      <div
        ref={bar3Ref}
        className="playing-spectrum__bar"
        style={{
          ...barStyle,
          height: initialHeights[2],
          borderRadius: 999,
          minHeight: 2,
        }}
      />
      <div
        ref={bar4Ref}
        className="playing-spectrum__bar"
        style={{
          ...barStyle,
          height: initialHeights[3],
          borderRadius: 999,
          minHeight: 2,
        }}
      />
    </div>
  )
})

PlayingSpectrum.displayName = 'PlayingSpectrum'

export { PlayingSpectrum }
