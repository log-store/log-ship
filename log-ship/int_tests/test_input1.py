from typing import Optional


def process(log: str) -> Optional[dict]:
    # just split by whitespace
    (method, path, status) = log.split(' ')

    return {
        "method": method,
        "path": path,
        "status": status
    }
