#!/bin/sh
# Cargo is one consumer-selected adapter. Locate its actual output via Cargo JSON.
set -eu
case "$APPCOAT_PLATFORM" in macos|linux) ;; *) echo 'Use the separate Scarlet example for cross builds' >&2; exit 1;; esac
mkdir -p build
cargo build --manifest-path ../../Cargo.toml --locked -p appcoat_resource_reader \
  --message-format=json-render-diagnostics >build/cargo.json
python3 - <<'PY'
import json
import shutil
from pathlib import Path
paths = []
for line in Path('build/cargo.json').read_text().splitlines():
    item = json.loads(line)
    target = item.get('target', {})
    if item.get('reason') == 'compiler-artifact' and target.get('name') == 'appcoat_resource_reader' and 'bin' in target.get('kind', []) and item.get('executable'):
        paths.append(item['executable'])
if len(paths) != 1:
    raise SystemExit('Cargo must report exactly one resource-reader executable')
shutil.copy2(paths[0], 'build/resource-reader')
PY
