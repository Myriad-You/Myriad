import type { ReadingProgress } from './progressStore'
import { useSyncExternalStore } from 'react'

export function useReadingProgressValue(progress: ReadingProgress): number {
  return useSyncExternalStore(progress.subscribe, progress.get, progress.get)
}

export function ReaderProgressRail({ progress }: { progress: ReadingProgress }) {
  const value = useReadingProgressValue(progress)
  return (
    <div className="brew-reader__rail">
      <div
        className="brew-reader__rail-fill"
        style={{ transform: `scaleX(${value / 100})` }}
      />
    </div>
  )
}

export function ReaderProgressRing({
  progress,
  accent,
  track,
  labelClass,
}: {
  progress: ReadingProgress
  accent: string
  track: string
  labelClass: string
}) {
  const value = useReadingProgressValue(progress)
  return (
    <>
      <svg className="w-10 h-10 -rotate-90">
        <circle
          cx="20"
          cy="20"
          r="16"
          fill="none"
          stroke={track}
          strokeWidth="3"
        />
        <circle
          cx="20"
          cy="20"
          r="16"
          fill="none"
          stroke={accent}
          strokeWidth="3"
          strokeLinecap="round"
          strokeDasharray={`${value} 100`}
          className="transition-all duration-300"
        />
      </svg>
      <span className={`absolute text-[10px] font-medium ${labelClass} tabular-nums`}>
        {value}
      </span>
    </>
  )
}

export function ReaderProgressPercent({
  progress,
  className,
}: {
  progress: ReadingProgress
  className: string
}) {
  const value = useReadingProgressValue(progress)
  return <span className={className}>{value}%</span>
}
