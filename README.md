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
monlin --layout "all"
```

And `all` currently defaults to two rows:

```sh
monlin --layout "all/2"
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
- `all/N`

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
metric[.view][:basis][+grow]
```

Where:

- `metric` selects what to render
- `.view` changes headline formatting
- `:basis` reserves width for that item
- `+grow` gives the item a share of leftover width

Rows are separated with:

- `;`
- or a literal newline

### Examples

Simple one-row layouts:

```sh
monlin --layout "cpu"
monlin --layout "cpu ram"
monlin --layout "sys gfx io net"
monlin --layout "spc.hum"
```

Multi-row layouts:

```sh
monlin --layout "sys gfx io net; cpu ram in out"
monlin --layout $'sys gfx io net\ncpu ram in out'
```

Balanced expansion of the full metric set:

```sh
monlin --layout "all"
monlin --layout "all/2"
monlin --layout "all/3"
```

### Views

Current views:

- `.pct`
- `.hum`

Examples:

```sh
monlin --layout "cpu.pct ram.pct"
monlin --layout "net.hum io.hum"
monlin --layout "in.hum out.hum"
```

Rules:

- `.pct` forces percent-style headline formatting
- `.hum` forces human-readable units where they make sense
- unsupported combinations fall back to sensible formatting for that metric

Examples:

```sh
monlin --layout "cpu.pct gpu.pct"
monlin --layout "net.hum:14 io.hum:14"
```

### Basis

`:N` means:

- reserve `N` columns for this item before distributing free space

Examples:

```sh
monlin --layout "cpu:12 ram:12 net"
monlin --layout "net.hum:14 io.hum:14"
```

If an item has a basis but no grow value, it is treated as fixed-width.

Example:

```sh
monlin --layout "cpu:12 ram:12 net:16"
```

### Grow

`+N` means:

- this item gets `N` shares of any width left after fixed bases are reserved

Examples:

```sh
monlin --layout "cpu+2 ram+1"
monlin --layout "net.hum+3 io.hum+2"
```

If no basis and no grow are specified, the item behaves like a default flexible
item with grow `1`.

So these are effectively equivalent in spirit:

```sh
monlin --layout "cpu ram"
monlin --layout "cpu+1 ram+1"
```

### Basis Plus Grow

`:N+M` means:

- reserve `N` columns first
- then participate in leftover width distribution with `M` shares

Example:

```sh
monlin --layout "cpu:12+2 ram:10 net.hum+3"
```

Interpretation:

- `cpu` gets at least 12 columns and can expand
- `ram` gets a fixed width of 10
- `net` gets no reserved base, but receives 3 grow shares

This is the preferred way to mix fixed and flexible columns in one row.

## Width Allocation Model

Each row behaves like a tiny flexbox:

1. reserve row separators
2. reserve all explicit bases
3. distribute remaining width across grow items
4. if the row is too narrow to satisfy all bases, compress proportionally
5. pad or trim the final rendered row to the requested line width

This is why the following all work naturally:

```sh
monlin --layout "cpu ram net"
monlin --layout "cpu:12 ram:12 net+2"
monlin --layout "cpu:12+2 ram:10 net.hum+3"
```

## Compatibility Forms

The preferred syntax is:

- `all/N`
- `+grow`

The parser still accepts older compatibility forms:

- `all:N`
- `*grow`

Examples:

```sh
monlin --layout "all:2"
monlin --layout "cpu*3 ram*4"
```

These still work, but new layouts should use:

```sh
monlin --layout "all/2"
monlin --layout "cpu+3 ram+4"
```

## Row Behavior

If you do not specify explicit row breaks:

- flat layouts auto-wrap after 5 metrics per row
- `all` expands to balanced rows automatically

Examples:

```sh
monlin --layout "cpu ram gpu vram io net"
monlin --layout "all"
```

If you do specify row breaks:

- they are preserved
- duplicates are deduped globally, first occurrence wins

Example:

```sh
monlin --layout "cpu ram; ram gpu cpu net"
```

This becomes effectively:

- first row: `cpu ram`
- second row: `gpu net`

## Common Invocations

CPU only:

```sh
monlin --layout "cpu"
```

Full default monitor:

```sh
monlin
monlin --layout "all"
monlin --layout "all/2"
```

Compact top-level view:

```sh
monlin --layout "sys gfx io net"
```

Show only network and I/O:

```sh
monlin --layout "io net in out"
monlin --layout "io.hum:14 net.hum:14 in.hum out.hum"
```

Mixed fixed and flexible widths:

```sh
monlin --layout "cpu:12+2 ram:10 net.hum+3"
```

Storage headline variants:

```sh
monlin --layout "spc"
monlin --layout "spc.pct"
monlin --layout "spc.hum"
```

Two explicit rows:

```sh
monlin --layout "sys gfx io net; cpu ram in out"
```

Right-aligned headline placement:

```sh
monlin --align right --layout "sys gfx io net"
```

No color:

```sh
monlin --color never --layout "all/2"
```

One-shot preview at fixed width:

```sh
monlin --once --interval-ms 0 --width 96 --color never --layout "all/2"
```

## CLI Flags

Core flags:

- `-l`, `--layout SPEC`
- `--history N`
- `--interval-ms N`
- `--align left|right`
- `--label TEXT`
- `--renderer braille|block`
- `--color auto|always|never`
- `--width N`
- `--once`
- `-h`, `--help`

Examples:

```sh
monlin --history 512 --interval-ms 1000 --layout "all/2"
monlin --label ono --layout "sys gfx io net"
monlin --renderer block --layout "cpu ram"
```

## Positional Metrics

Positional metrics still work as a short compatibility path:

```sh
monlin cpu ram net
```

But `--layout` is the canonical interface, and all new examples should use it.

## Rendering Notes

- default renderer: braille
- fallback renderer: block
- labels stay plain
- graph glyphs are colorized
- `vram` intentionally keeps a tighter label/value join, e.g. `vram37%`
- split metrics render upper and lower halves separately

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
- basis/grow width allocation
- rate-based transfer headlines
- multiline layouts
- combined metrics
- default full-layout rendering

## Verification

The repo is intended to verify cleanly through the flake:

```sh
nix flake check 'path:/home/flx/pro/git/monlin'
```

And quick smoke tests are typically:

```sh
nix run 'path:/home/flx/pro/git/monlin' -- --once --interval-ms 0 --width 96 --color never
nix run 'path:/home/flx/pro/git/monlin' -- --once --interval-ms 0 --width 96 --color never --layout 'all/2'
nix run 'path:/home/flx/pro/git/monlin' -- --once --interval-ms 0 --width 96 --color never --layout 'cpu:12+2 ram:10 net.hum+3'
```
