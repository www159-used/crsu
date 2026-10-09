.DEFAULT_GOAL := check

CARGO ?= cargo

ifneq ($(strip $(INSTALL_ROOT)),)
INSTALL_ARGS := --root "$(INSTALL_ROOT)"
endif

.PHONY: fmt fmt-check test lint build check install help

fmt:
	$(CARGO) fmt

fmt-check:
	$(CARGO) fmt --check

test:
	$(CARGO) test

lint:
	$(CARGO) clippy --all-targets -- -D warnings

build:
	$(CARGO) build --release --locked

check:
	$(CARGO) fmt --check
	$(CARGO) test
	$(CARGO) clippy --all-targets -- -D warnings

install:
	$(CARGO) install --path crates/crsu --locked --force $(INSTALL_ARGS)

help:
	@printf '%s\n' 'make fmt | fmt-check | test | lint | build | check | install' 'make install INSTALL_ROOT=/path/to/install-root'
