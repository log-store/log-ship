---
title: Introduction
description: An Introduction to log-ship
---

# Why log-ship?

There are a lot of other log shippers out there: [File Beat](https://www.elastic.co/beats/filebeat),
[Fluent](https://www.fluentd.org/), [Fluent Bit](https://fluentbit.io/), [Flume](https://flume.apache.org/), [Vector](https://vector.dev/),
[rsyslog](https://www.rsyslog.com/), [syslog-ng](https://www.syslog-ng.com/products/open-source-log-management/), and probably others we've missed.
So why create yet-another-log-shipper? To meet the needs of most system administrators, and developers, without a lot
of hassle. The shippers mentioned above either use a heavy runtime like Java or Ruby, require complex regular expressions to parse
log files (or don't provide any parsing or filtering capabilities at all!), or do not use now-standard formats such as JSON
and [Open Telemetry](https://opentelemetry.io/).

log-ship on the other hand is written in Rust, so it is lightweight (using little memory and small on disk), secure, and fast!
Most parsers for log-ship are written in [Python](/config#python), making them easy to [test](/integrations#testing-scripts),
and modify should the [parsers distributed with log-ship](/integrations#provided-python-scripts) not meet your needs.
log-ship also provides 

We think you'll come to love how easy yet adaptable log-ship is to get setup and running in your environment!

## How log-ship Works

log-ship provides an easy, but extensible, way to ship logs by using the concept of routes. A route is simply a configuration
of an [input](/config#input), zero or more [transforms](/config#transform) (parsers and filters), and an [output](/config#output).

Conceptually this is the same as if you were to run the following in a shell:

```shell
tail -F log_file | python parse_script.py | nc log-store-host 1234
```

The `tail -F log_file` command corresponds to the [file input](/config#file) plugin. The `python parse_script.py` corresponds
to [python transform](/config#python) plugin, which can parse and filter the logs via a Python script.
And the `nc log-store-host 1234` command corresponds to the [tcp_socket output](/config#tcp_socket) plugin.

Each route runs independently of the others; so one route cannot slow down another. The components of each route can
be easily reused in many routes, so you do not have to re-define a transform plugin multiple times for example. Finally,
the delivery of each route is tracked independently as well ensuring that a log in a route is delivered only once.

More information about configuring log-ship can be found in the [documentation section](/config) of this site.

## A Typical Logging Architecture

log-ship is just one component in the typical logging architecture. Most logging architectures consist of 2 or 3 parts:

1. **shipper** - Responsible for reading logs from your system or application, optionally parsing and filtering them,
   and passing them along to either a queue or storage system.
2. **queue** - An optional component responsible for queuing logs before they're inserted into a log storage system.
   Parsing and filtering may also occur at this step.
3. **storage & analytics** - Responsible for providing a user interface into your logs for searching and aggregating.

log-ship fulfills the requirements of the first component of this architecture. It allows you to easily parse, filter, and
send your logs from a file to the next component in the architecture: queue or storage & analytics. While log-store was
built by the same developers as [log-store](https://log-store.com) (could you tell from the name?), it can be used with
any queue or storage & analytics system.

