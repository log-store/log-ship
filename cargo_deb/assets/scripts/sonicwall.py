import shlex
from typing import Optional


def process(log: dict) -> Optional[dict]:
    # take the message field, and split it up based upon f=v
    if 'message' in log:
        key = 'message'
    elif '+message' in log:
        key = '+message'
    else:
        return log

    try:
        kv_pairs = shlex.split(log[key])
    except Exception as e:
        print("ERROR: {} - {}".format(e, log[key]))
        return log

    for kv in kv_pairs:
        i = kv.find('=')

        if i == -1:
            continue

        log[kv[0:i]] = kv[i:]

    return log


if __name__ == '__main__':
    import sys
    import json

    # read lines from STDIN
    for line in map(lambda l: l.strip(), sys.stdin):
        json_line = json.loads(line)
        ret = process(json_line)  # call the process function
        print("{}\n⤷ {}".format(line, json.dumps(ret)))
