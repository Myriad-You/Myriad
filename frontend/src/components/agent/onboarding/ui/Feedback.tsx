import type { ReactNode } from 'react'
import { useEffect, useState } from 'react'

export function Working({ children }: { children: ReactNode }) {
  const [elapsed, setElapsed] = useState(0)
  useEffect(() => {
    const id = window.setInterval(() => setElapsed((n) => n + 1), 1000)
    return () => window.clearInterval(id)
  }, [])
  const time =
    elapsed < 60
      ? `${elapsed}s`
      : `${Math.floor(elapsed / 60)}:${String(elapsed % 60).padStart(2, '0')}`
  return (
    <div className="merope-ob-bg" role="status" aria-live="polite">
      <span className="merope-loading__orb" aria-hidden />
      <p className="merope-ob-bg__text">{children}</p>
      <p className="merope-ob-bg__sub">{time}</p>
    </div>
  )
}

export function ErrorNote({ children }: { children: ReactNode }) {
  return (
    <div className="merope-error" role="alert">
      <span>{children}</span>
    </div>
  )
}
