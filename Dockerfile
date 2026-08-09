# 开发 / 打包工具链镜像：Rust 构建、pre-commit 常用组件、以及 `make package`（tar.gz + .deb）。
# 非生产**运行**镜像（产物装到宿主机或装进 systemd / 自行编排）。
# 业务 UI（Leptos/Trunk）在同级 Client 仓 `crabmate-client`；本镜像不装 wasm/trunk。
#
# 基座 Ubuntu 24.04（glibc 2.39）：deb `depends` 为 libc6 (>= 2.39)；勿装到更旧发行版。
#
# 构建镜像（默认 bridge；宿主机 DNS 异常时可加 --network=host）：
#   docker build -t crabmate-dev .
# 交互开发：
#   docker run --rm -it -v "$PWD":/workspace -w /workspace crabmate-dev
# 编译发行包（产物在宿主 dist/）：
#   docker run --rm -v "$PWD":/workspace -w /workspace crabmate-dev make package
#   # 或 make package-docker
# 对齐宿主 UID：
#   docker build --build-arg DEV_UID=$(id -u) --build-arg DEV_GID=$(id -g) -t crabmate-dev .
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8 \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

# 系统依赖：编译（OpenSSL / libssh2 / gcc→libstdc++）+ 打包（make / tar / dpkg）
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        git \
        make \
        pkg-config \
        build-essential \
        g++ \
        libssl-dev \
        libssh2-1-dev \
        libwayland-dev \
        cmake \
        clang-format \
        bc \
        sudo \
        tar \
        gzip \
        xz-utils \
        dpkg-dev \
        fakeroot \
        file \
    && rm -rf /var/lib/apt/lists/*

# rustup：stable + rustfmt/clippy（与 pre-commit 一致）
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal \
    && rustup component add rustfmt clippy

# cargo-deb：`make package` / scripts/package-release.sh 生成 .deb（与 CI taiki-e/install-action 对齐）
RUN cargo install cargo-deb --locked

# 非 root：挂载宿主目录时避免 root 写文件。Ubuntu 24.04 镜像已有 uid/gid 1000（ubuntu），须复用/改名。
ARG DEV_UID=1000
ARG DEV_GID=1000
RUN set -eux; \
    if getent group "${DEV_GID}" >/dev/null; then \
        existing_g="$(getent group "${DEV_GID}" | cut -d: -f1)"; \
        if [ "${existing_g}" != "dev" ]; then \
            groupmod -n dev "${existing_g}"; \
        fi; \
    else \
        groupadd -g "${DEV_GID}" dev; \
    fi; \
    if getent passwd "${DEV_UID}" >/dev/null; then \
        existing_u="$(getent passwd "${DEV_UID}" | cut -d: -f1)"; \
        if [ "${existing_u}" != "dev" ]; then \
            usermod -l dev "${existing_u}"; \
        fi; \
        usermod -g dev -s /bin/bash -d /home/dev -m dev; \
    else \
        useradd -m -u "${DEV_UID}" -g dev -s /bin/bash dev; \
    fi; \
    mkdir -p /home/dev; \
    echo 'dev ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/dev; \
    chmod 0440 /etc/sudoers.d/dev; \
    chown -R dev:dev /home/dev "${RUSTUP_HOME}" "${CARGO_HOME}"

WORKDIR /workspace
USER dev

# 默认进入 shell；挂载本仓后可 `cargo build` / `make package`
CMD ["bash", "-l"]
