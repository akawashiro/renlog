#! /bin/bash

set -eux -o pipefail

script_path=$(dirname "$(readlink -f "$0")")
renlog_path="$script_path/../target/debug/renlog"
renlog_zsh_rc=$(mktemp -t renlog-zsh-rc.XXXXXX)
renlog_dir=$(mktemp -d -t renlog-dir.XXXXXX)
session_name="renlog-log-filter-test-"$(date +%s%N)

${renlog_path} show-zsh-rc > "${renlog_zsh_rc}"

echo "Test 1: grep filter - should only keep lines with KEEP_THIS"
tmux new -s ${session_name} -d
tmux send-keys -t ${session_name} "${renlog_path} log --renlog-dir ${renlog_dir} --cmd 'zsh --no-globalrcs --no-rcs' --log-filter-cmd 'grep KEEP_THIS'" C-m
tmux send-keys -t ${session_name} "source ${renlog_zsh_rc}" C-m
tmux send-keys -t ${session_name} "echo 'KEEP_THIS line 1' && echo 'REMOVE_THIS line' && echo 'KEEP_THIS line 2'" C-m
sleep 3
tmux kill-session -t ${session_name}

renlog_latest_log=$(find ${renlog_dir} -name latest.log | head -1)
if [[ ! -f "${renlog_latest_log}" ]]; then
    echo "Log file not found: ${renlog_latest_log}"
    exit 1
fi

# Check that KEEP_THIS lines are present (excluding header lines starting with #)
if ! grep -v "^#" "${renlog_latest_log}" | grep -q "KEEP_THIS line 1"; then
    echo "Expected 'KEEP_THIS line 1' not found in ${renlog_latest_log}"
    cat "${renlog_latest_log}"
    exit 1
fi

if ! grep -v "^#" "${renlog_latest_log}" | grep -q "KEEP_THIS line 2"; then
    echo "Expected 'KEEP_THIS line 2' not found in ${renlog_latest_log}"
    cat "${renlog_latest_log}"
    exit 1
fi

# Check that REMOVE_THIS line is NOT present (excluding header lines)
if grep -v "^#" "${renlog_latest_log}" | grep -q "REMOVE_THIS line"; then
    echo "Unexpected 'REMOVE_THIS line' found in ${renlog_latest_log}"
    cat "${renlog_latest_log}"
    exit 1
fi

echo "Test 1 passed: grep filter works correctly"

# Clean up for test 2
rm -rf "${renlog_dir}"
renlog_dir=$(mktemp -d -t renlog-dir.XXXXXX)
session_name="renlog-log-filter-test-"$(date +%s%N)

echo "Test 2: sed filter - should replace text"
tmux new -s ${session_name} -d
tmux send-keys -t ${session_name} "${renlog_path} log --renlog-dir ${renlog_dir} --cmd 'zsh --no-globalrcs --no-rcs' --log-filter-cmd 'sed s/REPLACE_ME/REPLACED/g'" C-m
tmux send-keys -t ${session_name} "source ${renlog_zsh_rc}" C-m
tmux send-keys -t ${session_name} "echo 'REPLACE_ME first' && echo 'REPLACE_ME second'" C-m
sleep 3
tmux kill-session -t ${session_name}

renlog_latest_log=$(find ${renlog_dir} -name latest.log | head -1)
if [[ ! -f "${renlog_latest_log}" ]]; then
    echo "Log file not found: ${renlog_latest_log}"
    exit 1
fi

# Check that text was replaced (excluding header lines)
if ! grep -v "^#" "${renlog_latest_log}" | grep -q "REPLACED first"; then
    echo "Expected 'REPLACED first' not found in ${renlog_latest_log}"
    cat "${renlog_latest_log}"
    exit 1
fi

if ! grep -v "^#" "${renlog_latest_log}" | grep -q "REPLACED second"; then
    echo "Expected 'REPLACED second' not found in ${renlog_latest_log}"
    cat "${renlog_latest_log}"
    exit 1
fi

# Check that original text is NOT present (excluding header lines)
if grep -v "^#" "${renlog_latest_log}" | grep -q "^REPLACE_ME"; then
    echo "Unexpected 'REPLACE_ME' found in ${renlog_latest_log} (should have been replaced)"
    cat "${renlog_latest_log}"
    exit 1
fi

echo "Test 2 passed: sed filter works correctly"

# Clean up
rm -rf "${renlog_dir}"
rm -f "${renlog_zsh_rc}"

echo "All log_filter_cmd tests passed!"
