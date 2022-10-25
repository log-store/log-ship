---
title: Running
description: Running log-ship
---

# Running log-ship

log-ship can be run as either a long-running process, or on the command line. Long-running processes on Linux are called
daemons, and they must perform a number of steps to ensure the process can run for hours, days, or even years without
interruption. log-ship leverages [systemd](https://www.freedesktop.org/software/systemd/man/systemd.html) to provide
a ["new-style" daemon](https://www.freedesktop.org/software/systemd/man/daemon.html). As such, to run log-ship as a
daemon, you must configure a systemd service. This is all taken care of for you, if you install log-ship via a [package](/download).

You can also run log-ship via a terminal from the command line. This way of running log-ship is great for testing, and
one-off importing existing logs. There are various command line arguments which are described
[below](#command-line-arguments), or issuing by providing the `-h` command line option.


## Command Line Arguments

If you need to run log-store for testing, or on an ad hoc basis to import existing logs, you can run it from the command
line with the following options (obtained by running with `-h`):

```shell
The most versatile log shipper!

Usage: log-ship [OPTIONS]

Options:
      --log-file <LOG_FILE>        Optional log file location
      --config-file <CONFIG_FILE>  Optional config file location
      --check                      Check the config file, and print the routes
  -h, --help                       Print help information
  -V, --version                    Print version information
```

The most important option when testing log-ship, is the `--check` option. This option will read the configuration file,
and find any errors. It will also print the routes that are configured.

## Manually Configuring systemd for log-ship

:::warning
You **only** need to perform these steps if you have downloaded the `.tar.gz` archive and are installing log-ship
manually! If you installed log-ship via a package, systemd is already configured.
:::

log-ship's interface to systemd is simple, and does not leverage [D-Bus](https://en.wikipedia.org/wiki/D-Bus), or the
[`sd_notify`](https://www.freedesktop.org/software/systemd/man/sd_notify.html) interface. Instead, log-ship simply
logs to a file, and specifies the configuration file via command line option. Therefore, the service file for log-ship
is short, and to-the-point:

```ini
[Unit]
Description=The most versatile log shipper

[Service]
Type=exec
ExecStart=/usr/bin/log-ship --config-file /etc/log-ship.toml
KillSignal=SIGINT

[Install]
WantedBy=multi-user.target
```

To install log-ship as a service, run the following commands after copying the above into `/etc/systemd/system/` as
root:

```bash
sudo systemctl daemon-reload
sudo systemctl enable log-ship.service
sudo systemctl start log-ship.service
sudo systemctl status log-ship.service
```

