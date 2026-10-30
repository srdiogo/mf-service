#!/usr/bin/env bash
# Standalone installer for the verified MiniFuse installation kit.
set -euo pipefail
installer_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
if (( EUID == 0 )); then
  echo "Run this installer as your graphical-session user, without sudo." >&2
  exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
  case " ${*:-} " in
    *" --help "*|*" -h "*|*" --dry-run "*|*" --check "*)
      echo "Python 3.12+ is required. Normal installation can bootstrap it through pacman."
      exit 1 ;;
  esac
  command -v pacman >/dev/null || { echo "This kit requires Arch Linux." >&2; exit 1; }
  read -r -p "Install Python through pacman to start MiniFuse setup? [y/N] " installer_reply
  [[ $installer_reply =~ ^[Yy]$ ]] || exit 1
  sudo -- /usr/bin/pacman -S --needed -- python
fi
exec python3 "$installer_dir/installer.py" --bundle "$installer_dir" "$@"
