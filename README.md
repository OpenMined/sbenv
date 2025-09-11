# sbenv - SyftBox Environment Manager

A virtualenv-like tool for managing isolated SyftBox environments. Create, activate, and manage multiple SyftBox instances without conflicts.

## Installation

### Quick Install (via curl)

```bash
curl -sSL https://raw.githubusercontent.com/openmined/sbenv/main/install.sh | bash
```

### Install from crates.io

```bash
cargo install sbenv
```

### Build from Source

```bash
git clone https://github.com/openmined/sbenv
cd sbenv
cargo build --release
sudo cp target/release/sbenv /usr/local/bin/
```

## Quick Start

### 1. Install Shell Integration

First, install the shell helper utilities for your shell:

```bash
sbenv install-shell
```

Note: If your shell doesn't look like this with nice text on the right, then open a PR to fix it:
```
~/datasites/me@madhavajay.com                                    ✔  📦 me@madhavajay.com
```

This will detect your shell (bash/zsh) and add the necessary functions to your shell configuration file.

### 2. Create an Environment

```bash
sbenv init myproject
```

This creates a new SyftBox environment in the current directory.

### 3. Activate an Environment

```bash
sbenv activate
```

Or use the shell function (after installing shell integration):

```bash
sba
```

Your prompt will change to show the active environment:
```
~/datasites/me@madhavajay.com                                    ✔  📦 me@madhavajay.com
```

### 4. Start SyftBox

Once activated, start the SyftBox instance:

```bash
sbenv start
```

### 5. Deactivate

When done, deactivate the environment:

```bash
sbenv deactivate
```

Or use the shell function:

```bash
sbd
```

## Commands

### Environment Management

```bash
# Create a new environment
sbenv create <name>


# Remove an environment
sbenv remove <name>

# Show current environment info
sbenv info

# Show environment status
sbenv status
```

### Activation

```bash
# Activate an environment
sbenv activate <name>

# Deactivate current environment
sbenv deactivate
```

### SyftBox Control

```bash
# Start SyftBox in current environment
sbenv start [--skip-login-check]

# Stop SyftBox in current environment
sbenv stop

# View SyftBox logs
sbenv logs [--lines <n>] [--follow]
```


## Environment Structure

Each environment is isolated in `~/.sbenv/envs/<name>/` with:

```
apps
datasites
.syftbox <- config.json and logs go in here
```

## Tips

- Each environment runs its own SyftBox instance on a unique port
- Environments are completely isolated from each other
- Use `sbenv info` to see details about the current environment
- Use `sbenv status` to check if SyftBox is running

## Troubleshooting

### Shell functions not working

Make sure you've run `sbenv install-shell` and restarted your terminal or run:

```bash
source ~/.bashrc  # for bash
# or
source ~/.zshrc   # for zsh
```

### Port conflicts

Each environment automatically gets a unique port. If you have port issues, check:

```bash
sbenv status  # Shows port information
```

### SyftBox won't start

Check the logs for the current environment:

```bash
sbenv logs --lines 50
```

## Requirements

- Rust 1.70+ (for building from source)
- Bash or Zsh shell (for shell integration)
- SyftBox installed on your system

## License

Apache-2.0