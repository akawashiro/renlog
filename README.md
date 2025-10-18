# renlog - Rust Implementation of Zenlog

renlog is a Rust re-implementation of [Zenlog](https://github.com/omakoto/zenlog).

## Features

- **Command Logging**: Automatically logs all shell commands and their output to separate files
- **Password Masking**: Automatically masks passwords and sensitive credentials in log files for security
- **Shell Integration**: Works seamlessly with Bash and Zsh
- **Terminal Support**: Full PTY support with proper signal handling

### Password Masking

renlog automatically masks passwords and sensitive credentials in saved log files, making it safe to use with commands that contain:

- Command line flags: `-p password`, `--password=secret`
- API keys and tokens: `--api-key=value`, `--token=value`
- Connection strings: `mysql://user:pass@host`, `postgresql://user:pass@host`
- HTTP(S) URLs with credentials: `https://user:pass@host`
- Environment variables: `PASSWORD=secret`, `API_KEY=secret`
- Tool-specific formats: `mysql -pPassword`, `curl --user user:pass`

Example:
```bash
# Command entered:
mysql -u root -psecret123 -h localhost

# Saved in log:
mysql -u root -p*** -h localhost
```

## Install

```bash
git clone git@github.com:akawashiro/renlog.git
cd renlog
cargo install --path .
```

## Shell Configuration

### Zsh Configuration

Add the following to your `~/.zshrc` to automatically start renlog when opening a new shell:

```zsh
if which renlog > /dev/null 2>&1; then
    if [[ "$(ps -o comm= -p $PPID)" != "renlog" ]]; then
        renlog_dir=/tmp/renlog
        exec renlog --log-level info log --renlog-dir ${renlog_dir} --cmd 'zsh -l'
    else
        eval "$(renlog show-zsh-rc)"
    fi
fi
```
