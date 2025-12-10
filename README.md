# NetMonRS

NetMonRS is a real-time network connection monitor for *nix systems, written in Rust. It provides a terminal-based interface to track active network connections and connection history for a specified process.

## Features

- Real-time monitoring of network connections
- Dual-panel interface showing active connections and connection history
- Process name-based filtering
- Tab navigation between connection lists
- Keyboard controls for navigation and interaction

## Installation

### Prerequisites

- Rust toolchain (cargo)
- `lsof` utility (usually included in `lsof` package)
- `pgrep` utility (usually included in `procps` package)

### Build

```bash
cargo build --release
```

## Usage

```bash
./target/release/netmonrs <process_name>
```

Example:
```bash
./target/release/netmonrs firefox
```

## Controls

- `Tab` - Switch focus between active connections and history
- `Up` / `k` - Move up in list
- `Down` / `j` - Move down in list
- `PageUp` / `Ctrl+u` - Page up
- `PageDown` / `Ctrl+d` - Page down
- `q` - Quit application

## Requirements

The application requires `sudo` privileges to run `lsof` command for network connection information. The first time you run it, you may be prompted for your password.

## License

This project is licensed under the MIT License.
