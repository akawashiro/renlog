/// Command names used in shell integration
pub const START_COMMAND_NAME: &str = "renlog-start-command";
pub const END_COMMAND_NAME: &str = "renlog-end-command";

pub enum ShellType {
    Bash,
    Zsh,
}

pub fn show_shell_rc(shell: ShellType) {
    match shell {
        ShellType::Bash => show_bash_rc(),
        ShellType::Zsh => show_zsh_rc(),
    }
}

pub fn show_bash_rc() {
    println!(
        r#"
#==============================================================================
# Renlog Bash setup script
#
# Add the following line to your $HOME/.bashrc file:
#
# eval "$(renlog show-bash-rc)"
#==============================================================================

#==============================================================================
# Basic helper functions
#==============================================================================

# Execute this function before each command to tell renlog to start logging
function renlog_bash_preexec() {{
    local cmd="${{BASH_COMMAND}}"
    # Don't log PROMPT_COMMAND itself
    if [[ "$cmd" != "$PROMPT_COMMAND" ]] && [[ -n "$RENLOG_PID" ]]; then
        # Send command marker to renlog via invisible stdout with better terminal handling
        printf '{START_COMMAND_NAME} %s\n' "$cmd"
        # Use more reliable cursor manipulation with carriage return + clear line
        printf '\r\e[K'
        # Additional fallback: move cursor up and clear line if previous method insufficient
        printf '\e[1A\e[2K'
    fi
}}

# Execute this function after each command to tell renlog to stop logging
function renlog_bash_postexec() {{
    local last_status=$?
    if [[ -n "$RENLOG_PID" ]]; then
        # Send end command marker to renlog via invisible stdout with better terminal handling
        printf '{END_COMMAND_NAME} %d\n' "$last_status"
        # Use more reliable cursor manipulation with carriage return + clear line
        printf '\r\e[K'
        # Additional fallback: move cursor up and clear line if previous method insufficient
        printf '\e[1A\e[2K'
        
        # Display current log file if available
        if [[ -n "$RENLOG_LOG_FILE" && -f "$RENLOG_LOG_FILE" ]]; then
            local current_log="$(cat "$RENLOG_LOG_FILE" 2>/dev/null)"
            if [[ -n "$current_log" ]]; then
                echo "[$current_log]"
            fi
        fi
    fi
    # Return the original exit status to preserve it
    return $last_status
}}

#==============================================================================
# Set up pre/post exec hooks if we're running inside renlog
#==============================================================================

if [[ -n "$RENLOG_PID" ]]; then
    # Set up DEBUG trap for preexec functionality
    # The DEBUG trap is executed before every simple command
    trap 'renlog_bash_preexec' DEBUG
    
    # Set up PROMPT_COMMAND for postexec functionality
    # PROMPT_COMMAND is executed before displaying the prompt
    if [[ -z "$PROMPT_COMMAND" ]]; then
        PROMPT_COMMAND='renlog_bash_postexec'
    else
        # Append to existing PROMPT_COMMAND
        PROMPT_COMMAND="renlog_bash_postexec; $PROMPT_COMMAND"
    fi
    
    echo "Renlog bash integration loaded (PID: $RENLOG_PID)"
fi

"#
    );
}

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
        # Send command marker to renlog via invisible stdout with better terminal handling
        printf '{START_COMMAND_NAME} %s\n' "$cmd"
        # Use more reliable cursor manipulation with carriage return + clear line
        printf '\r\e[K'
        # Additional fallback: move cursor up and clear line if previous method insufficient
        printf '\e[1A\e[2K'
    fi
}}

# Execute this command in Zsh's precmd_functions to tell renlog to stop logging for the current command.
function renlog_zsh_postexec() {{
    local last_status=$?
    if [[ -n "$RENLOG_PID" ]]; then
        # Send end command marker to renlog via invisible stdout with better terminal handling
        printf '{END_COMMAND_NAME} %d\n' "$last_status"
        # Use more reliable cursor manipulation with carriage return + clear line
        printf '\r\e[K'
        # Additional fallback: move cursor up and clear line if previous method insufficient
        printf '\e[1A\e[2K'
        
        # Display current log file if available
        if [[ -n "$RENLOG_LOG_FILE" && -f "$RENLOG_LOG_FILE" ]]; then
            local current_log="$(cat "$RENLOG_LOG_FILE" 2>/dev/null)"
            if [[ -n "$current_log" ]]; then
                echo "[$current_log]"
            fi
        fi
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

"#
    );
}
