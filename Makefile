METRICS_URL ?= http://127.0.0.1:6379/metrics
INTERVAL_MS ?=

INSPECTOR = npx @modelcontextprotocol/inspector --cli \
	-e WYRM_METRICS_URL=$(METRICS_URL) cargo run --quiet

CALL = $(INSPECTOR) --method tools/call --tool-name

.PHONY: tools memory memory-gap

tools:
	$(INSPECTOR) --method tools/list

memory:
	$(CALL) fetch_info_memory_snapshot

memory-gap:
	$(CALL) explain_memory_gap $(if $(strip $(INTERVAL_MS)),--tool-arg interval_ms=$(INTERVAL_MS),)
