---
title: Download
description: Downloading log-ship
---

# Downloading log-ship

log-ship is provided in [`deb`](https://log-ship.com/log-ship_1.2.0_amd64.deb) and
[`rpm`](https://log-ship.com/log-ship-1.2.0-1.x86_64.rpm) packages, and also as a
[`tar.gz`](https://log-ship.com/log-ship_1.2.0.tar.gz) archive. Downloading and installing via either
the [`deb`](https://log-ship.com/log-ship_1.2.0_amd64.deb) or [`rpm`](https://log-ship.com/log-ship-1.2.0-1.x86_64.rpm)
package is recommended.

Installing via the [`tar.gz`](https://log-ship.com/log-ship_1.2.0.tar.gz) archive is a much more manual process. The archive simply provides all the files in a
common directory layout, but you are responsible for moving and installing things such as the systemd unit files.

## `deb` Package

Download the [`deb` package](https://log-ship.com/log-ship_1.2.0_amd64.deb), and install via:

```shell
sudo dpkg -i log-ship_1.2.0_amd64.deb
```

## `rpm` Package

Download the [`rpm` package](https://log-ship.com/log-ship-1.2.0-1.x86_64.rpm), and install via:

```shell
sudo rpm -i log-ship-1.2.0-1.x86_64.rpm
```

## `tar.gz` Package

Download the [`tar.gz` archive](https://log-ship.com/log-ship_1.2.0.tar.gz), and install via:

```shell
tar -zxf log-ship_1.2.0.tar.gz
```

