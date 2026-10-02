# Top-level entry points. The real work is in the Rust host command
# (host/phifmpeg). Run `make help` for the list.
#
# The card crates (card/) are a separate workspace that `phifmpeg build`
# compiles for the card. Here they are formatted, linted and type-checked for
# the host, which needs neither the stack nor a card.

.DEFAULT_GOAL := help
PHIFMPEG := target/debug/phifmpeg
CARD := --manifest-path card/Cargo.toml

.PHONY: help build test fmt clippy docs-check check ffmpeg clean

help: ## Show this help
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  %-14s %s\n", $$1, $$2}'

build: ## Build the host command
	cargo build

test: ## Run the tests, host and card runner (none needs a card)
	cargo test
	cargo test $(CARD) -p phifmpeg-card

fmt: ## Check formatting, both workspaces
	cargo fmt --all -- --check
	cargo fmt --all $(CARD) -- --check

clippy: ## Lint both workspaces (the card crates without test targets: phix is no_std)
	cargo clippy --all-targets -- -D warnings
	cargo clippy $(CARD) -- -D warnings

docs-check: ## Enforce sibling .md files, the no-dash rule and relative links
	scripts/check-docs.sh

check: docs-check fmt clippy build test ## Everything that needs no card

ffmpeg: build ## Fetch the pinned sources; build the card (c) and host variants a transcode needs
	$(PHIFMPEG) fetch
	$(PHIFMPEG) build --variant c
	$(PHIFMPEG) build --variant host

clean: ## Remove both workspaces' build outputs (not the FFmpeg build root)
	cargo clean
	cargo clean $(CARD)
