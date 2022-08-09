# Dev
- Remove the `cargo` dependency and fix compilation of projects using namespaced features
  ([#2](https://github.com/Kobzol/cargo-pgo/pull/2)).
- Fix resolve of `llvm-profdata` on Windows ([#1](https://github.com/Kobzol/cargo-pgo/pull/1)).
- Report errors if Cargo fails to build the target crate and if BOLT instrumentation fails
([#7](https://github.com/Kobzol/cargo-pgo/pull/7), [#8](https://github.com/Kobzol/cargo-pgo/pull/8)).
- Allow passing custom flags to BOLT commands ([#10](https://github.com/Kobzol/cargo-pgo/pull/10)).

# 0.1.0 (3. 8. 2022)
- Initial release.
