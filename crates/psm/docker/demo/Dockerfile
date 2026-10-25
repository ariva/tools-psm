# Demo recording for psm: a rust stage builds the binary, the VHS image
# (ttyd + ffmpeg + headless Chrome) plays demo.tape and writes the GIF.
# `just demo`. The tape runs inside a private PID namespace (see
# demo.tape), so the process table on screen holds only the fake workload.
FROM rust:1-slim-bookworm AS build
# .git is not in the build context; `just demo` passes the hash for the version line
ARG PSM_GIT_HASH=unknown
ENV PSM_GIT_HASH=$PSM_GIT_HASH
WORKDIR /src
COPY . .
RUN cargo build --release -p tool-psm --locked

FROM ghcr.io/charmbracelet/vhs
RUN apt-get update \
    && apt-get install -y --no-install-recommends jq \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --shell /bin/bash dev
COPY --from=build /src/target/release/psm /usr/local/bin/psm
COPY crates/psm/docker/demo /opt/demo/
WORKDIR /out
ENTRYPOINT ["vhs", "/opt/demo/demo.tape"]
