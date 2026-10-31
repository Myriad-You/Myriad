#!/usr/bin/env bash
# figure/ carries byte copies of the core contracts it reads (figure/contracts/),
# so it does not depend on core/. Their source stays core/shared/.
#
#   check  each copy is a version core/ has had: core may move ahead on its own,
#          but the figure may not carry a contract the core never had.
#   sync   copy core/shared/'s current version of each over the figure's.
#
# Run from the repository root.
set -euo pipefail

mode="${1:-check}"
cd "$(dirname "$0")/../.."

case "$mode" in
    sync)
        for copy in figure/contracts/*.json; do
            cp "core/shared/$(basename "$copy")" "$copy"
            echo "synced $copy"
        done
        ;;
    check)
        if [ "$(git -C core rev-parse --is-shallow-repository)" = true ]; then
            git -C core fetch --quiet --filter=blob:none --unshallow
        fi
        failed=0
        for copy in figure/contracts/*.json; do
            name="$(basename "$copy")"
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
                echo "::notice::$copy is core $(git -C core rev-parse --short "$found")'s version, older than core/'s. Run scripts/extra/figure-contracts.sh sync in a figure change when the figure is ready for it."
            else
                echo "::error::$copy matches no version of core/shared/$name. Change the contract in myriad-core, then sync it into the figure."
                failed=1
            fi
        done
        exit "$failed"
        ;;
    *)
        echo "usage: $0 [check|sync]" >&2
        exit 2
        ;;
esac
