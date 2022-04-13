# syslogrs

Basic UDP syslog server that will display syslog messages with no parsing.

syslogメッセージを表示するシンプルなsyslogサーバー

If running on Linux, enable port reuse by uncommenting out:
```udp4_server_s.set_reuse_port(true)?;

Build using cargo:
```cargo build

You can add some parsing of standard or custom syslog formats to make it more useful, or just use it for testing.