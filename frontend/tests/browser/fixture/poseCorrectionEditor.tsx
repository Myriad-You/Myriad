import type { Anime25DPlaybackV7, PoseCorrection } from '@myriad/figure/anime25drig'
import type { RigCharacterHandle } from '../../../src/features/merope/character/RigCharacter'
import type { StoredPoseCorrections } from '../../../src/features/merope/workbench/poseCorrectionPackage'
import { WORKBENCH_DRIVER } from '@myriad/figure/workbench'
import { createRoot } from 'react-dom/client'
import { I18nNamespace, I18nProvider } from '../../../src/contexts/I18nContext'
import { PoseCorrectionEditor } from '../../../src/features/merope/workbench/PoseCorrectionEditor'
import { saveLocale } from '../../../src/i18n'
import { loadLocale } from '../../../src/i18n/loadLocale'

/** The editor on a stored package of version 7, holding `stored` (in that version's axes) when given. */
export async function mountPoseEditor(stored?: unknown[]) {
  saveLocale('en-US')
  await loadLocale('en-US')
  const calls: (PoseCorrection[] | null)[] = []
  const saves: StoredPoseCorrections[] = []
  const root = createRoot(document.getElementById('root')!)
  const playback = {
    kind: 'anime-2.5d-rig',
    version: 7,
    engine: 'Anime2.5DRig',
    anchors: { face: { y1: 450 }, eyeL: { icx: 210, closeY: 260 }, mouth: { cx: 300, cy: 380 } },
    layers: [{ role: 'face' }],
    pixelCanvas: { width: 600, height: 800 },
    shellProfile: {
      head: { centerX: 300, centerY: 250, radiusX: 180, radiusY: 240 },
      ...(stored ? { poseCorrections: stored } : {}),
    },
  } as unknown as Anime25DPlaybackV7
  const characterRef = { current: {
    previewPoseCorrections: (value: PoseCorrection[] | null) => calls.push(structuredClone(value)),
    pickPoseCorrectionPoint: () => null,
    projectPoseCorrectionPatch: () => null,
    poseCorrectionDelta: () => null,
    poseCorrectionStageRect: () => null,
    zoomPoseCorrectionHead: () => {},
  } as unknown as RigCharacterHandle }
  const render = (key: string) => root.render(
<I18nProvider><I18nNamespace names={['merope']}><PoseCorrectionEditor
  key={key}
  playback={playback}
  characterRef={characterRef}
  driver={{ ...WORKBENCH_DRIVER, headYaw: 0.8, headPitch: -0.6 }}
  target="the worn bust"
  onDriver={() => {}}
  onSave={async value => { saves.push(structuredClone(value)) }}
                                                /></I18nNamespace></I18nProvider>,
)
  render('first-outfit')
  return { calls, saves, render, unmount: () => root.unmount() }
}
