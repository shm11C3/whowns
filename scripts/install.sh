#!/bin/sh
set -eu

usage() {
    cat <<'USAGE'
Install whowns from an extracted GitHub Release archive.

USAGE:
    ./install.sh [OPTIONS]

OPTIONS:
    --bin-dir DIR   Installation directory (default: $HOME/.local/bin)
    -h, --help      Print help
USAGE
}

if [ -n "${HOME:-}" ]; then
    bin_dir="$HOME/.local/bin"
else
    bin_dir=""
fi

while [ "$#" -gt 0 ]; do
    case "$1" in
        --bin-dir)
            if [ "$#" -lt 2 ]; then
                echo "error: --bin-dir requires a directory" >&2
                exit 2
            fi
            bin_dir=$2
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "error: unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [ -z "$bin_dir" ]; then
    echo "error: HOME is not set; pass --bin-dir DIR" >&2
    exit 2
fi

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
source_binary="$script_dir/whowns"

if [ ! -x "$source_binary" ]; then
    echo "error: whowns binary is missing beside install.sh" >&2
    exit 1
fi

mkdir -p "$bin_dir"
install -m 0755 "$source_binary" "$bin_dir/whowns"
echo "installed: $bin_dir/whowns"

case ":${PATH:-}:" in
    *:"$bin_dir":*) ;;
    *) echo "note: add $bin_dir to PATH" ;;
esac
