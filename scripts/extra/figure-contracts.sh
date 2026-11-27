#!/usr/bin/env bash
# The figure reads the core contracts from byte copies its renderer,
# @myriad-you/figure-player, carries in contracts/, so neither depends on core/.
# Their source stays core/shared/. The copies checked are those of the player
# figure/ pins, as frontend/'s install puts it under figure/node_modules.
#
#   check          each copy is a version core/ has had: core may move ahead on
#                  its own, but the figure may not read a contract the core never had.
#                  Run after `pnpm install` in frontend/.
#   sync <player>  copy core/shared/'s current version of each over the copies
#                  in a checkout of myriad-figure-player, to commit there.
#
# Run from the repository root.
set -euo pipefail

mode="${1:-check}"
cd "$(dirname "$0")/../.."
names="merope_performance_contract.json merope_rig_contract.json"
copies="figure/node_modules/@myriad-you/figure-player/contracts"

case "$mode" in
    sync)
        player="${2:?usage: $0 sync <myriad-figure-player checkout>}"
        for name in $names; do
            cp "core/shared/$name" "$player/contracts/$name"
            echo "synced $player/contracts/$name"
        done
        ;;
    check)
        if [ "$(git -C core rev-parse --is-shallow-repository)" = true ]; then
            git -C core fetch --quiet --filter=blob:none --unshallow
        fi
        failed=0
        for name in $names; do
            copy="$copies/$name"
            if [ ! -f "$copy" ]; then
                echo "::error::$copy is missing. Run pnpm install in frontend/ first."
                failed=1
                continue
            fi
            blob="$(git hash-object "$copy")"
            if [ "$(git -C core rev-parse -q --verify "HEAD:shared/$name" || true)" = "$blob" ]; then
                continue
            fi
            found=""
            for commit in $(git -C core rev-list HEAD -- "shared/$name"); do
                if [ "$(git -C core rev-parse -q --verify "$commit:shared/$name" || true)" = "$blob" ]; then
                    found="$commit"
                    break
                fi
            done
            if [ -n "$found" ]; then
                echo "::notice::$copy is core $(git -C core rev-parse --short "$found")'s version, older than core/'s. Sync it into myriad-figure-player (scripts/extra/figure-contracts.sh sync <checkout>) when the figure is ready for it."
            else
                echo "::error::$copy matches no version of core/shared/$name. Change the contract in myriad-core, then sync it into myriad-figure-player."
                failed=1
            fi
        done
        exit "$failed"
        ;;
    *)
        echo "usage: $0 [check|sync <myriad-figure-player checkout>]" >&2
        exit 2
        ;;
esac
