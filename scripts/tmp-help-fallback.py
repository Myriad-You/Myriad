from pathlib import Path
p = Path('/Users/hitomi/GitHub/Myriad/frontend/src/components/settings/SettingTitleHelp.css')
t = p.read_text()
repls = [
    ('var(--cfg-text-3)', 'var(--cfg-text-3, var(--text-secondary, rgb(60 60 67 / 48%)))'),
    ('var(--cfg-text-1)', 'var(--cfg-text-1, var(--text-primary, #1d1d1f))'),
    ('var(--cfg-text-2)', 'var(--cfg-text-2, var(--text-secondary, rgb(60 60 67 / 72%)))'),
    ('var(--cfg-accent)', 'var(--cfg-accent, var(--color-primary, #8b5cf6))'),
    ('var(--cfg-frost-border)', 'var(--cfg-frost-border, var(--chrome-stroke, rgb(255 255 255 / 50%)))'),
    ('var(--cfg-frost-elevated-bg)', 'var(--cfg-frost-elevated-bg, var(--chrome-elevated, rgb(255 255 255 / 96%)))'),
    ('var(--cfg-frost-filter)', 'var(--cfg-frost-filter, blur(20px) saturate(180%))'),
    ('var(--cfg-subtle-bg)', 'var(--cfg-subtle-bg, var(--chrome-inner, rgb(255 255 255 / 42%)))'),
    ('var(--sm-dur-fast)', 'var(--sm-dur-fast, 140ms)'),
    ('var(--sm-ease-standard)', 'var(--sm-ease-standard, ease)'),
]
for old, new in repls:
    t = t.replace(old, new)
p.write_text(t)
print('fallbacks', t.count('var(--cfg-text-3, var(--text-secondary'))
