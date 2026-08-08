# CrabMate 构建入口：后端、前端与清理。
# 桌面 / Android 壳：同级 Client 仓 ../crabmate-client（路径 A Phase 4）。
# 用法：make help

ROOT := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
FRONTEND_DIR := $(ROOT)/frontend
CARGO ?= cargo

# RELEASE=1 时使用 --release（make all 默认开启）
RELEASE ?= 0
CARGO_PROFILE := $(if $(filter 1 true yes,$(RELEASE)),--release,)

BACKEND_BIN_DEBUG := $(ROOT)/target/debug/crabmate
BACKEND_BIN_RELEASE := $(ROOT)/target/release/crabmate
BACKEND_BIN := $(if $(filter 1 true yes,$(RELEASE)),$(BACKEND_BIN_RELEASE),$(BACKEND_BIN_DEBUG))

TRUNK_BUILD_FLAGS := $(if $(filter 1 true yes,$(RELEASE)),--release,)

.DEFAULT_GOAL := help

.PHONY: help all all-dev \
	backend backend-release \
	frontend frontend-release \
	workspace workspace-release \
	test check fmt clippy \
	clean clean-backend clean-frontend clean-dist

help:
	@echo "CrabMate Makefile（仓库根目录执行）"
	@echo ""
	@echo "构建："
	@echo "  make backend          后端 debug（cargo build -p crabmate）"
	@echo "  make backend-release  后端 release"
	@echo "  make frontend         前端 debug（cd frontend && trunk build）"
	@echo "  make frontend-release 前端 release（供 serve / 打包；源码迁出见 Phase 4.2）"
	@echo "  make workspace        工作区全部 Rust crate（debug）"
	@echo "  make workspace-release 工作区全部 Rust crate（release）"
	@echo "  make all-dev          后端 + 前端（debug）"
	@echo "  make all              后端 + 前端（均为 release）"
	@echo "  桌面/Android 壳：cd ../crabmate-client && make help"
	@echo ""
	@echo "质检："
	@echo "  make test             cargo test --workspace"
	@echo "  make check            wasm check + cargo check --workspace"
	@echo "  make fmt              cargo fmt --all"
	@echo "  make clippy           cargo clippy --workspace --all-targets --all-features -- -D warnings"
	@echo ""
	@echo "清理："
	@echo "  make clean            清理后端 target、前端 dist、dist/"
	@echo "  make clean-backend    cargo clean（仓库根）"
	@echo "  make clean-frontend   删除 frontend/dist"
	@echo "  make clean-dist       删除 dist/ 发布目录"
	@echo ""
	@echo "变量：RELEASE=1 作用于 backend / frontend / workspace"

# --- 聚合 ---

all: backend-release frontend-release

all-dev: backend frontend

# --- 后端 ---

backend:
	$(CARGO) build -p crabmate $(CARGO_PROFILE)

backend-release:
	$(MAKE) backend RELEASE=1

# --- 前端 ---

frontend:
	@command -v trunk >/dev/null 2>&1 || { \
		echo "错误: 未找到 trunk。见 https://trunkrs.dev/ 或 cargo install trunk" >&2; \
		exit 1; \
	}
	rustup target add wasm32-unknown-unknown 2>/dev/null || true
	cd "$(FRONTEND_DIR)" && trunk build $(TRUNK_BUILD_FLAGS)

frontend-release:
	@command -v wasm-opt >/dev/null 2>&1 || { \
		echo "警告: 未找到 wasm-opt。trunk build --release 会产出空 .wasm 文件。建议: cargo install wasm-opt" >&2; \
	}
	$(MAKE) frontend RELEASE=1

# --- 工作区 Rust ---

workspace:
	$(CARGO) build --workspace $(CARGO_PROFILE)

workspace-release:
	$(MAKE) workspace RELEASE=1

# --- 质检（可选）---

test:
	$(CARGO) test --workspace

check:
	$(CARGO) check --workspace --all-targets
	cd "$(FRONTEND_DIR)" && $(CARGO) check --target wasm32-unknown-unknown

fmt:
	$(CARGO) fmt --all

clippy:
	$(CARGO) clippy --workspace --all-targets --all-features -- -D warnings
	cd "$(FRONTEND_DIR)" && $(CARGO) clippy --all-targets --all-features -- -D warnings

# --- 清理 ---

clean: clean-backend clean-frontend clean-dist

clean-backend:
	$(CARGO) clean

clean-frontend:
	rm -rf "$(FRONTEND_DIR)/dist"

clean-dist:
	rm -rf "$(ROOT)/dist"
