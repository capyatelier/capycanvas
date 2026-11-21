#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$root"
repository=capyatelier/capycanvas
url=https://flatpak.capycanvas.art
large_files=capycanvas-downloads/flatpak
large_files_url=https://downloads.capycanvas.art/flatpak
config=packaging/flatpak/cloudflare/wrangler.jsonc
allow_downloads=${CAPYCANVAS_WEB:?Set CAPYCANVAS_WEB to a capycanvas-web checkout}/site/scripts/allow-downloads.mjs
wrangler=(npx --yes wrangler@4.160.0)
latest=$(gh api "repos/$repository/releases/latest" --jq .tag_name)
tag=${1:-$latest}
if [[ $tag != "$latest" ]]; then
    echo "Only the latest stable release ($latest) can update Flatpak, not $tag" >&2
    exit 1
fi
work=$root/target/flatpak-publish
rm -rf "$work"
mkdir -p "$work/downloads" "$work/assets"
gh release download "$tag" --repo "$repository" --dir "$work/downloads" \
    --pattern '*-flatpak-repository.tar.zst' --pattern SHA256SUMS
(cd "$work/downloads" && grep -- '-flatpak-repository\.tar\.zst$' SHA256SUMS | sha256sum --check)
tar --zstd -xf "$work"/downloads/*-flatpak-repository.tar.zst --strip-components=1 -C "$work/assets"
gpg --batch --yes --dearmor --output "$work/release-key.gpg" packaging/flatpak/release-key.asc
python3 packaging/flatpak/flatpakref.py "$work/release-key.gpg" "$url" "$work/assets/capycanvas.flatpakref"
printf '%s\n' /capycanvas.flatpakref '  Content-Type: application/vnd.flatpak.ref' > "$work/assets/_headers"
while IFS= read -r -d '' file; do
    path=${file#"$work/assets/"}
    if [[ $path != objects/* && $path != deltas/* ]]; then
        echo "Only content-addressed repository files can be served from R2: $path" >&2
        exit 1
    fi
    "${wrangler[@]}" r2 object put "$large_files/$path" --remote --file "$file" \
        --content-type application/octet-stream --cache-control 'public, max-age=31536000, immutable' --config "$config"
    printf '/%s %s/%s 302\n' "$path" "$large_files_url" "$path" >> "$work/assets/_redirects"
    rm "$file"
done < <(find "$work/assets" -type f -size +25M -print0)
node "$allow_downloads"
"${wrangler[@]}" deploy --config "$config"
