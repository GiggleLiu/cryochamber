# Windows Testing Guide

Quick guide for testing cryochamber on Windows without admin privileges.

## Setup

```bash
# Build the project
cargo build

# Create test directory
cd /tmp && mkdir test-cryo && cd test-cryo

# Initialize project
cargo run --manifest-path /path/to/cryochamber/Cargo.toml --bin cryo -- init
```

## Running Without Admin

```bash
# Start daemon (no service, no admin required)
CRYO_NO_SERVICE=1 cargo run --manifest-path /path/to/cryochamber/Cargo.toml --bin cryo -- start

# Check status
cargo run --manifest-path /path/to/cryochamber/Cargo.toml --bin cryo -- status
```

## Testing Message Flow

```bash
# Create message (any text extension works: .md, .txt, .text)
echo "Test message" > messages/inbox/test.txt

# Wake daemon to process immediately
cargo run --manifest-path /path/to/cryochamber/Cargo.toml --bin cryo -- wake

# Check logs
tail -20 cryo.log
```

## Cleanup

```bash
# Stop daemon
cargo run --manifest-path /path/to/cryochamber/Cargo.toml --bin cryo -- cancel
```

## Common Issues

- **Build fails with "Access Denied"**: Stop the daemon first
- **Wake not working**: Ensure you're using the fixed version that always sends wake signals
- **Messages not read**: Check file extension is .md, .txt, or .text
