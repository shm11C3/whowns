#!/bin/sh
set -eu

repository_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
test_root=$(mktemp -d "${TMPDIR:-/tmp}/whowns-installer-test.XXXXXX")

cleanup() {
    rm -rf -- "$test_root"
}
trap cleanup 0
trap 'cleanup; exit 1' HUP INT TERM

fail() {
    echo "test failure: $*" >&2
    exit 1
}

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{ print $1 }'
    else
        shasum -a 256 "$1" | awk '{ print $1 }'
    fi
}

write_fixture_binary() {
    destination=$1
    cat >"$destination" <<'BINARY'
#!/bin/sh
echo fixture-whowns
BINARY
    chmod 0755 "$destination"
}

target_for_host() {
    case $(uname -s) in
        Darwin) os_suffix=apple-darwin ;;
        Linux) os_suffix=unknown-linux-musl ;;
        *) fail "test host is unsupported" ;;
    esac
    case $(uname -m) in
        x86_64|amd64) architecture=x86_64 ;;
        arm64|aarch64) architecture=aarch64 ;;
        *) fail "test architecture is unsupported" ;;
    esac
    printf '%s-%s\n' "$architecture" "$os_suffix"
}

test_extracted_archive_mode() {
    archive_dir="$test_root/extracted"
    bin_dir="$test_root/local-bin"
    mkdir -p "$archive_dir"
    cp "$repository_root/scripts/install.sh" "$archive_dir/install.sh"
    write_fixture_binary "$archive_dir/whowns"

    sh "$archive_dir/install.sh" --bin-dir "$bin_dir" >"$test_root/local.log"

    [ -x "$bin_dir/whowns" ] || fail "local archive binary was not installed"
    [ -L "$bin_dir/wio" ] || fail "default wio alias was not installed"
    [ "$(readlink "$bin_dir/wio")" = whowns ] || fail "wio alias has the wrong target"
    [ "$("$bin_dir/whowns")" = fixture-whowns ] || fail "installed local binary is invalid"
}

test_no_alias_option() {
    archive_dir="$test_root/no-alias-archive"
    bin_dir="$test_root/no-alias-bin"
    mkdir -p "$archive_dir"
    cp "$repository_root/scripts/install.sh" "$archive_dir/install.sh"
    write_fixture_binary "$archive_dir/whowns"

    sh "$archive_dir/install.sh" --bin-dir "$bin_dir" --no-alias >"$test_root/no-alias.log"

    [ -x "$bin_dir/whowns" ] || fail "binary was not installed with --no-alias"
    [ ! -e "$bin_dir/wio" ] || fail "wio alias was installed despite --no-alias"
}

prepare_download_fixture() {
    tag=v9.9.9
    target=$(target_for_host)
    archive_name="whowns-${tag}-${target}.tar.gz"
    package_dir="$test_root/whowns-${tag}-${target}"
    fixture_archive="$test_root/$archive_name"
    fixture_checksums="$test_root/SHA256SUMS"
    fake_bin="$test_root/fake-bin"

    mkdir -p "$package_dir" "$fake_bin"
    write_fixture_binary "$package_dir/whowns"
    tar -czf "$fixture_archive" -C "$test_root" "whowns-${tag}-${target}"
    checksum=$(sha256_file "$fixture_archive")
    printf '%s  ./%s\n' "$checksum" "$archive_name" >"$fixture_checksums"

    cat >"$fake_bin/curl" <<'CURL'
#!/bin/sh
set -eu

output=
url=
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o)
            output=$2
            shift 2
            ;;
        --proto|-w)
            shift 2
            ;;
        --tlsv1.2) shift ;;
        -*) shift ;;
        *)
            url=$1
            shift
            ;;
    esac
done

case "$url" in
    */releases/latest)
        printf 'https://github.com/example/whowns/releases/tag/%s' "$WHOWNS_TEST_TAG"
        ;;
    */SHA256SUMS) cp "$WHOWNS_TEST_CHECKSUMS" "$output" ;;
    *.tar.gz) cp "$WHOWNS_TEST_ARCHIVE" "$output" ;;
    *) echo "unexpected curl URL: $url" >&2; exit 1 ;;
esac
CURL
    chmod 0755 "$fake_bin/curl"
}

test_pipe_download_mode() {
    prepare_download_fixture
    bin_dir="$test_root/download-bin"

    PATH="$fake_bin:$PATH" \
        WHOWNS_REPOSITORY=example/whowns \
        WHOWNS_TEST_ARCHIVE="$fixture_archive" \
        WHOWNS_TEST_CHECKSUMS="$fixture_checksums" \
        WHOWNS_TEST_TAG="$tag" \
        sh -s -- --bin-dir "$bin_dir" <"$repository_root/scripts/install.sh" \
        >"$test_root/download.log"

    [ -x "$bin_dir/whowns" ] || fail "downloaded binary was not installed"
    [ "$("$bin_dir/whowns")" = fixture-whowns ] || fail "downloaded binary is invalid"
    [ -L "$bin_dir/wio" ] || fail "download mode did not install the default alias"
}

test_checksum_rejection() {
    prepare_download_fixture
    bin_dir="$test_root/rejected-bin"
    printf '%064d  ./%s\n' 0 "$archive_name" >"$fixture_checksums"

    if PATH="$fake_bin:$PATH" \
        WHOWNS_VERSION="$tag" \
        WHOWNS_REPOSITORY=example/whowns \
        WHOWNS_TEST_ARCHIVE="$fixture_archive" \
        WHOWNS_TEST_CHECKSUMS="$fixture_checksums" \
        sh -s -- --bin-dir "$bin_dir" <"$repository_root/scripts/install.sh" \
        >"$test_root/rejected.log" 2>&1; then
        fail "installer accepted an invalid checksum"
    fi

    [ ! -e "$bin_dir/whowns" ] || fail "installer wrote a binary after checksum failure"
}

test_extracted_archive_mode
test_no_alias_option
test_pipe_download_mode
test_checksum_rejection

echo "installer tests passed"
