# Makefile for cryochamber

.PHONY: help build test fmt fmt-check clippy check clean example-clean coverage run-plan logo example example-cancel time check-agent check-round-trip check-gh check-service cli

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
	@echo "  clean        - Clean build artifacts (cargo clean)"
	@echo "  example-clean - Remove auto-generated files from examples"
	@echo "  logo         - Compile logo (requires typst)"
	@echo "  run-plan     - Execute a plan with Claude headless autorun"
	@echo "  example      - Run an example (DIR=examples/mr-lazy WATCH=true)"
	@echo "  example-cancel - Stop a running example (DIR=examples/mr-lazy)"
	@echo "  time         - Show current time or compute offset (OFFSET=\"+1 day\")"
	@echo "  check-agent  - Quick agent smoke test (runs agent once)"
	@echo "  check-round-trip - Full round-trip test with mr-lazy (daemon, Ctrl-C to stop)"
	@echo "  check-gh     - Verify GitHub Discussion sync (requires gh auth)"
	@echo "  check-service - Verify OS service install/uninstall (launchd/systemd)"
	@echo "  cli          - Install the cryo CLI locally"

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

# Remove auto-generated files from examples (cancels running daemons first)
example-clean:
	@for dir in examples/*/; do \
		if [ -f "$(CURDIR)/$$dir/timer.json" ]; then \
			cd "$(CURDIR)/$$dir" && $(CURDIR)/target/debug/cryo cancel 2>/dev/null; \
		fi; \
	done; true
	rm -f examples/*/CLAUDE.md examples/*/AGENTS.md
	rm -f examples/*/*.log examples/*/*.json
	rm -rf examples/*/messages examples/*/.cryo

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

# Install the cryo CLI
cli:
	cargo install --path .

# Run an example
# Usage: make example DIR=examples/mr-lazy
#        make example DIR=examples/chess-by-mail AGENT=claude
#        make example DIR=examples/chess-by-mail WATCH=false  # no watch (interactive use)
DIR ?= examples/mr-lazy
WATCH ?= true
example: build
	@if [ -f "$(DIR)/timer.json" ]; then (cd "$(DIR)" && $(CURDIR)/target/debug/cryo cancel 2>/dev/null); fi; \
	cd "$(DIR)" && rm -rf .cryo timer.json cryo.log cryo-agent.log messages AGENTS.md CLAUDE.md && \
	$(CURDIR)/target/debug/cryo init --agent "$(AGENT)" && $(CURDIR)/target/debug/cryo start --agent "$(AGENT)"; \
	if [ "$(WATCH)" = "true" ]; then \
		$(CURDIR)/target/debug/cryo watch --all; \
	else \
		echo "Daemon started. Use 'cryo send', 'cryo watch', 'make example-cancel' to interact."; \
	fi

# Stop a running example
# Usage: make example-cancel DIR=examples/chess-by-mail
example-cancel:
	cd "$(DIR)" && $(CURDIR)/target/debug/cryo cancel

# Quick smoke test: force one agent wakeup cycle
# Usage: make check-agent                 # check default (opencode)
#        make check-agent AGENT=claude    # check claude
AGENT ?= opencode
CHECK_TIMEOUT ?= 3000

check-agent: build
	@TMPDIR=$$(mktemp -d /tmp/cryo-check-XXXXXX); \
	cp examples/mr-lazy/plan.md "$$TMPDIR/plan.md"; \
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
	cp examples/mr-lazy/plan.md "$$TMPDIR/plan.md"; \
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

# Verify GitHub Discussion sync (requires: gh auth login)
# Usage: make check-gh REPO="owner/repo"
REPO ?= GiggleLiu/cryochamber

check-gh: build
	@echo "=== GitHub Sync Check ==="
	@echo "1. Checking gh CLI..."; \
	if command -v gh >/dev/null 2>&1; then \
		echo "   OK: $$(command -v gh)"; \
	else \
		echo "   FAIL: 'gh' not found. Install: https://cli.github.com"; exit 1; \
	fi; \
	echo ""; \
	echo "2. Checking gh authentication..."; \
	if gh auth status >/dev/null 2>&1; then \
		echo "   OK: authenticated as $$(gh api user -q .login)"; \
	else \
		echo "   FAIL: not authenticated. Run: gh auth login"; exit 1; \
	fi; \
	echo ""; \
	echo "3. Creating test Discussion in $(REPO)..."; \
	TMPDIR=$$(mktemp -d /tmp/cryo-check-gh-XXXXXX); \
	printf '# Health Check\n\nThis is an automated test.\n' > "$$TMPDIR/plan.md"; \
	cd "$$TMPDIR" && \
	$(CURDIR)/target/debug/cryo-gh init --repo "$(REPO)" --title "[Cryo] Health Check $$(date +%Y%m%d-%H%M%S)"; \
	RC=$$?; \
	if [ $$RC -ne 0 ]; then \
		echo "   FAIL: could not create Discussion"; \
		rm -rf "$$TMPDIR"; \
		exit 1; \
	fi; \
	echo "   OK: Discussion created"; \
	echo ""; \
	echo "4. Posting test comment..."; \
	mkdir -p "$$TMPDIR/messages/inbox"; \
	printf '--- CRYO SESSION 1 ---\ntask: health check\nagent: gh-check\ninbox: 0 messages\n[00:00:01] agent started (pid 1)\n[00:00:02] hibernate: complete, exit=0, summary="Health check passed"\n[00:00:02] agent exited (code 0)\n--- CRYO END ---\n' > "$$TMPDIR/cryo.log"; \
	printf '{"plan_path":"plan.md","session_number":1,"last_command":null,"pid":null,"max_retries":1,"retry_count":0,"max_session_duration":300,"watch_inbox":false,"daemon_mode":false}' > "$$TMPDIR/timer.json"; \
	$(CURDIR)/target/debug/cryo-gh push; \
	RC=$$?; \
	if [ $$RC -ne 0 ]; then \
		echo "   FAIL: could not post comment"; \
		rm -rf "$$TMPDIR"; \
		exit 1; \
	fi; \
	echo "   OK: comment posted"; \
	rm -rf "$$TMPDIR"; \
	echo ""; \
	echo "=== GitHub sync check passed ==="

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
	cp examples/mr-lazy/plan.md "$$TMPDIR/plan.md"; \
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
		LABEL=$$(ls ~/Library/LaunchAgents/com.cryo.daemon.*.plist 2>/dev/null | head -1); \
		if [ -n "$$LABEL" ]; then \
			echo "   OK: plist found: $$(basename $$LABEL)"; \
		else \
			echo "   FAIL: no launchd plist found"; \
			cd "$$TMPDIR" && $(CURDIR)/target/debug/cryo cancel 2>/dev/null; \
			rm -rf "$$TMPDIR"; exit 1; \
		fi; \
	else \
		UNIT=$$(ls ~/.config/systemd/user/com.cryo.daemon.*.service 2>/dev/null | head -1); \
		if [ -n "$$UNIT" ]; then \
			echo "   OK: unit found: $$(basename $$UNIT)"; \
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
	if [ "$$(uname -s)" = "Darwin" ]; then \
		LABEL=$$(ls ~/Library/LaunchAgents/com.cryo.daemon.*.plist 2>/dev/null | grep "$$TMPDIR" | head -1); \
	else \
		LABEL=$$(ls ~/.config/systemd/user/com.cryo.daemon.*.service 2>/dev/null | grep "$$TMPDIR" | head -1); \
	fi; \
	if [ -z "$$LABEL" ]; then \
		echo "   OK: service file removed"; \
	else \
		echo "   FAIL: service file still exists: $$LABEL"; \
		rm -rf "$$TMPDIR"; exit 1; \
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
