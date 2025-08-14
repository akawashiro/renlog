#! /bin/bash

set -eux -o pipefail

script_path=$(dirname "$(readlink -f "$0")")
renlog_path="$script_path/../target/debug/renlog"
renlog_zsh_rc=$(mktemp -t renlog-zsh-rc.XXXXXX)
renlog_dir=$(mktemp -d -t renlog-dir.XXXXXX)
session_name="renlog-echo-test-"$(date +%s%N)

${renlog_path} show-zsh-rc > "${renlog_zsh_rc}"

tmux new -s ${session_name} -d
tmux send-keys -t ${session_name} "${renlog_path} log --renlog-dir ${renlog_dir} --cmd 'zsh --no-globalrcs --no-rcs'" C-m
tmux send-keys -t ${session_name} "source ${renlog_zsh_rc}" C-m
tmux send-keys -t ${session_name} "echo 'Hello'" C-m
sleep 3
tmux kill-session -t ${session_name}

renlog_latest_log=$(realpath ${renlog_dir}/latest.log)
if [[ ! -f "${renlog_latest_log}" ]]; then
    echo "Log file not found: ${renlog_latest_log}"
    exit 1
fi
if ! grep -q "Hello" "${renlog_latest_log}"; then
    echo "Expected log entry not found in ${renlog_latest_log}"
    exit 1
fi

rm -rf "${renlog_dir}"
rm -f "${renlog_zsh_rc}"
