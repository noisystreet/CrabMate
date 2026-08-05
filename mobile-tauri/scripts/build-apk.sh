#!/usr/bin/env bash
# 构建 CrabMate Android APK（远程薄客户端，无 sidecar）。
# 默认先 trunk build 前端（供本机 serve）；CM_MOBILE_SKIP_FRONTEND=1 可跳过。
# 用法（仓库根或任意目录）:
#   ./mobile-tauri/scripts/build-apk.sh
#   MOBILE_ANDROID_TARGET=aarch64 CM_MOBILE_GRADLE_STOP=1 ./mobile-tauri/scripts/build-apk.sh
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
mobile_root="$(cd "${script_dir}/.." && pwd)"
repo_root="$(cd "${mobile_root}/.." && pwd)"
frontend_dir="${repo_root}/frontend"
tauri_dir="${mobile_root}/src-tauri"
android_dir="${tauri_dir}/gen/android"

target="${MOBILE_ANDROID_TARGET:-aarch64}"
gradle_stop="${CM_MOBILE_GRADLE_STOP:-0}"
skip_frontend="${CM_MOBILE_SKIP_FRONTEND:-0}"

die() {
  echo "错误: $*" >&2
  exit 1
}

command -v cargo >/dev/null 2>&1 || die "未找到 cargo"
command -v cargo-tauri >/dev/null 2>&1 || command -v tauri >/dev/null 2>&1 || \
  die "未找到 Tauri CLI。请执行: cargo install tauri-cli --version \"^2\""

if [[ "${skip_frontend}" != "1" && "${skip_frontend}" != "true" && "${skip_frontend}" != "yes" ]]; then
  command -v trunk >/dev/null 2>&1 || die "未找到 trunk（前端构建需要）。见 https://trunkrs.dev/ 或: cargo install trunk"
  echo "building frontend (trunk)…"
  rustup target add wasm32-unknown-unknown 2>/dev/null || true
  trunk_flags=()
  if [[ "${RELEASE:-0}" == "1" || "${RELEASE:-}" == "true" || "${RELEASE:-}" == "yes" ]]; then
    trunk_flags+=(--release)
  fi
  (cd "${frontend_dir}" && trunk build "${trunk_flags[@]+"${trunk_flags[@]}"}")
  echo "提示: 远程 UI 来自本机 serve 的 frontend/dist；请重启 serve 后再用手机连接。"
else
  echo "跳过 frontend（CM_MOBILE_SKIP_FRONTEND=${skip_frontend}）"
fi

if ! command -v javac >/dev/null 2>&1; then
  die "未找到 javac（需要完整 JDK，不是仅 JRE）。设置 JAVA_HOME 后重试"
fi

if [[ -z "${ANDROID_HOME:-}${ANDROID_SDK_ROOT:-}" ]]; then
  die "未设置 ANDROID_HOME 或 ANDROID_SDK_ROOT"
fi

sdk_root="${ANDROID_HOME:-$ANDROID_SDK_ROOT}"
[[ -d "${sdk_root}" ]] || die "SDK 目录不存在: ${sdk_root}"

if [[ -z "${NDK_HOME:-}" ]]; then
  if [[ -d "${sdk_root}/ndk" ]]; then
    NDK_HOME="$(ls -d "${sdk_root}/ndk"/* 2>/dev/null | sort -V | tail -1 || true)"
    export NDK_HOME
  fi
fi
[[ -n "${NDK_HOME:-}" && -d "${NDK_HOME}" ]] || die "未设置 NDK_HOME，且未能从 ${sdk_root}/ndk 推断"

[[ -f "${tauri_dir}/tauri.conf.json" ]] || die "缺少 ${tauri_dir}/tauri.conf.json"
[[ -d "${android_dir}" ]] || die "缺少 Android 工程 ${android_dir}；请先: cd ${tauri_dir} && cargo tauri android init --ci"

if [[ "${gradle_stop}" == "1" || "${gradle_stop}" == "true" || "${gradle_stop}" == "yes" ]]; then
  if [[ -x "${android_dir}/gradlew" ]]; then
    echo "停止 Gradle daemon（CM_MOBILE_GRADLE_STOP=${gradle_stop}）…"
    (cd "${android_dir}" && ./gradlew --stop) || true
  fi
fi

echo "ANDROID_HOME=${sdk_root}"
echo "NDK_HOME=${NDK_HOME}"
echo "JAVA_HOME=${JAVA_HOME:-"(unset; using PATH javac)"}"
echo "target=${target}"
echo "building APK…"

cd "${tauri_dir}"
cargo tauri android build --apk --target "${target}" --ci

shopt -s nullglob
preferred="${android_dir}/app/build/outputs/apk/universal/release/crabmate.apk"
if [[ -f "${preferred}" ]]; then
  apks=("${preferred}")
else
  apks=("${android_dir}/app/build/outputs/apk/universal/release/"*.apk)
fi
if ((${#apks[@]} == 0)); then
  # split-per-abi 或其他变体布局
  apks=("${android_dir}/app/build/outputs/apk/"*/*/*.apk)
fi
shopt -u nullglob

if ((${#apks[@]} == 0)); then
  die "构建结束但未找到 APK（检查 ${android_dir}/app/build/outputs/apk/）"
fi

# 再复制一份到 mobile-tauri/crabmate.apk，方便从仓库根取用
out_apk="${mobile_root}/crabmate.apk"
cp -f "${apks[0]}" "${out_apk}"

echo ""
echo "Finished APK:"
echo "  ${out_apk}"
if [[ "${apks[0]}" != "${out_apk}" ]]; then
  echo "  (gradle) ${apks[0]}"
fi
echo ""
if [[ -f "${android_dir}/app/key.properties" ]]; then
  echo "提示: 已检测到 key.properties，release 应已签名。真机调试可用: cd ${tauri_dir} && cargo tauri android dev"
else
  echo "提示: 未找到 key.properties，release 可能为 unsigned。真机调试可用: cd ${tauri_dir} && cargo tauri android dev"
fi
