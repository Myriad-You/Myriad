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
    main: 'main.js',
    author: { name: options.author || 'Tapp Developer' },
    permissions: [
      ...(hasPage ? ['ui:notification'] : []),
      ...(hasWidget ? ['widget:register'] : []),
    ],
    ...(hasPage ? { hasPage: true, pageTemplate: 'page.html' } : {}),
    ...(hasWidget ? { widgets: [{ id: 'starter', name: 'Starter Widget', defaultSize: '2x2', sizes: ['2x2', '4x2'], templates: { '2x2': 'templates/widget-2x2.html', '4x2': 'templates/widget-4x2.html' } }] } : {}),
    styles: 'styles.css',
    cssMode: 'unified',
  }
  const page = `// ========== Page Code ==========
Tapp.lifecycle.onReady(async function () {
  var root = document.getElementById('tapp-root');
  root.querySelector('[data-action="notify"]').addEventListener('click', function () {
    Tapp.ui.showNotification({ title: 'My Tapp', message: 'The Page is running.', type: 'success' });
  });
});
`
  const widget = `// ========== Widget Code ==========
Tapp.widgets['starter'] = { render: function (container, props) { container.innerHTML = '<section class="widget"><strong>My Tapp</strong><span>' + props.size + '</span></section>'; } };
`
  return { type, hasPage, hasWidget, manifest, source: `${hasWidget ? widget : ''}${hasWidget && hasPage ? '\n' : ''}${hasPage ? page : ''}` }
}
