use crate::command_markers::{END_COMMAND_NAME, START_COMMAND_NAME};

pub fn show_zsh_rc() {
    println!(
        r#"
#==============================================================================
# Renlog Zsh setup script
#
# Add the following line to your $HOME/.zshrc file:
#
# eval "$(renlog show-zsh-rc)"
#==============================================================================

#==============================================================================
# Basic helper functions
#==============================================================================

# Execute this command in Zsh's preexec_functions to tell renlog to start logging for a command.
function renlog_zsh_preexec() {{
    local cmd="$3" # Current command line.
    if [[ -n "$RENLOG_PID" ]]; then
        # Send command marker to renlog via invisible stdout
        printf '{} %s\n' "$cmd"
        # Move cursor up and clear the line to make it invisible
        echo -ne "\e[A\e[2K"
    fi
}}

# Execute this command in Zsh's precmd_functions to tell renlog to stop logging for the current command.
function renlog_zsh_postexec() {{
    local last_status=$?
    if [[ -n "$RENLOG_PID" ]]; then
        # Send end command marker to renlog via invisible stdout
        printf '{} %d\n' "$last_status"
        # Move cursor up and clear the line to make it invisible
        echo -ne "\e[A\e[2K"
    fi
}}

#==============================================================================
# Set up pre/post exec hooks if we're running inside renlog
#==============================================================================

if [[ -n "$RENLOG_PID" ]]; then
    # By default, zsh always prints a percent followed by a bunch of spaces
    # before prompt, which will be picked up by renlog.
    # Disable it.
    # https://unix.stackexchange.com/questions/167582/
    unsetopt prompt_cr prompt_sp

    precmd_functions=(renlog_zsh_postexec "$precmd_functions[@]")
    preexec_functions+=(renlog_zsh_preexec)
    
    echo "Renlog zsh integration loaded (PID: $RENLOG_PID)"
fi

"#,
        START_COMMAND_NAME, END_COMMAND_NAME
    );
}
