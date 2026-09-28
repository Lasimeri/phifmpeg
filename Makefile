# Top-level entry points. The real work is in the Rust host command
# (host/phifmpeg). Run `make help` for the list.

.DEFAULT_GOAL := help
PHIFMPEG := target/debug/phifmpeg

.PHONY: help build test fmt clippy docs-check check ffmpeg asm clean

help: ## Show this help
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  %-14s %s\n", $$1, $$2}'

build: ## Build the host command
	cargo build

test: ## Run host tests that do not need a card
	cargo test

fmt: ## Check formatting
	cargo fmt --all -- --check

clippy: ## Lint
	cargo clippy --all-targets -- -D warnings

docs-check: ## Enforce sibling .md files, the no-dash rule and relative links
	scripts/check-docs.sh

check: docs-check fmt clippy build test ## Everything CI would run

ffmpeg: build ## Fetch the pinned sources; build the card (c) and host variants a transcode needs
	$(PHIFMPEG) fetch
	$(PHIFMPEG) build --variant c
	$(PHIFMPEG) build --variant host

asm: build ## Build the card variant with both projects' x86 SIMD (for the vector-unit work)
	$(PHIFMPEG) fetch
	$(PHIFMPEG) build --variant asm

clean: ## Remove the host command's build outputs (not the FFmpeg build root)
	cargo clean
