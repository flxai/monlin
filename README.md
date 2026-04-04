# monlin

`monlin` is a compact terminal monitor for narrow panes, bars, and shell-driven
status views.

It started as the monitor inside `nxu` tmux panes, but it has grown into a
general CLI for rendering multi-metric, multi-line monitors with a small layout
DSL.

## Goals

- dense output that still reads well in narrow terminals
- stable shell-facing CLI
- good defaults without a config file
- braille-first history rendering
- metric-aware colors and headline units

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

With no arguments, `monlin` renders the default full layout:

```sh
monlin
```

This is equivalent to:

```sh
monlin "all"
```

## Config File

`monlin` now prefers a TOML config file:

- `$XDG_CONFIG_HOME/monlin/config.toml`
- `~/.config/monlin/config.toml`

Legacy shell-word configs at `.../config` are still supported for compatibility.
CLI flags still override config-file values.

Example:

```toml
colors = "gruvbox"
renderer = "block"
history = 512
interval_ms = 1000
align = "left"
```

List-like fields can also be written as arrays:

```toml
colors = ["gruvbox", "320"]
labels = ["cpu", "ram", "net"]
```

## Metrics

Single metrics:

- `cpu`
- `ram`
- `mem` as an alias for `ram`
- `gpu`
- `vram`
- `spc`
- `in`
- `ingress` as an alias for `in`
- `out`
- `egress` as an alias for `out`

Combined metrics:

- `sys` = `cpu` over `ram`
- `gfx` = `gpu` over `vram`
- `io` = write over read
- `net` = ingress over egress

Special macro:

- `all`

## Headline Units

The graph is always rendered from normalized values.

The headline text is metric-aware:

- compute and memory metrics default to percentages
- transfer metrics default to humanized rates

Current defaults:

- `cpu`, `sys`, `ram`, `gpu`, `vram`, `gfx` -> percent
- `spc` -> percent by default
- `io`, `net`, `in`, `out` -> humanized rate (`B/s`, `K/s`, `M/s`, `G/s`, `T/s`)

Examples:

- `cpu` -> ` 37%`
- `ram` -> ` 42%`
- `in` -> `812K/s`
- `net` -> `3.5M/s`
- `spc.pct` -> ` 63%`
- `spc.hum` -> `412G/1.8T`

For combined transfer metrics:

- `net` headline is ingress + egress
- `io` headline is write + read

## Layout DSL

The canonical layout syntax is:

```text
metric[.view][:basis][/grow][+max][-min]
```

Where:

- `metric` selects what to render
- `.view` changes headline formatting
- `:basis` reserves width for that item
- `/grow` gives the item a share of leftover width
- `+max` caps how wide the item may expand
- `-min` gives the item a soft minimum width when there is room

Rows are separated with:

- `,`
- or a literal newline

### Examples

Simple one-row layouts:

```sh
monlin "cpu"
monlin "cpu ram"
monlin "sys gfx io net"
monlin "spc.hum"
```

Multi-row layouts:

```sh
monlin "sys gfx io net, cpu ram in out"
monlin $'sys gfx io net\ncpu ram in out'
```

Default full-layout expansion:

```sh
monlin "all"
```

### Views

Current views:

- `.pct`
- `.hum`

Examples:

```sh
monlin "cpu.pct ram.pct"
monlin "net.hum io.hum"
monlin "in.hum out.hum"
```

Rules:

- `.pct` forces percent-style headline formatting
- `.hum` forces human-readable units where they make sense
- unsupported combinations fall back to sensible formatting for that metric

Examples:

```sh
monlin "cpu.pct gpu.pct"
monlin "net.hum:14 io.hum:14"
```

### Basis

`:N` means:

- reserve `N` columns for this item before distributing free space

Examples:

```sh
monlin "cpu:12 ram:12 net"
monlin "net.hum:14 io.hum:14"
```

If an item has a basis but no grow value, it is treated as fixed-width.

Example:

```sh
monlin "cpu:12 ram:12 net:16"
```

### Grow

`/N` means:

- this item gets `N` shares of any width left after fixed bases are reserved

Examples:

```sh
monlin "cpu/2 ram/1"
monlin "net.hum/3 io.hum/2"
```

If no basis and no grow are specified, the item behaves like a default flexible
item with grow `1`.

So these are effectively equivalent in spirit:

```sh
monlin "cpu ram"
monlin "cpu/1 ram/1"
```

### Basis Plus Grow

`:N/M` means:

- reserve `N` columns first
- then participate in leftover width distribution with `M` shares

Example:

```sh
monlin "cpu:12/2 ram:10 net.hum/3"
```

Interpretation:

- `cpu` gets at least 12 columns and can expand
- `ram` gets a fixed width of 10
- `net` gets no reserved base, but receives 3 grow shares

This is the preferred way to mix fixed and flexible columns in one row.

### Max And Min

`+N` means:

- cap this item at `N` columns after growth is distributed

`-N` means:

- prefer at least `N` columns for this item when the row has room

Examples:

```sh
monlin "cpu/2+18 ram/1+14"
monlin "net.hum/3+24 in.hum-10 out.hum-10"
monlin "cpu:12/2+20-8"
```

## Width Allocation Model

Each row behaves like a tiny flexbox:

1. reserve row separators
2. reserve explicit bases and soft minimum widths
3. distribute remaining width across grow items
4. clamp any items that exceed their max width and redistribute the freed space
5. if the row is too narrow to satisfy the preferred widths, compress proportionally
6. pad or trim the final rendered row to the requested line width

This is why the following all work naturally:

```sh
monlin "cpu ram net"
monlin "cpu:12 ram:12 net/2"
monlin "cpu:12/2 ram:10 net.hum/3+24"
```

## Row Behavior

If you do not specify explicit row breaks:

- flat layouts auto-wrap after 5 metrics per row
- `all` expands to balanced rows automatically

Examples:

```sh
monlin "cpu ram gpu vram io net"
monlin "all"
```

If you do specify row breaks:

- they are preserved
- duplicates are deduped globally, first occurrence wins

Example:

```sh
monlin "cpu ram, ram gpu cpu net"
```

This becomes effectively:

- first row: `cpu ram`
- second row: `gpu net`

## Common Invocations

CPU only:

```sh
monlin "cpu"
```

Full default monitor:

```sh
monlin
monlin "all"
```

Compact top-level view:

```sh
monlin "sys gfx io net"
```

Show only network and I/O:

```sh
monlin "io net in out"
monlin "io.hum:14 net.hum:14 in.hum out.hum"
```

Mixed fixed and flexible widths:

```sh
monlin "cpu:12/2 ram:10 net.hum/3"
monlin "cpu:12/2+20-8 ram/1+14"
```

Storage headline variants:

```sh
monlin "spc"
monlin "spc.pct"
monlin "spc.hum"
```

Two explicit rows:

```sh
monlin "sys gfx io net, cpu ram in out"
```

Right-aligned headline placement:

```sh
monlin "sys gfx io net" --align right
```

No color:

```sh
monlin "all" --color never
```

One-shot preview at fixed width:

```sh
monlin "all" --once --interval-ms 0 --width 96 --color never
```

## CLI Flags

Core flags:

- `LAYOUT` as positional arguments, e.g. `monlin "sys gfx io net"`
- `--history N`
- `--interval-ms N`
- `--align left|right`
- `--label TEXT`
- `--renderer braille|block`
- `--color auto|always|never`
- `--output terminal|i3bar`
- `--width N`
- `--once`
- `-h`, `--help`

Examples:

```sh
monlin "all" --history 512 --interval-ms 1000
monlin "sys gfx io net" --label ono
monlin "cpu ram" --renderer block
monlin "all" --output i3bar
```

## Positional Metrics

Short positional forms still work:

```sh
monlin cpu ram net
```

Positional layout arguments are the canonical interface.

## Rendering Notes

- default renderer: braille
- fallback renderer: block
- labels stay plain
- graph glyphs are colorized
- `vram` intentionally keeps a tighter label/value join, e.g. `vram37%`
- split metrics render upper and lower halves separately

When `--output i3bar` is used:

- `monlin` emits the i3bar JSON protocol, not JSONL
- the header is printed once
- each sampled frame becomes one i3bar status-line array
- each rendered `monlin` row becomes one i3bar block
- ANSI colors are disabled automatically in this mode

To test the `i3bar` JSON protocol directly:

```sh
cargo test i3bar_once_mode_emits_i3bar_protocol
```

And to verify that `i3bar` mode keeps one block per rendered row without ANSI
escapes:

```sh
cargo test i3bar_once_mode_uses_one_block_per_row_without_ansi
```

Combined metric semantics:

- `sys`: upper = CPU, lower = RAM
- `gfx`: upper = GPU, lower = VRAM
- `io`: upper = write, lower = read
- `net`: upper = ingress, lower = egress

## Project Status

Current release line:

- `0.2.0`

Recent additions in this line:

- richer layout DSL
- basis/grow/min/max width allocation
- rate-based transfer headlines
- multiline layouts
- combined metrics
- default full-layout rendering

## Verification

One local verification sequence is:

```sh
cargo fmt --check
cargo clippy
cargo test
nix flake check 'path:/home/flx/pro/git/monlin'
```

`nix flake check` now evaluates the package build plus explicit `fmt`, `clippy`,
and `test` checks from the flake.

For an interactive pinned toolchain shell, use:

```sh
nix develop 'path:/home/flx/pro/git/monlin'
```

The repo is intended to verify cleanly through the flake:

```sh
nix flake check 'path:/home/flx/pro/git/monlin'
```

And quick smoke tests are typically:

```sh
nix run 'path:/home/flx/pro/git/monlin' -- --once --interval-ms 0 --width 96 --color never
nix run 'path:/home/flx/pro/git/monlin' -- --once --interval-ms 0 --width 96 --color never 'all'
nix run 'path:/home/flx/pro/git/monlin' -- --once --interval-ms 0 --width 96 --color never 'cpu:12/2 ram:10 net.hum/3+24'
```
