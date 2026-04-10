#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
PREFIX="/usr/local"
PROFILE="release"
UNINSTALL=false

for arg in "$@"; do
    case "$arg" in
        --prefix=*) PREFIX="${arg#*=}" ;;
        --profile=*)
            PROFILE="${arg#*=}"
            ;;
        --debug)
            PROFILE="debug"
            ;;
        --release)
            PROFILE="release"
            ;;
        --uninstall)
            UNINSTALL=true
            ;;
        -h|--help)
            echo "Usage: ./scripts/install.sh [--prefix=/usr/local] [--profile=release|debug] [--uninstall]"
            exit 0
            ;;
        *)
            echo "Unknown argument: $arg" >&2
            echo "Usage: ./scripts/install.sh [--prefix=/usr/local] [--profile=release|debug] [--uninstall]" >&2
            exit 1
            ;;
    esac
done

case "${PROFILE}" in
    release|debug) ;;
    *)
        echo "ERROR: unsupported profile '${PROFILE}'. Expected 'release' or 'debug'." >&2
        exit 1
        ;;
esac

BINARY="${ROOT_DIR}/target/${PROFILE}/chostty"
GHOSTTY_DIR="${ROOT_DIR}/ghostty"
GHOSTTY_SO="${GHOSTTY_DIR}/zig-out/lib/libghostty.so"
GHOSTTY_SHARE_DIR=""
GHOSTTY_TERMINFO_DIR=""
ICONS_DIR="${ROOT_DIR}/rust/chostty-host-linux/icons"
APP_ICONS_DIR="${ROOT_DIR}/rust/chostty-host-linux/icons/app"
DESKTOP_FILE="${ROOT_DIR}/rust/chostty-host-linux/dev.turbinebmw.chostty.desktop"
METADATA_FILE="${ROOT_DIR}/rust/chostty-host-linux/dev.turbinebmw.chostty.metainfo.xml"

need_root() {
    if [ "$(id -u)" -ne 0 ]; then
        echo "This operation requires root. Re-running with sudo..."
        exec sudo "$0" "$@"
    fi
}

remove_tree() {
    local path="$1"

    if [ ! -e "$path" ]; then
        return 0
    fi

    find "$path" -depth -mindepth 1 ! -type d -exec rm -f {} +
    find "$path" -depth -mindepth 1 -type d -exec rmdir {} + 2>/dev/null || true
    rmdir "$path" 2>/dev/null || true
}

copy_ghostty_terminfo_entries() {
    local source_dir="$1"
    local dest_dir="$2"

    mkdir -p "${dest_dir}/g" "${dest_dir}/x"

    if [ -f "${source_dir}/g/ghostty" ]; then
        cp "${source_dir}/g/ghostty" "${dest_dir}/g/ghostty"
    fi

    if [ -f "${source_dir}/x/xterm-ghostty" ]; then
        cp "${source_dir}/x/xterm-ghostty" "${dest_dir}/x/xterm-ghostty"
    fi
}

resolve_ghostty_share_dir() {
    local candidate

    for candidate in \
        "${GHOSTTY_DIR}/zig-out/share/ghostty" \
        "/usr/local/share/ghostty" \
        "/usr/share/ghostty"
    do
        if [ -d "${candidate}" ]; then
            printf '%s\n' "${candidate}"
            return 0
        fi
    done

    return 1
}

resolve_ghostty_terminfo_dir() {
    local candidate
    local parent

    parent="$(dirname "${GHOSTTY_SHARE_DIR}")"

    for candidate in \
        "${parent}/terminfo" \
        "/usr/local/share/terminfo" \
        "/usr/share/terminfo"
    do
        if [ -f "${candidate}/g/ghostty" ] || [ -f "${candidate}/x/xterm-ghostty" ]; then
            printf '%s\n' "${candidate}"
            return 0
        fi
    done

    return 1
}

ensure_ghostty_artifacts() {
    if [ -f "${GHOSTTY_SO}" ]; then
        if GHOSTTY_SHARE_DIR="$(resolve_ghostty_share_dir)" && \
            GHOSTTY_TERMINFO_DIR="$(resolve_ghostty_terminfo_dir)"; then
            return 0
        fi
    fi

    if ! command -v zig >/dev/null 2>&1; then
        echo "ERROR: zig not found in PATH." >&2
        echo "Install Zig, then rerun ./scripts/install.sh." >&2
        exit 1
    fi

    if [ ! -f "${GHOSTTY_DIR}/build.zig" ]; then
        echo "ERROR: Ghostty submodule is missing or uninitialized at ${GHOSTTY_DIR}" >&2
        echo "Run: git submodule update --init --recursive" >&2
        exit 1
    fi

    echo "Ghostty artifacts missing. Building libghostty (ReleaseFast)..."
    (
        cd "${GHOSTTY_DIR}"
        zig build -Dapp-runtime=none -Doptimize=ReleaseFast
    )

    if [ ! -f "${GHOSTTY_SO}" ]; then
        echo "ERROR: libghostty.so not found at ${GHOSTTY_SO} after build" >&2
        exit 1
    fi

    if ! GHOSTTY_SHARE_DIR="$(resolve_ghostty_share_dir)"; then
        echo "ERROR: Ghostty resources directory not found." >&2
        echo "Looked for:" >&2
        echo "  ${GHOSTTY_DIR}/zig-out/share/ghostty" >&2
        echo "  /usr/local/share/ghostty" >&2
        echo "  /usr/share/ghostty" >&2
        exit 1
    fi

    if ! GHOSTTY_TERMINFO_DIR="$(resolve_ghostty_terminfo_dir)"; then
        echo "ERROR: Ghostty terminfo entries not found." >&2
        echo "Looked for:" >&2
        echo "  $(dirname "${GHOSTTY_SHARE_DIR}")/terminfo" >&2
        echo "  /usr/local/share/terminfo" >&2
        echo "  /usr/share/terminfo" >&2
        exit 1
    fi
}

ensure_binary() {
    local cargo_args=(build --manifest-path "${ROOT_DIR}/Cargo.toml")

    if [ -f "${BINARY}" ]; then
        return 0
    fi

    if ! command -v cargo >/dev/null 2>&1; then
        echo "ERROR: cargo not found in PATH." >&2
        exit 1
    fi

    if [ "${PROFILE}" = "release" ]; then
        cargo_args+=(--release)
    fi

    echo "Built binary missing. Building Cargo profile '${PROFILE}'..."
    cargo "${cargo_args[@]}"

    if [ ! -f "${BINARY}" ]; then
        echo "ERROR: built binary not found at ${BINARY} after cargo build" >&2
        exit 1
    fi
}

if $UNINSTALL; then
    need_root "$@"
    echo "Uninstalling Chostty from ${PREFIX}..."

    rm -f "${PREFIX}/bin/chostty"
    remove_tree "${PREFIX}/lib/chostty"
    remove_tree "${PREFIX}/share/chostty"
    rm -f /etc/ld.so.conf.d/chostty.conf
    ldconfig 2>/dev/null || true

    rm -f "${PREFIX}/share/applications/chostty.desktop"
    rm -f "${PREFIX}/share/applications/dev.turbinebmw.chostty.desktop"
    rm -f "${PREFIX}/share/metainfo/dev.turbinebmw.chostty.metainfo.xml"

    for size in 16 32 128 256 512; do
        rm -f "${PREFIX}/share/icons/hicolor/${size}x${size}/apps/chostty.png"
    done
    rm -f "${PREFIX}/share/icons/hicolor/scalable/actions/chostty-globe-symbolic.svg"
    rm -f "${PREFIX}/share/icons/hicolor/scalable/actions/chostty-split-horizontal-symbolic.svg"
    rm -f "${PREFIX}/share/icons/hicolor/scalable/actions/chostty-split-vertical-symbolic.svg"

    gtk-update-icon-cache -f -t "${PREFIX}/share/icons/hicolor" 2>/dev/null || true
    update-desktop-database "${PREFIX}/share/applications" 2>/dev/null || true
    appstreamcli refresh-cache --force 2>/dev/null || true

    echo "Chostty uninstalled."
    exit 0
fi

ensure_ghostty_artifacts
ensure_binary

need_root "$@"
echo "Installing Chostty from source tree to ${PREFIX}..."

install -Dm755 "${BINARY}" "${PREFIX}/bin/chostty"
install -Dm644 "${GHOSTTY_SO}" "${PREFIX}/lib/chostty/libghostty.so"

remove_tree "${PREFIX}/share/chostty"
mkdir -p "${PREFIX}/share/chostty/ghostty"
cp -r "${GHOSTTY_SHARE_DIR}"/. "${PREFIX}/share/chostty/ghostty"
copy_ghostty_terminfo_entries "${GHOSTTY_TERMINFO_DIR}" "${PREFIX}/share/chostty/terminfo"

echo "${PREFIX}/lib/chostty" > /etc/ld.so.conf.d/chostty.conf
ldconfig 2>/dev/null || true

rm -f "${PREFIX}/share/applications/chostty.desktop"
install -Dm644 "${DESKTOP_FILE}" "${PREFIX}/share/applications/dev.turbinebmw.chostty.desktop"
install -Dm644 "${METADATA_FILE}" "${PREFIX}/share/metainfo/dev.turbinebmw.chostty.metainfo.xml"

mkdir -p "${PREFIX}/share/icons/hicolor/scalable/actions"
if [ -d "${ICONS_DIR}/hicolor" ]; then
    cp -r "${ICONS_DIR}/hicolor/scalable" "${PREFIX}/share/icons/hicolor/" 2>/dev/null || true
fi
for svg in "${ICONS_DIR}"/*.svg; do
    [ -f "${svg}" ] && cp "${svg}" "${PREFIX}/share/icons/hicolor/scalable/actions/"
done

if [ -d "${APP_ICONS_DIR}" ]; then
    for size in 16 32 128 256 512; do
        src="${APP_ICONS_DIR}/${size}.png"
        if [ -f "${src}" ]; then
            install -Dm644 "${src}" "${PREFIX}/share/icons/hicolor/${size}x${size}/apps/chostty.png"
        fi
    done
fi

gtk-update-icon-cache -f -t "${PREFIX}/share/icons/hicolor" 2>/dev/null || true
update-desktop-database "${PREFIX}/share/applications" 2>/dev/null || true
appstreamcli refresh-cache --force 2>/dev/null || true

echo ""
echo "Chostty installed successfully!"
echo "  Binary:  ${PREFIX}/bin/chostty"
echo "  Library: ${PREFIX}/lib/chostty/libghostty.so"
echo "  Profile: ${PROFILE}"
echo "  Run:     chostty"
