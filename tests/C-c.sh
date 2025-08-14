#! /bin/bash

set -ux -o pipefail

script_path=$(dirname "$(readlink -f "$0")")
renlog_path="$script_path/../target/debug/renlog"
renlog_zsh_rc=$(mktemp -t renlog-zsh-rc.XXXXXX)
renlog_dir=$(mktemp -d -t renlog-dir.XXXXXX)
session_name="renlog-C-c-"$(date +%s%N)

${renlog_path} show-zsh-rc > "${renlog_zsh_rc}"

tmux new -s ${session_name} -d
tmux send-keys -t ${session_name} "${renlog_path} log --renlog-dir ${renlog_dir} --cmd 'zsh --no-globalrcs --no-rcs'" C-m
tmux send-keys -t ${session_name} "source ${renlog_zsh_rc}" C-m
tmux send-keys -t ${session_name} "watch ls ${renlog_dir}" C-m
sleep 1
tmux send-keys -t ${session_name} C-c
sleep 1
is_watch_ls_running=$(ps aux | grep -v grep | grep "watch ls ${renlog_dir}" | wc -l)

if [ "${is_watch_ls_running}" -ne 0 ]; then
    echo "C-c did not stop the watch command."
    exit 1
else
    echo "C-c successfully stopped the watch command."
fi

tmux kill-session -t ${session_name}
rm -rf "${renlog_dir}"
rm -f "${renlog_zsh_rc}"
