# syslogrs

Basic syslog server with optional stdout display, log rotation, and compression.

## Config

config.toml will be searched for in the following order (for now):
* `./config.toml`
* `/etc/syslogrs/config.toml`

Sample config with all current available options:
```toml
[config]
# pipe logs to stdout (default: true)
display_stdout = true
# store logs (default: false)
store_logs = false
# compress logs on rotation (default: false)
compress = false
# interval in seconds to rotate logs (default: 86400)
log_rotate_interval = 86400
# directory to store logs (default: './')
log_dir = "./"
# listening port (default: 514)
port = 514
```

## Build

Build using cargo:
```
cargo build
```

## TODO
* After rotation, do not create new log until syslog is received (terminate thread?)
* Implement config option to rotate logs at a specific time (i.e. 00:00:00)
* Threading and writing needs to be reviewed for performance
* Add TLS support
* Support parsing BSD and IETF syslog formats
