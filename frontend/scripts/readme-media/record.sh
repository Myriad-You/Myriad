#!/bin/zsh
# Records every scene in every language and theme, then encodes them into docs/assets/readme.
# See README.md in this directory for the instance it needs.
set -e
cd ${0:a:h}
LOGS=$(node -e "import('./render.mjs').then((m) => console.log(m.WORK))")/logs
mkdir -p $LOGS
for scene in ${${=SCENES:-home chat face report library tapp}:#blog}; do for L in ${=LANGS:-zh ja en}; do for S in light dark; do echo "$scene $L $S"; done; done; done | \
  xargs -P ${PAR:-2} -L 1 zsh -c 'node scenes/$0.mjs $1 $2 > '$LOGS'/$0-$1-$2.log 2>&1 && echo "ok $0 $1 $2" || echo "FAIL $0 $1 $2 (see '$LOGS'/$0-$1-$2.log)"'
# The blog shares one instance, so its languages run one after another.
if [[ -z $SCENES || $SCENES == *blog* ]]; then
  for L in ${=LANGS:-zh ja en}; do
    node blog-prep.mjs $L && node scenes/blog.mjs $L light > $LOGS/blog-$L-light.log 2>&1 && echo "ok blog $L" || echo "FAIL blog $L"
  done
fi
node encode.mjs ../../../docs/assets/readme
