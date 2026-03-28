# renlog - Rust Implementation of Zenlog

renlog is a Rust re-implementation of [Zenlog](https://github.com/omakoto/zenlog).

## Install

```bash
git clone git@github.com:akawashiro/renlog.git
cd renlog
cargo install --path .
```

## Shell Configuration

### Bash Configuration

```bash
if which renlog > /dev/null 2>&1; then
    if [[ "$(ps -o comm= -p $PPID)" != "renlog" ]]; then
        renlog_dir=/tmp/renlog
        exec renlog --log-level info log --renlog-dir ${renlog_dir} --cmd 'bash -l'
    else
        eval "$(renlog show-bash-rc)"
    fi
fi
```

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
