import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

// Widgets render on the home grid / control panel, outside the /journal and
// /tapp routes that wrap pages in I18nNamespace. The *exported* widget must
// be the withI18nNamespace wrapper; wrapping a sibling in the same file
// still leaves t.<namespace> undefined on the home path.
const NAMESPACED_WIDGETS: Array<{
  file: string
  namespace: string
  exportName: string
}> = [
  { file: 'TappWidget.tsx', namespace: 'tapp', exportName: 'default' },
  { file: 'MeropeWidget.tsx', namespace: 'merope', exportName: 'MeropeWidget' },
  {
    file: '../phantasi/tiles/PhantasiFeaturedTile.tsx',
    namespace: 'phantasi',
    exportName: 'PhantasiFeaturedWidget',
  },
]

describe('widget i18n namespace boundaries', () => {
  for (const { file, namespace, exportName } of NAMESPACED_WIDGETS) {
    it(`${file} exports ${exportName} through withI18nNamespace(['${namespace}'])`, () => {
      const source = readFileSync(new URL(file, import.meta.url), 'utf8')
      const exported =
        exportName === 'default'
          ? `export default withI18nNamespace\\(\\s*\\[\\s*'${namespace}'\\s*\\]`
          : `export const ${exportName} = withI18nNamespace\\(\\s*\\[\\s*'${namespace}'\\s*\\]`
      assert.match(source, new RegExp(exported))
    })
  }
})
