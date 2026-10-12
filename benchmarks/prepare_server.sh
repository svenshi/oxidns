#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Sven Shi
# SPDX-License-Identifier: GPL-3.0-or-later

set -euo pipefail

BASE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$BASE_DIR/.." && pwd)"
TOOLS_DIR="${BENCH_TOOLS_DIR:-$BASE_DIR/.tools}"
DNSPERF_VERSION="${DNSPERF_VERSION:-2.15.1}"
MOSDNS_VERSION="${MOSDNS_VERSION:-v5.3.4}"
OXIDNS_VERSION="${OXIDNS_VERSION:-latest}"
ADGUARDHOME_VERSION="${ADGUARDHOME_VERSION:-latest}"
SMARTDNS_VERSION="${SMARTDNS_VERSION:-latest}"

mkdir -p "$TOOLS_DIR"

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "prepare_server.sh supports Linux benchmark hosts only" >&2
  exit 1
fi

if [[ "${BENCH_INSTALL_DEPS:-1}" == "1" ]]; then
  if ! command -v apt-get >/dev/null 2>&1; then
    echo "automatic dependency installation currently supports Debian/Ubuntu (apt-get)" >&2
    exit 1
  fi
  apt-get update
  DEBIAN_FRONTEND=noninteractive apt-get install -y \
    build-essential ca-certificates curl git libcap-dev libck-dev libjson-c-dev libkrb5-dev \
    libnghttp2-dev libssl-dev libxml2-dev pkg-config unzip xz-utils
fi

case "$(uname -m)" in
  x86_64) mosdns_arch="amd64"; oxidns_target="x86_64-unknown-linux-musl"; adguard_arch="amd64"; smartdns_asset_arch="x86_64-linux-all" ;;
  aarch64|arm64) mosdns_arch="arm64"; oxidns_target="aarch64-unknown-linux-musl"; adguard_arch="arm64"; smartdns_asset_arch="aarch64-linux-all" ;;
  armv7l|armv7) mosdns_arch="armv7"; oxidns_target="armv7-unknown-linux-musleabihf"; adguard_arch="armv7"; smartdns_asset_arch="arm-linux-all" ;;
  i386|i686) mosdns_arch="386"; oxidns_target="i686-unknown-linux-musl"; adguard_arch="386"; smartdns_asset_arch="x86-linux-all" ;;
  *) echo "unsupported mosdns architecture: $(uname -m)" >&2; exit 1 ;;
esac

if [[ "${BENCH_BUILD_OXIDNS:-0}" == "1" ]]; then
  if ! command -v cargo >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o "$TOOLS_DIR/rustup-init.sh"
    sh "$TOOLS_DIR/rustup-init.sh" -y --profile minimal
    export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
  fi
  echo "building OxiDNS from $(git -C "$REPO_DIR" describe --tags --always --dirty)"
  cargo build --release --manifest-path "$REPO_DIR/Cargo.toml"
  install -m 0755 "$REPO_DIR/target/release/oxidns" "$BASE_DIR/oxidns"
else
  if [[ "$OXIDNS_VERSION" == "latest" ]]; then
    OXIDNS_VERSION="$(curl -fsSL https://api.github.com/repos/SvenShi/oxidns/releases/latest | python3 -c 'import json,sys; print(json.load(sys.stdin)["tag_name"])')"
  fi
  oxidns_archive="$TOOLS_DIR/oxidns-${OXIDNS_VERSION}-${oxidns_target}.tar.gz"
  curl -fL "https://github.com/SvenShi/oxidns/releases/download/${OXIDNS_VERSION}/oxidns-${oxidns_target}.tar.gz" -o "$oxidns_archive"
  mkdir -p "$TOOLS_DIR/oxidns-extract"
  tar -xzf "$oxidns_archive" -C "$TOOLS_DIR/oxidns-extract"
  oxidns_source="$(find "$TOOLS_DIR/oxidns-extract" -type f -name oxidns -perm -u+x -print -quit)"
  if [[ -z "$oxidns_source" ]]; then
    echo "oxidns executable not found in $oxidns_archive" >&2
    exit 1
  fi
  install -m 0755 "$oxidns_source" "$BASE_DIR/oxidns"
fi

mosdns_zip="$TOOLS_DIR/mosdns-linux-${mosdns_arch}.zip"
curl -fL "https://github.com/IrineSistiana/mosdns/releases/download/${MOSDNS_VERSION}/mosdns-linux-${mosdns_arch}.zip" -o "$mosdns_zip"
unzip -jo "$mosdns_zip" 'mosdns' -d "$TOOLS_DIR/mosdns-extract"
install -m 0755 "$TOOLS_DIR/mosdns-extract/mosdns" "$BASE_DIR/mosdns"

if [[ "$ADGUARDHOME_VERSION" == "latest" ]]; then
  ADGUARDHOME_VERSION="$(curl -fsSL https://api.github.com/repos/AdguardTeam/AdGuardHome/releases/latest | python3 -c 'import json,sys; print(json.load(sys.stdin)["tag_name"])')"
fi
adguard_archive="$TOOLS_DIR/AdGuardHome_${ADGUARDHOME_VERSION}_linux_${adguard_arch}.tar.gz"
curl -fL "https://github.com/AdguardTeam/AdGuardHome/releases/download/${ADGUARDHOME_VERSION}/AdGuardHome_linux_${adguard_arch}.tar.gz" -o "$adguard_archive"
mkdir -p "$TOOLS_DIR/adguardhome-extract"
tar -xzf "$adguard_archive" -C "$TOOLS_DIR/adguardhome-extract"
adguard_source="$(find "$TOOLS_DIR/adguardhome-extract" -type f -name AdGuardHome -perm -u+x -print -quit)"
install -m 0755 "$adguard_source" "$BASE_DIR/AdGuardHome"

smartdns_release_json="$TOOLS_DIR/smartdns-release.json"
if [[ "$SMARTDNS_VERSION" == "latest" ]]; then
  curl -fsSL https://api.github.com/repos/pymumu/smartdns/releases/latest -o "$smartdns_release_json"
else
  curl -fsSL "https://api.github.com/repos/pymumu/smartdns/releases/tags/${SMARTDNS_VERSION}" -o "$smartdns_release_json"
fi
SMARTDNS_VERSION="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["tag_name"])' "$smartdns_release_json")"
smartdns_url="$(python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); needle=sys.argv[2]; print(next(a["browser_download_url"] for a in d["assets"] if needle in a["name"] and a["name"].endswith(".tar.gz")))' "$smartdns_release_json" "$smartdns_asset_arch")"
smartdns_archive="$TOOLS_DIR/smartdns-${SMARTDNS_VERSION}-${smartdns_asset_arch}.tar.gz"
curl -fL "$smartdns_url" -o "$smartdns_archive"
mkdir -p "$TOOLS_DIR/smartdns-extract"
tar -xzf "$smartdns_archive" -C "$TOOLS_DIR/smartdns-extract"
smartdns_source="$(find "$TOOLS_DIR/smartdns-extract" -type f -name smartdns -perm -u+x -print -quit)"
smartdns_runner="$(find "$(dirname "$smartdns_source")" -maxdepth 1 -type f -name run-smartdns -perm -u+x -print -quit)"
ln -sfn "$smartdns_runner" "$BASE_DIR/smartdns"

dnsperf_archive="$TOOLS_DIR/dnsperf-${DNSPERF_VERSION}.tar.gz"
curl -fL "https://www.dns-oarc.net/files/dnsperf/dnsperf-${DNSPERF_VERSION}.tar.gz" -o "$dnsperf_archive"
tar -xzf "$dnsperf_archive" -C "$TOOLS_DIR"
(
  cd "$TOOLS_DIR/dnsperf-${DNSPERF_VERSION}"
  ./configure --prefix="$TOOLS_DIR/dnsperf-install"
  make -j"$(getconf _NPROCESSORS_ONLN)"
  make install
)

dnsperf_bin="$TOOLS_DIR/dnsperf-install/bin/dnsperf"
if ! "$dnsperf_bin" -h 2>&1 | grep -q -- '-j'; then
  echo "built dnsperf does not expose JSON output (-j)" >&2
  exit 1
fi
if ! "$dnsperf_bin" -H 2>&1 | grep -q 'latency-histogram'; then
  echo "built dnsperf does not expose latency histograms" >&2
  exit 1
fi

cat <<EOF
Benchmark tools are ready:
  OxiDNS: $BASE_DIR/oxidns
  mosdns: $BASE_DIR/mosdns ($MOSDNS_VERSION)
  AdGuard Home: $BASE_DIR/AdGuardHome ($ADGUARDHOME_VERSION)
  SmartDNS: $BASE_DIR/smartdns ($SMARTDNS_VERSION)
  dnsperf: $dnsperf_bin ($DNSPERF_VERSION)

Run the publishable matrix:
  DNSPERF_BIN_PATH=$dnsperf_bin ./run_publishable_compare.py --publish-docs

Run the separate OxiDNS/mosdns native-equivalent suite:
  DNSPERF_BIN_PATH=$dnsperf_bin ./run_publishable_compare.py --engines oxidns,mosdns --publish-native-specialized native-specialized

Run the OxiDNS short-circuit A/B diagnostic:
  DNSPERF_BIN_PATH=$dnsperf_bin ./run_publishable_compare.py --engines oxidns oxidns-features
EOF
