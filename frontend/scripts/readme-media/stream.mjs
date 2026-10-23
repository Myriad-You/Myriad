// Stand-ins for streamed answers: a fetch to `match` gets SSE events that arrive on the page's
// (virtual) clock, so text and steps appear over time instead of all at once.
export async function installStream(page, match, events) {
  await page.addInitScript(({ match, events }) => {
    const orig = window.fetch
    window.fetch = function (...args) {
      const url = typeof args[0] === 'string' ? args[0] : args[0] && args[0].url
      if (!url || !url.includes(match)) return orig.apply(this, args)
      const enc = new TextEncoder()
      const stream = new ReadableStream({
        start(ctrl) {
          let last = 0
          // The reader may cancel once it has what it needs; later writes are then dropped.
          for (const e of events) {
            last = Math.max(last, e.at)
            setTimeout(() => { try { ctrl.enqueue(enc.encode(`data: ${JSON.stringify(e.data)}\n\n`)) } catch {} }, e.at)
          }
          setTimeout(() => { try { ctrl.close() } catch {} }, last + 50)
        },
      })
      return Promise.resolve(new Response(stream, { status: 200, headers: { 'content-type': 'text/event-stream' } }))
    }
  }, { match, events })
}

/** Latin text streams a word at a time, CJK two characters at a time, roughly as a model's tokens would. */
export function chatTokens(reply) {
  if (/^[\x00-\x7F’“”—]*$/.test(reply)) return reply.match(/\S+\s*/g)
  const chars = Array.from(reply)
  const out = []
  for (let i = 0; i < chars.length; i += 2) out.push(chars.slice(i, i + 2).join(''))
  return out
}

const TOKEN_MS = 2000 / 9 // one token about every 0.22 s
const DELAY = 900 // before the first token

/** The agent's chat stream for one reply, in the events the panel reads. */
export function chatEvents(reply) {
  const sessionId = 'demo-session'
  const ev = [{ at: 200, data: { type: 'run_started', runId: 'demo-run', sessionId } }]
  let at = DELAY
  for (const token of chatTokens(reply)) { at += TOKEN_MS; ev.push({ at, data: { type: 'summary_token', token, done: false } }) }
  ev.push({ at: at + 300, data: { type: 'summary_token', token: '', done: true } })
  ev.push({ at: at + 300, data: { type: 'task_completed', taskId: '', success: true, response: { success: true, responseType: 'answer', message: reply, suggestions: [], sessionId } } })
  return ev
}
export const chatStreamMs = (reply) => DELAY + chatTokens(reply).length * TOKEN_MS + 300

/** Chat scene timing: the question is typed from TYPE_AT at 70 ms a character and sent half a second later. */
export const TYPE_AT = 3400
export const chatSendAt = (ask) => TYPE_AT + 70 * Array.from(ask).length + 500
/** When her first words arrive. */
export const chatReplyAt = (ask) => chatSendAt(ask) + DELAY
