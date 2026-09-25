#!/bin/sh
set -eu

source_dir=$(CDPATH= cd -- "$(dirname -- "$0")/../../.githooks" && pwd)
if [ -n "$(git config --get core.hooksPath || true)" ]; then
    echo 'error: core.hooksPath is already configured; integrate the .githooks guards there.' >&2
    exit 1
fi
hooks_dir=$(git rev-parse --git-path hooks)
mkdir -p "$hooks_dir"

# Do not replace unrelated hooks. Installed copies work in every linked worktree,
# including branches that predate these tracked files.
for hook in commit-msg pre-push check-commit-messages.sh; do
    if [ -e "$hooks_dir/$hook" ] &&
        ! grep -Fxq '# Capy Canvas commit message guard.' "$hooks_dir/$hook"; then
        echo "error: existing $hooks_dir/$hook must be integrated manually." >&2
        exit 1
    fi
done
for hook in commit-msg pre-push check-commit-messages.sh; do
    cp "$source_dir/$hook" "$hooks_dir/$hook"
    chmod +x "$hooks_dir/$hook"
done
printf 'Installed commit and push guards in %s\n' "$hooks_dir"
