#!/bin/sh
# Mock agent: exits quickly with code 0 but never calls hibernate.
# Tests: quick-exit detection path (possible API key issue).
echo "Quick exit without hibernate"
exit 0
