# Renlog - Rust Implementation of Zenlog

Renlog is a Rust re-implementation of [zenlog](https://github.com/omakoto/zenlog), a terminal session logger that automatically saves all command output to organized log files.

## Zenlog Architecture Analysis

Based on research of the zenlog codebase, this document outlines the core architecture and functionality that needs to be implemented in Rust.

### Core Components

#### 1. Main Entry Point (`zenlog/cmd/zenlog/main.go`)
- Handles subcommand routing
- Starts the logger session when no subcommand is provided
- Manages process restart functionality
- Sets GOMAXPROCS to 1 for performance

#### 2. Logger Session Management (`zenlog/starter.go`)
- **StartZenlog**: Main function that initializes a zenlog session
- Creates a logger instance and starts the child shell
- Sets up signal handlers for:
  - `SIGCHLD`: Child process termination
  - `SIGWINCH`: Terminal window resize
  - `SIGHUP`: Session hangup
  - `SIGUSR2`: Stack trace dumping for debugging
- Implements emergency shell fallback on critical failures
- Returns exit code and resurrection status

#### 3. PTY-Based Logger (`zenlog/logger/logger.go`)
- **Core Structure**:
  - Creates a pseudo-terminal (PTY) master/slave pair
  - Uses named pipes (FIFOs) for bidirectional communication
  - Maintains sanitizer for escape sequence removal
- **Key Components**:
  - `ForwardPipe`: Commands from shell to logger
  - `ReversePipe`: Responses from logger to shell
  - Terminal state management with raw mode
  - Environment variable export for child processes

#### 4. I/O Forwarding (`zenlog/logger/tee.go`)
- Implements tee functionality to duplicate output
- Two variants:
  - `forwardSimple`: Direct copy from input to output
  - `teeSimple`: Duplicates to both stdout and log pipe
- Linux-specific optimization using `splice()` system call
- Buffer size: 32KB for efficient I/O

#### 5. Configuration System (`zenlog/config/config.go`)
- **Configuration File**: `~/.zenlog.toml`
- **Key Settings**:
  - `ZENLOG_DIR`: Log directory (default: `$HOME/zenlog/`)
  - `ZENLOG_START_COMMAND`: Shell startup command
  - `ZENLOG_PREFIX_COMMANDS`: Ignored command prefixes (sudo, time, etc.)
  - `ZENLOG_ALWAYS_NO_LOG_COMMANDS`: Commands to never log (vim, emacs, etc.)
  - `ZENLOG_AUTO_FLUSH`: Automatic log flushing
  - `ZENLOG_USE_EXPERIMENTAL_COMMAND_PARSER`: Advanced parsing mode
  - `ZENLOG_USE_SPLICE`: Linux splice optimization
- Environment variable overrides for all settings

#### 6. Log File Generation (`zenlog/logfiles/loggenerator.go`)
- **Directory Structure**:
  ```
  $ZENLOG_DIR/
  ├── SAN/       # Sanitized logs (escape sequences removed)
  ├── RAW/       # Raw output with all escape sequences
  ├── ENV/       # Environment metadata
  ├── cmds/      # Per-command logs
  │   └── <command>/
  │       ├── SAN/
  │       ├── RAW/
  │       └── ENV/
  ├── pids/      # Per-session logs
  ├── tags/      # Tagged command logs
  ├── S, SS, ... # Symlinks to recent sanitized logs
  ├── R, RR, ... # Symlinks to recent raw logs
  └── E, EE, ... # Symlinks to recent env logs
  ```
- Creates up to 10 previous log symlinks (S, SS, SSS, etc.)
- Automatic directory creation with 0700 permissions
- Symlink management for TODAY and THISMONTH

#### 7. Command Parsing (`zenlog/logfiles/commandparser.go`)
- **Command Structure**:
  - Extracts executable names from command line
  - Identifies comments (text after #)
  - Detects no-log markers (184 prefix)
  - Detects force-log markers (186 prefix)
- **Two Parsing Modes**:
  - Regex-based (configurable patterns)
  - Shell parser-based (more accurate)
- Default splitters for commands and comments
- Handles complex command lines with pipes, redirects, etc.

#### 8. Built-in Commands (`zenlog/builtins/`)
- Session management (start, stop, restart)
- History browsing and searching
- Log file access and viewing
- Environment detection (InZenlog check)
- Direct logger writing
- Outer TTY access

#### 9. Shell Integration
- **Bash Support** (requires 4.4+):
  - Uses PS0 for pre-exec hook
  - PROMPT_COMMAND for post-exec hook
  - Hotkey bindings (ALT+1, ALT+2, etc.)
- **Zsh Support**:
  - preexec and precmd hooks
  - Similar hotkey bindings
- **Protocol Communication**:
  - Start/end command markers
  - Command metadata transmission
  - Flush requests

### Key Technologies Used in Go Implementation

1. **PTY Handling**: `github.com/creack/pty`
2. **Terminal Control**: `github.com/pkg/term`
3. **TOML Configuration**: `github.com/BurntSushi/toml`
4. **Terminal Detection**: `github.com/mattn/go-isatty`
5. **System Calls**: Direct syscall usage for FIFOs, splice, etc.

## Rust Implementation Plan

### Required Crates

### Implementation Phases

#### Phase 1: Core Logger
- [ ] Basic PTY creation and management
- [ ] Child process spawning
- [ ] Simple I/O forwarding
- [ ] FIFO pipe creation for communication

#### Phase 1.5: Signal Handling
- [x] Signal handling framework (SIGCHLD, SIGWINCH, SIGHUP, SIGUSR2)
- [x] Terminal resize propagation
- [x] Child process cleanup
- [x] Emergency shell fallback

#### Phase 2: Log File System
- [ ] Directory structure creation
- [ ] Log file writing (RAW, SAN, ENV)
- [ ] Symlink management
- [ ] Basic command parsing

#### Phase 3: Configuration
- [ ] TOML configuration loading
- [ ] Environment variable overrides
- [ ] Default configuration generation

#### Phase 4: Shell Integration
- [ ] Bash integration scripts
- [ ] Zsh integration scripts
- [ ] Protocol communication via FIFOs

#### Phase 5: Built-in Commands
- [ ] Session management commands
- [ ] History and log access commands
- [ ] Utility commands

#### Phase 6: Advanced Features
- [ ] Command tagging
- [ ] Log rotation and cleanup
- [ ] Performance optimizations (splice on Linux)
- [ ] Emergency shell fallback

### Key Differences from Go Implementation

1. **Async I/O**: Use Tokio for efficient async I/O and signal handling
2. **Error Handling**: Leverage Rust's Result type system
3. **Memory Safety**: No manual memory management needed
4. **Platform Support**: Initial focus on Linux, with potential for macOS support

### Testing Strategy

1. **Unit Tests**: For individual components (parser, config, etc.)
2. **Integration Tests**: For PTY handling and session management
3. **E2E Tests**: Port zenlog's existing test suite
4. **Performance Tests**: Ensure comparable performance to Go version

## Development Status

- [x] Architecture analysis complete
- [ ] Basic Rust project setup
- [ ] Core logger implementation
- [ ] Configuration system
- [ ] Shell integration
- [ ] Built-in commands
- [ ] Testing suite

## License

This project maintains the same license as the original zenlog project.
