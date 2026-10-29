#!/usr/bin/env bash
# scripts/update-mp3rgui-pot.sh
#
# Refresh mp3rgui/locales/mp3rgui.pot from the tr! strings in the GUI.
# xtr has to be the same release as the tr crate (0.1.11):
#   cargo install xtr --version 0.1.11
#
# Translators then update their .po with Poedit (Update from POT File)
# or msgmerge. An empty msgstr keeps showing the English text.

set -euo pipefail

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
out="$root/mp3rgui/locales/mp3rgui.pot"

if ! command -v xtr >/dev/null 2>&1; then
    echo "xtr 0.1.11 is required: cargo install xtr --version 0.1.11" >&2
    exit 1
fi

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

(
    cd "$root/mp3rgui"
    xtr src/main.rs -o "$tmp"
)

{
    cat << 'EOF'
# mp3rgui translation template.
#
# Translate only the msgstr lines. Leave msgid and msgctxt unchanged.
# Keep placeholders such as {count}, {n}, and {} exactly as they appear;
# named ones may be reordered. An empty msgstr shows the English text.
# Do not mark an entry fuzzy: that flag is compiled as a finished translation.
# Name yourself in your language file, in a header comment:
#   # Translators:
#   # Your Name
# The entry whose context is "language name" is this language's own name
# in the Language menu, for example "Português (Brasil)".
# Refresh this file with scripts/update-mp3rgui-pot.sh when the GUI text changes.

EOF
    # Drop the title comments xtr writes; keep its catalogue header and entries.
    awk 'BEGIN { skip = 1 } skip && /^#/ { next } skip && NF == 0 { next } { skip = 0; print }' "$tmp"
} > "$out"

echo "wrote $out"
