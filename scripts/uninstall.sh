#!/bin/sh
# Uninstall OxiDNS files installed by scripts/install.sh on Linux and macOS,
# or luci-app-oxidns plus OxiDNS core files on OpenWrt.
#
# Common overrides:
#   OXIDNS_INSTALL_DIR=/opt/oxidns
#   OXIDNS_BIN_DIR=/usr/local/bin
#   OXIDNS_UNINSTALL_SERVICE=1
#   OXIDNS_PURGE=1
#   OXIDNS_OPENWRT_UNINSTALL=auto

set -eu

INSTALL_DIR="${OXIDNS_INSTALL_DIR:-}"
BIN_DIR="${OXIDNS_BIN_DIR:-}"
NO_PATH="${OXIDNS_NO_PATH:-0}"
UNINSTALL_SERVICE="${OXIDNS_UNINSTALL_SERVICE:-auto}"
PURGE="${OXIDNS_PURGE:-0}"
HOME_DIR="${HOME:-}"
OPENWRT_UNINSTALL="${OXIDNS_OPENWRT_UNINSTALL:-auto}"
OPENWRT_CONFIG_PATH="${OXIDNS_OPENWRT_CONFIG_PATH:-}"
OPENWRT_WORKING_DIR="${OXIDNS_OPENWRT_WORKING_DIR:-}"
OPENWRT_BIN="/usr/bin/oxidns"
OPENWRT_WEBUI_DIR="/usr/share/oxidns/webui"
OPENWRT_UCI_CONFIG="/etc/config/oxidns"
OPENWRT_DEFAULT_CONFIG_PATH="/etc/oxidns/config.yaml"
OPENWRT_DEFAULT_WORKING_DIR="/var/lib/oxidns"

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

is_truthy() {
    case "$1" in
        1|true|TRUE|yes|YES|on|ON) return 0 ;;
        *) return 1 ;;
    esac
}

is_root() {
    [ "$(id -u 2>/dev/null || printf '1')" = "0" ]
}

is_openwrt() {
    [ -f /etc/openwrt_release ] && return 0
    if [ -r /etc/os-release ] && grep -qi 'openwrt' /etc/os-release 2>/dev/null; then
        return 0
    fi
    return 1
}

should_uninstall_openwrt() {
    case "$OPENWRT_UNINSTALL" in
        auto|"")
            is_openwrt
            ;;
        *)
            is_truthy "$OPENWRT_UNINSTALL"
            ;;
    esac
}

should_uninstall_service() {
    case "$UNINSTALL_SERVICE" in
        auto|"")
            is_root
            ;;
        *)
            is_truthy "$UNINSTALL_SERVICE"
            ;;
    esac
}

uci_get_oxidns() {
    option="$1"
    fallback="$2"

    if command -v uci >/dev/null 2>&1; then
        value="$(uci -q get "oxidns.main.$option" 2>/dev/null || true)"
        if [ -n "$value" ]; then
            printf '%s' "$value"
            return 0
        fi
    fi

    printf '%s' "$fallback"
}

oxidns_named_path() {
    path_value="$1"
    base_name="$(basename "$path_value" 2>/dev/null || printf '')"
    dir_name="$(dirname "$path_value" 2>/dev/null || printf '')"
    dir_base="$(basename "$dir_name" 2>/dev/null || printf '')"

    case "$base_name:$dir_base" in
        *oxidns*|*:oxidns*) return 0 ;;
        *) return 1 ;;
    esac
}

path_has_parent_ref() {
    case "$1" in
        ..|../*|*/..|*/../*) return 0 ;;
        *) return 1 ;;
    esac
}

safe_remove_openwrt_config_path() {
    path_value="$1"

    [ -n "$path_value" ] || return 1
    path_has_parent_ref "$path_value" && return 1
    case "$path_value" in
        /*) ;;
        *) return 1 ;;
    esac
    [ -d "$path_value" ] && return 1
    oxidns_named_path "$path_value"
}

safe_remove_openwrt_working_dir() {
    path_value="$1"

    [ -n "$path_value" ] || return 1
    path_has_parent_ref "$path_value" && return 1
    case "$path_value" in
        /*) ;;
        *) return 1 ;;
    esac
    case "$path_value" in
        /|/bin|/dev|/etc|/lib|/mnt|/overlay|/proc|/rom|/root|/sbin|/sys|/tmp|/usr|/var|/www)
            return 1
            ;;
    esac
    oxidns_named_path "$path_value"
}

openwrt_package_manager() {
    if command -v opkg >/dev/null 2>&1; then
        printf 'opkg'
    elif command -v apk >/dev/null 2>&1; then
        printf 'apk'
    else
        err "OpenWrt package manager not found: expected opkg or apk"
    fi
}

openwrt_package_installed() {
    package_manager="$1"
    package_name="$2"

    case "$package_manager" in
        opkg)
            opkg status "$package_name" 2>/dev/null | grep -q '^Status: .* installed'
            ;;
        apk)
            apk info -e "$package_name" >/dev/null 2>&1
            ;;
        *)
            return 1
            ;;
    esac
}

openwrt_remove_package() {
    package_manager="$1"
    package_name="$2"

    if ! openwrt_package_installed "$package_manager" "$package_name"; then
        log "OpenWrt package not installed: $package_name"
        return 0
    fi

    log "Removing OpenWrt package: $package_name"
    case "$package_manager" in
        opkg)
            opkg remove "$package_name"
            ;;
        apk)
            apk del "$package_name"
            ;;
        *)
            err "unsupported OpenWrt package manager: $package_manager"
            ;;
    esac
}

openwrt_stop_service() {
    init="/etc/init.d/oxidns"

    if [ ! -x "$init" ]; then
        return 0
    fi

    "$init" stop >/dev/null 2>&1 || true
    "$init" disable >/dev/null 2>&1 || true
    log "Stopped and disabled OpenWrt service: oxidns"
}

restore_openwrt_uci_config() {
    backup_file="$1"

    [ -n "$backup_file" ] || return 0
    [ -f "$backup_file" ] || return 0
    [ ! -e "$OPENWRT_UCI_CONFIG" ] || return 0

    mkdir -p "$(dirname "$OPENWRT_UCI_CONFIG")"
    cp "$backup_file" "$OPENWRT_UCI_CONFIG"
    log "Restored LuCI settings: $OPENWRT_UCI_CONFIG"
}

install_openwrt_uninstall_trap() {
    tmp_dir="$1"
    trap 'rm -rf "$tmp_dir"' EXIT HUP INT TERM
}

uninstall_openwrt() {
    is_root || err "OpenWrt uninstall requires root; rerun as root"

    package_manager="$(openwrt_package_manager)"
    config_path="${OPENWRT_CONFIG_PATH:-$(uci_get_oxidns config_path "$OPENWRT_DEFAULT_CONFIG_PATH")}"
    working_dir="${OPENWRT_WORKING_DIR:-$(uci_get_oxidns working_dir "$OPENWRT_DEFAULT_WORKING_DIR")}"
    tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/oxidns-openwrt-uninstall.XXXXXX")"
    uci_backup=""

    install_openwrt_uninstall_trap "$tmp_dir"

    if ! is_truthy "$PURGE" && [ -f "$OPENWRT_UCI_CONFIG" ]; then
        uci_backup="$tmp_dir/uci-oxidns"
        cp "$OPENWRT_UCI_CONFIG" "$uci_backup"
    fi

    if should_uninstall_service; then
        openwrt_stop_service
    fi

    openwrt_remove_package "$package_manager" "luci-i18n-oxidns-zh-cn"
    openwrt_remove_package "$package_manager" "luci-app-oxidns"
    restore_openwrt_uci_config "$uci_backup"

    rm -f "$OPENWRT_BIN"
    rm -rf "$OPENWRT_WEBUI_DIR"
    rmdir "$(dirname "$OPENWRT_WEBUI_DIR")" 2>/dev/null || true
    log "Removed OxiDNS core binary: $OPENWRT_BIN"
    log "Removed OxiDNS WebUI: $OPENWRT_WEBUI_DIR"

    if is_truthy "$PURGE"; then
        if [ -e "$config_path" ]; then
            safe_remove_openwrt_config_path "$config_path" || err "refusing to delete unsafe OpenWrt config path: $config_path"
            rm -f "$config_path"
            rmdir "$(dirname "$config_path")" 2>/dev/null || true
            log "Removed config: $config_path"
        fi

        if [ -e "$working_dir" ]; then
            safe_remove_openwrt_working_dir "$working_dir" || err "refusing to delete unsafe OpenWrt working directory: $working_dir"
            rm -rf "$working_dir"
            log "Removed working directory: $working_dir"
        fi

        if [ -e "$OPENWRT_UCI_CONFIG" ]; then
            safe_remove_openwrt_config_path "$OPENWRT_UCI_CONFIG" || err "refusing to delete unsafe OpenWrt LuCI settings path: $OPENWRT_UCI_CONFIG"
            rm -f "$OPENWRT_UCI_CONFIG"
            log "Removed LuCI settings: $OPENWRT_UCI_CONFIG"
        fi
    else
        if [ -f "$config_path" ]; then
            log "Kept config: $config_path"
        fi
        if [ -d "$working_dir" ]; then
            log "Kept working directory: $working_dir"
        fi
        log "Use OXIDNS_PURGE=1 to remove OpenWrt config and working directory."
    fi

    if [ -x /etc/init.d/rpcd ]; then
        /etc/init.d/rpcd restart >/dev/null 2>&1 || warn "failed to restart rpcd"
    fi

    log "OxiDNS OpenWrt uninstall complete"
}

same_file() {
    a="$1"
    b="$2"

    if [ ! -e "$a" ] || [ ! -e "$b" ]; then
        return 1
    fi

    if command -v cmp >/dev/null 2>&1; then
        cmp -s "$a" "$b"
    else
        return 1
    fi
}

safe_purge_dir() {
    dir="$1"

    [ -n "$dir" ] || return 1

    if [ -d "$dir" ]; then
        dir_check="$(cd "$dir" && pwd -P)"
    else
        dir_check="$dir"
    fi

    if [ -n "$HOME_DIR" ] && [ "$dir_check" = "$HOME_DIR" ]; then
        return 1
    fi

    case "$dir_check" in
        /|/bin|/sbin|/usr|/usr/bin|/usr/local|/usr/local/bin|/opt|/etc|/var|/tmp)
            return 1
            ;;
        *)
            return 0
            ;;
    esac
}

remove_command_shim() {
    link_path="$BIN_DIR/oxidns"

    if [ "$BIN_DIR" = "$INSTALL_DIR" ]; then
        return 0
    fi

    if [ -L "$link_path" ]; then
        target="$(readlink "$link_path" 2>/dev/null || printf '')"
        if [ "$target" = "$INSTALL_DIR/oxidns" ]; then
            rm -f "$link_path"
            log "Removed command shim: $link_path"
        else
            warn "$link_path points to $target; leaving it unchanged"
        fi
        return 0
    fi

    if [ -f "$link_path" ]; then
        if same_file "$link_path" "$INSTALL_DIR/oxidns"; then
            rm -f "$link_path"
            log "Removed copied command: $link_path"
        else
            warn "$link_path is not managed by this installer; leaving it unchanged"
        fi
    fi
}

uninstall_service() {
    bin="$INSTALL_DIR/oxidns"

    if [ ! -x "$bin" ]; then
        warn "cannot uninstall service because $bin was not found"
        return 0
    fi

    "$bin" service stop >/dev/null 2>&1 || true
    if "$bin" service uninstall >/dev/null 2>&1; then
        log "Removed OxiDNS service"
    else
        warn "service uninstall failed or service was not installed"
    fi
}

if should_uninstall_openwrt; then
    uninstall_openwrt
    exit 0
fi

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

if should_uninstall_service; then
    uninstall_service
fi

if ! is_truthy "$NO_PATH"; then
    remove_command_shim
fi

if is_truthy "$PURGE"; then
    if safe_purge_dir "$INSTALL_DIR"; then
        rm -rf "$INSTALL_DIR"
        log "Purged OxiDNS install directory: $INSTALL_DIR"
    else
        err "refusing to purge unsafe install directory: $INSTALL_DIR"
    fi
else
    rm -f "$INSTALL_DIR/oxidns" "$INSTALL_DIR/oxidns.tmp" "$INSTALL_DIR/LICENSE"
    rm -rf "$INSTALL_DIR/webui"
    log "Removed OxiDNS binary and WebUI from $INSTALL_DIR"
    if [ -f "$INSTALL_DIR/config.yaml" ]; then
        log "Kept config: $INSTALL_DIR/config.yaml"
        log "Use OXIDNS_PURGE=1 to remove the install directory and config."
    fi
fi

log "OxiDNS uninstall complete"
