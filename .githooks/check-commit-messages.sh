#!/bin/sh
# Capy Canvas commit message guard.
set -eu
export LC_ALL=C

# Match agent names and their attribution identities, without rejecting ordinary
# prose about an agent or human coauthors. Keep both hooks on this same rule.
agents='claude|anthropic|codex|chatgpt|gpt[0-9]*|openai|grok|x[.]ai|gemini|copilot|cursor|windsurf|codeium|devin|aider|cline|roo([[:blank:]_-]+code)?|deepseek|qwen|kimi|mistral|perplexity|jules|openhands|open-hands|swe-agent|amazon[[:blank:]_-]+q|codewhisperer|augment([[:blank:]_-]+code)?|tabnine|replit|goose|factory|droid|continue|bolt|lovable|v0|ai[[:blank:]_-]+(assistant|agent|bot)'
pattern="^[[:blank:]]*Co-Authored-By[[:blank:]]*:([[:blank:]]*|.*[^[:alnum:]])($agents)([^[:alnum:]]|$)"

reject() {
    echo 'error: AI agents must not appear in Co-Authored-By trailers.' >&2
    echo 'Remove the agent trailer before committing or pushing; human coauthors are allowed.' >&2
}

check_history() {
    # Inspect every ancestor, including merged branches and previously published
    # commits. Checking only remote..local would permit old attribution to return.
    matches=$(git log --format='%h %s' --extended-regexp --regexp-ignore-case \
        --grep="$pattern" "$1" --) || return 1
    if [ -n "$matches" ]; then
        reject
        printf '%s\n' "$matches" >&2
        return 1
    fi
}

case "${1-}" in
    message)
        # Distinguish no match from an unreadable message or a grep failure.
        status=0
        grep -Ei "$pattern" "$2" >&2 || status=$?
        case "$status" in
            0) reject; exit 1 ;;
            1) exit 0 ;;
            *) exit "$status" ;;
        esac
        ;;
    history)
        shift
        for ref in "$@"; do
            check_history "$ref" || exit 1
        done
        ;;
    push)
        while read -r local_ref local_oid remote_ref remote_oid; do
            # Deleted refs introduce no commits (also works with SHA-256 repos).
            case "$local_oid" in *[!0]*) ;; *) continue ;; esac
            if commit=$(git rev-parse --verify "$local_oid^{commit}" 2>/dev/null); then
                check_history "$commit" || exit 1
            else
                # Tags may point at trees or blobs, which have no commit message.
                # A missing object is an error, not a reason to allow the push.
                git cat-file -e "$local_oid" || exit 1
            fi
        done
        ;;
    *) echo 'usage: check-commit-messages.sh message FILE | history REF... | push' >&2; exit 2 ;;
esac
