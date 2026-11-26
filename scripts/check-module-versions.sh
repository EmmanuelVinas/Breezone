#!/usr/bin/env bash
# Fails when a module changed without a higher version. A server only replaces a module by a
# higher version (the library offers no update, the image does not install it at start), so a
# change kept under the same number never reaches the servers that already have that version.
#
# Compared with the last Breezone release (its image ships the modules) and with the tag of the
# module's version (`module-<id>-v<version>`, what the library publishes). Needs the tags
# (`fetch-depth: 0` in CI). Its README and other Markdown files may change freely.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

release=$(git describe --tags --abbrev=0 --match 'v*' HEAD 2>/dev/null || true)
failed=0
for manifest in modules/*/manifest.json; do
  dir=$(dirname "$manifest")
  id=$(basename "$dir")
  version=$(jq -r .version "$manifest")
  refs=()
  [[ -n "$release" ]] && refs+=("$release")
  git rev-parse -q --verify "refs/tags/module-$id-v$version" >/dev/null && refs+=("module-$id-v$version")
  for ref in "${refs[@]}"; do
    was=$(git show "$ref:$manifest" 2>/dev/null | jq -r .version 2>/dev/null || true)
    [[ "$was" == "$version" ]] || continue
    # Against the working tree: the same in CI, and it also catches what is not committed yet.
    if ! git diff --quiet "$ref" -- "$dir" ":(exclude)$dir/*.md"; then
      echo "::error file=$manifest::$id changed since $ref, which already has $version: raise its version (manifest.json and Cargo.toml)"
      git diff --stat "$ref" -- "$dir" ":(exclude)$dir/*.md" | sed 's/^/    /'
      failed=1
    fi
  done
done
exit "$failed"
