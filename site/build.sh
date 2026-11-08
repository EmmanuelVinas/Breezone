#!/usr/bin/env sh
# Compiles the site's Tailwind styles: site/src/site.css -> site/assets/site.css.
# Run before previewing the site locally; the Pages workflow runs it on every deployment.
set -eu
cd "$(dirname "$0")"
[ -d node_modules ] || npm ci --silent
npm run -s build
