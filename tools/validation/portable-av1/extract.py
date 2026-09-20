"""Extract the primary AV1 item from the locally generated fixtures only."""
import argparse
from pathlib import Path


def boxes(data):
    at = 0
    while at < len(data):
        size = int.from_bytes(data[at:at + 4], "big")
        assert 8 <= size <= len(data) - at
        yield data[at + 4:at + 8], data[at + 8:at + size]
        at += size


def extract(path):
    data = path.read_bytes()
    children = dict(boxes(dict(boxes(data))[b"meta"][4:]))
    primary = int.from_bytes(children[b"pitm"][4:], "big")
    iloc = children[b"iloc"]
    version = iloc[0]
    assert version <= 2
    off_len, extent_len = iloc[4] >> 4, iloc[4] & 15
    base_len, index_len = iloc[5] >> 4, iloc[5] & 15 if version else 0
    at = 6

    def read(n):
        nonlocal at
        assert at + n <= len(iloc)
        value = int.from_bytes(iloc[at:at + n], "big")
        at += n
        return value

    result = None
    for _ in range(read(4 if version == 2 else 2)):
        item = read(4 if version == 2 else 2)
        method = read(2) & 15 if version else 0
        reference, base, count = read(2), read(base_len), read(2)
        assert reference == 0 and method == 0
        payload = bytearray()
        for _ in range(count):
            read(index_len)
            offset, size = read(off_len), read(extent_len)
            assert base + offset + size <= len(data)
            payload.extend(data[base + offset:base + offset + size])
        if item == primary:
            result = payload
    assert result is not None
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    args.destination.mkdir(parents=True, exist_ok=True)
    for depth in (8, 10, 12):
        name = f"p3-{depth}bit"
        (args.destination / (name + ".obu")).write_bytes(extract(args.source / (name + ".avif")))
