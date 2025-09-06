ARG TARGETARCH
FROM rust:1.89-slim-bookworm AS src

COPY . .

RUN cargo install --path .

FROM rust:1.89-slim-bookworm
ARG TARGETARCH

COPY --from=src /usr/local/cargo/bin/cargo-pgo /usr/local/bin/cargo-pgo

RUN apt update \
    && apt install -y wget gnupg \
    && wget -O - https://apt.llvm.org/llvm-snapshot.gpg.key | apt-key add - \
    && echo "deb http://apt.llvm.org/bookworm/ llvm-toolchain-bookworm-21 main" > /etc/apt/sources.list.d/llvm-toolchain.list \
    && echo "deb-src http://apt.llvm.org/bookworm/ llvm-toolchain-bookworm-21 main" >> /etc/apt/sources.list.d/llvm-toolchain.list \
    && apt-get update \
    && apt install -y \
    bolt-21 \
    musl-tools \
    && ln -s /usr/bin/merge-fdata-21 /usr/bin/merge-fdata \
    && ln -s /usr/bin/llvm-bolt-21 /usr/bin/llvm-bolt \
    && ln -s /usr/lib/llvm-21/lib/libbolt_rt_instr.a /usr/lib/libbolt_rt_instr.a \
    && apt autoremove -y wget gnupg \
    && rm -rf /var/lib/apt/lists/* /etc/apt/sources.list.d/llvm-toolchain.list

RUN <<EOF
set -eux
case "${TARGETARCH}" in
    amd64)  export ARCH="x86_64";;
    arm64)  export ARCH="aarch64";;
    *)      echo "❌ Unknown TARGETARCH: ${TARGETARCH} (supported: amd64, arm64)" >&2; exit 1 ;; \
esac
rustup component add llvm-tools-preview
rustup target add $ARCH-unknown-linux-musl
EOF

WORKDIR /workdir
