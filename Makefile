# Makefile for cryochamber

.PHONY: help build test fmt fmt-check clippy check clean example-clean coverage run-plan logo example example-start-all example-cancel example-hub time check-agent check-round-trip check-service check-mock cli console-build console-check book book-serve book-deploy copilot-review release

RUNNER ?= codex
CLAUDE_MODEL ?= opus
CODEX_MODEL ?= gpt-5.5

# Default target
help:
	@echo "Available targets:"
	@echo "  build        - Build the project"
	@echo "  test         - Run all tests"
	@echo "  fmt          - Format code with rustfmt"
	@echo "  fmt-check    - Check code formatting"
	@echo "  clippy       - Run clippy lints"
	@echo "  check        - Quick check (fmt + clippy + test + console-check)"
	@echo "  console-check - Type-check and unit-test the Agent Console (npm ci, tsc, vitest)"
	@echo "  coverage     - Generate coverage report (requires cargo-llvm-cov)"
	@echo "  clean        - Clean build artifacts (cargo clean)"
	@echo "  example-clean - Remove auto-generated files from examples"
	@echo "  logo         - Compile logo (requires typst)"
	@echo "  run-plan     - Execute a plan with Codex or Claude"
	@echo "  example      - Run an example (DIR=examples/chambers/mr-lazy or .../chess-by-mail)"
	@echo "  example-start-all - Start all example chambers (AGENT=opencode|claude)"
	@echo "  example-cancel - Stop a running example (DIR=examples/...)"
	@echo "  example-hub  - Start global cryohub in foreground (PORT=8765)"
	@echo "  time         - Show current time or compute offset (OFFSET=\"+1 day\")"
	@echo "  check-agent  - Quick agent smoke test (runs agent once)"
	@echo "  check-round-trip - Full round-trip test with mr-lazy (daemon, Ctrl-C to stop)"
	@echo "  check-service - Verify OS service install/uninstall (launchd/systemd)"
	@echo "  check-mock   - Run mock agent integration tests"
	@echo "  cli          - Install the cryo CLI locally"
	@echo "  console-build  - Build the Agent Console (embedded into cryohub on the next cargo build)"
	@echo "  book         - Build mdbook documentation (en + zh)"
	@echo "  book-serve   - Build and serve the full book (en + zh) at :3000"
	@echo "  book-serve-live - mdbook serve with live reload (English book only; zh links 404 here)"
	@echo "  book-serve-zh - Serve the Chinese book locally with live reload"
	@echo "  book-deploy  - Deploy mdbook to GitHub Pages (gh-pages branch)"
	@echo "  copilot-review - Request Copilot code review on current PR"
	@echo "  release V=x.y.z - Tag and push a release (triggers CI publish)"
	@echo ""
	@echo "  Set RUNNER=claude to use Claude instead of Codex (default: codex)"
	@echo "  Override CODEX_MODEL or CLAUDE_MODEL to pick a different model"

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
check: fmt-check clippy test console-check
	@echo "All checks passed!"

# Type-check and unit-test the Agent Console. Uses `npm ci` so the lockfile is
# never rewritten by a local run.
console-check:
	cd console && npm ci && npx tsc --noEmit && npx vitest run

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

# Remove auto-generated files from examples (cancels running daemons first)
example-clean:
	@for dir in examples/chambers/*/; do \
		if [ -f "$(CURDIR)/$$dir/timer.json" ]; then \
			cd "$(CURDIR)/$$dir" && $(CURDIR)/target/debug/cryo cancel 2>/dev/null; \
		fi; \
	done; true
	rm -f examples/chambers/*/CLAUDE.md examples/chambers/*/AGENTS.md
	rm -f examples/chambers/*/*.log examples/chambers/*/*.json
	rm -rf examples/chambers/*/messages examples/chambers/*/.cryo

# Run a plan with Codex or Claude
# Usage: make run-plan [INSTRUCTIONS="..."] [OUTPUT=output.log] [AGENT_TYPE=<codex|claude>]
# PLAN_FILE defaults to the most recently modified file in docs/plans/
INSTRUCTIONS ?=
OUTPUT ?= run-plan-output.log
AGENT_TYPE ?= $(RUNNER)
PLAN_FILE ?= $(shell ls -t docs/plans/*.md 2>/dev/null | head -1)

run-plan:
	@. scripts/make_helpers.sh; \
	NL=$$(printf '\n.'); \
	NL=$${NL%.}; \
	BRANCH=$$(git branch --show-current); \
	PLAN_FILE="$(PLAN_FILE)"; \
	if [ "$(AGENT_TYPE)" = "claude" ]; then \
		PROCESS="1. Read the plan file$${NL}2. Execute the plan; it specifies which skill(s) to use$${NL}3. Push: git push origin $$BRANCH$${NL}4. If a PR already exists for this branch, skip. Otherwise create one."; \
	else \
		PROCESS="1. Read the plan file$${NL}2. Treat slash-command references as workflow instructions rather than requiring Claude slash-command support.$${NL}3. Execute the tasks step by step. For each task, implement and test before moving on.$${NL}4. Push: git push origin $$BRANCH$${NL}5. If a PR already exists for this branch, skip. Otherwise create one."; \
	fi; \
	PROMPT="Execute the plan in '$$PLAN_FILE'."; \
	if [ "$(AGENT_TYPE)" != "claude" ]; then \
		PROMPT="$${PROMPT}$${NL}$${NL}Treat any slash-command references in the plan as workflow instructions; do not assume Claude slash-command support."; \
	fi; \
	if [ -n "$(INSTRUCTIONS)" ]; then \
		PROMPT="$${PROMPT}$${NL}$${NL}## Additional Instructions$${NL}$(INSTRUCTIONS)"; \
	fi; \
	PROMPT="$${PROMPT}$${NL}$${NL}## Process$${NL}$${PROCESS}$${NL}$${NL}## Rules$${NL}- Tests should be strong enough to catch regressions.$${NL}- Do not modify tests to make them pass.$${NL}- Test failure must be reported."; \
	echo "=== Prompt ===" && echo "$$PROMPT" && echo "===" ; \
	RUNNER="$(AGENT_TYPE)" run_agent "$(OUTPUT)" "$$PROMPT"

# Install the cryo CLI
cli:
	cargo install --path .

# Build the Agent Console into console/dist/. The next `cargo build` embeds it
# into the cryohub binary; `console_dir` in cryohub.toml can override that with
# any built directory (absolute path).
console-build:
	cd console && npm ci && npm run build

# Run an example
# Usage: make example DIR=examples/chambers/mr-lazy
#        make example DIR=examples/chambers/chess-by-mail AGENT=claude
example: build
	@if [ -z "$(DIR)" ]; then echo "Usage: make example DIR=examples/chambers/mr-lazy"; exit 1; fi
	@if [ -f "$(DIR)/timer.json" ]; then (cd "$(DIR)" && $(CURDIR)/target/debug/cryo cancel 2>/dev/null); fi; \
	cd "$(DIR)" && rm -rf .cryo timer.json cryo.log cryo-agent.log messages AGENTS.md CLAUDE.md && \
	$(CURDIR)/target/debug/cryo init --agent "$(AGENT)" && $(CURDIR)/target/debug/cryo start --agent "$(AGENT)" && \
	cd "$(CURDIR)" && $(CURDIR)/target/debug/cryohub start --foreground

# Start all bundled examples and register them for global Cryohub.
# Usage: make example-start-all
#        make example-start-all AGENT=claude
example-start-all: build
	@set -e; \
	for dir in "$(CURDIR)"/examples/chambers/*/; do \
		name=$$(basename "$$dir"); \
		echo "=== Starting example: $$name ==="; \
		if [ -f "$$dir/timer.json" ]; then \
			(cd "$$dir" && "$(CURDIR)"/target/debug/cryo cancel 2>/dev/null || true); \
		fi; \
		cd "$$dir"; \
		"$(CURDIR)"/target/debug/cryo init --agent "$(AGENT)"; \
		"$(CURDIR)"/target/debug/cryo start --agent "$(AGENT)"; \
	done

# Stop a running example
# Usage: make example-cancel DIR=examples/chambers/chess-by-mail
example-cancel:
	cd "$(DIR)" && $(CURDIR)/target/debug/cryo cancel

# Start global cryohub in the foreground. Examples appear after they have been
# registered with `cryo start`, for example by running `make example DIR=...`.
# Usage: make example-hub
#        make example-hub PORT=8080
PORT ?= 8765

example-hub: build
	$(CURDIR)/target/debug/cryohub start --foreground --port $(PORT)

# Quick smoke test: force one agent wakeup cycle
# Usage: make check-agent                 # check default (opencode)
#        make check-agent AGENT=claude    # check claude
AGENT ?= opencode
CHECK_TIMEOUT ?= 3000

check-agent: build
	@TMPDIR=$$(mktemp -d /tmp/cryo-check-XXXXXX); \
	cp examples/chambers/mr-lazy/plan.md "$$TMPDIR/plan.md"; \
	cd "$$TMPDIR" && $(CURDIR)/target/debug/cryo init --agent "$(AGENT)"; \
	echo "=== Agent Health Check ==="; \
	echo "Agent: $(AGENT)"; \
	echo ""; \
	cd "$$TMPDIR" && $(CURDIR)/target/debug/cryo start \
		--agent "$(AGENT)" \
		--max-session-duration $(CHECK_TIMEOUT) 2>&1; \
	RC=$$?; \
	if [ $$RC -ne 0 ]; then \
		echo "FAIL: cryo start failed (exit code $$RC)"; \
		rm -rf "$$TMPDIR"; \
		exit 1; \
	fi; \
	echo ""; \
	echo "=== Session Log (Ctrl-C to stop) ==="; \
	trap 'cd "'"$$TMPDIR"'" && '"$(CURDIR)"'/target/debug/cryo cancel 2>/dev/null; rm -rf "'"$$TMPDIR"'"; exit 0' INT TERM; \
	cd "$$TMPDIR" && $(CURDIR)/target/debug/cryo watch --all; \
	cd "$$TMPDIR" && $(CURDIR)/target/debug/cryo cancel 2>/dev/null; \
	rm -rf "$$TMPDIR"

# Full round-trip test with mr-lazy example (daemon mode)
# Runs until plan completes or Ctrl-C, then cleans up.
# Usage: make check-round-trip                 # check default (opencode)
#        make check-round-trip AGENT=claude    # check claude
check-round-trip: build
	@echo "=== Round-Trip Test (mr-lazy) ==="
	@PROG=$$(echo "$(AGENT)" | awk '{print $$1}'); \
	echo "Agent:   $(AGENT)"; \
	echo "Timeout: $(CHECK_TIMEOUT)s"; \
	echo ""; \
	echo "1. Checking if $$PROG is in PATH..."; \
	if command -v "$$PROG" >/dev/null 2>&1; then \
		echo "   OK: $$(command -v $$PROG)"; \
	else \
		echo "   FAIL: '$$PROG' not found in PATH"; exit 1; \
	fi; \
	echo ""; \
	echo "2. Starting mr-lazy daemon..."; \
	TMPDIR=$$(mktemp -d /tmp/cryo-check-XXXXXX); \
	cp examples/chambers/mr-lazy/plan.md "$$TMPDIR/plan.md"; \
	cd "$$TMPDIR" && $(CURDIR)/target/debug/cryo init --agent "$(AGENT)"; \
	cd "$$TMPDIR" && $(CURDIR)/target/debug/cryo start \
		--agent "$(AGENT)" \
		--max-session-duration $(CHECK_TIMEOUT) 2>&1; \
	RC=$$?; \
	echo ""; \
	if [ $$RC -ne 0 ]; then \
		echo "   FAIL: cryo daemon failed to start (exit code $$RC)"; \
		echo "   Last 10 lines of log:"; \
		tail -10 "$$TMPDIR/cryo.log" 2>/dev/null | sed 's/^/   | /' || echo "   (no log)"; \
		rm -rf "$$TMPDIR"; \
		exit 1; \
	fi; \
	echo "   OK: Daemon started. Watching log (Ctrl-C to stop)..."; \
	echo ""; \
	trap 'echo ""; echo "Stopping daemon..."; cd "'"$$TMPDIR"'" && '"$(CURDIR)"'/target/debug/cryo cancel 2>/dev/null; rm -rf "'"$$TMPDIR"'"; echo "=== Done ==="; exit 0' INT TERM; \
	cd "$$TMPDIR" && $(CURDIR)/target/debug/cryo watch --all; \
	echo ""; \
	cd "$$TMPDIR" && $(CURDIR)/target/debug/cryo cancel 2>/dev/null; \
	rm -rf "$$TMPDIR"; \
	echo "=== Round-trip test done ==="


# Verify OS service install/uninstall lifecycle (launchd on macOS, systemd on Linux)
# This test installs a real service, verifies it runs, cancels it, and cleans up.
# Usage: make check-service
#        make check-service AGENT="opencode run"
check-service: build
	@echo "=== Service Lifecycle Check ==="
	@echo "Platform: $$(uname -s)"
	@echo ""
	@echo "1. Setting up test project..."
	@TMPDIR=$$(mktemp -d /tmp/cryo-check-svc-XXXXXX); \
	cp examples/chambers/mr-lazy/plan.md "$$TMPDIR/plan.md"; \
	cd "$$TMPDIR" && $(CURDIR)/target/debug/cryo init --agent "$(AGENT)"; \
	echo "   OK: $$TMPDIR"; \
	echo ""; \
	echo "2. Installing daemon service (cryo start)..."; \
	cd "$$TMPDIR" && $(CURDIR)/target/debug/cryo start \
		--agent "/bin/sh -c 'sleep 600'" \
		--max-session-duration 600 2>&1; \
	RC=$$?; \
	if [ $$RC -ne 0 ]; then \
		echo "   FAIL: cryo start failed (exit $$RC)"; \
		rm -rf "$$TMPDIR"; exit 1; \
	fi; \
	echo "   OK: service installed"; \
	echo ""; \
	echo "3. Verifying service is running..."; \
	sleep 2; \
	if [ "$$(uname -s)" = "Darwin" ]; then \
		SVC_FILE=$$(ls -t ~/Library/LaunchAgents/com.cryo.daemon.*.plist 2>/dev/null | head -1); \
		if [ -n "$$SVC_FILE" ]; then \
			echo "   OK: plist found: $$(basename $$SVC_FILE)"; \
		else \
			echo "   FAIL: no launchd plist found"; \
			cd "$$TMPDIR" && $(CURDIR)/target/debug/cryo cancel 2>/dev/null; \
			rm -rf "$$TMPDIR"; exit 1; \
		fi; \
	else \
		SVC_FILE=$$(ls -t ~/.config/systemd/user/com.cryo.daemon.*.service 2>/dev/null | head -1); \
		if [ -n "$$SVC_FILE" ]; then \
			echo "   OK: unit found: $$(basename $$SVC_FILE)"; \
		else \
			echo "   FAIL: no systemd unit found"; \
			cd "$$TMPDIR" && $(CURDIR)/target/debug/cryo cancel 2>/dev/null; \
			rm -rf "$$TMPDIR"; exit 1; \
		fi; \
	fi; \
	PID=$$(cd "$$TMPDIR" && cat timer.json 2>/dev/null | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('pid',''))" 2>/dev/null); \
	if [ -n "$$PID" ] && kill -0 "$$PID" 2>/dev/null; then \
		echo "   OK: daemon process alive (PID $$PID)"; \
	else \
		echo "   WARN: daemon PID not found in timer.json (may still be starting)"; \
	fi; \
	echo ""; \
	echo "4. Cancelling (cryo cancel)..."; \
	cd "$$TMPDIR" && $(CURDIR)/target/debug/cryo cancel 2>&1; \
	RC=$$?; \
	if [ $$RC -ne 0 ]; then \
		echo "   FAIL: cryo cancel failed (exit $$RC)"; \
		rm -rf "$$TMPDIR"; exit 1; \
	fi; \
	echo "   OK: cancelled"; \
	echo ""; \
	echo "5. Verifying service removed..."; \
	if [ -e "$$SVC_FILE" ]; then \
		echo "   FAIL: service file still exists: $$SVC_FILE"; \
		rm -rf "$$TMPDIR"; exit 1; \
	else \
		echo "   OK: service file removed ($$SVC_FILE)"; \
	fi; \
	rm -rf "$$TMPDIR"; \
	echo ""; \
	echo "=== Service lifecycle check passed ==="; \
	echo ""; \
	echo "To test reboot persistence, run manually:"; \
	echo "  cd /tmp/cryo-reboot-test && cryo init && cryo start --agent '/bin/sh -c sleep 999'"; \
	echo "  # Reboot your machine"; \
	echo "  # After reboot, verify:"; \
	echo "  #   macOS:  launchctl list | grep com.cryo"; \
	echo "  #   Linux:  systemctl --user status com.cryo.daemon.*"

# Run mock agent scenario tests (no external agent required)
check-mock:
	cargo test --test mock_agent_tests -- --nocapture --test-threads=1

# Build mdbook documentation (English at book/, Chinese at book/zh/)
book:
	@command -v mdbook >/dev/null 2>&1 || { echo "Installing mdbook..."; cargo install mdbook; }
	mdbook build
	MDBOOK_BOOK__LANGUAGE=zh \
	MDBOOK_BOOK__SRC=docs/zh \
	MDBOOK_OUTPUT__HTML__EDIT_URL_TEMPLATE="https://github.com/GiggleLiu/cryochamber/edit/main/{path}" \
	MDBOOK_OUTPUT__HTML__SEARCH__ENABLE=false \
	mdbook build -d book/zh

# Build and serve the full book (en + zh) so the language switcher works
# end-to-end locally. A static server is used because mdbook serve only
# serves the book it is running, at the root path — the zh pages at /zh/
# would 404 under it.
book-serve: book
	@command -v python3 >/dev/null 2>&1 || { echo "python3 is required to serve the book; run \`make book\` and serve book/ yourself, or use \`make book-serve-live\` (en only)"; exit 1; }
	@echo "Serving the full book at http://127.0.0.1:3000/ (Ctrl-C to stop)"
	@cd book && python3 -m http.server 3000

# mdbook serve with live reload (English book only; the zh pages at /zh/
# are not served by mdbook, so the language switcher will 404 on them)
book-serve-live:
	@command -v mdbook >/dev/null 2>&1 || { echo "Installing mdbook..."; cargo install mdbook; }
	mdbook serve --open

# Serve the Chinese book locally with live reload
book-serve-zh:
	@command -v mdbook >/dev/null 2>&1 || { echo "Installing mdbook..."; cargo install mdbook; }
	MDBOOK_BOOK__LANGUAGE=zh MDBOOK_BOOK__SRC=docs/zh MDBOOK_OUTPUT__HTML__SEARCH__ENABLE=false mdbook serve -d book/zh --open

# Deploy mdbook to GitHub Pages (gh-pages branch)
book-deploy: book
	@echo "=== Deploying to gh-pages ==="
	@TMPDIR=$$(mktemp -d); \
	cp -r book/* "$$TMPDIR/"; \
	cd "$$TMPDIR" && \
	git init && \
	git checkout -b gh-pages && \
	git add -A && \
	git commit -m "Deploy mdbook" && \
	git remote add origin "$$(cd "$(CURDIR)" && git remote get-url origin)" && \
	git push --force origin gh-pages; \
	rm -rf "$$TMPDIR"; \
	echo "=== Deployed to gh-pages ==="

# Tag and push a release (triggers CI publish to crates.io)
# Usage: make release V=x.y.z
release:
ifndef V
	$(error Usage: make release V=x.y.z)
endif
	@echo "Releasing v$(V)..."
	perl -i -pe 's/^version = ".*"/version = "$(V)"/' Cargo.toml
	cargo check
	git add Cargo.toml Cargo.lock
	git commit -m "release: v$(V)"
	git tag -a "v$(V)" -m "Release v$(V)"
	git push origin main --tags
	@echo "v$(V) pushed — CI will publish to crates.io"

# Request Copilot code review on the current PR
# Requires: gh extension install ChrisCarini/gh-copilot-review
copilot-review:
	@PR=$$(gh pr view --json number --jq .number 2>/dev/null) || { echo "No PR found for current branch"; exit 1; }; \
	echo "Requesting Copilot review on PR #$$PR..."; \
	gh copilot-review $$PR
