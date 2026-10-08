#!/usr/bin/env bash
# Publish missing support-crate versions before the root crate.

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$repo_root"
metadata=$(cargo metadata --locked --no-deps --format-version 1)
response_path=$(mktemp)
trap 'rm -f "$response_path"' EXIT

# Publish dependencies before their consumers.
for manifest in \
  crates/proto/Cargo.toml \
  crates/macros/Cargo.toml \
  crates/ripset/Cargo.toml \
  crates/zoneparser/Cargo.toml; do
  crate_info=$(jq -er --arg manifest "$repo_root/$manifest" \
    '.packages[] | select(.manifest_path == $manifest) | [.name, .version] | @tsv' <<< "$metadata")
  read -r crate_name crate_version <<< "$crate_info"
  status=$(curl --silent --show-error --location --retry 3 --retry-delay 2 \
    --connect-timeout 10 --max-time 60 \
    --user-agent 'OxiDNS release workflow (github.com/svenshi/oxidns)' \
    --output "$response_path" --write-out '%{http_code}' \
    "https://crates.io/api/v1/crates/$crate_name/$crate_version")

  case "$status" in
    200)
      if ! jq -e --arg name "$crate_name" --arg version "$crate_version" \
        '.version | .crate == $name and .num == $version and .yanked == false' \
        "$response_path" > /dev/null; then
        echo "::error::Invalid registry metadata or yanked version: $crate_name $crate_version"
        exit 1
      fi
      echo "Skipping $crate_name $crate_version: already published"
      ;;
    404)
      cargo publish --locked --manifest-path "$manifest"
      ;;
    *)
      echo "::error::Registry lookup failed for $crate_name $crate_version (HTTP $status)"
      exit 1
      ;;
  esac
done
