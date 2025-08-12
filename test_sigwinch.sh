#!/bin/bash

# Test script for SIGWINCH handling in renlog

echo "Building renlog..."
cargo build --release

if [ $? -ne 0 ]; then
    echo "Build failed"
    exit 1
fi

# Create a test log directory
TEST_DIR="/tmp/renlog_sigwinch_test"
mkdir -p "$TEST_DIR"

echo ""
echo "Starting renlog session with logging to $TEST_DIR"
echo "Test Instructions:"
echo "1. After renlog starts, type 'stty size' to see the current terminal size"
echo "2. Resize your terminal window"
echo "3. Type 'stty size' again to verify the size changed"
echo "4. Check the log file for SIGWINCH messages"
echo "5. Type 'exit' to quit"
echo ""
echo "Press Enter to start..."
read

# Run renlog with debug logging
RUST_LOG=debug ./target/release/renlog log --renlog-dir "$TEST_DIR"

echo ""
echo "Session ended. Checking logs for SIGWINCH handling..."
echo ""

# Check the latest log file for SIGWINCH-related messages
LOG_FILE=$(ls -t "$TEST_DIR"/renlog_*.log | head -n1)

if [ -z "$LOG_FILE" ]; then
    echo "No log file found"
    exit 1
fi

echo "Checking log file: $LOG_FILE"
echo "SIGWINCH-related log entries:"
echo "================================"
grep -i "sigwinch\|terminal size\|window size" "$LOG_FILE" || echo "No SIGWINCH messages found"
echo "================================"