import type { RefObject } from 'react'
import type { Anime25DDriver } from '../anime25drig/driver'
import type { PoseCorrectionRegion } from '../anime25drig/poseAuthoring'
import type { PoseCorrection } from '../anime25drig/poseCorrections'
import type { Anime25DPlayback } from '../anime25drig/types'
import type { PoseCorrectionStagePick } from '../anime25drig/workbenchPort'
import type { RigCharacterHandle } from '../character/RigCharacter'
import { useEffect, useRef, useState } from 'react'
import { SettingGroup, SettingsButton, SettingTitleTag, SliderItem } from '../../../components/settings'
import { useI18n } from '../../../contexts/I18nContext'
import { showStickyToast } from '../../../utils/toastManager'
import {
  appendPoseCorrectionPatch,
  clampPoseCorrectionPatch,
  countPoseCorrectionChanges,
  nearestPoseCorrectionRegion,
  newPoseCorrectionPatch,
  POSE_CORRECTION_REGIONS,
  poseCorrectionCorner,
  poseCorrectionPreviewDriver,
  poseCorrectionRegionPoint,
  poseCorrectionTransitionDriver,
} from '../anime25drig/poseAuthoring'
import { isPoseCorrections } from '../anime25drig/poseCorrections'
import { PoseCorrectionOverlay } from './PoseCorrectionOverlay'

type Patch = PoseCorrection['patches'][number]

const NO_CORRECTIONS: PoseCorrection[] = []

interface Props {
  playback: Anime25DPlayback
  characterRef: RefObject<RigCharacterHandle | null>
  driver: Anime25DDriver
  /** Where a save lands, in words: the worn bust or a full-body set. */
  target: string
  onDriver: (driver: Anime25DDriver) => void
  onSave: (corrections: PoseCorrection[]) => Promise<void>
}

const PRESETS = [
  { key: 'presetUpLeft', angleX: -0.7, angleY: 0.5 },
  { key: 'presetUpRight', angleX: 0.7, angleY: 0.5 },
  { key: 'presetDownLeft', angleX: -0.7, angleY: -0.5 },
  { key: 'presetDownRight', angleX: 0.7, angleY: -0.5 },
] as const

const FINE_TUNE = [
  { field: 'dx', label: 'pushX', min: -0.25, max: 0.25 },
  { field: 'dy', label: 'pushY', min: -0.25, max: 0.25 },
  { field: 'radiusX', label: 'reachX', min: 0.1, max: 2 },
  { field: 'radiusY', label: 'reachY', min: 0.1, max: 2 },
  { field: 'x', label: 'centerX', min: -2, max: 2 },
  { field: 'y', label: 'centerY', min: -2, max: 2 },
] as const

/**
 * Corrections for poses that combine a head turn with a nod, a closed eye or
 * an open mouth. Pose the head, name the place, drag it back on the stage.
 * Mounted per immutable asset id; drafts never touch the live package.
 */
export function PoseCorrectionEditor({ playback, characterRef, driver, target, onDriver, onSave }: Props) {
  const { t, format } = useI18n()
  const copy = t.merope.poseCorrection
  const baseline = playback.shellProfile.poseCorrections ?? []
  const [draft, setDraft] = useState<PoseCorrection[]>(() => structuredClone(baseline))
  const [selected, setSelected] = useState<number | null>(null)
  const [patchIndex, setPatchIndex] = useState(0)
  const [picking, setPicking] = useState(false)
  const [comparing, setComparing] = useState(false)
  const [transition, setTransition] = useState(1)
  const [notice, setNotice] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  // A full figure's head is small on stage: zoom onto it unless asked not to.
  const smallHead = 2 * playback.shellProfile.head.radiusY / playback.pixelCanvas.height < 0.25
  const [zoomed, setZoomed] = useState(smallHead)
  const alive = useRef(true)
  useEffect(() => {
    alive.current = true
    return () => { alive.current = false }
  }, [])

  const changes = countPoseCorrectionChanges(baseline, draft)
  const correction = selected === null ? null : draft[selected] ?? null
  const patch = correction?.patches[patchIndex] ?? null
  const readiness = poseCorrectionCorner(playback, driver, 'head', { x: 0, y: 0 })

  const pct = (value: number) => Math.round(Math.abs(value) * 100)
  const describe = (at: PoseCorrection['at']) => {
    const parts: string[] = []
    if (at.angleX !== undefined) parts.push(format(at.angleX < 0 ? copy.turnLeft : copy.turnRight, { n: pct(at.angleX) }))
    if (at.angleY !== undefined) parts.push(format(at.angleY > 0 ? copy.lookUp : copy.lookDown, { n: pct(at.angleY) }))
    if (at.eyeCloseL !== undefined) parts.push(format(copy.eyeCloseL, { n: pct(at.eyeCloseL) }))
    if (at.eyeCloseR !== undefined) parts.push(format(copy.eyeCloseR, { n: pct(at.eyeCloseR) }))
    if (at.mouthOpen !== undefined) parts.push(format(copy.mouthOpen, { n: pct(at.mouthOpen) }))
    return parts.join(' + ') || copy.poseFront
  }
  const surfaceName = (surface: PoseCorrection['surface']) =>
    surface === 'head' ? copy.surfaceHead : surface === 'front-hair' ? copy.surfaceFrontHair : copy.surfaceBackHair
  const patchName = (c: PoseCorrection, p: Patch, index: number) => {
    const region = nearestPoseCorrectionRegion(playback, c.surface, p)
    if (!region) return format(copy.otherPatch, { surface: surfaceName(c.surface), n: index + 1 })
    // Two spots on one feature are told apart by their order.
    const same = c.patches.filter(q => nearestPoseCorrectionRegion(playback, c.surface, q) === region)
    return same.length > 1 ? `${copy[region]} ${same.indexOf(p) + 1}` : copy[region]
  }
  const currentPose = describe({
    ...(Math.abs(driver.angleX) >= 0.05 ? { angleX: driver.angleX } : {}),
    ...(Math.abs(driver.angleY) >= 0.05 ? { angleY: driver.angleY } : {}),
  })

  const show = (corrections: PoseCorrection[], index: number) => {
    setTransition(1)
    onDriver(poseCorrectionPreviewDriver(corrections[index], driver))
  }
  const select = (index: number, patch = 0) => {
    setSelected(index)
    setPatchIndex(patch)
    setPicking(false)
    setNotice(null)
    show(draft, index)
  }
  const addAt = (surface: PoseCorrection['surface'], point: { x: number; y: number }) => {
    setPicking(false)
    const corner = poseCorrectionCorner(playback, driver, surface, point)
    if ('problem' in corner) {
      setNotice(copy[corner.problem])
      return
    }
    const candidate: PoseCorrection = { surface, at: corner.at, patches: [newPoseCorrectionPatch(playback, surface, point)] }
    const result = isPoseCorrections([candidate]) ? appendPoseCorrectionPatch(draft, candidate) : null
    if (!result) {
      setNotice(copy.limit)
      return
    }
    setNotice(null)
    setDraft(result.corrections)
    setSelected(result.index)
    setPatchIndex(result.patch)
    show(result.corrections, result.index)
  }
  const addRegion = (region: PoseCorrectionRegion) => {
    const anchor = poseCorrectionRegionPoint(playback, region)
    if (anchor) addAt(anchor.surface, anchor)
  }
  const onPick = (pick: PoseCorrectionStagePick | null) => {
    if (!pick) {
      setNotice(copy.pickMissed)
      return
    }
    addAt(pick.surface, pick)
  }
  const editPatch = (index: number, next: Patch) => {
    if (selected === null) return
    setDraft(current => current.map((c, i) => i === selected
      ? { ...c, patches: c.patches.map((p, k) => k === index ? next : p) }
      : c))
  }
  const removePatch = (corner: number, index: number) => {
    setDraft(current => current
      .map((c, i) => i === corner ? { ...c, patches: c.patches.filter((_, k) => k !== index) } : c)
      .filter(c => c.patches.length))
    if (draft[corner]?.patches.length === 1) setSelected(null)
    setPatchIndex(0)
  }
  const removeCorner = (corner: number) => {
    setDraft(current => current.filter((_, i) => i !== corner))
    setSelected(null)
    setPatchIndex(0)
  }
  const discard = () => {
    setDraft(structuredClone(baseline))
    setSelected(null)
    setPatchIndex(0)
    setNotice(null)
  }
  const save = async () => {
    setSaving(true)
    try { await onSave(structuredClone(draft)) }
    catch (reason) {
      if (alive.current) {
        showStickyToast({
          message: reason instanceof Error && reason.message ? reason.message : copy.failed,
          type: 'error',
          replaceKey: 'merope-pose',
        })
      }
    }
    finally { if (alive.current) setSaving(false) }
  }

  return (
    <SettingGroup
      title={copy.title}
      titleExtra={<SettingTitleTag variant="muted">{copy.badge}</SettingTitleTag>}
      description={copy.description}
      descriptionVisible
      collapsible
      defaultExpanded={false}
      id="merope-pose-corrections"
    >
      <PoseCorrectionPreview characterRef={characterRef} corrections={comparing ? NO_CORRECTIONS : draft} />
      <PoseCorrectionZoom characterRef={characterRef} on={zoomed} />
      <PoseCorrectionOverlay
        characterRef={characterRef}
        correction={correction}
        active={patchIndex}
        picking={picking}
        visible={!comparing && transition === 1}
        disabled={saving}
        labels={{ push: copy.gripPush, origin: copy.gripOrigin, reach: copy.gripReach, picking: copy.picking }}
        onPick={onPick}
        onCancelPick={() => setPicking(false)}
        onSelect={setPatchIndex}
        onChange={editPatch}
      />
      <div className="merope-pose">
        <div className="merope-pose__top">
          <p className="merope-pose__target">{format(copy.target, { target })}</p>
          {smallHead ? (
            <SettingsButton type="button" size="sm" variant="secondary" aria-pressed={zoomed} onClick={() => setZoomed(on => !on)}>
              {zoomed ? copy.zoomOut : copy.zoomIn}
            </SettingsButton>
          ) : null}
        </div>

        <section className="merope-pose__step">
          <h4>{copy.stepPose}</h4>
          <p className="merope-pose__hint">{copy.stepPoseHint}</p>
          <div className="merope-pose__chips">
            {PRESETS.map(preset => (
              <SettingsButton
                key={preset.key}
                type="button"
                size="sm"
                variant="secondary"
                disabled={saving}
                onClick={() => {
                  setSelected(null)
                  setTransition(1)
                  onDriver(poseCorrectionPreviewDriver({ surface: 'head', at: { angleX: preset.angleX, angleY: preset.angleY }, patches: [] }, driver))
                }}
              >
                {copy[preset.key]}
              </SettingsButton>
            ))}
          </div>
          <p className={`merope-pose__status${'problem' in readiness ? '' : ' is-ready'}`}>
            {format(copy.poseNow, { pose: currentPose })}
            {' · '}
            {'problem' in readiness ? copy[readiness.problem] : copy.poseReady}
          </p>
        </section>

        <section className="merope-pose__step">
          <h4>{copy.stepPlace}</h4>
          <div className="merope-pose__chips">
            <SettingsButton
              type="button"
              size="sm"
              disabled={saving || 'problem' in readiness && readiness.problem === 'needsTurn'}
              aria-pressed={picking}
              onClick={() => {
                setNotice(null)
                setPicking(on => !on)
              }}
            >
              {picking ? t.common.cancel : copy.pick}
            </SettingsButton>
            {POSE_CORRECTION_REGIONS.map(region => (
              <SettingsButton
                key={region}
                type="button"
                size="sm"
                variant="secondary"
                disabled={saving || !poseCorrectionRegionPoint(playback, region)}
                onClick={() => addRegion(region)}
              >
                {copy[region]}
              </SettingsButton>
            ))}
          </div>
          {notice ? <p className="merope-pose__notice" role="status">{notice}</p> : null}
        </section>

        <section className="merope-pose__step">
          <h4>{copy.stepDrag}</h4>
          <p className="merope-pose__hint">{correction ? copy.stepDragHint : copy.stepDragEmpty}</p>
          {correction && patch ? (
            <details className="merope-pose__fine">
              <summary>{format(copy.fineTune, { name: patchName(correction, patch, patchIndex) })}</summary>
              {FINE_TUNE.map(item => (
                <SliderItem
                  key={item.field}
                  itemKey={`pose-correction-${item.field}`}
                  label={copy[item.label]}
                  value={patch[item.field]}
                  disabled={saving}
                  min={item.min}
                  max={item.max}
                  step={0.005}
                  formatValue={value => `${Math.round(value * 100)}%`}
                  onChange={value => editPatch(patchIndex, clampPoseCorrectionPatch({ ...patch, [item.field]: value }))}
                />
              ))}
            </details>
          ) : null}
        </section>

        <section className="merope-pose__step">
          <h4>{copy.list}</h4>
          {draft.length === 0 ? <p className="merope-pose__hint">{copy.listEmpty}</p> : (
            <ul className="merope-pose__list">
              {draft.map((c, index) => (
                <li key={index} className={`merope-pose__corner${index === selected ? ' is-on' : ''}`}>
                  <div className="merope-pose__corner-head">
                    <button type="button" className="merope-pose__corner-name" disabled={saving} onClick={() => select(index)}>
                      <b>{describe(c.at)}</b>
                      <span>{surfaceName(c.surface)} · {format(copy.patchCount, { n: c.patches.length })}</span>
                    </button>
                    <button type="button" className="merope-pose__remove" disabled={saving} onClick={() => removeCorner(index)}>
                      {copy.removeCorner}
                    </button>
                  </div>
                  <div className="merope-pose__patches">
                    {c.patches.map((p, k) => (
                      <span key={k} className={`merope-pose__patch${index === selected && k === patchIndex ? ' is-on' : ''}`}>
                        <button type="button" disabled={saving} onClick={() => select(index, k)}>{patchName(c, p, k)}</button>
                        <button type="button" disabled={saving} aria-label={copy.removePatch} onClick={() => removePatch(index, k)}>×</button>
                      </span>
                    ))}
                  </div>
                </li>
              ))}
            </ul>
          )}
        </section>

        {correction ? (
          <section className="merope-pose__step">
            <SliderItem
              itemKey="pose-correction-transition"
              label={copy.transition}
              description={copy.transitionHint}
              value={transition}
              min={0}
              max={1}
              step={0.01}
              disabled={saving}
              formatValue={value => `${Math.round(value * 100)}%`}
              onChange={(value) => {
                setTransition(value)
                onDriver(poseCorrectionTransitionDriver(correction, driver, value))
              }}
            />
          </section>
        ) : null}

        <div className="merope-pose__footer">
          <button
            type="button"
            className="merope-pose__compare"
            disabled={saving}
            onPointerDown={() => setComparing(true)}
            onPointerUp={() => setComparing(false)}
            onPointerLeave={() => setComparing(false)}
            onKeyDown={(event) => { if (event.key === ' ' || event.key === 'Enter') setComparing(true) }}
            onKeyUp={() => setComparing(false)}
            onBlur={() => setComparing(false)}
          >
            {copy.holdCompare}
          </button>
          <span className="merope-pose__changes">
            {changes ? format(copy.unsaved, { n: changes }) : copy.clean}
          </span>
          <SettingsButton type="button" size="sm" disabled={saving || !changes} loading={saving} onClick={() => void save()}>
            {copy.save}
          </SettingsButton>
          <SettingsButton type="button" size="sm" variant="secondary" disabled={saving || !changes} onClick={discard}>
            {copy.discard}
          </SettingsButton>
        </div>
      </div>
    </SettingGroup>
  )
}

// CollapseRegion unmounts this effect when closed, returning the preview to
// saved geometry. Draft state stays in the editor, so collapsing loses no work.
function PoseCorrectionPreview({ characterRef, corrections }: {
  characterRef: Props['characterRef']
  corrections: PoseCorrection[] | null
}) {
  useEffect(() => {
    characterRef.current?.previewPoseCorrections(corrections)
  }, [characterRef, corrections])
  useEffect(() => {
    const character = characterRef.current
    return () => { character?.previewPoseCorrections(null) }
  }, [characterRef])
  return null
}

/** Zooms the stage onto the head while the editor is open; collapsing undoes it. */
function PoseCorrectionZoom({ characterRef, on }: { characterRef: Props['characterRef']; on: boolean }) {
  useEffect(() => {
    const character = characterRef.current
    character?.zoomPoseCorrectionHead(on)
    return () => { character?.zoomPoseCorrectionHead(false) }
  }, [characterRef, on])
  return null
}
