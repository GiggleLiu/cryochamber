#!/usr/bin/env python3
"""Simple demo agent for Cryochamber book club"""
import sys
import os

# Fix Windows encoding
sys.stdout.reconfigure(encoding='utf-8')

print("=== Book Club Assistant ===")
print()

# Read plan
with open("plan.md", "r", encoding="utf-8") as f:
    plan = f.read()
    print("Reading plan.md...")

print()
print("Current task: Arrange next book club meeting")
print()
print("Action: Would send message to collect availability")
print("(In real scenario, would use Zulip/GitHub/etc)")
print()
print("Next: Hibernate for 8 hours, then check responses")
print()
print("Next: Hibernate for 8 hours")
print()

# Set next wake time (8 hours from now)
os.system('cryo-agent time "+8 hours"')
# Then hibernate
os.system('cryo-agent hibernate')
