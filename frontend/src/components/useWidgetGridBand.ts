import type { BandSwitchPhase } from './widgetGridBand'
import { useEffect, useRef, useState } from 'react'
import { resolveHomeGridColumns } from '../utils/viewportBands'
import {
  decideHomeBandMorph,
  GRID_BAND_IN_MS,
  GRID_BAND_OUT_MS,
  lockedHomeGridColumns,
  readInitialHomeGridColumns,
} from './widgetGridBand'

export function useWidgetGridBand(input: {
  customGridColumns?: number
  isFreeLayout: boolean
  hardCut: boolean
  windowWidth: number
}): {
  gridColumns: number
  bandSwitch: BandSwitchPhase
} {
  const [gridColumns, setGridColumns] = useState(() =>
    readInitialHomeGridColumns(input.customGridColumns),
  )
  const [bandSwitch, setBandSwitch] = useState<BandSwitchPhase>(null)
  const prevColumnsRef = useRef(gridColumns)
  const switchingRef = useRef(false)
  const settledOnceRef = useRef(false)
  const timersRef = useRef<{ out?: number; in?: number; raf?: number }>({})

  useEffect(() => {
    const clearBandTimers = () => {
      if (timersRef.current.out) {
        clearTimeout(timersRef.current.out)
        timersRef.current.out = undefined
      }
      if (timersRef.current.in) {
        clearTimeout(timersRef.current.in)
        timersRef.current.in = undefined
      }
      if (timersRef.current.raf) {
        cancelAnimationFrame(timersRef.current.raf)
        timersRef.current.raf = undefined
      }
    }

    const lock = lockedHomeGridColumns({
      isFreeLayout: input.isFreeLayout,
      customGridColumns: input.customGridColumns,
    })
    if (lock.locked) {
      clearBandTimers()
      switchingRef.current = false
      setBandSwitch(null)
      if (lock.columns != null) {
        setGridColumns(lock.columns)
        prevColumnsRef.current = lock.columns
      }
      settledOnceRef.current = true
      return clearBandTimers
    }

    const readDesired = () =>
      resolveHomeGridColumns(
        typeof window !== 'undefined' ? window.innerWidth : input.windowWidth,
        prevColumnsRef.current,
      )

    const hardApply = (cols: number) => {
      clearBandTimers()
      switchingRef.current = false
      setBandSwitch(null)
      prevColumnsRef.current = cols
      setGridColumns(cols)
      settledOnceRef.current = true
    }

    const tryBandMorph = () => {
      const decision = decideHomeBandMorph({
        desired: readDesired(),
        previous: prevColumnsRef.current,
        switching: switchingRef.current,
        settledOnce: settledOnceRef.current,
        hardCut: input.hardCut,
      })
      if (decision.type === 'already-current') {
        settledOnceRef.current = true
        return
      }
      if (decision.type === 'busy') return
      if (decision.type === 'hard-apply') {
        hardApply(decision.columns)
        return
      }

      switchingRef.current = true
      setBandSwitch('out')
      clearBandTimers()

      timersRef.current.out = window.setTimeout(() => {
        const target = readDesired()
        prevColumnsRef.current = target
        setGridColumns(target)
        setBandSwitch('in')

        timersRef.current.in = window.setTimeout(() => {
          setBandSwitch(null)
          switchingRef.current = false
          settledOnceRef.current = true
          timersRef.current.raf = requestAnimationFrame(() => {
            timersRef.current.raf = undefined
            tryBandMorph()
          })
        }, GRID_BAND_IN_MS)
      }, GRID_BAND_OUT_MS)
    }

    tryBandMorph()
    return clearBandTimers
  }, [
    input.windowWidth,
    input.customGridColumns,
    input.hardCut,
    input.isFreeLayout,
  ])

  useEffect(() => {
    return () => {
      if (timersRef.current.out) clearTimeout(timersRef.current.out)
      if (timersRef.current.in) clearTimeout(timersRef.current.in)
      if (timersRef.current.raf) {
        cancelAnimationFrame(timersRef.current.raf)
      }
      switchingRef.current = false
    }
  }, [])

  return { gridColumns, bandSwitch }
}
