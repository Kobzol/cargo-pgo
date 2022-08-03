# `cargo-pgo` [![Build Status]][actions] [![Latest Version]][crates.io]

[Build Status]: https://github.com/kobzol/cargo-pgo/actions/workflows/check.yml/badge.svg
[actions]: https://github.com/kobzol/cargo-pgo/actions?query=branch%3Amain
[Latest Version]: https://img.shields.io/crates/v/cargo-pgo.svg
[crates.io]: https://crates.io/crates/cargo-pgo

**Cargo subcommand that makes it easier to use [PGO](https://doc.rust-lang.org/rustc/profile-guided-optimization.html)
and [BOLT](https://github.com/llvm/llvm-project/tree/main/bolt) to optimize Rust binaries.**

## Installation
```bash
$ cargo install cargo-pgo
```

You will also need some LLVM tools for both PGO and BOLT.
Specifically, you will need the `llvm-profdata` binary for PGO and `llvm-bolt` and `merge-fdata`
binaries for BOLT.

You can install the PGO helper binary by adding the `llvm-tools-preview` component to your toolchain
with `rustup`:
```bash
$ rustup component add llvm-tools-preview
```

For BOLT, it's unfortunately more complicated. See [below](#bolt-installation) for BOLT installation
guide.

## Features
This command makes it simpler to use **feedback-directed optimizations** for optimizing Rust binaries.

- Optimize binaries with PGO
- Optimize binaries with BOLT

[//]: # (- Optimize binaries with both PGO and BOLT &#40;currently not implemented&#41;)

### PGO/BOLT workflow
It is important to understand the workflow of using feedback-directed optimizations. Put simply, it
consists of three general steps:

1) **Build binary with instrumentation**
    - Perform a special build of your executable which will add additional instrumentation code to it.
2) **Gather performance profiles**
    - Run your instrumented binary on representative workloads. The binary will generate profile files
    on disk which will be then used to optimize the binary.
3) **Build an optimized binary using generated profiles**
    - The compiler will use the generated profiles to build an optimized version of your binary.
    - The binary will be optimized with respect to the profiled workloads. If you execute it on a
    substantially different workload, the optimizations might not work (or they might even make your
    binary slower!).

## Usage


## BOLT installation
Here's a short guide how to compile LLVM with BOLT. You will need a recent compiler, `CMake` and
`ninja`.

1) Download LLVM
    ```bash
    $ git clone https://github.com/llvm/llvm-project
    $ cd llvm-project 
    ```
2) Prepare the build
    ```bash
    $ cmake -S llvm -B build -G ninja \
      -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_INSTALL_PREFIX=${PWD}/llvm-install \
      -DLLVM_ENABLE_PROJECTS="clang;lld;compiler-rt;bolt"
    ```
3) Compile LLVM with BOLT
    ```bash
    $ cd build
    $ ninja
    $ ninja install 
    ```
    The built files should be located at `<llvm-dir>/llvm-install/bin`. You should add this directory
    to `$PATH` to make BOLT usable with `cargo-pgo`.

## Related work
- [cargo-pgo](https://github.com/vadimcn/cargo-pgo) I basically reimplemented this cargo command
independently, it uses an almost identical approach, but doesn't support BOLT. It's not maintained
anymore and I got the permission from its author to (re)use its name.

## License
[MIT](LICENSE)
