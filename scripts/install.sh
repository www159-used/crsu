#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/install.sh [--root <directory>]

Builds and installs the crsu binary from this checkout.

Options:
  --root <directory>  Install under <directory>/bin instead of Cargo's default.
  -h, --help          Show this help message.
EOF
}

install_root=""
while (($# > 0)); do
    case "$1" in
        --root)
            [[ $# -ge 2 ]] || { echo "--root requires a directory" >&2; exit 2; }
            install_root="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

command -v cargo >/dev/null 2>&1 || {
    echo "cargo is required; install Rust first: https://rustup.rs" >&2
    exit 1
}

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
package_root="$project_root/crates/crsu"
arguments=(install --path "$package_root" --locked --force)
if [[ -n "$install_root" ]]; then
    arguments+=(--root "$install_root")
fi

cargo "${arguments[@]}"

if [[ -n "$install_root" ]]; then
    binary="$install_root/bin/crsu"
else
    binary="${CARGO_HOME:-$HOME/.cargo}/bin/crsu"
fi

echo "Installed crsu: $binary"
