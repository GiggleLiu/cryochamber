#!/bin/sh
# Mock agent: crashes immediately without calling hibernate.
# Tests: retry logic, backoff, provider rotation.
echo "Agent crashing..."
exit 1
