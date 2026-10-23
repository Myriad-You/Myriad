// The "generated" project the Playground shows: a small focus timer written for the demo.
const PAGE_HTML = `<div id="tapp-background"><div class="pf-bg"></div></div>
<div id="tapp-content">
  <main class="pf">
    <p id="pf-label" class="pf-label"></p>
    <div class="pf-ring">
      <svg viewBox="0 0 120 120"><circle class="pf-track" cx="60" cy="60" r="52"/><circle id="pf-arc" class="pf-arc" cx="60" cy="60" r="52"/></svg>
      <div id="pf-time" class="pf-time">25:00</div>
    </div>
    <div class="pf-dots" id="pf-dots"></div>
    <div class="pf-actions">
      <button id="pf-start" class="pf-btn pf-primary"></button>
      <button id="pf-reset" class="pf-btn"></button>
    </div>
  </main>
</div>`
const STYLES = `#tapp-background{position:absolute;inset:0;overflow:hidden}
.pf-bg{position:absolute;inset:0;background:radial-gradient(circle at 30% 20%,rgba(244,114,182,.18),transparent 55%),radial-gradient(circle at 75% 80%,rgba(139,92,246,.16),transparent 55%)}
#tapp-content{position:relative;height:100%;display:grid;place-items:center;color:var(--tapp-text,#1f1f1f)}
.pf{display:flex;flex-direction:column;align-items:center;gap:18px;padding:24px}
.pf-label{margin:0;font-size:15px;letter-spacing:.08em;opacity:.7}
.pf-ring{position:relative;width:220px;height:220px}
.pf-ring svg{width:100%;height:100%;transform:rotate(-90deg)}
.pf-track{fill:none;stroke:color-mix(in srgb,currentColor 10%,transparent);stroke-width:8}
.pf-arc{fill:none;stroke:#ec6fa5;stroke-width:8;stroke-linecap:round;stroke-dasharray:326.7;stroke-dashoffset:0;transition:stroke-dashoffset .4s}
.pf-time{position:absolute;inset:0;display:grid;place-items:center;font:600 46px/1 ui-rounded,system-ui,sans-serif;font-variant-numeric:tabular-nums}
.pf-dots{display:flex;gap:8px}.pf-dots span{width:10px;height:10px;border-radius:50%;background:color-mix(in srgb,currentColor 15%,transparent)}.pf-dots span.on{background:#ec6fa5}
.pf-actions{display:flex;gap:10px}
.pf-btn{border:0;border-radius:999px;padding:10px 22px;font-size:15px;cursor:pointer;background:color-mix(in srgb,currentColor 8%,transparent);color:inherit}
.pf-primary{background:#ec6fa5;color:#fff}
.dark #tapp-content{color:#f2f0f7}
.dark .pf-bg{background:radial-gradient(circle at 30% 20%,rgba(244,114,182,.22),transparent 55%),radial-gradient(circle at 75% 80%,rgba(139,92,246,.2),transparent 55%)}`
const CORE = `const copy = {
  'zh-CN': { label: '专注', start: '开始', pause: '暂停', reset: '重置' },
  'ja-JP': { label: '集中', start: 'スタート', pause: '一時停止', reset: 'リセット' },
  'en-US': { label: 'FOCUS', start: 'Start', pause: 'Pause', reset: 'Reset' },
};
let locale = 'en-US';
function t(key) { return (copy[locale] || copy['en-US'])[key]; }
function setLocale(next) { locale = copy[next] ? next : (String(next).startsWith('zh') ? 'zh-CN' : String(next).startsWith('ja') ? 'ja-JP' : 'en-US'); }
module.exports = { t, setLocale };
`
const PAGE = `const { t, setLocale } = require('../core.js');
const TOTAL = 25 * 60;
let left = TOTAL, timer = null, done = 1;
const $ = (id) => document.getElementById(id);
function render() {
  Tapp.dom.setText($('pf-time'), String(Math.floor(left / 60)).padStart(2, '0') + ':' + String(left % 60).padStart(2, '0'));
  $('pf-arc').style.strokeDashoffset = String(326.7 * (1 - left / TOTAL));
  Tapp.dom.setText($('pf-label'), t('label'));
  Tapp.dom.setText($('pf-start'), timer ? t('pause') : t('start'));
  Tapp.dom.setText($('pf-reset'), t('reset'));
  const dots = $('pf-dots'); dots.textContent = '';
  for (let i = 0; i < 4; i++) { const s = document.createElement('span'); if (i < done) s.className = 'on'; dots.appendChild(s); }
}
function toggle() {
  if (timer) { clearInterval(timer); timer = null; } else { timer = setInterval(() => { left = Math.max(0, left - 1); if (!left) { clearInterval(timer); timer = null; done = Math.min(4, done + 1); left = TOTAL; } render(); }, 1000); }
  render();
}
Tapp.lifecycle.onReady(async function () {
  setLocale(await Tapp.ui.getLocale());
  Tapp.ui.onLocaleChange((l) => { setLocale(l); render(); });
  $('pf-start').addEventListener('click', toggle);
  $('pf-reset').addEventListener('click', () => { if (timer) clearInterval(timer); timer = null; left = TOTAL; render(); });
  render();
});
`
export const PROMPT = {
  zh: '做一个番茄钟，25 分钟一轮，用圆环显示剩余时间',
  ja: 'ポモドーロタイマーを作って。25 分で 1 回、残り時間はリングで表示',
  en: 'Make a pomodoro timer: 25-minute rounds, a ring showing the time left',
}
export const EXPLANATION = {
  zh: '做了一个番茄钟：圆环随剩余时间收缩，下方圆点记录完成的轮数，可以开始、暂停和重置。界面跟随站点的语言和明暗主题。',
  ja: 'ポモドーロタイマーを作りました。残り時間に合わせてリングが縮み、下の点で終えた回数を数えます。スタート・一時停止・リセットに対応し、サイトの言語とテーマに合わせて表示します。',
  en: 'Built a pomodoro timer: the ring shrinks with the time left, dots below count finished rounds, and it can start, pause and reset. It follows the site language and light/dark theme.',
}
export function project() {
  return {
    manifest: {
      id: 'com.playground.focus-timer', name: 'Focus Timer', version: '0.1.0', minSystemVersion: '0.4.0',
      description: 'A pomodoro timer with a progress ring', category: 'productivity',
      core: { entry: 'core.js', styles: 'styles.css' }, page: { entry: 'page/index.js', template: 'page.html' },
      permissions: ['ui:theme'], themeColor: '#EC6FA5',
      iconSvg: "<svg viewBox='0 0 24 24' fill='none' xmlns='http://www.w3.org/2000/svg'><circle cx='12' cy='13' r='8' stroke='currentColor' stroke-width='2'/><path d='M12 9v4l2.5 2.5M9 3h6' stroke='currentColor' stroke-width='2' stroke-linecap='round'/></svg>",
    },
    code: { core: CORE, page: PAGE, styles: STYLES, pageHtml: PAGE_HTML },
  }
}
export function streamEvents(L) {
  const steps = [
    ['inspect_project', 'success', 'Started a new temporary Tapp workspace'],
    ['load_design_spec', 'success', 'Injected the Myriad UI design spec into the generation context'],
    ['plan_context', 'running', 'Planning documentation queries for the requested capabilities'],
    ['plan_context', 'success', 'Planned 3 documentation queries'],
    ['search_docs', 'success', 'Retrieved 6 bounded sections from the repository Tapp contract'],
    ['validate_project', 'success', 'Manifest, resources, permissions, HTML, and size checks passed on attempt 1'],
    ['checkpoint', 'success', 'Created a validated temporary project checkpoint; installation remains explicit'],
  ]
  const trace = steps.filter(s => s[1] !== 'running').map(([tool, status, summary]) => ({ tool, status, summary }))
  const ev = steps.map(([tool, status, summary], i) => ({ at: 500 + i * 900, data: { type: 'step', tool, status, summary } }))
  ev.push({ at: 500 + steps.length * 900 + 300, data: { type: 'done', response: {
    project: project(), explanation: EXPLANATION[L], warnings: [], modelTier: 'pro', agentTrace: trace, knowledgeSources: [],
    validation: { passed: true, attempts: 1, checks: ['production_manifest', 'resource_consistency', 'permission_usage', 'template_security', 'project_size'] },
  } } })
  return ev
}
