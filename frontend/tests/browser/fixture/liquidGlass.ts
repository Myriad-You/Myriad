import { mountSurfaceLenses } from '../../../src/utils/liquidGlass/surfaceLenses'
import { createHyalite } from '../../../src/utils/liquidGlass/vendor/hyalite'
import '../../../src/styles/theme.css'
import '../../../src/styles/performance.css'
import '../../../src/components/GlobalControlPanel.css'

const engine = createHyalite()
let builds = 0
const originalAttach = engine.attach.bind(engine)
engine.attach = (element, options) => originalAttach(element, {
  ...options, onBuild: () => { builds++ },
})
let stop = mountSurfaceLenses(engine)
const api = {
  engine,
  builds: () => builds,
  stop: () => stop(),
  start: () => { stop(); stop = mountSurfaceLenses(engine) },
}
declare global {
  interface Window { __liquid: typeof api }
}
window.__liquid = api
