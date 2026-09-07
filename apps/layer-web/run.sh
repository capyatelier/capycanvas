#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "$0")/../.."

bash apps/layer-web/build.sh
exec python3 -m http.server "${LAYER_WEB_PORT:-4173}" --bind 127.0.0.1 --directory apps/layer-web
