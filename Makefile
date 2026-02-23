# Makefile for cryochamber

.PHONY: help build test fmt fmt-check clippy check clean coverage run-plan logo chess time check-agent

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
	@echo "  logo         - Compile logo (requires typst)"
	@echo "  run-plan     - Execute a plan with Claude headless autorun"
	@echo "  chess        - Run the chess-by-mail example"
	@echo "  time         - Show current time or compute offset (OFFSET=\"+1 day\")"
	@echo "  check-agent  - Verify agent is installed and supports headless mode"

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

# Compile logo (requires typst)
logo:
	typst compile docs/logo/logo.typ docs/logo/logo.svg
	typst compile docs/logo/logo.typ docs/logo/logo.png --ppi 300

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

# Run the chess-by-mail example
chess: build
	cargo run -- start examples/chess-by-mail

# Verify agent is installed and can run headlessly
# Usage: make check-agent                    # check default (opencode)
#        make check-agent AGENT="claude"     # check claude
#        make check-agent AGENT="opencode run"  # check opencode in headless mode
AGENT ?= opencode

check-agent:
	@echo "=== Agent Health Check ==="
	@PROG=$$(echo "$(AGENT)" | awk '{print $$1}'); \
	echo "Agent command: $(AGENT)"; \
	echo "Executable:    $$PROG"; \
	echo ""; \
	echo "1. Checking if $$PROG is in PATH..."; \
	if command -v "$$PROG" >/dev/null 2>&1; then \
		echo "   OK: $$(command -v $$PROG)"; \
	else \
		echo "   FAIL: '$$PROG' not found in PATH"; exit 1; \
	fi; \
	echo ""; \
	echo "2. Checking --prompt flag support..."; \
	if "$$PROG" --help 2>&1 | grep -q '\-\-prompt'; then \
		echo "   OK: --prompt flag supported"; \
	else \
		echo "   WARN: --prompt flag not found in help output"; \
	fi; \
	echo ""; \
	echo "3. Checking headless (non-interactive) mode..."; \
	case "$$PROG" in \
		opencode) \
			if echo "$(AGENT)" | grep -q "run"; then \
				echo "   OK: 'opencode run' is headless"; \
			else \
				echo "   FAIL: bare 'opencode' starts an interactive TUI"; \
				echo "   FIX:  use --agent \"opencode run\" with cryo"; \
				exit 1; \
			fi ;; \
		claude) \
			echo "   OK: claude supports headless via -p/--prompt" ;; \
		*) \
			echo "   INFO: unknown agent, cannot verify headless support"; \
			echo "   Make sure '$(AGENT) --prompt <text>' runs non-interactively" ;; \
	esac; \
	echo ""; \
	echo "=== Health check passed ==="
