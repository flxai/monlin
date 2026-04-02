# monlin

`monlin` is a small terminal monitor for compact status bars.

The first consumer is `nxu`, where it runs in narrow tmux side panes. That
drives the design:

- one line by default
- dense history rendering
- stable CLI for shell integration
- strong visual defaults without a config file

## Problem Statement

`monlin` should render compact live monitors for system activity in one or more
compact terminal lines.

The simplest invocation should show the default full monitor layout. The canonical
interface is a layout specification which determines the active metrics. More
advanced invocations should lay out multiple metrics across one or more lines.

Supported single metrics are:

- `cpu`
- `gpu`
- `vram`
- `ram`
- `in`
- `out`

Supported combined metrics are:

- `sys` = `cpu` / `ram`
- `gfx` = `gpu` / `vram`
- `io` = write / read
- `net` = ingress / egress

Special layout token:

- `all[/N]` expands to the full current metric set and is deduped with any explicit tokens

## Rendering Rules

- The default renderer is braille, not block characters.
- Braille is used because it encodes more information per cell.
- Colors are semantic by metric:
  - CPU: blue
  - GPU: green
  - memory: red
  - IO: yellow
  - ingress: cyan
  - egress: magenta
- Colors interpolate from light to strong/dark with rising usage.
- Truecolor ANSI output is the default target.

## Layout Rules

- `monlin` with no layout behaves like `monlin --layout "all"`.
- `monlin --layout "cpu"` forces the single CPU-only view.
- `--layout "cpu gpu"` renders two equal-width segments on one line.
- `--layout "all"` expands to all supported metrics in two rows by default.
- `--layout "all/3"` expands to all supported metrics across three balanced rows.
- `;` or a literal newline separates rows explicitly.
- Item syntax is `metric[.view][:basis][+grow]`.
- `--layout "net.hum:12+2 io.hum+2"` reserves width for `net`, then lets `net` and `io` grow.
- Simple layout tokens split the available width evenly.
- Flat layouts auto-wrap after 5 metrics per row.
- Duplicate metrics in a layout are ignored after the first occurrence.
- More elaborate ratios or templates can come later if needed.

## CLI Rules

`monlin` is the CLI surface:

- `--history`
- `--interval-ms`
- `--align`
- `--label`

`--layout` is the primary interface. Positional metrics remain as a short compatibility path for concise shell use.

## Immediate Implementation Order

1. Write tests for argument parsing, color interpolation, layout splitting, and
   braille rendering.
2. Refactor the crate into testable modules.
3. Implement the new renderer and layout model.
4. Add more metrics while keeping CPU-only mode simple.
5. Verify through the flake so the standalone repo is trustworthy.
