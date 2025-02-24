# `cargo-pgo` [![Build Status]][actions] [![Latest Version]][crates.io]

[Build Status]: https://github.com/kobzol/cargo-pgo/actions/workflows/check.yml/badge.svg
[actions]: https://github.com/kobzol/cargo-pgo/actions?query=branch%3Amain
[Latest Version]: https://img.shields.io/crates/v/cargo-pgo.svg
[crates.io]: https://crates.io/crates/cargo-pgo

**Cargo subcommand that makes it easier to use [PGO](https://doc.rust-lang.org/rustc/profile-guided-optimization.html)
and [BOLT](https://github.com/llvm/llvm-project/tree/main/bolt) to optimize Rust binaries.**

For an example on how to use `cargo-pgo` to optimize a binary on GitHub Actions CI, see [this workflow](ci/pgo.yml).

## Installation

```bash
$ cargo install cargo-pgo
```

You will also need the `llvm-profdata` binary for PGO and `llvm-bolt` and `merge-fdata`
binaries for BOLT.

You can install the PGO helper binary by adding the `llvm-tools-preview` component to your toolchain
with `rustup`:
```bash
$ rustup component add llvm-tools-preview
```

For BOLT, it is highly recommended to use [Docker](#docker).
See [below](#bolt-installation) for BOLT installation guide.

> BOLT and Docker support is currently *experimental*.

## Docker

To use latest `cargo-pgo` with Docker, you need to build the image first:

```bash
git clone https://github.com/Kobzol/cargo-pgo.git && cd cargo-pgo
docker build -t cargo-pgo .
```

Then run this in your project directory to create a container:

```bash
docker run -v $(pwd):/workdir --rm -it cargo-pgo
```

In the container, you can run `cargo-pgo` as you would on your system.

Note that with `--rm` argument, the container will be removed after you exit.

## PGO/BOLT workflow
It is important to understand the workflow of using feedback-directed optimizations. Put simply, it
consists of three general steps:

1) **Build binary with instrumentation**
    - Perform a special build of your executable which will add additional instrumentation code to it.
2) **Gather performance profiles**
    - Run your instrumented binary on representative workloads. The binary will generate profile files
    on disk which will be then used to optimize the binary.
    - Try to gather as much data as possible. Ideally, exercise all the important parts of the codebase (in the coverage sense).
3) **Build an optimized binary using generated profiles**
    - The compiler will use the generated profiles to build an optimized version of your binary.
    - The binary will be optimized with respect to the profiled workloads. If you execute it on a
    substantially different workload, the optimizations might not work (or they might even make your
    binary slower!).

## Example
![Example usage of the tool](docs/terminal.gif)

# **Usage**
Before you start to optimize your binaries, you should first check if your environment is set up
correctly, at least for PGO (BOLT is more complicated). You can do that using the `info` command:
```bash
$ cargo pgo info
```

## PGO
`cargo-pgo` provides subcommands that wrap common Cargo commands. It will automatically add
`--release` to wrapped commands where it is applicable, since it doesn't really make sense to perform
PGO on debug builds.

### Generating the profiles
First, you need to generate the PGO profiles by performing an *instrumented* build.
You can currently do that in several ways. The most generic command for creating an instrumented
artifact is `cargo pgo instrument`:

```bash
$ cargo pgo instrument [<command>] -- [cargo-args]
```

The `command` specifies what command will be executed by `cargo`. It is optional and by default it
is set to `build`. You can pass additional arguments for `cargo` after `--`.

There are several ways of producing the profiles:

- **Building a binary**
    ```bash
    $ cargo pgo build
    # or
    $ cargo pgo instrument build
    ```

    This is the simplest and recommended approach. You build an instrumented binary and then run it
    on some workloads. Note that the binary will be located at `<target-dir>/<target-triple>/release/<binary-name>`.

- **Running an instrumented program**
    ```bash
    $ cargo pgo run
    # or
    $ cargo pgo instrument run
    ```

    You can also directly execute an instrumented binary with the `cargo pgo run` command,
    which is a shortcut for `cargo pgo instrument run`. This command will instrument the binary and
    then execute it right away.

- **Run instrumented tests**
    ```bash
    $ cargo pgo test
    # or
    $ cargo pgo instrument test
    ```
    This command will generate profiles by executing tests. Note that unless your test suite
    is really comprehensive, it might be better to create a binary and run it on some specific
    workloads instead.

- **Run instrumented benchmarks**
    ```bash
    $ cargo pgo bench
    # or
    $ cargo pgo instrument bench
    ```
    This command will generate profiles by executing benchmarks.

### Building an optimized binary
Once you have generated some profiles, you can execute `cargo pgo optimize` to build an optimized
version of your binary.

If you want, you can also pass a command to `cargo pgo optimize` to e.g. run PGO-optimized benchmarks
or tests:

```bash
$ cargo pgo optimize bench
$ cargo pgo optimize test
```

### Analyzing PGO profiles
You can analyze gathered PGO profiles using the `llvm-profdata` binary:
```console
$ llvm-profdata show <profile>.profdata
```

## BOLT
Using BOLT with `cargo-pgo` is similar to using PGO, however you either have to [build](#bolt-installation)
BOLT manually or download it from the GitHub releases archive (for LLVM 16+). Support for BOLT is currently in an
experimental stage.

BOLT is not supported directly by `rustc`, so the instrumentation and optimization commands are not
directly applied to binaries built by `rustc`. Instead, `cargo-pgo` creates additional binaries that
you have to use for gathering profiles and executing the optimized code.

### Generating the profiles
First, you need to generate the BOLT profiles. To do that, execute the following command:
```bash
$ cargo pgo bolt build
```
The instrumented binary will be located at `<target-dir>/<target-triple>/release/<binary-name>-bolt-instrumented`.
Execute it on several workloads to gather as much data as possible.

Note that for BOLT, the profile gathering step is optional. You can also simply run the optimization
step (see below) without any profiles, although it will probably not have a large effect.

### Building an optimized binary
Once you have generated some profiles, you can execute `cargo pgo bolt optimize` to build an
optimized version of your binary. The optimized binary will be named `<binary-name>-bolt-optimized`.

## BOLT + PGO
Yes, BOLT and PGO can even be combined :) To do that, you should first generate PGO profiles and
then use BOLT on already PGO optimized binaries. You can do that using the `--with-pgo` flag:

```bash
# Build PGO instrumented binary
$ cargo pgo build
# Run binary to gather PGO profiles
$ ./target/.../<binary>
# Build BOLT instrumented binary using PGO profiles
$ cargo pgo bolt build --with-pgo
# Run binary to gather BOLT profiles
$ ./target/.../<binary>-bolt-instrumented
# Optimize a PGO-optimized binary with BOLT
$ cargo pgo bolt optimize --with-pgo
```

> Do not strip symbols from your release binary when using BOLT! If you do it, you might encounter
> linker errors.

### BOLT installation
Here's a short guide how to compile LLVM with BOLT manually. You will need a recent compiler, `CMake` and
`ninja`.

> Note: LLVM BOLT is slowly getting into package repositories, although it's not fully working out of the box yet.
> You can find more details [here](https://github.com/Kobzol/cargo-pgo/issues/31) if you're interested.

1) Download LLVM
    ```bash
    $ git clone https://github.com/llvm/llvm-project
    $ cd llvm-project 
    ```
2) (Optional) Checkout a stable version, at least 14.0.0
    ```bash
    $ git checkout llvmorg-14.0.5
    ```
   Note that BOLT is being actively fixed, so a `trunk` version of LLVM might actually work better.
3) Prepare the build
    ```bash
    $ cmake -S llvm -B build -G Ninja \
      -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_INSTALL_PREFIX=${PWD}/llvm-install \
      -DLLVM_ENABLE_PROJECTS="clang;lld;compiler-rt;bolt"
    ```
4) Compile LLVM with BOLT
    ```bash
    $ cd build
    $ ninja
    $ ninja install 
    ```
    The built files should be located at `<llvm-dir>/llvm-install/bin`. You should add this directory
    to `$PATH` to make BOLT usable with `cargo-pgo`.

## Caveats
- `cargo-pgo` needs to set RUSTFLAGS for the crate being compiled. If you pass your own RUSTFLAGS using `config.toml` file, please make sure to use the `[target.<...>] rustflags = ...` section, instead of the `[build] rustflags = ...` section. With `target`, your flags will be combined with the PGO flags. If you use `build`, your flags will be overridden instead. See [#49](https://github.com/Kobzol/cargo-pgo/issues/49) for more context.

# Related work
- [cargo-pgo](https://github.com/vadimcn/cargo-pgo) I basically independently reimplemented this
crate. It uses an almost identical approach, but doesn't support BOLT. It's not maintained
anymore, I got a permission from its author to (re)use its name.

# License
[MIT](LICENSE)
