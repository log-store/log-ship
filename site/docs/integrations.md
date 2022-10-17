---
title: Integrations
description: Pre-written Python scripts to parse common log formats
---

# Integrations

log-ship comes with a handful of Python scripts that make it easy to integrate with common application and system logs.
You can use these Python scripts through the [Python transform plugin](/config#python). You are free to modify or create
your own Python scripts to parse logs from any application.

::: tip Modifying the Python Scripts
If you decide to modify one of the provided Python scripts, it is recommended you copy it, and add it to your version
control system, so you can easily track changes to it, and deploy it to your systems.
:::

## Provided Python Scripts

The following sections contain the list of scripts provided by log-ship for parsing common application logs.
Because log-ship is open source, we encourage everyone to contribute their scripts for parsing common applications!

### `web_servers.py` - httpd & Nginx Web Servers

Parses access, and error logs from Apache's [httpd](https://httpd.apache.org/), and the [Nginx](https://www.nginx.com/) 
web server. You MUST specify the function used to parse the logs in the config depending upon the log source.

To parse access logs, use the `process_combined` function; `process_error` for error logs.

```toml
[[transform]]
name = "httpd access logs"
type = "python"
[transform.args]
path = "web_servers.py"
arg_type = "str"
function = "process_combined"
```

## Testing Scripts

When modifying or creating your own script, it can be cumbersome to test your script using log-ship. Instead, it is
recommended that you write a wrapper around your parsing method, and then pipe a sample log file to your script via
standard input, and see the results via standard output. The following section of code can be added to the bottom of
any the provided Python scripts to aid in testing.

```python
if __name__ == '__main__':
    import sys
    import json

    # read lines from STDIN
    for line in map(lambda l: l.strip(), sys.stdin):
        ret = process(line)  # call the process function
        print("{}\n⤷ {}".format(line, json.dumps(ret)))
```

You would then run your script for testing as follows:

```shell
cat log_file | python3 my_script.py
```

The section of code above will read your log file line-by-line, passing it to the `process` function (you will need to
call a different function if you changed the name), and then print the original log line, and the result of your function.

::: tip
If your process function takes a `dict` instead of a string, simply insert the following line before the call to `process`:

`line = json.loads(line)  # convert the line to JSON`
:::

