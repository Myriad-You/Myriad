#!/usr/bin/env node
// Sync aro.ts template from the modular runtime package.
const fs = require('fs');
const path = require('path');

const root = path.resolve(__dirname, '../..');
const runtimeDir = path.join(root, 'backend/data/tapps/1/com.myriad.aro');
const templatePath = path.join(root, 'frontend/src/tapp/examples/tapps/aro.ts');
const manifestJsonPath = path.join(runtimeDir, 'manifest.json');

const html = fs.readFileSync(path.join(runtimeDir, 'page.html'), 'utf8');
const css = fs.readFileSync(path.join(runtimeDir, 'styles.css'), 'utf8');
const manifestJson = JSON.parse(fs.readFileSync(manifestJsonPath, 'utf8'));
const existing = fs.readFileSync(templatePath, 'utf8');

const headerEnd = existing.indexOf('const PAGE_HTML');
const manifestStart = existing.indexOf('// ==================== Manifest ====================');

if (headerEnd < 0 || manifestStart < 0) {
  throw new Error('Unexpected aro.ts structure');
}

const header = existing.substring(0, headerEnd);
let footer = existing.substring(manifestStart);

function escTmpl(s) {
  return s.replace(/\\/g, '\\\\').replace(/`/g, '\\`').replace(/\$\{/g, '\\${');
}

function constNameForModule(name) {
  return 'PAGE_MOD_' + name.replace(/\.js$/, '').replace(/[^a-zA-Z0-9]+/g, '_').toUpperCase();
}

function tmplConst(name, value) {
  return `const ${name} = \`\\\n${escTmpl(value)}\`\n\n`;
}

function readI18n() {
  const dir = path.join(runtimeDir, 'i18n');
  const out = {};
  for (const file of fs.readdirSync(dir).sort()) {
    if (!file.endsWith('.json')) continue;
    const key = path.basename(file, '.json');
    out[key] = JSON.parse(fs.readFileSync(path.join(dir, file), 'utf8'));
  }
  return out;
}

function readPageModules(order) {
  const dir = path.join(runtimeDir, 'page');
  const names = order && order.length
    ? order
    : fs.readdirSync(dir).filter((file) => file.endsWith('.js')).sort();
  return names.map((name) => ({
    name,
    constName: constNameForModule(name),
    source: fs.readFileSync(path.join(dir, name), 'utf8'),
  }));
}

const i18n = readI18n();
const modules = readPageModules(manifestJson.pageModules || []);

const moduleConsts = modules
  .map((mod) => tmplConst(mod.constName, mod.source))
  .join('');

const pageModuleMap = `const PAGE_MODULES: Record<string, string> = {\n${modules
  .map((mod) => `  '${mod.name}': ${mod.constName},`)
  .join('\n')}\n}\n\n`;

const monolithModules = modules
  .filter((mod) => mod.name !== 'i18n.js')
  .map((mod) => `    ${mod.constName},`)
  .join('\n');

// Prefer template literals so eslint prefer-template stays clean on regenerations.
const buildCoreCode = `// ==================== Generated Monolith (from modules + inline LANG) ====================
function buildCoreCode(): string {
  const inlineLang = [
    '  // ==================== i18n ====================',
    \`  var LANG = \${
      JSON.stringify(ARO_I18N, null, 2)
        .split('\\n')
        .map((l, i) => (i === 0 ? l : \`  \${l}\`))
        .join('\\n')
      };\`,
    '',
    '  var lang = LANG.zh;',
    "  var currentLocale = 'zh';",
    '',
    '  function setLocale(locale) {',
    "    currentLocale = locale || 'zh';",
    "    var key = currentLocale.startsWith('zh') ? 'zh' : currentLocale.startsWith('ja') ? 'ja' : 'en';",
    '    lang = LANG[key] || LANG.en;',
    '  }',
  ].join('\\n')

  const otherModules = [
${monolithModules}
  ]
    .map((m) =>
      m
        .split('\\n')
        .map((l) => (l ? \`  \${l}\` : l))
        .join('\\n'),
    )
    .join('\\n\\n')

  return [
    '(function () {',
    "  'use strict';",
    '',
    inlineLang,
    '',
    otherModules,
    '})();',
  ].join('\\n')
}

const CORE_CODE = buildCoreCode()

`;

/** JSON.stringify has no trailing commas; eslint style/comma-dangle requires them in TS. */
function jsonWithTrailingCommas(value) {
  return JSON.stringify(value, null, 2)
    .replace(/(["\w.\]}])(\n\s*[}\]])/g, '$1,$2');
}

const permsStr = (manifestJson.permissions || []).map((p) => `    '${p}'`).join(',\n');
let settingsStr = '';
if (manifestJson.settings && manifestJson.settings.length > 0) {
  settingsStr = manifestJson.settings
    .map((s) => {
      const parts = [`key: '${s.key}'`, `type: '${s.type}'`];
      if (s.defaultValue !== undefined && s.defaultValue !== null) {
        parts.push(`defaultValue: ${JSON.stringify(s.defaultValue)}`);
      }
      if (s.label) parts.push(`label: '${s.label}'`);
      if (s.min !== undefined && s.min !== null) parts.push(`min: ${s.min}`);
      if (s.max !== undefined && s.max !== null) parts.push(`max: ${s.max}`);
      if (s.step !== undefined && s.step !== null) parts.push(`step: ${s.step}`);
      return `    { ${parts.join(', ')} }`;
    })
    .join(',\n');
}

footer = footer.replace(/version:\s*'[^']*'/, `version: '${manifestJson.version || '1.0.0'}'`);
footer = footer.replace(
  /permissions:\s*\[[\s\S]*?\]/,
  `permissions: [\n${permsStr},\n  ]`,
);
if (settingsStr && footer.includes('settings:')) {
  footer = footer.replace(
    /settings:\s*\[[\s\S]*?\]/,
    `settings: [\n${settingsStr},\n  ]`,
  );
}
footer = footer.replace(
  /pageModules: PAGE_MODULES,\n(?!\s*pageModuleOrder)/,
  'pageModules: PAGE_MODULES,\n  pageModuleOrder: manifest.pageModules,\n',
);

const output = header
  + tmplConst('PAGE_HTML', html)
  + tmplConst('STYLES', css)
  + `const ARO_I18N: Record<string, Record<string, string>> = ${jsonWithTrailingCommas(i18n)}\n\n`
  + '// ==================== Page Modules ====================\n'
  + moduleConsts
  + pageModuleMap
  + buildCoreCode
  + footer;

fs.writeFileSync(templatePath, output, 'utf8');
console.log('aro.ts regenerated. Lines:', output.split('\n').length);
