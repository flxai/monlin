# monlin

`monlin` is a compact terminal monitor for narrow panes, bars, and shell-driven
status views.

It renders dense multi-metric layouts with a small DSL, sane defaults, and
braille-first history graphs.

## Quick Start

Default compact layout:

```sh
monlin
monlin avail
```

Exhaustive built-in layout:

```sh
monlin all
```

One-shot preview:

```sh
monlin --once --interval-ms 0 --width 96 --color never
```

Packed graph-only output:

```sh
monlin -p avail
printf '10 20 30\n' | monlin -p '@1 @2 @3'
```

Explicit stdin columns:

```sh
printf '10 20 30\n' | monlin '@1 @2 @3'
printf '10 20 30\n' | monlin '@1.bare @2.bare @3.bare'
```

Polled sources:

```sh
monlin "a=p:printf '10\n' b=p:printf '20\n'"
monlin "left=f:/tmp/a right=f:/tmp/b"
```

## Build And Run

Build from the flake:

```sh
nix build 'path:/home/flx/pro/git/monlin#'
```

Run directly:

```sh
nix run 'path:/home/flx/pro/git/monlin'
```

Show help:

```sh
nix run 'path:/home/flx/pro/git/monlin' -- --help
```

## Config Files

`monlin` prefers TOML config files and merges them in this order:

1. built-in defaults
2. system config from `/etc/xdg/monlin/config.toml`
3. user config from `$XDG_CONFIG_HOME/monlin/config.toml`
4. user config fallback `~/.config/monlin/config.toml`
5. explicit CLI flags

Later layers override earlier ones.

Example:

```toml
align = "right"
colors = "gruvbox"
solid_colors = true
invert_vertical = false
history = 128
interval_ms = 250
window = "tail"
layout = "avail"
```

List-like fields can also be written as arrays:

```toml
colors = ["gruvbox", "320"]
```

Boolean config values are explicit, so `solid_colors = false` or `invert_vertical = false`
can override an earlier config layer that enabled them.

## Layout DSL

The layout syntax is the main interface.

These are equivalent:

```sh
monlin cpu ram spc io net
monlin "cpu ram spc io net"
```

Positional layout arguments are joined with spaces before parsing. Quote the
whole layout when it contains spaces, parentheses, or `p:...` commands.

The canonical item syntax is:

```text
source[.modifier...][:size][+max][-min]
```

Examples:

```sh
monlin "cpu"
monlin "cpu.abs.value"
monlin "cpu.abs.value:12+20-8"
monlin "rx tx, rx.inv tx.inv"
monlin "@1.bare"
monlin "temp=p:printf '42\n'"
monlin "a=p:'printf 10'," "b=p:'printf 20'.inv"
```

Read `cpu.abs.value:12+20-8` left to right:

- `cpu`: source
- `.abs`: absolute value mode
- `.value`: show only the value text, not the label
- `:12`: basis width
- `+20`: maximum width
- `-8`: preferred minimum width

An item can be:

- a native source like `cpu`, `ram`, or `net`
- a stream column like `@1`
- a file source like `f:/tmp/data`
- a process source like `p:printf '10\n'`
- a labeled item like `cpu=@1` or `load=p:'cut -f1 /proc/loadavg'`

Modifiers:

- `.pct` or `.abs` selects the value mode
- `.full`, `.value`, or `.bare` selects how much text to show
- `.inv` flips the graph vertically for that item
- `:size` sets the item's basis width
- `+max` caps how wide the item may expand
- `-min` asks for at least that many columns when there is room

Legacy `!full`, `!value`, and `!bare` suffixes are still accepted for compatibility.

Items on the same row are separated by spaces. Rows are separated by:

- `,`
- or a literal newline

Shell arguments are still joined with spaces before parsing, so a trailing comma at
an argv boundary still means "end this row". For example:

```sh
monlin "a=p:'printf 10'," "b=p:'printf 20'.inv"
```

Without explicit row separators, flat layouts auto-wrap after 5 items.

Labels and groups:

- `label=source`
- `label=(source source)`
- `label=(source, source)`

Read those as:

- `label=source`: label one item
- `label=(a b)`: label a group whose items stay on the same row
- `label=(a, b)`: label a group split across multiple rows

Split composition uses `+` inside a single item:

- `cpu gpu`: two separate items
- `cpu+gpu`: one split item

Examples:

```sh
monlin "cpu ram spc io net"
monlin "xpu mem spc, io net"
monlin "spcram=(spc+ram)"
monlin "spcram=(spc,ram)"
monlin "rx tx, rx.inv tx.inv"
monlin "graph=(cpu=@1 ram=@2)"
monlin "disk=(in.abs out.abs) net=(rx.abs tx.abs)"
monlin "a=p:printf '10\n' b=p:printf '20\n'"
monlin "a=p:'printf 10'," "b=p:'printf 20'.inv"
```

## Sources, Aliases, And Defaults

Native sources:

- `cpu`
- `gpu`
- `ram` or `memory`
- `vram` or `vrm`
- `spc`, `storage`, `disk`, or `space`
- `in`, `out`
- `rx`, `tx`
- `rnd`

Grouped or split aliases:

- `xpu` = `cpu gpu`
- `mem` = `ram vram`
- `sys` = `cpu+ram`
- `gfx` = `gpu+vram`
- `io` = `in+out`
- `net` = `rx+tx`

Special layouts:

- `avail`
- `all`

External sources:

- `@1`, `@2`, ... for stdin stream columns
- `f:/path/to/file` to poll a file for numeric rows
- `p:command` to poll a shell command for numeric rows

`f:/...` works especially well for sysfs and other generated numeric files:

```sh
monlin "cpu=f:/sys/class/thermal/thermal_zone0/temp"
monlin "bat=f:/sys/class/power_supply/BAT0/capacity"
monlin "bl=f:/sys/class/backlight/intel_backlight/brightness"
monlin "fan=f:/sys/class/hwmon/hwmon1/fan1_input"
monlin "eth=f:/sys/class/net/enp3s0/speed"
```

If you want transformed values, normalize or convert them before feeding them to
`monlin`:

```sh
monlin "bl=p:awk '{ print $1 / $2 * 100 }' /sys/class/backlight/intel_backlight/brightness /sys/class/backlight/intel_backlight/max_brightness"
monlin "cpu=p:awk '{ print $1 / 1000 }' /sys/class/thermal/thermal_zone0/temp"
```

Value modes:

- `.pct`
- `.abs`

Display modifiers:

- `.full`
- `.value`
- `.bare`

Graph modifiers:

- `.inv`

Graph modifiers:

- `.inv`

Defaults are metric-aware:

- compute-style metrics default to `.pct`
- memory, storage, disk, and network metrics default to `.abs`

For example:

- `cpu` -> ` 37%`
- `ram` -> ` 15G`
- `spc` -> `412G`
- `net` -> `3.5M`
- `rx` -> `812K`

## CLI Flags

Core flags:

- `LAYOUT` as positional layout arguments
- `--history N`
- `-i`, `--interval-ms N`
- `--align left|right`
- `-w`, `--window tail|agg`
- `-p`, `--packed`
- `--solid-colors`, `--no-solid-colors`
- `--invert-vertical`, `--no-invert-vertical`
- `-e`, `--engine auto|flow|flex|grid|pack`
- `--renderer braille|block`
- `-c`, `--colors ...`
- `--color auto|always|never`
- `--output terminal|i3bar`
- `--width N`
- `--once`
- `-h`, `--help`

Examples:

```sh
monlin avail --history 512 -i 1000
monlin io net --renderer block
monlin -c gruvbox --solid-colors
monlin --invert-vertical io net
monlin --output i3bar all
```

## Completion

Generate shell completion scripts:

```sh
monlin completion zsh
monlin completion bash
monlin completion fish
```

List supported `-c` names:

```sh
monlin completion colors
```

## Debugging The Renderer

`monlin` includes deterministic debug commands for windowing and braille output.

Inspect history windowing:

```sh
monlin debug window --samples '0,1,0,1' --renderer braille --width 2
```

Inspect scalar braille cells:

```sh
monlin debug braille --samples '0,1,0,1' --width 2
```

Inspect split braille cells:

```sh
monlin debug braille --split-upper '1,0' --split-lower '0,1' --width 1
```

Preview propagation frame by frame:

```sh
monlin debug braille --samples '0,0,1,1,0' --frames
```

## Rendering Notes

- default renderer: braille
- fallback renderer: block
- default window mode: `tail`
- default alignment: `right`
- labels stay plain
- graph glyphs are colorized
- split metrics render upper and lower halves separately

When `--output i3bar` is used:

- `monlin` emits the i3bar JSON protocol, not JSONL
- the header is printed once
- each sampled frame becomes one i3bar status-line array
- each rendered `monlin` row becomes one i3bar block
- ANSI colors are disabled automatically in this mode

## Verification

One local verification sequence is:

```sh
cargo fmt --check
cargo clippy
cargo test
nix flake check 'path:/home/flx/pro/git/monlin'
```

For an interactive pinned toolchain shell, use:

```sh
nix develop 'path:/home/flx/pro/git/monlin'
```

Quick smoke tests:

```sh
nix run 'path:/home/flx/pro/git/monlin' -- --once --interval-ms 0 --width 96 --color never
nix run 'path:/home/flx/pro/git/monlin' -- --once --interval-ms 0 --width 96 --color never avail
nix run 'path:/home/flx/pro/git/monlin' -- --once --interval-ms 0 --width 96 --color never 'spcram=(spc+ram)'
nix run 'path:/home/flx/pro/git/monlin' -- debug braille --samples '0,1,0,1' --width 2
```
