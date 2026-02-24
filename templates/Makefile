# Cryochamber agent utilities
# These targets are available for the AI agent to call during sessions.

.PHONY: time clean

# Show current time or compute a future time
# Usage: make time                     # current time
#        make time OFFSET="+1 day"     # 1 day from now
#        make time OFFSET="+2 hours"   # 2 hours from now
#        make time OFFSET="+30 minutes"
OFFSET ?=

time:
ifeq ($(OFFSET),)
	@date +%Y-%m-%dT%H:%M
else
	@if date --version >/dev/null 2>&1; then \
		date -d "$(OFFSET)" +%Y-%m-%dT%H:%M; \
	else \
		N=$$(echo "$(OFFSET)" | sed 's/+//;s/[^0-9].*//');\
		U=$$(echo "$(OFFSET)" | sed 's/.*[0-9] *//;s/s$$//'); \
		case "$$U" in \
			minute) date -v+$${N}M +%Y-%m-%dT%H:%M ;; \
			hour)   date -v+$${N}H +%Y-%m-%dT%H:%M ;; \
			day)    date -v+$${N}d +%Y-%m-%dT%H:%M ;; \
			week)   date -v+$${N}w +%Y-%m-%dT%H:%M ;; \
			month)  date -v+$${N}m +%Y-%m-%dT%H:%M ;; \
			year)   date -v+$${N}y +%Y-%m-%dT%H:%M ;; \
			*) echo "Unknown unit: $$U" >&2; exit 1 ;; \
		esac; \
	fi
endif

# Kill daemon and remove all runtime files
clean:
	@if [ -f timer.json ]; then \
		echo "Stopping daemon..."; \
		cryo cancel 2>/dev/null || true; \
	fi
	@echo "Removing runtime files..."
	rm -f timer.json cryo.log cryo-agent.log gh-sync.json
	rm -rf messages .cryo
	@echo "Clean."
