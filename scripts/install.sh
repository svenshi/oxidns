#!/bin/sh
# Install OxiDNS release archives on Linux and macOS, or luci-app-oxidns on OpenWrt.
#
# Common overrides:
#   OXIDNS_VERSION=v1.0.1
#   OXIDNS_INSTALL_DIR=/opt/oxidns
#   OXIDNS_BIN_DIR=/usr/local/bin
#   OXIDNS_TARGET=x86_64-unknown-linux-musl
#   OXIDNS_BUNDLE=standard
#   OXIDNS_INSTALL_SERVICE=0
#   OXIDNS_START_SERVICE=0
#   OXIDNS_OPENWRT_INSTALL=auto
#   OXIDNS_OPENWRT_REPO=svenshi/luci-app-oxidns
#   OXIDNS_OPENWRT_VERSION=latest
#   OXIDNS_OPENWRT_I18N=auto

set -eu

REPO="${OXIDNS_REPO:-svenshi/oxidns}"
VERSION="${OXIDNS_VERSION:-latest}"
TARGET="${OXIDNS_TARGET:-}"
BUNDLE="${OXIDNS_BUNDLE:-full}"
INSTALL_DIR="${OXIDNS_INSTALL_DIR:-}"
BIN_DIR="${OXIDNS_BIN_DIR:-}"
NO_PATH="${OXIDNS_NO_PATH:-0}"
INSTALL_SERVICE="${OXIDNS_INSTALL_SERVICE:-1}"
START_SERVICE="${OXIDNS_START_SERVICE:-1}"
OPENWRT_INSTALL="${OXIDNS_OPENWRT_INSTALL:-auto}"
OPENWRT_REPO="${OXIDNS_OPENWRT_REPO:-svenshi/luci-app-oxidns}"
OPENWRT_VERSION="${OXIDNS_OPENWRT_VERSION:-latest}"
OPENWRT_I18N="${OXIDNS_OPENWRT_I18N:-auto}"

log() {
    printf '%s\n' "$*"
}

warn() {
    printf 'warning: %s\n' "$*" >&2
}

err() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

need_cmd() {
    command -v "$1" >/dev/null 2>&1 || err "required command not found: $1"
}

is_truthy() {
    case "$1" in
        1|true|TRUE|yes|YES|on|ON) return 0 ;;
        *) return 1 ;;
    esac
}

is_falsey() {
    case "$1" in
        0|false|FALSE|no|NO|off|OFF) return 0 ;;
        *) return 1 ;;
    esac
}

is_root() {
    [ "$(id -u 2>/dev/null || printf '1')" = "0" ]
}

detect_target() {
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os:$arch" in
        Linux:x86_64|Linux:amd64)
            printf 'x86_64-unknown-linux-musl'
            ;;
        Linux:aarch64|Linux:arm64)
            printf 'aarch64-unknown-linux-musl'
            ;;
        Linux:i386|Linux:i686)
            printf 'i686-unknown-linux-musl'
            ;;
        Linux:armv7l|Linux:armv7)
            printf 'arm-unknown-linux-musleabihf'
            ;;
        Darwin:x86_64|Darwin:amd64)
            printf 'x86_64-apple-darwin'
            ;;
        Darwin:arm64|Darwin:aarch64)
            printf 'aarch64-apple-darwin'
            ;;
        FreeBSD:x86_64|FreeBSD:amd64)
            printf 'x86_64-unknown-freebsd'
            ;;
        *)
            err "unsupported platform: $os $arch. Set OXIDNS_TARGET to override."
            ;;
    esac
}

download() {
    url="$1"
    out="$2"

    if command -v curl >/dev/null 2>&1; then
        curl -fL --retry 3 --retry-delay 2 -o "$out" "$url"
    elif command -v wget >/dev/null 2>&1; then
        wget -O "$out" "$url"
    else
        err "curl or wget is required to download OxiDNS"
    fi
}

is_openwrt() {
    [ -f /etc/openwrt_release ] && return 0
    if [ -r /etc/os-release ] && grep -qi 'openwrt' /etc/os-release 2>/dev/null; then
        return 0
    fi
    return 1
}

should_install_openwrt() {
    case "$OPENWRT_INSTALL" in
        auto|"")
            is_openwrt
            ;;
        *)
            is_truthy "$OPENWRT_INSTALL"
            ;;
    esac
}

openwrt_release_api_url() {
    if [ "$OPENWRT_VERSION" = "latest" ]; then
        printf 'https://api.github.com/repos/%s/releases/latest' "$OPENWRT_REPO"
    else
        printf 'https://api.github.com/repos/%s/releases/tags/%s' "$OPENWRT_REPO" "$OPENWRT_VERSION"
    fi
}

openwrt_asset_url() {
    release_json="$1"
    package_name="$2"
    package_ext="$3"

    sed -n 's#.*"browser_download_url"[[:space:]]*:[[:space:]]*"\([^"]*/'"$package_name"'_[^"]*_all\.'"$package_ext"'\)".*#\1#p' "$release_json" | head -n 1
}

openwrt_install_package() {
    package_manager="$1"
    package_path="$2"

    case "$package_manager" in
        opkg)
            opkg install "$package_path"
            ;;
        apk)
            apk add --allow-untrusted --no-network "$package_path"
            ;;
        *)
            err "unsupported OpenWrt package manager: $package_manager"
            ;;
    esac
}

install_openwrt_luci() {
    is_root || err "OpenWrt LuCI installation requires root; rerun as root"
    need_cmd sed
    need_cmd head
    need_cmd mktemp

    if command -v opkg >/dev/null 2>&1; then
        openwrt_package_manager="opkg"
        openwrt_package_ext="ipk"
    elif command -v apk >/dev/null 2>&1; then
        openwrt_package_manager="apk"
        openwrt_package_ext="apk"
    else
        err "OpenWrt package manager not found: expected opkg or apk"
    fi

    openwrt_tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/oxidns-openwrt-install.XXXXXX")"
    cleanup_openwrt() {
        rm -rf "$openwrt_tmp_dir"
    }
    trap cleanup_openwrt EXIT HUP INT TERM

    openwrt_release_json="$openwrt_tmp_dir/release.json"
    openwrt_api_url="$(openwrt_release_api_url)"

    log "Downloading luci-app-oxidns release metadata from $OPENWRT_REPO ($OPENWRT_VERSION)..."
    download "$openwrt_api_url" "$openwrt_release_json"

    openwrt_app_url="$(openwrt_asset_url "$openwrt_release_json" "luci-app-oxidns" "$openwrt_package_ext")"
    [ -n "$openwrt_app_url" ] || err "could not find luci-app-oxidns .$openwrt_package_ext asset in $OPENWRT_REPO release $OPENWRT_VERSION"

    openwrt_app_pkg="$openwrt_tmp_dir/${openwrt_app_url##*/}"
    log "Downloading ${openwrt_app_url##*/}..."
    download "$openwrt_app_url" "$openwrt_app_pkg"

    log "Installing ${openwrt_app_url##*/} with $openwrt_package_manager..."
    openwrt_install_package "$openwrt_package_manager" "$openwrt_app_pkg"

    openwrt_i18n_url="$(openwrt_asset_url "$openwrt_release_json" "luci-i18n-oxidns-zh-cn" "$openwrt_package_ext")"
    openwrt_install_i18n=0
    case "$OPENWRT_I18N" in
        auto|"")
            [ -n "$openwrt_i18n_url" ] && openwrt_install_i18n=1
            ;;
        *)
            if is_truthy "$OPENWRT_I18N"; then
                [ -n "$openwrt_i18n_url" ] || err "OXIDNS_OPENWRT_I18N=1 but luci-i18n-oxidns-zh-cn .$openwrt_package_ext asset was not found"
                openwrt_install_i18n=1
            elif is_falsey "$OPENWRT_I18N"; then
                openwrt_install_i18n=0
            else
                err "unsupported OXIDNS_OPENWRT_I18N=$OPENWRT_I18N; expected auto, 1, or 0"
            fi
            ;;
    esac

    if [ "$openwrt_install_i18n" = "1" ]; then
        openwrt_i18n_pkg="$openwrt_tmp_dir/${openwrt_i18n_url##*/}"
        log "Downloading ${openwrt_i18n_url##*/}..."
        download "$openwrt_i18n_url" "$openwrt_i18n_pkg"

        log "Installing ${openwrt_i18n_url##*/} with $openwrt_package_manager..."
        openwrt_install_package "$openwrt_package_manager" "$openwrt_i18n_pkg"
    fi

    if [ -x /etc/init.d/rpcd ]; then
        if /etc/init.d/rpcd restart >/dev/null 2>&1; then
            log "Restarted rpcd"
        else
            warn "failed to restart rpcd; run /etc/init.d/rpcd restart if the LuCI menu does not appear"
        fi
    fi

    log "luci-app-oxidns installed"
    log "Open LuCI: Services -> OxiDNS"
    log "Then use Core -> Install Core to download and install the OxiDNS core binary."
}

contains_path() {
    dir="$1"
    case ":${PATH:-}:" in
        *:"$dir":*) return 0 ;;
        *) return 1 ;;
    esac
}

install_link() {
    mkdir -p "$BIN_DIR"

    link_path="$BIN_DIR/oxidns"
    if [ "$BIN_DIR" = "$INSTALL_DIR" ]; then
        return 0
    fi

    if [ -e "$link_path" ] && [ ! -L "$link_path" ]; then
        warn "$link_path already exists and is not a symlink; leaving it unchanged"
        return 0
    fi

    if ln -sf "$INSTALL_DIR/oxidns" "$link_path" 2>/dev/null; then
        return 0
    fi

    warn "failed to create symlink at $link_path; copying binary instead"
    cp "$INSTALL_DIR/oxidns" "$link_path"
    chmod 755 "$link_path"
}

if should_install_openwrt; then
    install_openwrt_luci
    exit 0
fi

if [ -z "$TARGET" ]; then
    TARGET="$(detect_target)"
fi
BUNDLE="$(printf '%s' "$BUNDLE" | tr '[:upper:]' '[:lower:]')"

if is_truthy "$INSTALL_SERVICE" && ! is_root; then
    err "service installation is the default; rerun with sudo or set OXIDNS_INSTALL_SERVICE=0 for a user install"
fi

case "$TARGET" in
    *windows*|*msvc*)
        err "Windows targets are installed with scripts/install.ps1"
        ;;
    *)
        ARCHIVE_EXT="tar.gz"
        ;;
esac

case "$BUNDLE" in
    full)
        ASSET="oxidns-$TARGET.$ARCHIVE_EXT"
        ;;
    minimal|standard)
        case "$TARGET" in
            x86_64-unknown-linux-musl|aarch64-unknown-linux-musl)
                ASSET="oxidns-$BUNDLE-$TARGET.$ARCHIVE_EXT"
                ;;
            *)
                err "OXIDNS_BUNDLE=$BUNDLE is only published for x86_64-unknown-linux-musl and aarch64-unknown-linux-musl"
                ;;
        esac
        ;;
    *)
        err "unsupported OXIDNS_BUNDLE=$BUNDLE; expected full, minimal, or standard"
        ;;
esac

if [ -z "$INSTALL_DIR" ]; then
    if is_root; then
        INSTALL_DIR="/opt/oxidns"
    else
        [ -n "${HOME:-}" ] || err "HOME is not set; set OXIDNS_INSTALL_DIR explicitly"
        INSTALL_DIR="$HOME/.oxidns"
    fi
fi

if [ -z "$BIN_DIR" ]; then
    if is_root; then
        BIN_DIR="/usr/local/bin"
    else
        [ -n "${HOME:-}" ] || err "HOME is not set; set OXIDNS_BIN_DIR explicitly"
        BIN_DIR="$HOME/.local/bin"
    fi
fi

if [ "$VERSION" = "latest" ]; then
    URL="https://github.com/$REPO/releases/latest/download/$ASSET"
else
    URL="https://github.com/$REPO/releases/download/$VERSION/$ASSET"
fi

need_cmd tar
need_cmd mkdir
need_cmd cp
need_cmd chmod
need_cmd mv

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/oxidns-install.XXXXXX")"
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT HUP INT TERM

ARCHIVE="$TMP_DIR/$ASSET"
UNPACK_DIR="$TMP_DIR/unpack"

log "Downloading $ASSET from $REPO ($VERSION)..."
download "$URL" "$ARCHIVE"

mkdir -p "$UNPACK_DIR"
tar -xzf "$ARCHIVE" -C "$UNPACK_DIR"

[ -f "$UNPACK_DIR/oxidns" ] || err "archive does not contain oxidns"
[ -f "$UNPACK_DIR/config.yaml" ] || err "archive does not contain config.yaml"

mkdir -p "$INSTALL_DIR"

cp "$UNPACK_DIR/oxidns" "$INSTALL_DIR/oxidns.tmp"
chmod 755 "$INSTALL_DIR/oxidns.tmp"
mv "$INSTALL_DIR/oxidns.tmp" "$INSTALL_DIR/oxidns"

if [ -f "$INSTALL_DIR/config.yaml" ]; then
    cp "$UNPACK_DIR/config.yaml" "$INSTALL_DIR/config.yaml.example"
    CONFIG_PATH="$INSTALL_DIR/config.yaml"
    log "Keeping existing config: $CONFIG_PATH"
    log "Wrote release example config: $INSTALL_DIR/config.yaml.example"
else
    cp "$UNPACK_DIR/config.yaml" "$INSTALL_DIR/config.yaml"
    CONFIG_PATH="$INSTALL_DIR/config.yaml"
fi

if [ -f "$UNPACK_DIR/LICENSE" ]; then
    cp "$UNPACK_DIR/LICENSE" "$INSTALL_DIR/LICENSE"
fi

if [ -d "$UNPACK_DIR/webui" ]; then
    rm -rf "$INSTALL_DIR/webui"
    cp -R "$UNPACK_DIR/webui" "$INSTALL_DIR/webui"
fi

if ! is_truthy "$NO_PATH"; then
    install_link
fi

if "$INSTALL_DIR/oxidns" check -c "$CONFIG_PATH" -d "$INSTALL_DIR" >/dev/null 2>&1; then
    log "Config check passed: $CONFIG_PATH"
else
    warn "installed binary is ready, but config check failed: $CONFIG_PATH"
fi

if is_truthy "$INSTALL_SERVICE"; then
    is_root || err "OXIDNS_INSTALL_SERVICE=1 requires root; rerun with sudo"
    "$INSTALL_DIR/oxidns" service install -d "$INSTALL_DIR" -c "$CONFIG_PATH"
    if is_truthy "$START_SERVICE"; then
        "$INSTALL_DIR/oxidns" service start
    fi
fi

log "OxiDNS installed to $INSTALL_DIR"
if ! is_truthy "$NO_PATH"; then
    log "Command shim: $BIN_DIR/oxidns"
    if ! contains_path "$BIN_DIR"; then
        warn "$BIN_DIR is not in PATH; add it to your shell profile or run $INSTALL_DIR/oxidns directly"
    fi
fi
log "Try: oxidns start -c $CONFIG_PATH -d $INSTALL_DIR"
