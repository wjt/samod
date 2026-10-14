FROM mcr.microsoft.com/devcontainers/rust:1-trixie

# Build dependencies:
#   - pkg-config, libssl-dev: native-tls (via the `tungstenite` feature)
#   - libglib2.0-dev: the `gio` feature
#   - nodejs, npm: the JS interop tests (samod/tests/js_interop)
#
# Debugging and profiling tools:
#   - gdb, lldb, strace, ltrace: debuggers and syscall/library call tracers
#   - linux-perf: CPU profiling (`perf record`, `perf stat`, ...)
#   - valgrind: callgrind, cachegrind, massif, dhat
#   - heaptrack: heap allocation profiling
#   - hyperfine: command-line benchmarking
#   - time: GNU time, for peak memory usage (`/usr/bin/time -v`)
#   - sysstat, htop, procps, psmisc, lsof: system and process inspection
RUN export DEBIAN_FRONTEND=noninteractive \
    && apt-get update \
    && apt-get install -y --no-install-recommends \
        pkg-config \
        libssl-dev \
        libglib2.0-dev \
        nodejs \
        npm \
        gdb \
        lldb \
        strace \
        ltrace \
        linux-perf \
        valgrind \
        heaptrack \
        hyperfine \
        time \
        sysstat \
        htop \
        procps \
        psmisc \
        lsof \
        jq \
        ripgrep \
        fd-find \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Prebuilt binaries installed with cargo-binstall into $CARGO_HOME/bin:
#   - jj-cli: Jujutsu version control
#   - samply: sampling profiler with a Firefox Profiler UI
#   - flamegraph: `cargo flamegraph`, perf-based flamegraph generation
#   - inferno: flamegraph tooling (`inferno-collapse-perf`, `inferno-flamegraph`)
#   - rustfilt: demangles Rust v0 symbols, e.g. `perf report --stdio | rustfilt`
RUN curl -L --proto '=https' --tlsv1.2 -sSf \
        https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh \
        | bash \
    && cargo binstall --no-confirm --locked \
        jj-cli \
        samply \
        flamegraph \
        inferno \
        rustfilt \
    && rm -rf "$CARGO_HOME/registry" \
    && chown -R :rustlang "$CARGO_HOME" \
    && chmod -R g+w "$CARGO_HOME"
