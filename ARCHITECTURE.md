# Architecture

```mermaid
graph TB
    subgraph "Main Process (Synchronous)"
        MP[Main Thread]
        SH[Signal Handler Thread<br/>std::thread]
        ST[Stdin Forwarder Thread<br/>std::thread]
        OT[Output Reader Thread<br/>std::thread]
    end
    
    subgraph "PTY (Pseudo-Terminal)"
        PM[PTY Master]
        PS[PTY Slave]
    end
    
    subgraph "Child Process"
        SHELL[Shell Process<br/>e.g., zsh, bash]
    end
    
    subgraph "User Terminal"
        STDIN[User stdin]
        STDOUT[User stdout]
        STDERR[User stderr]
    end
    
    subgraph "Command Logging"
        CL[Command Logger<br/>Detects START/END markers]
        LF[Log Files<br/>renlog_*.log]
    end
    
    %% User input flow
    STDIN -->|User Input| ST
    ST -->|Forwards| PM
    PM <-->|PTY Connection| PS
    
    %% Shell I/O
    PS -->|stdin| SHELL
    SHELL -->|stdout/stderr| PS
    
    %% Output flow
    PM -->|Output Data| OT
    OT -->|Display| STDOUT
    OT -->|Command Detection| CL
    CL -->|Save Commands| LF
    
    %% Signal handling
    SH -->|SIGWINCH| PM
    SH -->|SIGINT/SIGTERM| SHELL
    SH -->|SIGCHLD/SIGHUP| MP
    
    style MP fill:#e1f5fe
    style SHELL fill:#fff3e0
    style PM fill:#f3e5f5
    style PS fill:#f3e5f5
    style CL fill:#e8f5e9
    style SH fill:#fce4ec
    style ST fill:#fce4ec
    style OT fill:#fce4ec
```

## Data Flow Description

1. **User Input Path**: User input from stdin is forwarded by a dedicated std::thread to the PTY master, which passes it to the PTY slave connected to the shell process.

2. **Shell Output Path**: The shell's stdout/stderr outputs to the PTY slave, which is read by the PTY master. A unified output reader thread (std::thread) then:
   - Displays the output to the user's stdout
   - Detects command boundaries using special markers
   - Logs command output to separate files

3. **Signal Handling**: A dedicated std::thread using signal-hook handles signals synchronously:
   - SIGWINCH for terminal window resize events (forwarded to PTY master)
   - SIGCHLD for child process termination (handled by main process)
   - SIGHUP for session hangup (handled by main process)
   - SIGINT/SIGTERM for interrupt/termination (forwarded to child process)

4. **Signal Forwarding**: When users press Ctrl+C or send termination signals, the signal handler forwards SIGINT/SIGTERM to the child process, enabling proper termination of interactive applications like `top`, `htop`, `vim`, etc.

5. **Command Logging**: The output reader detects START_COMMAND and END_COMMAND markers in the stream, buffering and saving each command's output to individual log files.
