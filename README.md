# mtr-rs

[![Crates.io](https://img.shields.io/crates/v/mtr-rs.svg)](https://crates.io/crates/mtr-rs)
[![License](https://img.shields.io/crates/l/mtr-rs.svg)](LICENSE)

A modern network diagnostic tool combining ping and traceroute with real-time updating path tables. Built in pure Rust, powered by [multiprobe](https://crates.io/crates/multiprobe).

## Features

- **Real-time TUI** - Live-updating hop statistics with color-coded loss indicators
- **Paris Traceroute Mode** - ECMP-aware path discovery that works correctly behind load balancers
- **Comprehensive Metrics** - Loss%, sent, received, last/avg/best/worst RTT, jitter
- **JSON Output** - Machine-readable output for scripting and automation
- **Cross-platform** - Linux, macOS, Windows

## Installation

```bash
cargo install mtr-rs
```

Or build from source:

```bash
git clone https://github.com/biplabku/mtr-rs
cd mtr-rs
cargo build --release
```

## Usage

```bash
# Basic usage
sudo mtr-rs example.com

# Paris Traceroute mode (ECMP-aware)
sudo mtr-rs --paris example.com

# Custom settings
sudo mtr-rs -m 20 -t 3 -i 500 example.com

# JSON output (non-interactive)
sudo mtr-rs --json example.com

# Limited probe count
sudo mtr-rs -c 10 example.com
```

## Options

```
Usage: mtr-rs [OPTIONS] <TARGET>

Arguments:
  <TARGET>  Target hostname or IP address

Options:
  -m, --max-hops <MAX_HOPS>  Maximum number of hops [default: 30]
  -t, --timeout <TIMEOUT>    Timeout per hop in seconds [default: 2]
  -i, --interval <INTERVAL>  Interval between probe cycles in milliseconds [default: 1000]
      --paris                Use Paris Traceroute mode (ECMP-aware)
      --json                 Output in JSON format (non-interactive)
  -c, --count <COUNT>        Number of probe cycles (0 = unlimited) [default: 0]
  -h, --help                 Print help
  -V, --version              Print version
```

## TUI Controls

- `q` or `Esc` - Quit

## Example Output

```
┌─ mtr-rs: example.com | Mode: Paris | Probes: 42 | Press 'q' to quit ─┐
└──────────────────────────────────────────────────────────────────────┘
┌─ Hops ───────────────────────────────────────────────────────────────┐
│ #  Host             Loss%  Sent Recv    Last     Avg    Best    Wrst │
│  1 192.168.1.1        0.0%   42   42    1.23    1.45    0.98    2.34 │
│  2 10.0.0.1           0.0%   42   42    8.45    9.12    7.23   12.45 │
│  3 72.14.215.85       2.4%   42   41   15.67   16.23   14.56   22.34 │
│  4 142.250.169.174    0.0%   42   42   18.34   19.45   17.89   24.56 │
│  5 93.184.216.34      0.0%   42   42   22.56   23.12   21.34   28.78 │
└──────────────────────────────────────────────────────────────────────┘
```

## Permissions

Raw sockets are required for ICMP probes:

```bash
# Linux: Grant capability (preferred)
sudo setcap cap_net_raw+ep ./target/release/mtr-rs

# Or run with sudo
sudo mtr-rs example.com
```

## Powered By

- [multiprobe](https://crates.io/crates/multiprobe) - Multi-protocol network probing library
- [ratatui](https://crates.io/crates/ratatui) - Terminal UI framework
- [tokio](https://crates.io/crates/tokio) - Async runtime

## License

MIT OR Apache-2.0
