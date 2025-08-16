# Renlog - Rust Implementation of Zenlog

Renlog is a Rust re-implementation of [zenlog](https://github.com/omakoto/zenlog).

## Architecture

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
    SH -->|SIGCHLD/SIGHUP/SIGUSR2| MP
    
    style MP fill:#e1f5fe
    style SHELL fill:#fff3e0
    style PM fill:#f3e5f5
    style PS fill:#f3e5f5
    style CL fill:#e8f5e9
    style SH fill:#fce4ec
    style ST fill:#fce4ec
    style OT fill:#fce4ec
```

### Data Flow Description

1. **User Input Path**: User input from stdin is forwarded by a dedicated std::thread to the PTY master, which passes it to the PTY slave connected to the shell process.

2. **Shell Output Path**: The shell's stdout/stderr outputs to the PTY slave, which is read by the PTY master. A unified output reader thread (std::thread) then:
   - Displays the output to the user's stdout
   - Detects command boundaries using special markers
   - Logs command output to separate files

3. **Signal Handling**: A dedicated std::thread using signal-hook handles signals synchronously:
   - SIGWINCH for terminal window resize events (forwarded to PTY master)
   - SIGCHLD for child process termination (handled by main process)
   - SIGHUP for session hangup (handled by main process)
   - SIGUSR2 for debug information (handled by main process)
   - SIGINT/SIGTERM for interrupt/termination (forwarded to child process)

4. **Signal Forwarding**: When users press Ctrl+C or send termination signals, the signal handler forwards SIGINT/SIGTERM to the child process, enabling proper termination of interactive applications like `top`, `htop`, `vim`, etc.

5. **Command Logging**: The output reader detects START_COMMAND and END_COMMAND markers in the stream, buffering and saving each command's output to individual log files.

## Zenlog vs Renlog Comparison

Renlog is a Rust reimplementation of [zenlog](https://github.com/omakoto/zenlog), designed to be simpler, more focused, and easier to understand while maintaining the core functionality of automatic command logging.

### Architecture Comparison

#### Zenlog (Go Implementation)
```mermaid
graph TB
    subgraph "Zenlog (Go)"
        ZMP[Main Process]
        ZGR1[Goroutine: Input Forward]
        ZGR2[Goroutine: Output Tee]
        ZGR3[Goroutine: Log Processor]
    end
    
    subgraph "Zenlog PTY"
        ZPM[PTY Master]
        ZPS[PTY Slave]
    end
    
    subgraph "Zenlog Child"
        ZSHELL[Shell Process]
    end
    
    subgraph "Zenlog FIFO"
        ZFF[Forward FIFO]
        ZRF[Reverse FIFO]
    end
    
    subgraph "Zenlog Logs"
        ZSAN[SAN/ Directory]
        ZRAW[RAW/ Directory] 
        ZENV[ENV/ Directory]
        ZCMD[cmds/ Directory]
        ZPID[pids/ Directory]
        ZTAG[tags/ Directory]
    end
    
    ZGR1 -->|stdin| ZPM
    ZPM <--> ZPS
    ZPS <--> ZSHELL
    ZPM -->|output| ZGR2
    ZGR2 -->|tee| ZFF
    ZFF --> ZGR3
    ZGR3 --> ZSAN
    ZGR3 --> ZRAW
    ZGR3 --> ZENV
    ZGR3 --> ZCMD
    ZGR3 --> ZPID
    ZGR3 --> ZTAG
    
    style ZMP fill:#fff2cc
    style ZSHELL fill:#d5e8d4
    style ZFF fill:#f8cecc
    style ZSAN fill:#e1d5e7
```

#### Renlog (Rust Implementation)
```mermaid
graph TB
    subgraph "Renlog (Rust)"
        RMP[Main Thread]
        RSH[Signal Handler Thread]
        RST[Stdin Forwarder Thread]
        ROT[Output Reader Thread]
    end
    
    subgraph "Renlog PTY"
        RPM[PTY Master]
        RPS[PTY Slave]
    end
    
    subgraph "Renlog Child"
        RSHELL[Shell Process]
    end
    
    subgraph "Renlog Logs"
        RLF[Log Files<br/>renlog_*.log]
    end
    
    RST -->|stdin| RPM
    RPM <--> RPS
    RPS <--> RSHELL
    RPM -->|output| ROT
    ROT -->|display| STDOUT
    ROT -->|detect commands| RLF
    RSH -->|SIGWINCH| RPM
    RSH -->|SIGINT/SIGTERM| RSHELL
    RSH -->|SIGCHLD/SIGHUP/SIGUSR2| RMP
    
    style RMP fill:#e1f5fe
    style RSHELL fill:#fff3e0
    style RSH fill:#fce4ec
    style RLF fill:#e8f5e9
```

### Feature Comparison

| Feature | Zenlog (Go) | Renlog (Rust) |
|---------|-------------|---------------|
| **Language** | Go | Rust |
| **Concurrency Model** | Goroutines | std::thread |
| **PTY Management** | creack/pty library | nix crate |
| **Signal Handling** | Basic | Enhanced (SIGINT/SIGTERM forwarding) |
| **FIFO Pipes** | ✅ Used for communication | ❌ Removed (unused) |
| **Log Structure** | Complex (SAN/RAW/ENV/cmds/pids/tags) | Simple (individual command logs) |
| **Command Detection** | Shell hooks + markers | START/END markers |
| **Interactive Apps** | Limited Ctrl+C support | ✅ Full Ctrl+C support |
| **Memory Safety** | Garbage collected | Zero-cost abstractions |
| **Binary Size** | Larger (Go runtime) | Smaller (static linking) |
| **Startup Time** | Slower (GC initialization) | Faster (compiled binary) |
| **Dependencies** | Many Go modules | Minimal Rust crates |
| **Configuration** | TOML file | Command line flags |
| **Log Organization** | Per-command/PID/tag directories | Flat structure with timestamps |
| **Shell Integration** | Complex setup scripts | Simple configuration |

### Key Differences

#### **Simplicity vs Features**
- **Zenlog** provides a comprehensive logging ecosystem with complex directory structures, tagging, and multiple output formats
- **Renlog** focuses on core functionality with a simpler, more maintainable design

#### **Performance**
- **Zenlog** uses Go's garbage collector and goroutines, which can introduce latency
- **Renlog** uses Rust's zero-cost abstractions and manual memory management for predictable performance

#### **Signal Handling**
- **Zenlog** has basic signal handling that may not work well with interactive applications
- **Renlog** implements comprehensive signal forwarding, making Ctrl+C work properly with `top`, `htop`, `vim`, etc.

#### **Architecture Philosophy**
- **Zenlog** uses FIFO pipes and complex async communication patterns
- **Renlog** uses a simpler thread-based approach with direct PTY communication

#### **When to Choose What**

**Choose Zenlog if you need**:
- Complex log organization with per-command/PID/tag directories
- Advanced shell integration features
- Mature ecosystem with extensive tooling

**Choose Renlog if you want**:
- Simpler, more maintainable codebase
- Better performance and lower resource usage
- Proper signal handling for interactive applications
- Modern Rust safety guarantees
- Easier debugging and modification
