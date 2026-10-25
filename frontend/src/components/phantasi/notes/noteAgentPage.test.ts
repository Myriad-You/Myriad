import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { it } from 'node:test'
import { NOTE_AGENT_SELECTION_MAX, noteAgentPageContent, noteAgentSelection } from './noteAgentPage'

const doc = { docId: 12, title: 'Morning desk', topic: 'Essays', status: 'draft' }
const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8')

it('points the global Agent at the cloud document, never at a content snapshot', () => {
  const page = noteAgentPageContent(doc)
  assert.equal(page.title, 'Morning desk')
  assert.equal(page.content, undefined)
  assert.deepEqual(page.metadata, { surface: 'note_editor', noteDocId: 12, topic: 'Essays', status: 'draft' })
})

it('says when the note is not in the cloud yet', () => {
  const page = noteAgentPageContent({ ...doc, docId: null, title: '  ' })
  assert.equal(page.metadata?.noteDocId, null)
  assert.equal(page.title, undefined)
  assert.match(page.summary ?? '', /not been saved/)
})

it('hands over a bounded source selection and nothing for a caret', () => {
  assert.equal(noteAgentSelection('Original text', 3, 3), null)
  assert.equal(noteAgentSelection('Original text', 0, 8), 'Original')
  const long = 'x'.repeat(NOTE_AGENT_SELECTION_MAX + 10)
  assert.equal(noteAgentSelection(long, 0, long.length)?.length, NOTE_AGENT_SELECTION_MAX)
})

it('keeps the composer resident: folding returns to it, never to nothing', () => {
  const panel = read('../../agent-panel/AgentPanel.tsx')
  // Only the site's panel ever closes; resident folding lands on the composer.
  assert.equal(panel.match(/dispatch\(\{ type: 'close' \}\)/g)?.length, 1)
  assert.match(panel, /if \(!resident\) \{\s*dispatch\(\{ type: 'close' \}\)/)
  assert.match(panel, /rawDispatch\(\{ type: 'open', stage: 'overlay' \}\)/)
  assert.match(panel, /resident \? null : <AgentPanelAurora/)
})

it('typing into the Agent is not editing the article', () => {
  assert.match(read('./useNoteEditorFormat.ts'), /e\.target\.closest\('\.phantasi-note__agent'\)\) return/)
  // Rendered beside the editor's root, so its input never reaches onInputCapture={markEditing}.
  assert.match(read('./NoteEditorView.tsx'), /<\/motion\.div>\s*<NoteResidentAgent dock=\{agentDock\} \/>/)
})

it('the resident composer drops everything that is not about editing this note', () => {
  const composer = read('../../agent-panel/AgentPanelComposer.tsx')
  assert.match(composer, /\{resident \? null : <ModeTag mode=\{mode\} \/>\}/)
  assert.equal(composer.match(/open=\{shortcuts && context\.kind/g)?.length, 4)
  assert.match(composer, /items=\{shortcuts \? favorites : \[\]\}/)
  assert.match(composer, /canMute && !resident \?/)
  const panel = read('../../agent-panel/AgentPanel.tsx')
  assert.match(panel, /if \(!showsComposer \|\| resident\) return/)
})

it('each opened note is a fresh Work conversation with no session switcher', () => {
  const resident = read('./NoteResidentAgent.tsx')
  assert.match(resident, /setAgentPanelMode\('work'\)\s*dispatchAgentPanelCommand\('new-session'\)/)
  // After the engine, so the engine is already listening for the command.
  assert.match(resident, /<AgentEngine \/>\s*<AgentPanel resident \/>\s*<FreshWorkConversation \/>/)
  const panel = read('../../agent-panel/AgentPanel.tsx')
  const residentTrailing = panel.slice(panel.indexOf('trailing={resident ? ('), panel.indexOf(') : (', panel.indexOf('trailing={resident ? (')))
  assert.ok(residentTrailing.length > 0)
  assert.doesNotMatch(residentTrailing, /AgentPanelSessionChrome/)
})

it('no "talked before" pager inside the editor', () => {
  assert.match(read('../../agent-panel/AgentPanelFull.tsx'), /mode === 'work' && sessionId && !resident \?/)
  assert.match(read('../../agent-panel/AgentPanel.tsx'), /<AgentPanelFull\s+resident=\{resident\}/)
})

it('a toggle folds the conversation away and opens it again, only once there is one', () => {
  const panel = read('../../agent-panel/AgentPanel.tsx')
  assert.match(panel, /trailing=\{resident \? \(\s*<AgentPresence open=\{messageCount > 0\}/)
  assert.match(panel, /showsFull && !folding\s*\? collapse\(\)\s*: dispatch\(\{ type: 'open', stage: 'full' \}\)/)
  assert.match(panel, /aria-expanded=\{showsFull && !folding\}/)
})

it('folding plays the close-out for the cards only, then settles on the composer', () => {
  const panel = read('../../agent-panel/AgentPanel.tsx')
  assert.match(panel, /data-phase=\{folding \? 'closing' : stage\.phase\}/)
  assert.match(panel, /agentPanelSettleTimeoutMs\('full', messageCountRef\.current\)/)
  // Reopening mid-fold cancels it rather than letting the timer fold it again.
  assert.match(panel, /if \(action\.type === 'open' && action\.stage === 'full'\) cancelFold\(\)/)
  assert.match(panel, /prefers-reduced-motion: reduce/)
  const css = read('../ui/css/note.css')
  assert.match(css, /\[data-resident='true'\]\[data-phase='closing'\] \{\s*opacity: 1;\s*translate: -50% 0;\s*scale: 1;\s*pointer-events: auto;/)
})
