#!/bin/sh
set -eu

repository=${WHOWNS_REPOSITORY:-shm11C3/whowns}
download_dir=

usage() {
    cat <<'USAGE'
Install whowns from GitHub Releases or an extracted release archive.

USAGE:
    install.sh [OPTIONS]

OPTIONS:
    --bin-dir DIR   Installation directory (default: $HOME/.local/bin)
    --no-alias      Do not install the wio shorthand alias
    -h, --help      Print help

ENVIRONMENT:
    WHOWNS_VERSION      Release version to install, such as v0.1.0 (default: latest)
    WHOWNS_REPOSITORY   GitHub repository in owner/name form
USAGE
}

fail() {
    echo "error: $*" >&2
    exit 1
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

cleanup() {
    if [ -n "$download_dir" ] && [ -d "$download_dir" ]; then
        rm -rf -- "$download_dir"
    fi
}

target_for_host() {
    os=$(uname -s)
    architecture=$(uname -m)

    case "$os" in
        Darwin) os_suffix=apple-darwin ;;
        Linux) os_suffix=unknown-linux-musl ;;
        *) fail "unsupported operating system: $os" ;;
    esac

    case "$architecture" in
        x86_64|amd64) architecture=x86_64 ;;
        arm64|aarch64) architecture=aarch64 ;;
        *) fail "unsupported architecture: $architecture" ;;
    esac

    printf '%s-%s\n' "$architecture" "$os_suffix"
}

release_tag() {
    if [ -n "${WHOWNS_VERSION:-}" ]; then
        case "$WHOWNS_VERSION" in
            v*) printf '%s\n' "$WHOWNS_VERSION" ;;
            *) printf 'v%s\n' "$WHOWNS_VERSION" ;;
        esac
        return
    fi

    latest_url=$(curl --proto '=https' --tlsv1.2 -fsSL \
        -o /dev/null -w '%{url_effective}' \
        "https://github.com/$repository/releases/latest")
    tag=${latest_url##*/}
    case "$tag" in
        v*) printf '%s\n' "$tag" ;;
        *) fail "could not determine the latest release from $latest_url" ;;
    esac
}

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{ print $1 }'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{ print $1 }'
    else
        fail "sha256sum or shasum is required to verify the release archive"
    fi
}

download_release_binary() {
    require_command curl
    require_command tar
    require_command awk
    require_command mktemp

    target=$(target_for_host)
    tag=$(release_tag)
    archive_name="whowns-${tag}-${target}.tar.gz"
    release_url="https://github.com/$repository/releases/download/$tag"
    download_dir=$(mktemp -d "${TMPDIR:-/tmp}/whowns-install.XXXXXX")
    archive_path="$download_dir/$archive_name"
    checksums_path="$download_dir/SHA256SUMS"

    echo "downloading: whowns $tag for $target"
    curl --proto '=https' --tlsv1.2 -fsSL \
        -o "$archive_path" "$release_url/$archive_name"
    curl --proto '=https' --tlsv1.2 -fsSL \
        -o "$checksums_path" "$release_url/SHA256SUMS"

    expected=$(awk -v name="$archive_name" '
        {
            file = $2
            sub(/^\*/, "", file)
            sub(/^\.\//, "", file)
            if (file == name) {
                print $1
                exit
            }
        }
    ' "$checksums_path")
    [ -n "$expected" ] || fail "$archive_name is missing from SHA256SUMS"

    actual=$(sha256_file "$archive_path")
    [ "$actual" = "$expected" ] || fail "checksum verification failed for $archive_name"

    tar -xzf "$archive_path" -C "$download_dir"
    source_binary="$download_dir/whowns-${tag}-${target}/whowns"
    [ -x "$source_binary" ] || fail "release archive does not contain an executable whowns binary"
}

install_alias() {
    alias_path="$bin_dir/wio"
    if [ -L "$alias_path" ]; then
        current_target=$(readlink "$alias_path" 2>/dev/null || true)
        if [ "$current_target" = whowns ]; then
            echo "alias: $alias_path -> whowns"
        else
            echo "note: not replacing existing alias: $alias_path" >&2
        fi
    elif [ -e "$alias_path" ]; then
        echo "note: not replacing existing file: $alias_path" >&2
    else
        ln -s whowns "$alias_path"
        echo "alias: $alias_path -> whowns"
    fi
}

if [ -n "${HOME:-}" ]; then
    bin_dir="$HOME/.local/bin"
else
    bin_dir=
fi
create_alias=true

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
        --no-alias)
            create_alias=false
            shift
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

trap cleanup 0
trap 'cleanup; exit 1' HUP INT TERM

source_binary=
case ${0##*/} in
    install.sh)
        script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
        if [ -x "$script_dir/whowns" ]; then
            source_binary="$script_dir/whowns"
        fi
        ;;
esac

if [ -z "$source_binary" ]; then
    download_release_binary
fi

mkdir -p "$bin_dir"
require_command install
install -m 0755 "$source_binary" "$bin_dir/whowns"
echo "installed: $bin_dir/whowns"

if [ "$create_alias" = true ]; then
    require_command ln
    install_alias
fi

case ":${PATH:-}:" in
    *:"$bin_dir":*) ;;
    *) echo "note: add $bin_dir to PATH" ;;
esac
