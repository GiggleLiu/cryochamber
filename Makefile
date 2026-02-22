# Makefile for cryochamber

.PHONY: help build test fmt fmt-check clippy check clean coverage run-plan

# Default target
help:
	@echo "Available targets:"
	@echo "  build        - Build the project"
	@echo "  test         - Run all tests"
	@echo "  fmt          - Format code with rustfmt"
	@echo "  fmt-check    - Check code formatting"
	@echo "  clippy       - Run clippy lints"
	@echo "  check        - Quick check (fmt + clippy + test)"
	@echo "  coverage     - Generate coverage report (requires cargo-llvm-cov)"
	@echo "  clean        - Clean build artifacts"
	@echo "  run-plan     - Execute a plan with Claude headless autorun"

# Build the project
build:
	cargo build

# Run all tests
test:
	cargo test

# Format code
fmt:
	cargo fmt --all

# Check formatting
fmt-check:
	cargo fmt --all -- --check

# Run clippy
clippy:
	cargo clippy --all-targets -- -D warnings

# Quick check before commit
check: fmt-check clippy test
	@echo "All checks passed!"

# Generate coverage report (requires: cargo install cargo-llvm-cov)
coverage:
	@command -v cargo-llvm-cov >/dev/null 2>&1 || { echo "Installing cargo-llvm-cov..."; cargo install cargo-llvm-cov; }
	cargo llvm-cov --workspace --html --open

# Clean build artifacts
clean:
	cargo clean

# Run a plan with Claude in headless mode
# Usage: make run-plan [INSTRUCTIONS="..."] [OUTPUT=output.log] [AGENT_TYPE=claude]
# PLAN_FILE defaults to the most recently modified file in docs/plans/
INSTRUCTIONS ?=
OUTPUT ?= claude-output.log
AGENT_TYPE ?= claude
PLAN_FILE ?= $(shell ls -t docs/plans/*.md 2>/dev/null | head -1)

run-plan:
	@NL=$$'\n'; \
	BRANCH=$$(git branch --show-current); \
	if [ "$(AGENT_TYPE)" = "claude" ]; then \
		PROCESS="1. Read the plan file$${NL}2. Use /subagent-driven-development to execute tasks$${NL}3. Push: git push origin $$BRANCH$${NL}4. Create a pull request"; \
	else \
		PROCESS="1. Read the plan file$${NL}2. Execute the tasks step by step. For each task, implement and test before moving on.$${NL}3. Push: git push origin $$BRANCH$${NL}4. Create a pull request"; \
	fi; \
	PROMPT="Execute the plan in '$(PLAN_FILE)'."; \
	if [ -n "$(INSTRUCTIONS)" ]; then \
		PROMPT="$${PROMPT}$${NL}$${NL}## Additional Instructions$${NL}$(INSTRUCTIONS)"; \
	fi; \
	PROMPT="$${PROMPT}$${NL}$${NL}## Process$${NL}$${PROCESS}$${NL}$${NL}## Rules$${NL}- Tests should be strong enough to catch regressions.$${NL}- Do not modify tests to make them pass.$${NL}- Test failure must be reported."; \
	echo "=== Prompt ===" && echo "$$PROMPT" && echo "===" ; \
	claude --dangerously-skip-permissions \
		--model opus \
		--verbose \
		--max-turns 500 \
		-p "$$PROMPT" 2>&1 | tee "$(OUTPUT)"
