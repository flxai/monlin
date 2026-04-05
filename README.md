# monlin

`monlin` is a compact terminal monitor for narrow panes, bars, and shell-driven
status views.

It started as the monitor inside `nxu` tmux panes, but it has grown into a
general CLI for rendering multi-metric, multi-line monitors with a small layout
DSL.

## Goals

- dense output that still reads well in narrow terminals
- stable shell-facing CLI
- good defaults with or without config files
- braille-first history rendering
- explicit, inspectable layout and rendering behavior

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

One-shot render:

```sh
nix run 'path:/home/flx/pro/git/monlin' -- --once --interval-ms 0
```

## Default Behavior

With no arguments, `monlin` renders the compact available layout:

```sh
monlin
monlin avail
```

The exhaustive layout is:

```sh
monlin all
```

`avail` adapts to the host. For example, on non-GPU systems it naturally
collapses from `xpu mem spc io net` to something like `cpu ram spc io net`.

## Config Files

`monlin` prefers TOML config files and merges them in this order:

1. built-in defaults
2. system config from `/etc/xdg/monlin/config.toml`
3. user config from `$XDG_CONFIG_HOME/monlin/config.toml`
4. user config fallback `~/.config/monlin/config.toml`
5. explicit CLI flags

Legacy shell-word configs at `.../config` are still supported for compatibility.

Example:

```toml
colors = "gruvbox"
solid_colors = true
history = 128
interval_ms = 250
window = "tail"
layout = "avail"
```

List-like fields can also be written as arrays:

```toml
colors = ["gruvbox", "320"]
```

## Sources And Aliases

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

External and streamed sources:

- `@1`, `@2`, ... for stdin stream columns
- `f:/path/to/file` to poll a file for numeric rows
- `p:command` to poll a shell command for numeric rows

## Views And Display Modes

Value modes:

- `.pct`
- `.abs`

Display modes:

- `.full`
- `.value`
- `.bare`

Examples:

```sh
monlin "cpu.pct ram.abs"
monlin "net.abs io.abs"
monlin "@1.bare @2.bare @3.bare"
monlin "cpu.value gpu.value"
```

Defaults are metric-aware:

- compute-style metrics default to `.pct`
- memory, storage, disk, and network metrics default to `.abs`

So for example:

- `cpu` -> ` 37%`
- `ram` -> ` 15G`
- `spc` -> `412G`
- `net` -> `3.5M`
- `rx` -> `812K`

## Layout DSL

The canonical item syntax is:

```text
source[.value_mode][.display_mode][:size][+max][-min]
```

Where:

- `source` selects what to render
- `.pct` or `.abs` selects the value mode
- `.full`, `.value`, or `.bare` selects how much text to show
- `:size` reserves width for the item
- `+max` caps how wide the item may expand
- `-min` prefers at least that many columns when there is room

Rows are separated with:

- `,`
- or a literal newline

Labels and groups:

- `label=source`
- `label=(source source)`
- `label=(source, source)`

Split composition uses `+`:

- `spcram=(spc+ram)`
- `cpu+gpu`
- `rx+tx`

Examples:

```sh
monlin "cpu ram spc io net"
monlin "xpu mem spc, io net"
monlin "spcram=(spc+ram)"
monlin "spcram=(spc,ram)"
monlin "graph=(cpu=@1 ram=@2)"
monlin "disk=(in.abs out.abs) net=(rx.abs tx.abs)"
```

## Common Invocations

Compact host overview:

```sh
monlin
monlin avail
```

Exhaustive built-in layout:

```sh
monlin all
```

Packed graph-only output:

```sh
monlin -p avail
printf '10 20 30\n' | monlin -p '@1 @2 @3'
```

Explicit stream columns:

```sh
printf '10 20 30\n' | monlin '@1 @2 @3'
printf '10 20 30\n' | monlin '@1.bare @2.bare @3.bare'
```

Dense split summaries:

```sh
monlin "io net"
monlin "in out rx tx"
```

Alignment and window behavior:

```sh
monlin --align right
monlin --align left
monlin --window tail
monlin --window agg
```

No color:

```sh
monlin --color never
```

One-shot preview at fixed width:

```sh
monlin --once --interval-ms 0 --width 96 --color never avail
```

## CLI Flags

Core flags:

- `LAYOUT` as positional layout arguments
- `--history N`
- `-i`, `--interval-ms N`
- `--align left|right`
- `-w`, `--window tail|agg`
- `-p`, `--packed`
- `--solid-colors`
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
