# CrabMate 构建入口：后端（serve / CLI）与清理。
# 业务 UI：同级 Client 仓 ../crabmate-client（路径 A Phase 4.2）。
# 用法：make help

ROOT := $(abspath $(dir $(lastword $(MAKEFILE_LIST))))
CARGO ?= cargo

# RELEASE=1 时使用 --release（make all 默认开启）
RELEASE ?= 0
CARGO_PROFILE := $(if $(filter 1 true yes,$(RELEASE)),--release,)

BACKEND_BIN_DEBUG := $(ROOT)/target/debug/crabmate
BACKEND_BIN_RELEASE := $(ROOT)/target/release/crabmate
BACKEND_BIN := $(if $(filter 1 true yes,$(RELEASE)),$(BACKEND_BIN_RELEASE),$(BACKEND_BIN_DEBUG))

.DEFAULT_GOAL := help

.PHONY: help all all-dev \
	backend backend-release \
	workspace workspace-release \
	test check fmt clippy \
	clean clean-backend clean-dist

help:
	@echo "CrabMate Makefile（仓库根目录执行）"
	@echo ""
	@echo "构建："
	@echo "  make backend          后端 debug（cargo build -p crabmate）"
	@echo "  make backend-release  后端 release"
	@echo "  make workspace        工作区全部 Rust crate（debug）"
	@echo "  make workspace-release 工作区全部 Rust crate（release）"
	@echo "  make all / all-dev    同 backend-release / backend"
	@echo "  业务 UI：cd ../crabmate-client && make frontend"
	@echo "  桌面/Android 壳：cd ../crabmate-client && make help"
	@echo ""
	@echo "质检："
	@echo "  make test             cargo test --workspace"
	@echo "  make check            cargo check --workspace"
	@echo "  make fmt              cargo fmt --all"
	@echo "  make clippy           cargo clippy --workspace --all-targets --all-features -- -D warnings"
	@echo ""
	@echo "清理："
	@echo "  make clean            清理后端 target、dist/"
	@echo "  make clean-backend    cargo clean（仓库根）"
	@echo "  make clean-dist       删除 dist/ 发布目录"
	@echo ""
	@echo "变量：RELEASE=1 作用于 backend / workspace"
	@echo "serve UI：CM_WEB_STATIC_DIR=../crabmate-client/frontend/dist 或 --no-web"

# --- 聚合 ---

all: backend-release

all-dev: backend

# --- 后端 ---

backend:
	$(CARGO) build -p crabmate $(CARGO_PROFILE)

backend-release:
	$(MAKE) backend RELEASE=1

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

fmt:
	$(CARGO) fmt --all

clippy:
	$(CARGO) clippy --workspace --all-targets --all-features -- -D warnings

# --- 清理 ---

clean: clean-backend clean-dist

clean-backend:
	$(CARGO) clean

clean-dist:
	rm -rf "$(ROOT)/dist"
