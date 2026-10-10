# Shell completion check for psm: one image with bash, zsh and fish, the
# generated scripts installed the way USAGE.md says, and run.sh driving
# each shell's completion through cases.txt. `just test-completions`.
FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p tool-psm --locked

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends bash zsh fish \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/psm /usr/local/bin/psm
COPY crates/psm/docker/completions/run.sh crates/psm/docker/completions/zsh-complete.zsh crates/psm/docker/completions/cases.txt /opt/completions/
RUN mkdir -p /root/.zfunc /root/.config/fish/completions /etc/bash_completion.d \
    && psm completions bash > /etc/bash_completion.d/psm \
    && psm completions zsh > /root/.zfunc/_psm \
    && psm completions fish > /root/.config/fish/completions/psm.fish
ENTRYPOINT ["/opt/completions/run.sh"]
