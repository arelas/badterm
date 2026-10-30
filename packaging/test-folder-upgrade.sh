#!/bin/sh
# Check that BadTerm moves Phosphor's folders (from before the rename, up
# to 0.1.16) to its own on first start. Used by CI on Linux and macOS.
#
#   packaging/test-folder-upgrade.sh path/to/badterm
#
# On Linux the folders follow XDG_CONFIG_HOME and XDG_DATA_HOME, so they are
# made in a scratch folder. On macOS they are always under ~/Library, so the
# test uses the real ones: it only touches folders holding its own marker,
# and leaves anything else alone.
set -eu
bin=$1
marker=ci-upgrade-marker

case $(uname) in
Linux)
    scratch=$(mktemp -d)
    trap 'rm -rf "$scratch"' EXIT
    export XDG_CONFIG_HOME="$scratch/config" XDG_DATA_HOME="$scratch/data"
    old_config=$XDG_CONFIG_HOME/phosphor old_data=$XDG_DATA_HOME/phosphor
    new_config=$XDG_CONFIG_HOME/badterm new_data=$XDG_DATA_HOME/badterm
    ;;
Darwin)
    support="$HOME/Library/Application Support"
    old_config="$support/org.Phosphor.Phosphor" old_data=$old_config
    new_config="$support/dev.Bad-Cluster.BadTerm" new_data=$new_config
    for d in "$old_config" "$new_config"; do
        if [ -e "$d" ]; then
            if [ -e "$d/$marker" ]; then rm -rf "$d"
            else echo "::warning::$d exists and is not this test's; skipping"; exit 0; fi
        fi
    done
    trap 'rm -rf "$new_config" "$old_config"' EXIT
    ;;
*) echo "unsupported OS"; exit 1 ;;
esac

mkdir -p "$old_config" "$old_data/captures"
echo "from Phosphor" > "$old_config/$marker"
"$bin" --paths

fail=0
[ -e "$new_config/$marker" ] || { echo "::error::settings were not moved to $new_config"; fail=1; }
[ -d "$new_data/captures" ] || { echo "::error::captures were not moved to $new_data"; fail=1; }
[ ! -e "$old_config" ] || { echo "::error::$old_config is still there"; fail=1; }
[ "$fail" -eq 0 ] && echo "Phosphor's folders moved to BadTerm's"
exit $fail
