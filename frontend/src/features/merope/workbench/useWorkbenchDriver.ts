import type { RefObject } from 'react'
import type { Anime25DDriver } from '../anime25drig/driver'
import type { Anime25DDebugSnapshot } from '../anime25drig/player'
import type { RigCharacterHandle } from '../rig/RigCharacter'
import { useEffect, useRef, useState } from 'react'
import { WORKBENCH_DRIVER } from '../anime25drig/driver'
import { PreviewMotionScope } from '../motion/previewScope'

/**
 * The hand-set driver behind the motion tab. Writes go straight to the
 * preview rig inside its own motion scope; nothing reaches the live runtime.
 */
export function useWorkbenchDriver(
  characterRef: RefObject<RigCharacterHandle | null>,
  /** The motion tab is open over a playable rig. */
  inspecting: boolean,
  /** Changes when the portrait does, so a new rig takes the current driver. */
  source: string,
) {
  const [driver, setDriver] = useState<Anime25DDriver>({ ...WORKBENCH_DRIVER })
  const driverRef = useRef(driver)
  const syncedDriverRef = useRef(false)
  const previewScopeRef = useRef<PreviewMotionScope | null>(null)
  if (previewScopeRef.current === null) {
    previewScopeRef.current = new PreviewMotionScope()
  }
  driverRef.current = driver
  const [snapshot, setSnapshot] = useState<Anime25DDebugSnapshot | null>(null)

  const writePreview = (write: (rig: RigCharacterHandle) => void) => {
    const scope = previewScopeRef.current
    const rig = characterRef.current
    if (!scope) return
    scope.take()
    if (!rig) return
    rig.setMotionPolicy(scope.policy())
    write(rig)
  }

  useEffect(() => {
    syncedDriverRef.current = false
  }, [source])

  useEffect(() => {
    if (!inspecting) {
      setSnapshot(null)
      return undefined
    }
    const refresh = () => {
      const next = characterRef.current?.debugSnapshot() ?? null
      setSnapshot(next)
      if (next && !syncedDriverRef.current) {
        syncedDriverRef.current = true
        writePreview((rig) => rig.replaceDriver(driverRef.current))
      }
    }
    refresh()
    const timer = window.setInterval(refresh, 200)
    return () => window.clearInterval(timer)
  }, [characterRef, inspecting])

  const applyDriver = (next: Anime25DDriver) => {
    setDriver(next)
    writePreview((rig) => rig.replaceDriver(next))
  }

  const patchDriver = (partial: Partial<Anime25DDriver>) => {
    const next = { ...driver, ...partial }
    setDriver(next)
    writePreview((rig) => rig.setDriver(partial))
  }

  const applyPreset = (partial: Partial<Anime25DDriver>) => {
    applyDriver({
      ...WORKBENCH_DRIVER,
      ...partial,
      idle: false,
      rand: false,
      talk: false,
      mouse: false,
    })
  }

  const resetPose = () => {
    applyDriver({ ...WORKBENCH_DRIVER })
  }

  return { driver, snapshot, applyDriver, patchDriver, applyPreset, resetPose }
}

export type WorkbenchDriver = ReturnType<typeof useWorkbenchDriver>
