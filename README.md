# monlin

`monlin` is a small terminal monitor for compact one-line status bars.

The first consumer is `nxu`, where it runs in narrow tmux side panes. That
drives the design:

- one line by default
- dense history rendering
- stable CLI for shell integration
- strong visual defaults without a config file

## Problem Statement

`monlin` should render compact live monitors for system activity in a single
terminal line.

The simplest invocation should show a CPU history bar. More advanced
invocations should lay out multiple metrics on the same line.

The first supported metrics are:

- `cpu`
- `gpu`
- `memory`
- `io`
- `ingress`
- `egress`

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

- `monlin` with no positional metric renders a single CPU segment.
- `monlin cpu` behaves the same.
- `--layout "cpu gpu"` renders two equal-width segments on one line.
- Simple layout tokens split the available width evenly.
- More elaborate ratios or templates can come later if needed.

## Compatibility Rules

The existing `nxu-cpu` CLI surface must keep working for now:

- `--history`
- `--interval-ms`
- `--align`
- `--label`

Additive flags are fine, but the existing `nxu` integration should not break.

## Immediate Implementation Order

1. Write tests for argument parsing, color interpolation, layout splitting, and
   braille rendering.
2. Refactor the crate into testable modules.
3. Implement the new renderer and layout model.
4. Add more metrics while keeping CPU-only mode simple.
5. Verify through the flake so the standalone repo is trustworthy.
