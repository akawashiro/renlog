#!/bin/bash

set -eux -o pipefail

script_path=$(dirname "$(readlink -f "$0")")
renlog_path="$script_path/../target/debug/renlog"
renlog_bash_rc=$(mktemp -t renlog-bash-rc.XXXXXX)
renlog_dir=$(mktemp -d -t renlog-dir.XXXXXX)
session_name="renlog-first-command-test-"$(date +%s%N)

${renlog_path} show-bash-rc > "${renlog_bash_rc}"

# Start tmux session and run commands
tmux new -s ${session_name} -d
tmux send-keys -t ${session_name} "${renlog_path} log --renlog-dir ${renlog_dir} --cmd 'bash --norc --noprofile'" C-m
tmux send-keys -t ${session_name} "source ${renlog_bash_rc}" C-m
# Wait for shell to be ready
sleep 1
# Send the first command - this is the one that was previously not logged
tmux send-keys -t ${session_name} "echo 'FirstCommand'" C-m
sleep 2
# Send a second command to verify normal operation continues
tmux send-keys -t ${session_name} "echo 'SecondCommand'" C-m
sleep 2
tmux kill-session -t ${session_name}

# Check that the first command was logged
first_log=$(find ${renlog_dir} -name "*FirstCommand*.log" | head -n 1)
if [[ ! -f "${first_log}" ]]; then
    echo "FAIL: First command log not found in ${renlog_dir}"
    ls -la ${renlog_dir}
    exit 1
fi

if ! grep -q "FirstCommand" "${first_log}"; then
    echo "FAIL: Expected text 'FirstCommand' not found in ${first_log}"
    cat "${first_log}"
    exit 1
fi

echo "SUCCESS: First command was logged correctly"

# Check that the second command was also logged
second_log=$(find ${renlog_dir} -name "*SecondCommand*.log" | head -n 1)
if [[ ! -f "${second_log}" ]]; then
    echo "FAIL: Second command log not found in ${renlog_dir}"
    ls -la ${renlog_dir}
    exit 1
fi

if ! grep -q "SecondCommand" "${second_log}"; then
    echo "FAIL: Expected text 'SecondCommand' not found in ${second_log}"
    cat "${second_log}"
    exit 1
fi

echo "SUCCESS: Second command was also logged correctly"

rm -rf "${renlog_dir}"
rm -f "${renlog_bash_rc}"
