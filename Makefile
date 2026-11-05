METRICS_URL ?= http://127.0.0.1:6379/metrics
INTERVAL_MS ?= 5000
HEAP ?= data

INSPECTOR = npx @modelcontextprotocol/inspector --cli \
	-e WYRM_METRICS_URL=$(METRICS_URL) cargo run --quiet

CALL = $(INSPECTOR) --method tools/call --tool-name

.PHONY: tools memory connections sample-connections arena defrag

tools:
	$(INSPECTOR) --method tools/list

memory:
	$(CALL) fetch_info_memory_snapshot

connections:
	$(CALL) fetch_connection_snapshot

sample-connections:
	$(CALL) sample_connection_memory --tool-arg interval_ms=$(INTERVAL_MS)

arena:
	$(CALL) fetch_arena_snapshot --tool-arg heap=$(HEAP)

defrag:
	$(CALL) fetch_defrag_snapshot
