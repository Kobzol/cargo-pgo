FROM rust:1.76-slim

RUN apt update \
    && apt install -y wget gnupg \
    && wget -O - https://apt.llvm.org/llvm-snapshot.gpg.key | apt-key add - \
    && echo "deb http://apt.llvm.org/bookworm/ llvm-toolchain-bookworm-18 main" > /etc/apt/sources.list.d/llvm-toolchain.list \
    && echo "deb-src http://apt.llvm.org/bookworm/ llvm-toolchain-bookworm-18 main" >> /etc/apt/sources.list.d/llvm-toolchain.list \
    && apt-get update \
    && apt install -y \
    bolt-18 \
    && ln -s /usr/bin/merge-fdata-18 /usr/bin/merge-fdata \
    && ln -s /usr/bin/llvm-bolt-18 /usr/bin/llvm-bolt \
    && ln -s /usr/lib/llvm-18/lib/libbolt_rt_instr.a /usr/lib/libbolt_rt_instr.a \
    && apt autoremove -y wget gnupg \
    && rm -rf /var/lib/apt/lists/* /etc/apt/sources.list.d/llvm-toolchain.list

RUN rustup component add llvm-tools-preview \
    && cargo install cargo-pgo

WORKDIR /workdir
