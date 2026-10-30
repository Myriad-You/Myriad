/** #669：提示要挂在 body 上，不能关在 #app-root 自成的那一层里。 */

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { it } from 'node:test'

const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8')

it('#app-root 带着 view-transition-name，自成一层；提示因此要挂到 body 上', () => {
  // 这条一旦拿掉，挂在 body 上就不再是必须的；留着就说明 #app-root 里的 z-index 出不了这一层。
  assert.match(read('../styles/site-transition.css'), /#app-root\s*\{\s*view-transition-name:/)
  const container = read('./ToastContainer.tsx')
  assert.match(container, /createPortal\(\s*<div className="toast-container-wrapper">/)
  assert.match(container, /document\.body,\s*\)/)
})

it('提示压得住挂在 body 上的弹层（创作台写作器）', () => {
  const toastZ = Number(/\.toast-container-wrapper\s*\{[^}]*z-index:\s*(\d+)/.exec(read('./Toast.css'))?.[1])
  const noteZ = Number(/\.phantasi-note\s*\{[^}]*z-index:\s*(\d+)/.exec(read('./phantasi/ui/css/note.css'))?.[1])
  assert.ok(toastZ > noteZ, `toast ${toastZ} should be above the note editor ${noteZ}`)
})
