# Build stage
FROM rust:1.85 AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    libclang-dev \
    pkg-config \
    libssl-dev \
    git \
    && rm -rf /var/lib/apt/lists/*


WORKDIR /build

# Cargo in official rust images uses /usr/local/cargo; cache this path explicitly.
ENV CARGO_HOME=/usr/local/cargo
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
ENV RUSTUP_HOME=/usr/local/rustup
ENV RUSTUP_PROFILE=minimal

# Copy only what's needed for the build
COPY . .

# Build the server
RUN --mount=type=ssh \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/usr/local/rustup \
    --mount=type=cache,target=/build/target \
    rustup set profile minimal && \
    rustup toolchain install nightly-2025-08-02 --profile minimal --component llvm-tools,rustc-dev --no-self-update && \
    cargo +nightly-2025-08-02 build --bin validity --release && \
    cp target/release/validity /build/validity-proposer

# Final stage
FROM rust:1.85-slim

WORKDIR /app

# Install required runtime dependencies
RUN apt-get update && apt-get install -y \
    curl \
    clang \
    pkg-config \
    libssl-dev \
    ca-certificates \
    git \
    libclang-dev \
    jq \
    postgresql-client \
    && rm -rf /var/lib/apt/lists/*

# Install SP1
RUN curl -L https://sp1.succinct.xyz | bash && \
    ~/.sp1/bin/sp1up && \
    ~/.sp1/bin/cargo-prove prove --version

# Copy only the built binaries from builder
COPY --from=builder /build/validity-proposer /usr/local/bin/validity-proposer

# Run the server from its permanent location
CMD ["/usr/local/bin/validity-proposer"]
