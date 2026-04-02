# TODO

## Immediate

- Add per-frame structured output mode.
  - `--output i3bar` for whole-line JSON stream compatibility.
  - Optionally later add a single-block `i3blocks` JSON mode.
- Lock down the layout DSL in tests.
  - more cases for `all/N`
  - more mixed `:basis+grow` cases
  - explicit `.pct` / `.hum` behavior tests

## Rendering

- Revisit fixed-width rendering edge cases.
  - very narrow rows
  - rows with only fixed-width items
  - rows with a mix of fixed and high-grow items
- Decide whether more metrics should use compact label/value joins like `vram`.
- Add explicit tests for multiline repaint behavior in the live loop.

## Metrics

- Improve GPU backends beyond the current best-effort probes.
  - better non-NVIDIA utilization
  - non-NVIDIA VRAM
- Revisit network and I/O scaling.
  - consider windowed maxima or decay tuning
  - keep spike visibility without washing out sustained activity
- Decide whether storage metrics should include:
  - `spc`
  - `free`
  - filesystem selection

## Layout DSL

- Decide whether to drop old compatibility syntax eventually.
  - `all:N`
  - `*grow`
- Consider whether explicit min/max width constraints are worth adding later.
- Keep the current grammar small unless a real need appears:
  - `metric[.view][:basis][+grow]`
  - `;`
  - `all/N`

## Output Modes

- Add JSON output for machine consumption.
- Decide whether the default JSON mode should emit:
  - rendered text only
  - rendered text plus normalized values
  - rendered text plus raw headline values
- Keep the plain terminal renderer as the primary mode.

## Packaging And Release Hygiene

- Add a `.gitignore` for `target/` and `result/`.
- Consider adding:
  - `cargo fmt --check`
  - `cargo clippy`
  - `cargo test`
  - `nix flake check`
  as one documented local verification sequence.
- Decide whether to tag releases or keep version bumps lightweight.

## Integration

- Update `nixos-configuration` to relock to the latest `monlin` commit cleanly.
- Keep `nxu` and `nxc` on the canonical layout syntax (`all/2`).
- Consider whether any status-bar integrations should use `monlin` directly.
