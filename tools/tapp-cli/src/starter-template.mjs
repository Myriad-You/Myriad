import { basename, resolve } from 'node:path'

function starterId(directory) {
  const slug = basename(resolve(directory))
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, '-')
    .replace(/^[^a-z0-9]+/, '')
    .replace(/[^a-z0-9]+$/, '')
  return `com.example.${slug || 'my-tapp'}`
}

function starterName(directory) {
  return basename(resolve(directory))
    .split(/[-_.\s]+/)
    .filter(Boolean)
    .map((part) => part[0].toUpperCase() + part.slice(1))
    .join(' ') || 'My Tapp'
}

export function createStarterTemplate(directory, options = {}) {
  const type = options.type || 'page'
  if (!['page', 'widget', 'both'].includes(type)) {
    throw new Error('type must be page, widget, or both')
  }
  const hasPage = type === 'page' || type === 'both'
  const hasWidget = type === 'widget' || type === 'both'
  const manifest = {
    id: options.id || starterId(directory),
    name: options.name || starterName(directory),
    version: '0.1.0',
    description: 'A Myriad Tapp',
    category: 'utility',
    core: { entry: 'core.js', styles: 'styles.css' },
    author: { name: options.author || 'Tapp Developer' },
    permissions: [
      ...(hasPage ? ['ui:notification'] : []),
      ...(hasWidget ? ['widget:register'] : []),
    ],
    ...(hasPage
      ? { page: { entry: 'page/index.js', template: 'page.html' } }
      : {}),
    ...(hasWidget ? { widgets: [{ id: 'starter', name: 'Starter Widget', defaultSize: '2x2', sizes: ['2x2', '4x2'], entry: 'widget/index.js', templates: { '2x2': 'templates/starter-2x2.html', '4x2': 'templates/starter-4x2.html' } }] } : {}),
  }
  // core 是共享层：三种模式都先执行它，层入口 require 它拿导出。
  const core = `module.exports = {
  appName: 'My Tapp',
};
`
  const page = `var core = require('../core.js');

Tapp.lifecycle.onReady(async function () {
  var root = document.getElementById('tapp-root');
  root.querySelector('[data-action="notify"]').addEventListener('click', function () {
    Tapp.ui.showNotification({ title: core.appName, message: 'The Page is running.', type: 'success' });
  });
});
`
  const widget = `var core = require('../core.js');

Tapp.widgets['starter'] = { render: function (container, props) { container.innerHTML = '<section class="widget"><strong>' + core.appName + '</strong><span>' + props.size + '</span></section>'; } };
`
  return {
    type,
    hasPage,
    hasWidget,
    manifest,
    modules: {
      'core.js': core,
      ...(hasPage ? { 'page/index.js': page } : {}),
      ...(hasWidget ? { 'widget/index.js': widget } : {}),
    },
  }
}
