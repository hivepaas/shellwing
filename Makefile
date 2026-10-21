# Building shellwing in one word: `make build`, `make run`; `make` alone lists
# the rest. Needs Rust (rustup) and Zig 0.16 on PATH, plus each platform's
# tools (docs/platforms.md). On Windows, run it from Git Bash or MSYS2.
#
# The desktop app does not cross-compile (GPUI links each system's own window
# libraries), so build-linux runs on Linux and build-windows on Windows;
# CI builds all three (Actions > CI > Run workflow). Binaries land in dist/.

# rustup's tools, for shells that have not sourced ~/.cargo/env. Make 3.81
# (macOS) looks commands up on the PATH it started with, so they are named by
# path too.
CARGO_BIN := $(HOME)/.cargo/bin
export PATH := $(CARGO_BIN):$(PATH)
# Zig 0.16 elsewhere than on PATH: make build ZIG_DIR=/path/to/zig-0.16
ifdef ZIG_DIR
export PATH := $(ZIG_DIR):$(PATH)
endif

CARGO ?= $(if $(wildcard $(CARGO_BIN)/cargo),$(CARGO_BIN)/cargo,cargo)
RUSTUP ?= $(if $(wildcard $(CARGO_BIN)/rustup),$(CARGO_BIN)/rustup,rustup)
DIST := dist
# Linux window systems; `make build-linux LINUX_FEATURES=wayland` without the X11 libraries.
LINUX_FEATURES ?= wayland,x11

ifeq ($(OS),Windows_NT)
HOST := Windows
else
HOST := $(shell uname -s)
endif
ARCH := $(shell uname -m 2>/dev/null)

.DEFAULT_GOAL := help
.PHONY: help build run dev build-mac build-linux build-windows build-ios-core \
	build-android-core test lint fmt check install-desktop package doctor clean zig

help: ## List the targets
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk -F ':.*## ' '{printf "  make %-20s %s\n", $$1, $$2}'

## The desktop app

build: ## Release build for this computer (build-mac, build-linux or build-windows)
ifeq ($(HOST),Darwin)
	@$(MAKE) --no-print-directory build-mac
else ifeq ($(HOST),Linux)
	@$(MAKE) --no-print-directory build-linux
else ifeq ($(HOST),Windows)
	@$(MAKE) --no-print-directory build-windows
else
	@echo "no desktop build for $(HOST)"; exit 1
endif

run: zig ## Build (debug) and start the app
	$(CARGO) run -p shellwing

dev: zig ## Debug build: quicker to make, slower to run
	$(CARGO) build -p shellwing

build-mac: zig ## macOS release build -> dist/shellwing-macos-<arch>
ifneq ($(HOST),Darwin)
	@echo "build-mac runs on a Mac (this is $(HOST)); CI: Run workflow > macos"; exit 1
endif
	$(CARGO) build --release -p shellwing
	@mkdir -p $(DIST) && cp target/release/shellwing $(DIST)/shellwing-macos-$(ARCH)
	@echo "built $(DIST)/shellwing-macos-$(ARCH)"

build-linux: zig ## Linux release build -> dist/shellwing-linux-<arch>
ifneq ($(HOST),Linux)
	@echo "build-linux runs on Linux (this is $(HOST)): GPUI links the system's Wayland and X11"
	@echo "libraries. Build in a Linux VM or container, or CI: Run workflow > linux."; exit 1
endif
	$(CARGO) build --release -p shellwing --no-default-features --features $(LINUX_FEATURES)
	@mkdir -p $(DIST) && cp target/release/shellwing $(DIST)/shellwing-linux-$(ARCH)
	@echo "built $(DIST)/shellwing-linux-$(ARCH)"

build-windows: zig ## Windows release build -> dist/shellwing-windows-<arch>.exe
ifneq ($(HOST),Windows)
	@echo "build-windows runs on Windows (this is $(HOST)), with Visual Studio Build Tools;"
	@echo "see docs/platforms.md, or CI: Run workflow > windows."; exit 1
endif
	$(CARGO) build --release -p shellwing
	@mkdir -p $(DIST) && cp target/release/shellwing.exe $(DIST)/shellwing-windows-$(ARCH).exe
	@echo "built $(DIST)/shellwing-windows-$(ARCH).exe"

install-desktop: ## Linux: add the built app to the desktop's launcher
	scripts/install-desktop.sh

package: build ## What a release ships for this computer (docs/releasing.md) -> dist/
ifeq ($(HOST),Darwin)
	scripts/package_macos.sh
else ifeq ($(HOST),Linux)
	scripts/package_linux.sh
else
	scripts/package_windows.sh
endif

## The core for phones (no app yet: checks that it builds)

# Xcode rather than the Command Line Tools, which have no iOS SDK.
build-ios-core: export DEVELOPER_DIR ?= /Applications/Xcode.app/Contents/Developer
build-ios-core: zig ## shellwing-core for the iOS Simulator (needs Xcode)
	$(RUSTUP) target add aarch64-apple-ios-sim
	$(CARGO) build -p shellwing-core --target aarch64-apple-ios-sim

ANDROID_NDK_HOME ?= $(firstword $(wildcard /opt/homebrew/share/android-ndk $(HOME)/Library/Android/sdk/ndk/* $(HOME)/Android/Sdk/ndk/*))
NDK_BIN = $(ANDROID_NDK_HOME)/toolchains/llvm/prebuilt/$(if $(filter Darwin,$(HOST)),darwin,linux)-x86_64/bin
build-android-core: export ANDROID_NDK_HOME := $(ANDROID_NDK_HOME)
build-android-core: export CC_aarch64_linux_android = $(NDK_BIN)/aarch64-linux-android24-clang
build-android-core: export AR_aarch64_linux_android = $(NDK_BIN)/llvm-ar
build-android-core: zig ## shellwing-core for Android arm64 (needs the NDK)
	@test -d "$(ANDROID_NDK_HOME)" || { echo "no Android NDK found: set ANDROID_NDK_HOME (docs/platforms.md)"; exit 1; }
	$(RUSTUP) target add aarch64-linux-android
	$(CARGO) build -p shellwing-core --target aarch64-linux-android

## Before a pull request (CONTRIBUTING.md)

fmt: ## Format the code
	$(CARGO) fmt --all

lint: zig ## Formatting and clippy, as CI checks them
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --workspace --all-targets -- -D warnings

test: zig ## All tests
	$(CARGO) test --workspace

check: lint test ## lint, then test

## Housekeeping

doctor: ## Which tools are here, and which are missing
	@printf "%-12s " cargo; $(CARGO) --version 2>/dev/null || echo "missing: https://rustup.rs"
	@printf "%-12s " zig; zig version 2>/dev/null || echo "missing: Zig 0.16 from https://ziglang.org/download/"
ifeq ($(HOST),Darwin)
	@printf "%-12s " xcode; DEVELOPER_DIR=$${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer} xcrun --sdk iphonesimulator --show-sdk-version 2>/dev/null | sed 's/^/iOS SDK /' || echo "no iOS SDK (only for build-ios-core)"
endif
	@printf "%-12s " android-ndk; test -d "$(ANDROID_NDK_HOME)" && echo "$(ANDROID_NDK_HOME)" || echo "none (only for build-android-core)"

clean: ## Remove build output
	$(CARGO) clean
	rm -rf $(DIST)

# Zig must be on PATH and 0.16: other versions fail deep in libghostty's build.
zig:
	@command -v zig >/dev/null 2>&1 || { echo "zig not found: install Zig 0.16 (docs/platforms.md) or pass ZIG_DIR=/path/to/zig"; exit 1; }
	@zig version | grep -q '^0\.16\.' || { echo "zig $$(zig version) found; libghostty-vt needs 0.16.x"; exit 1; }
