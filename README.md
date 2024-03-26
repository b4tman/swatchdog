# swatchdog - simple watchdog!

Just... Send an HTTP request per interval to a given URL. You can use any uptime monitoring service to monitor your machine.

## Fork changes

This fork is specifically designed for use with [uptime-kuma](https://github.com/louislam/uptime-kuma) monitoring service.

### Modifications

- URL now includes query parameters:
  - `status`: "up"
  - `msg`: system uptime (e.g., "up 4 months 1 day 2 hours 4 minutes 2 seconds")
  - `ping`: time of ping to host (e.g., "2ms")
- Added support for running as a Windows service (no impact on Linux/MacOS compilation)
- Enhanced logging functionality for more control over log management
- Implemented graceful shutdown for proper resource cleanup and reliable log delivery
- Added the `--insecure` option to disregard SSL certificate errors
- Added the `--from` option to designate the local IP address, enabling the selection of the IP version for sending requests (use "::" for IPv6 and "0.0.0.0" for IPv4).

## Download & Install

The latest version is available at [GitHub Releases](https://github.com/b4tman/swatchdog/releases). It's just a single binary file, so you can download it and run it directly.

## Usage

Just as simple as... No need to explain anything! Just run it with `--help` to see the help message.

```
Usage: swatchdog [OPTIONS] --url <URL>

Options:
  -u, --url <URL>             target url
      --method <METHOD>       http method [default: GET]
      --interval <INTERVAL>   heartbeats interval [default: 60s]
  -k, --insecure              ignore certificate errors
  -s, --from <LOCAL_ADDRESS>  optional local ip ("0.0.0.0" for ipv4, "::" for ipv6)
      --verbose               verbose messages
      --log <LOG>             optional log variant (none | stdout | stderr | file | dir ) default is dir, one of (current_exe, current_dir) + stdout, if writable dir found, or just stdout
      --service <SERVICE>     service command ( install | uninstall | start | stop | run ) "run" is used for windows service entrypoint
  -h, --help                  Print help
  -V, --version               Print version
```

The tool is tested with [uptime-kuma](https://github.com/louislam/uptime-kuma) and I personally recommend it.

## Configuration

### Logging Setup

Customize logging behavior using the `--log` option with the following configuration options:

- `none`: Disable logging
- `stdout`: Write logs to stdout
- `stderr`: Write logs to stderr
- `<filepath>`: Write logs to a specific file
- `<directory path>`: Rotate logs in a specified directory

By default, logs are written to stdout. swatchdog will search for a writable directory and write logs there if found.

### Run as service

To run swatchdog as a service, follow these guidelines:

#### Windows

Use the `--service` option with commands like `install`, `uninstall`, `start`, or `stop`.

For example:

```powershell
swatchdog --url http://example.com/api/push/example --service install
```

#### Linux

Example unit file for systemd:

```ini
[Unit]
Description=swatchdog

[Service]
User=nobody
Group=nobody
ExecStart=/path/to/swatchdog -u http://example.com --interval 60s
ExecStop=kill -s SIGINT $MAINPID

[Install]
WantedBy=multi-user.target
```

(place it under `/lib/systemd/system/swatchdog.service` and run `systemctl enable swatchdog`)

#### MacOS

Example plist file for launchd:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
  <dict>
    <key>Label</key>
    <string>me.singee.swatchdog</string>
    <key>ProgramArguments</key>
    <array>
      <string>/path/to/your/swatchdog</string>
      <string>-u=http://example.com</string>
      <string>--interval=60s</string>
      <string>--log=stdout</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>/Users/USERNAME/.swatchdog.log</string>
    <key>StandardErrorPath</key>
    <string>/Users/USERNAME/.swatchdog.log</string>
  </dict>
</plist>
```

(place it under `~/Library/LaunchAgents/me.singee.swatchdog.plist` and run `launchctl load ~/Library/LaunchAgents/me.singee.swatchdog.plist`)

## License

This project is licensed under the MIT License.
