.DEFAULT_GOAL := check

.PHONY: fmt check test lint build install help

fmt:
	cargo fmt

test:
	cargo test

lint:
	cargo clippy --all-targets -- -D warnings

build:
	cargo build --release --locked

check:
	cargo fmt --check
	cargo test
	cargo clippy --all-targets -- -D warnings

install:
	./scripts/install.sh

help:
	@printf '%s\n' 'make fmt | test | lint | build | check | install'
