#! /bin/bash

set -eux -o pipefail

script_path=$(dirname "$(readlink -f "$0")")
renlog_path="$script_path/../target/debug/renlog"
session_name=$(basename $(readlink -f "$0") .sh)
renlog_dir=/tmp/${session_name}-dir
renlog_zsh_rc=${renlog_dir}/zshrc
mkdir -p "${renlog_dir}"

${renlog_path} show-zsh-rc > "${renlog_zsh_rc}"

tmux new -s ${session_name} -d
tmux send-keys -t ${session_name} "${renlog_path} log --renlog-dir ${renlog_dir} --cmd 'zsh --no-globalrcs --no-rcs'" C-m
tmux send-keys -t ${session_name} "source ${renlog_zsh_rc}" C-m

echo "renlog dir: ${renlog_dir}"
read -p "Press enter to stop the session and clean up..."

tmux kill-session -t ${session_name}
