# 0.2.8 (28. 3. 2024)
## Fixes

- Fix passing of `--config` Cargo argument when using `cargo pgo run` (https://github.com/Kobzol/cargo-pgo/pull/54).

# 0.2.7 (28. 3. 2024)
## Fixes

- Try to respect existing RUSTFLAGS in `.cargo/config.toml` when compiling
  with `cargo-pgo` (https://github.com/Kobzol/cargo-pgo/pull/50).

# 0.2.6 (15. 2. 2024)
## Fixes

- Respect the `--profile` cargo argument.
- Do not filter or modify `cargo pgo run` arguments passed to the executed binary.

# 0.2.5 (31. 12. 2023)
## Fixes

- Respect the `--target-dir` cargo argument and improve parsing of cargo arguments in general.

# 0.2.4 (16. 1. 2023)
## Fixes

- Do not close `stdin` when executing `cargo pgo run` ([#28](https://github.com/Kobzol/cargo-pgo/issues/28)).

# 0.2.3 (8. 11. 2022)
## New features

- Add `--keep-profiles` flag to `cargo pgo run` to avoid clearing the gathered PGO profiles from
  previous runs ([#23](https://github.com/Kobzol/cargo-pgo/issues/23)).

# 0.2.2 (14. 10. 2022)
## Changes

- The output of Cargo is now streamed interactively, rather than being buffered up until the build exits.
  This should make the output of `cargo pgo` commands much more
  interactive ([#20](https://github.com/Kobzol/cargo-pgo/pull/20)).

# 0.2.1 (27. 9. 2022)
## Fixes

- Fix file moving across different devices in `cargo pgo optimize` ([#17](https://github.com/Kobzol/cargo-pgo/pull/17)).

# 0.2.0 (12. 8. 2022)
## New features

- Allow running `cargo pgo bolt optimize` without any profiles ([#14](https://github.com/Kobzol/cargo-pgo/pull/14)).
- Add `cargo pgo bench` command and generalize instrumentation and optimization so that you can instrument
  or optimize any supported Cargo command
  ([#13](https://github.com/Kobzol/cargo-pgo/pull/13), [#9](https://github.com/Kobzol/cargo-pgo/pull/9)).
- Allow passing custom flags to BOLT commands ([#10](https://github.com/Kobzol/cargo-pgo/pull/10)).

## Fixes

- Remove the `cargo` dependency and fix compilation of projects using namespaced features
  ([#2](https://github.com/Kobzol/cargo-pgo/pull/2)).
- Properly invalidate PGO profiles. ([#15](https://github.com/Kobzol/cargo-pgo/pull/15)).
- Fix resolve of `llvm-profdata` on Windows ([#1](https://github.com/Kobzol/cargo-pgo/pull/1)).
- Report errors if Cargo fails to build the target crate and if BOLT instrumentation fails
  ([#7](https://github.com/Kobzol/cargo-pgo/pull/7), [#8](https://github.com/Kobzol/cargo-pgo/pull/8)).
- Correctly add a newline to text messages produced during compilation
  ([#12](https://github.com/Kobzol/cargo-pgo/pull/12)).

# 0.1.0 (3. 8. 2022)

- Initial release.
