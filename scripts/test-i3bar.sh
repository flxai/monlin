#!/usr/bin/env bash
set -euo pipefail

display_num="${1:-1}"
layout_spec="${2:-all/2}"
xephyr_screen="${XEPHYR_SCREEN:-1280x720}"
font_spec="${I3_FONT:-pango:monospace 10}"

if [[ -f "$PWD/flake.nix" ]]; then
  repo_root="$PWD"
else
  repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fi

pick_terminal() {
  if [[ -n "${MONLIN_TEST_TERM:-}" ]]; then
    printf '%s\n' "${MONLIN_TEST_TERM}"
    return 0
  fi

  local candidate
  for candidate in \
    i3-sensible-terminal \
    x-terminal-emulator \
    alacritty \
    kitty \
    foot \
    wezterm \
    gnome-terminal \
    xterm
  do
    if command -v "${candidate}" >/dev/null 2>&1; then
      printf '%s\n' "${candidate}"
      return 0
    fi
  done

  return 1
}

pick_monlin() {
  if [[ -n "${MONLIN_TEST_MONLIN_CMD:-}" ]]; then
    printf '%s\n' "${MONLIN_TEST_MONLIN_CMD}"
    return 0
  fi

  if command -v monlin >/dev/null 2>&1; then
    if monlin --help 2>/dev/null | grep -q -- '--output'; then
      command -v monlin
      return 0
    fi
  fi

  return 1
}

if ! command -v Xephyr >/dev/null 2>&1; then
  echo "test-i3bar: Xephyr not found" >&2
  exit 1
fi

if ! command -v i3 >/dev/null 2>&1; then
  echo "test-i3bar: i3 not found" >&2
  exit 1
fi

if ! terminal_cmd="$(pick_terminal)"; then
  echo "test-i3bar: no terminal found; set MONLIN_TEST_TERM explicitly" >&2
  exit 1
fi

tmpdir="$(mktemp -d)"
runtime_dir="${tmpdir}/runtime"
mkdir -p "${runtime_dir}/i3"
chmod 700 "${runtime_dir}"

if ! monlin_cmd="$(pick_monlin)"; then
  echo "test-i3bar: building local monlin because no suitable installed binary was found" >&2
  nix build "path:${repo_root}#" --out-link "${tmpdir}/result" >/dev/null
  monlin_cmd="${tmpdir}/result/bin/monlin"
fi

cleanup() {
  if [[ -n "${i3_pid:-}" ]]; then
    kill "${i3_pid}" 2>/dev/null || true
  fi
  if [[ -n "${xephyr_pid:-}" ]]; then
    kill "${xephyr_pid}" 2>/dev/null || true
  fi
  rm -rf "${tmpdir}"
}
trap cleanup EXIT

cat >"${tmpdir}/status-command.sh" <<EOF
#!/usr/bin/env bash
exec "${monlin_cmd}" --output i3bar --layout "${layout_spec}" 2>>"${tmpdir}/status-command.log"
EOF
chmod 700 "${tmpdir}/status-command.sh"

cat >"${tmpdir}/i3-monlin.conf" <<EOF
font ${font_spec}

bar {
  id monlin-test
  position top
  status_command ${tmpdir}/status-command.sh
}

exec ${terminal_cmd}
EOF

Xephyr ":${display_num}" -screen "${xephyr_screen}" &
xephyr_pid=$!
sleep 1

env -u I3SOCK \
  DISPLAY=":${display_num}" \
  XDG_RUNTIME_DIR="${runtime_dir}" \
  i3 -c "${tmpdir}/i3-monlin.conf" &
i3_pid=$!

wait "${i3_pid}"
