import re

from typing import Optional, Dict, Any
from datetime import datetime

# pattern for: %t \"%r\" %>s %O \"%{Referer}i\" \"%{User-Agent}i\"
combined_pattern = re.compile(r'\[(.+)] "([A-Z]+) (.+) (.+)" (\d+) (\d+) "(.+)" "(.+)"')


def process_combined(log: str) -> Optional[Dict[str, Any]]:
    """ Process "combined" logs from Apache httpd 2.4 and Nginx
    See: https://httpd.apache.org/docs/2.4/logs.html#accesslog
    and: https://docs.nginx.com/nginx/admin-guide/monitoring/logging/
    """
    log_dict = dict()

    # first split on space to get %h %l %u and rest
    try:
        host, _, user, rest = log.split(' ', 3)
    except ValueError:
        return None

    log_dict['host'] = host

    if user != '-':
        log_dict['user'] = user

    m = combined_pattern.match(rest)

    if m:
        dt = datetime.strptime(str(m.group(1)), "%d/%b/%Y:%H:%M:%S %z")

        log_dict['t'] = int(dt.timestamp() * 1000)

        log_dict['method'] = str(m.group(2))
        log_dict['path'] = str(m.group(3))
        # remove the redundant HTTP/ prefix
        log_dict['proto'] = str(m.group(4)).replace("HTTP/", "")
        log_dict['status'] = int(m.group(5))
        log_dict['size'] = int(m.group(6))

        ref = str(m.group(7))

        if ref != '-':
            log_dict['ref'] = ref

        log_dict['user_agent'] = str(m.group(8))

        return log_dict
    else:
        return None


def process_error(log: str) -> Optional[Dict[str, Any]]:
    """ Process the error logs from Apache httpd 2.4
    See: https://httpd.apache.org/docs/2.4/logs.html#errorlog
    and: https://httpd.apache.org/docs/2.4/mod/core.html#errorlogformat
    """
    from dateutil.parser import parse

    try:
        date, level, pid, rest = log.split('] ', 3)
    except ValueError:  # just skip if there are not enough values
        return None

    log_dict = dict()

    dt = parse(timestr=date.replace("[", ""))
    log_dict['t'] = int(dt.timestamp() * 1000)

    log_dict['level'] = level.replace("[", "")

    # some formats have a tid as well
    pid_tid = pid.split(":")

    if len(pid_tid) == 2:
        log_dict['pid'] = int(pid_tid[0].replace("[pid ", ""))
        log_dict['tid'] = int(pid_tid[1].replace("tid ", ""))
    else:
        log_dict['pid'] = int(pid_tid[0].replace("[pid ", ""))

    if rest.startswith("[client"):
        client, rest = rest.split("] ", 1)
        log_dict['client'] = client.replace("[client ", "")
        log_dict['message'] = rest
    else:
        log_dict['message'] = rest

    return log_dict


if __name__ == '__main__':
    import sys
    import json

    # read lines from STDIN
    for line in map(lambda l: l.strip(), sys.stdin):
        # uncomment the function you want to test
        # ret = process_combined(line)
        ret = process_error(line)
        print("{}\n⤷ {}".format(line, json.dumps(ret)))
