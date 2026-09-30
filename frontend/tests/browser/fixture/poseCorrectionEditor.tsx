import type { PoseCorrection } from '../../../src/features/merope/anime25drig/poseCorrections'
import type { Anime25DPlayback } from '../../../src/features/merope/anime25drig/types'
import type { RigCharacterHandle } from '../../../src/features/merope/rig/RigCharacter'
import { createRoot } from 'react-dom/client'
import { I18nNamespace, I18nProvider } from '../../../src/contexts/I18nContext'
import { WORKBENCH_DRIVER } from '../../../src/features/merope/anime25drig/driver'
import { PoseCorrectionEditor } from '../../../src/features/merope/workbench/PoseCorrectionEditor'
import { saveLocale } from '../../../src/i18n'
import { loadLocale } from '../../../src/i18n/loadLocale'

export async function mountPoseEditor() {
  saveLocale('en-US')
  await loadLocale('en-US')
  const calls: (PoseCorrection[] | null)[] = []
  const saves: PoseCorrection[][] = []
  const root = createRoot(document.getElementById('root')!)
  const playback = {
    anchors: { face: { y1: 450 }, eyeL: { icx: 210, closeY: 260 }, mouth: { cx: 300, cy: 380 } },
    layers: [{ role: 'face' }],
    shellProfile: { head: { centerX: 300, centerY: 250, radiusX: 180, radiusY: 240 } },
  } as Anime25DPlayback
  const characterRef = { current: {
    previewPoseCorrections: (value: PoseCorrection[] | null) => calls.push(structuredClone(value)),
  } as unknown as RigCharacterHandle }
  const render = (key: string) => root.render(
<I18nProvider><I18nNamespace names={['merope']}><PoseCorrectionEditor
  key={key}
  playback={playback}
  characterRef={characterRef}
  driver={{ ...WORKBENCH_DRIVER, angleX: 0.8, angleY: -0.6 }}
  onDriver={() => {}}
  onSave={async value => { saves.push(structuredClone(value)) }}
                                                /></I18nNamespace></I18nProvider>,
)
  render('first-outfit')
  return { calls, saves, render, unmount: () => root.unmount() }
}
