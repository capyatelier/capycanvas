#!/usr/bin/env sh
# Use the same startup-safe GTK for every packaged launch, including file opens.
set -eu
capy_bin=$(CDPATH= cd -- "$(dirname -- "$(readlink -f -- "$0")")" && pwd)
capy_gtk="$capy_bin/../lib/capycanvas/gtk"
test -r "$capy_gtk/libgtk-4.so.1" || { echo 'Missing packaged GTK runtime' >&2; exit 1; }
export LD_LIBRARY_PATH="$capy_gtk${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$capy_bin/capycanvas-bin" "$@"
