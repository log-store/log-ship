import re

from typing import Optional
from dateutil.parser import parse

pattern = re.compile(r"\[(\d+)\.(\d+)]")


def process(log: str) -> Optional[dict]:
    """ Process logs typically found at /var/log/kern
    """
    log_dict = dict()

    # logs look like this:
    # Oct  3 08:28:06 ES kernel: [307228.938154] ACPI: EC: EC stopped
    try:
        # split on the first ': ', but only 2 parts
        date, rest = log.split('kernel: ', 1)
    except ValueError:
        return None

    date = date[0:date.strip().rfind(' ')]

    dt = parse(timestr=date, fuzzy=True)

    log_dict['t'] = int(dt.timestamp() * 1000)

    m = pattern.match(rest)

    if m:
        log_dict['since_start_sec'] = int(m.group(1))
        log_dict['since_start_ns'] = int(m.group(2))
        log_dict['message'] = rest[len(m.group(0)):].strip()

        return log_dict
    else:
        return None


if __name__ == '__main__':
    import sys
    import json

    # read lines from STDIN
    for line in map(lambda l: l.strip(), sys.stdin):
        ret = process(line)  # call the process function
        print("{}\n⤷ {}".format(line, json.dumps(ret)))
