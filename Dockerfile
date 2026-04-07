# 开发环境镜像：预装 CrabMate 源码构建与前端 WASM 构建常用依赖（非生产运行镜像）。
# 基础：Ubuntu 24.04 LTS
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8 \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

# 系统依赖（与 AGENTS.md / .cargo/config.toml 一致：OpenSSH2、OpenSSL、gcc 链接 libstdc++、可选 clang-format/bc）
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        git \
        pkg-config \
        build-essential \
        g++ \
        libssl-dev \
        libssh2-1-dev \
        cmake \
        clang-format \
        bc \
        sudo \
    && rm -rf /var/lib/apt/lists/*

# rustup：stable + wasm32（Leptos）+ fmt/clippy（与 pre-commit 一致）
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal \
    && rustup component add rustfmt clippy \
    && rustup target add wasm32-unknown-unknown

# Trunk：构建 frontend-leptos（开发 `trunk build`；发布体积用 `trunk build --release`）
RUN cargo install trunk --locked

# 非 root：挂载宿主目录时避免 root 写文件权限问题
ARG DEV_UID=1000
ARG DEV_GID=1000
RUN groupadd -g "${DEV_GID}" dev \
    && useradd -m -u "${DEV_UID}" -g dev -s /bin/bash dev \
    && echo 'dev ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/dev \
    && chmod 0440 /etc/sudoers.d/dev \
    && chown -R dev:dev "${RUSTUP_HOME}" "${CARGO_HOME}"

WORKDIR /workspace
USER dev

# 默认进入 shell；挂载仓库到 /workspace 后执行 cargo / trunk
CMD ["bash", "-l"]
