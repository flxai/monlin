# TODO

## 0. Settle Repo Identity

- Decide whether the project name is `monlin` or `nxu-cpu`.
- If `monlin` is the repo/project name, decide whether the binary should stay `nxu-cpu` for compatibility.
- Add a short `README.md` explaining what the tool does, who uses it, and how it fits into `nxu`.

## 1. Clean Packaging

- Keep the flake output stable:
  - `packages.<system>.default`
  - `packages.<system>.nxu-cpu`
- Decide whether `flake.lock` should be committed long-term or omitted for this tiny crate.
- Add `checks` to the flake:
  - build the package
  - run formatting/linting if added

## 2. Preserve Compatibility

- Keep the current CLI stable for now:
  - `--history`
  - `--interval-ms`
  - `--align`
  - `--label`
- Treat `nxu` integration as the compatibility boundary.
- Avoid renaming flags until `nixos-configuration` has been updated deliberately.

## 3. Improve the Program Itself

- Add `--help`.
- Add basic argument validation and clearer error messages.
- Consider terminal resize handling instead of only sampling width opportunistically.

## 4. Build the Real Display Model

- Add multiple metrics:
  - `cpu`
  - `gpu`
  - `memory`
  - `io`
  - `ingress`
  - `egress`
- Keep the simplest invocation trivial:
  - `monlin` renders one CPU line by default
  - `monlin cpu` behaves the same
- Add a layout option that composes multiple monitors on one line.
- Start with the simple layout grammar first:
  - `--layout "cpu gpu"`
  - tokens split the available width evenly
  - `cpu gpu` means 50% / 50%
- Later, extend layouting to explicit ratios or templates if still needed.

## 5. Rendering Upgrades

- Replace the current block-based sparkline with braille glyph rendering like `btop`.
- Treat braille as the primary renderer, not an optional afterthought.
- Keep a simpler block renderer only as a compatibility fallback if necessary.
- Make the renderer width-aware so the denser braille mode actually uses the extra horizontal resolution.

## 6. Color Model

- Add semantic default colors:
  - CPU: blue
  - GPU: green
  - memory: red
  - IO: yellow
  - ingress: cyan
  - egress: magenta
- Render gradients within each metric:
  - low usage = lighter shade
  - high usage = darker / stronger shade
- Prefer 24-bit truecolor output first.
- Only add a 256-color fallback if terminal compatibility actually requires it.
- Keep colors overridable later, but ship strong defaults first.

## 7. CLI / Config Shape

- Keep current flags working:
  - `--history`
  - `--interval-ms`
  - `--align`
  - `--label`
- Add new flags carefully:
  - `--layout`
  - `--metrics` or positional metric tokens
  - possibly `--renderer braille|block`
- Avoid a config file until the CLI shape has stabilized.

## 8. Add Tests

- Extract pure logic into testable functions where useful.
- Add unit tests for:
  - CPU delta calculation
  - braille rendering
  - alignment behavior
  - argument parsing
  - layout splitting
  - color interpolation
- Add at least one smoke test that the binary starts successfully.

## 9. Improve Release Hygiene

- Add `rustfmt.toml` only if you want non-default formatting.
- Add CI or a minimal local check script for:
  - `nix build`
  - `cargo test`
  - `cargo fmt --check`
- `cargo clippy`
- Decide whether releases matter, or whether this stays an internal utility.

## 10. Tighten Nix Integration

- Consider exposing a small `nixosModule` or overlay only if another repo needs it.
- Keep the current flake output minimal until there is a real second consumer.
- Once the repo path is final, update `nixos-configuration` to reference the canonical location only.

## 11. Near-Term Sequence

- Write `README.md`.
- Add `--help`.
- Refactor the renderer so metrics, colors, and layout are separate concepts.
- Implement braille rendering.
- Implement truecolor gradients.
- Add `--layout`.
- Add memory, IO, ingress, and egress.
- Add a first test module.
- Add flake `checks`.
- Run `nxu` end-to-end using the external `monlin` package on at least one host.
