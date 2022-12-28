---
title: Configuration
description: Configuring log-ship
---

# Configuring log-ship
log-ship cannot run without a configuration file. Even for deployments where a single log file is shipped, a config
file is required to specify all the [plugins](#plugins) in the [route](#routes). Each plugin must also be configured.
This sounds daunting, and there is a lot of configuration; however, most of it is handled already.
The bulk of the configuration comes from the [transform plugins](#transform), and a number of these are configured already.

In the most basic form, where your logs are already in JSON, only an [input](#input) and [output](#output) plugin must be configured, then
a single route. For most setups, you would also have a [transform plugin](#transform) configured, and specified in the route.

The configure file is written in [TOML](https://toml.io/), and is split into 5 sections: `globals`, `inputs`, `transforms`, `outputs`, and `routes`.
Each section, except the global section, uses TOML's [Array of Tables](https://toml.io/en/v1.0.0#array-of-tables) to specify
the configuration. The global section is simply a [Table](https://toml.io/en/v1.0.0#table) in TOML.

## Global Section

There are only 2 configurations in the global section, both optional.

```toml
[global]
channel_size = 128  # default size
log_file = "/path/to/log/file" # defaults to STDOUT if not supplied
```

* `channel_size` specifies the number of logs that can be simultaneously traversing a route from input to output.
This value must be between 2 and 1024.
* `log_file` specifies a file to record logs for log-ship. You can of course setup a route to ship these logs to [log-store](https://log-store.com)
If left blank, logs are printed to standard out.

## Plugins

All plugins require the `name` and `type` field to be specified. The `name` is simply a descriptor or label to use when
specifying a route, and reporting errors. It can be any string, but short descriptive labels are best. The `type` field
specifies which plugin you are configuring. The various types of plugins are listed in the sections below.

All plugins have an optional `description` field which is used when printing the list of defined plugins; see [running log-ship](/running).

Plugins also take various other arguments. These arguments are specified in a [table](https://toml.io/en/v1.0.0#table),
under the plugin configuration. See each plugin category below for a full description.

Plugins of a given type (`file` for example) can be configured multiple times, for multiple different configurations of
that plugin. For example, if you want to specify 2 log files as inputs, then you would specify an `[[input]]` with a type
of `file` twice, one for each file.

### Input

Logs must be consumed from some source. The input plugin specifies the configuration of this source.
While [log-store](https://log-store.com) requires all logs to be in JSON, the source does not need to be in JSON.
This is the job of the transforms; to parse logs into JSON.

The most common input plugin is the `file` plugin which reads from a file (much like `tail -F`), and keeps track of the
lines that have been processed.

#### `file`

Reads logs from a file, line-by-line, optionally parsing the line as JSON.

```toml
[[input]]
name = "kern_logs"
type = "file"
[input.args]
path = "/var/log/kern.log"
parse_json = false
from_beginning = true
state_file_dir = "/tmp/"
```

##### Arguments
* `name` a descriptive label for the configuration
* `type = "file"` this must be specified to configure this plugin
* `path` the possibly [globbed](https://en.wikipedia.org/wiki/Glob_(programming)) path to the log file(s) to read.
Whenever possible, it is better to specify multiple files via multiple `file` input plugins configurations than using globbing.
File globbing **only** occurs once, so newly added files will not automatically be "picked up" by this plugin.
The file will be monitored (even if it doesn't exist), and all writes will be processed line-by-line. File rotation log
files should **not** immediately compress (gzip) the file, or lines might be missed. See the logrotate man page for more information on how to setup log rotation.
* `parse_json` an optional argument to indicate if the line should be treated as JSON and parsed before sending it to
the next plugin in the route; defaults to `false`. Parsing the input as JSON via the `file` input plugin is faster than doing so in Python in a transform plugin.
If the line cannot be parsed as JSON, a warning is printed, and the line is discarded.
* `from_beginning` a boolean indicating that the file should be read from the beginning. This will discard any state saved in the `state_file`.
If this is the first time reading the file (ie, there is no `state_file`), then it will be read from the beginning regardless. Defaults to `false`.
* `state_file_dir` an optional argument specifying what directory state files should be stored in.
A state file is created for each file  is not specified, it defaults to a file in the same directory as the `path`, with a suffix of `.state` added.

#### `journald`

Reads systemd entries from the specified namespace (or all), and parses them as JSON.

```toml
[[input]]
name = "system_logs"
type = "journald"
[input.args]
journal = "system"
from_beginning = true
cursor_file = "/tmp/journald.cursor"
```

##### Arguments
* `name` a descriptive label for the configuration
* `type = "journald"` this must be specified to configure this plugin
* `journal` the journal to read the logs from; one of `all`, `system`, or `user`. Defaults to `all`.
* `from_beginning` a boolean indicating that the file should be read from the beginning. This will discard the cursor saved in the `cursor_file`.
  If there is no `cursor_file`, then it will be read from the beginning; defaults to `false`.
* `cursor_file` specifies where the cursor should be stored to track which entries have been processed.


#### `metrics`

Polls the system for various metrics about the CPU, memory, disk, and network. These are very basic system metrics, but
enough to understand the general health of the system.

```toml
[[input]]
name = "metrics"
type = "metrics"
[input.args]
metrics = ["cpu", "memory", "disk", "net"]
cpu_poll_secs = 5
mem_poll_secs = 5
disk_poll_secs = 30
net_poll_secs = 5
```

##### Arguments
* `name` a descriptive label for the configuration
* `type = "metrics"` this must be specified to configure this plugin
* `metrics` an array or string indicating the metrics to be collected. The possible values are `cpu`, `memory`, `disk`,
or `net`. Defaults to all metrics if not specified.
* `cpu_poll_secs` number of seconds to wait between polling for CPU metrics. Must be in the range [5, 3600], defaults to 5.
* `mem_poll_secs` number of seconds to wait between polling for CPU metrics. Must be in the range [5, 3600], defaults to 5.
* `disk_poll_secs` number of seconds to wait between polling for CPU metrics. Must be in the range [5, 3600], defaults to 30.
* `net_poll_secs` number of seconds to wait between polling for CPU metrics. Must be in the range [5, 3600], defaults to 5.


#### `stdin`

Reads from standard input. This plugin is mostly for debugging a route, or loading some other input.

```toml
[[input]]
name = "dmesg"
type = "stdin"
[input.args]
parse_json = false
```

##### Arguments
* `name` a descriptive label for the configuration
* `type = "stdin"` this must be specified to configure this plugin
* `parse_json` an optional argument to indicate if the line should be treated as JSON and parsed before sending it to
the next plugin in the route; defaults to `false`. Parsing the input as JSON via the input plugin is faster
than doing so in Python in a transform plugin.



### Transform

Transform plugins take logs as either a string (line of text), or as JSON, and converts them to JSON, or filters out the log line.
The most common transform plugin is the `python` plugin, which calls a Python script.

::: warning Performance
Other transform plugins are provided
for common functions:
* [`insert_field`](#insert-field) for inserting a field & value
* [`insert_ts`](#insert-ts) for inserting a timestamp

These transform plugins are usually more efficient than the `python`
plugin for these operations. It is recommended that you use them instead of a separate `python` plugin instance.
:::

#### `python`

Calls a Python function on each log line as either a `str`, or `dict` if the line has already been converted to JSON.
The Python transform plugin is the most powerful and common. You can parse any log using this plugin; however, if you just
want to insert a field or timestamp, use one of the transform plugins below, as they are much faster.

```toml
[[transform]]
name = "httpd access logs"
type = "python"
[transform.args]
path = "httpd.py"
arg_type = "str"
function = "parse_access_log"
```

##### Arguments
* `name` a descriptive label for the configuration
* `type = "python"` this must be specified to configure this plugin
* `path` the path to the script where the function resides. There can be additional code that tests the transform function; see the [Intro](/intro#transforms) page for an example.
* `arg_type` the type of argument (`str` or `dict`) to be passed to the function. A `dict` can be passed if the log has already been converted to JSON.
* `function` optional name of the function to call; defaulting to `process`.

::: tip Function Signature
The function must have one of the two following signatures, with type hints included:
```python
from typing import Optional


def process(log: str) -> Optional[dict]:   # if arg_type = "str"
def process(log: dict) -> Optional[dict]:  # if arg_type = "dict"
```
:::

The function **must** convert the log line to a `dict` (JSON), or return None if the log should be filtered out. Any
exceptions that occur should be handled (caught) by the script. Errors can be printed to standard error, and will be logged
by log-ship.

::: warning Performance
For the best performance, use a single instance of the Python transform plugin with a script that can handel all of the
parsing and filtering, instead of using multiple Python transform plugins.
:::

#### `insert_field`

Inserts a field in an already parsed (JSON) log, optionally overwriting the value if it already exists.

```toml
[[transform]]
name = "add source"
type = "insert_field"
[transform.args]
field = "source"
value = "my_app"
overwrite = false
```

##### Arguments
* `name` a descriptive label for the configuration
* `type = "insert_field"` this must be specified to configure this plugin
* `field` the field to insert into the JSON object
* `value` the [JSON value](https://www.json.org/) to insert
* `overwrite` if `true`, overwrites any existing value found. If `false` (the default), leaves existing values unchanged.

::: danger Warning!
If this is the first transform in the chain, and the log has not already been parsed into JSON, this transform will
generate an error in the logs.
:::

#### `insert_ts`

Inserts the current time as a timestamp field in an already parsed (JSON) log, optionally overwriting the value if it already exists.

```toml
[[transform]]
name = "add timestamp"
type = "insert_ts"
[transform.args]
field = "t"
ts_type = "epoch"
overwrite = false
```

##### Arguments
* `name` a descriptive label for the configuration
* `type = "insert_ts"` this must be specified to configure this plugin
* `field` the field to insert the timestamp into; defaults to `t`.
* `ts_type` the optional type of timestamp to use; defaults to `epoch`. See the [log-store](https://log-store.com/documentation.html#config_file) documentation for the various types.
* `overwrite` if `true`, overwrites any timestamp value found. If `false` (the default), leaves existing timestamp unchanged.

::: danger Warning!
If this is the first transform in the chain, and the log has not already been parsed into JSON, this transform will
generate an error in the logs.
:::



### Output

These plugins specify where the logs are to be sent at the end of a route. All logs should be converted to JSON before they
are sent to an output plugin. Output plugins are responsible for ensuring delivery of your logs.

#### `tcp_socket`

Sends logs to a TCP socket. When used with [log-store](https://log-store.com), this output plugin should be used
when log-ship and log-store are on different machines.

```toml
[[output]]
name = "log-store tcp socket"
type = "tcp_socket"
[output.args]
host = "93.184.216.34"
port = 1234
```

##### Arguments
* `name` a descriptive label for the configuration
* `type = "tcp_socket"` this must be specified to configure this plugin
* `host` the host or IP address to send the logs to.
* `port` the port the receiving server is listening on.

#### `unix_socket`

Sends logs to a Unix domain socket. When used with [log-store](https://log-store.com), this output plugin should be used
when log-ship and log-store are on the same machine.

```toml
[[output]]
name = "log-store unix socket"
type = "unix_socket"
[output.args]
path = "/tmp/log-store.socket"
```

##### Arguments
* `name` a descriptive label for the configuration
* `type = "unix_socket"` this must be specified to configure this plugin
* `path` the path of the unix domain socket to send logs to. This socket should accept logs line-by-line.


#### `stdout`

Writes to from standard out. This plugin is mostly for debugging a route.

```toml
[[output]]
name = "test output"
type = "stdout"
```

##### Arguments
* `name` a descriptive label for the configuration
* `type = "stdout"` this must be specified to configure this plugin

_There are no additional arguments to configure this plugin._

#### `speed_test`

Simply consumes all logs, and displays how many logs-per-second are consumed by the route.

```toml
[[output]]
name = "speed test"
type = "speed_test"
```

##### Arguments
* `name` a descriptive label for the configuration
* `type = "speed_test"` this must be specified to configure this plugin

_There are no additional arguments to configure this plugin._

::: danger Not For Production
This plugin will consume all of your logs without sending them anywhere! Be very careful using it with input plugins
which track offsets into files or journald, as the offset will be lost when using this plugin. It is _only_ for testing
the performance of a route.
:::

## Routes

Routes configure the flow of logs from input, through transforms, to one or more outputs. They specify configurations of
plugins described above. This way you can have the same plugin, configured multiple times, and used in multiple routes.

To configure a route, 4 values must be set:

* `name`  a descriptive label for the route, used only for reporting errors.
* `input` the `name` of an input plugin previously configured.
* `transforms` an array of `name`s of previously configured transform plugins.
* `outputs` an array of `name`s of previously configured output plugins.

Below is an example route configured to read kernel logs from a file, parse them via a python script, then insert a 
timestamp, then finally sends the logs to [log-store](https://log-store.com) via the `tcp_socket` output plugin.

```toml
[[route]]
name = "kern logs"
input = "kern_logs"
transforms = ["kern_logs_python", "insert_ts"]
outputs = ["log-store tcp socket"]
```

::: danger Warning!
All the plugin names specified above must be previously configured, or an error will be generated.
:::
