# TODO

## Immediate

- Decide whether to add a single-block `i3blocks` JSON mode on top of the new
  `i3bar` output stream.

## Rendering

- Decide whether more metrics should use compact label/value joins like `vram`.

## Metrics

- Improve GPU backends beyond the current best-effort probes.
  - better non-NVIDIA utilization
  - non-NVIDIA VRAM
- Decide whether storage metrics should include:
  - `spc`
  - `free`
  - filesystem selection

## Layout DSL

- Keep the current grammar small unless a real need appears:
  - `metric[.view][:basis][/grow][+max][-min]`
  - `,`
  - `all`

## Output Modes

- Add JSON output for machine consumption.
- Decide whether the default JSON mode should emit:
  - rendered text only
  - rendered text plus normalized values
  - rendered text plus raw headline values
- Keep the plain terminal renderer as the primary mode.

## Packaging And Release Hygiene

- Decide whether to tag releases or keep version bumps lightweight.
