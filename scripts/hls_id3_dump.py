#!/usr/bin/env python3
"""Read the timed-ID3 tags in the MPEG-TS segments of a live HLS playlist.

The script downloads the media playlist, then each segment in it. It finds
the PID of the timed-ID3 stream in the PMT (stream type 0x15), joins the PES
packets of that PID and parses each ID3v2 tag. It prints one JSON line for
each tag.

Use --expect-txxx DESCRIPTION=VALUE to make the script exit with status 1
when no tag holds a TXXX frame with that description and value.

The script reports an ID3 packet that it cannot parse and then exits with
status 2. It does not read unsynchronisation, an extended header, or
compressed, encrypted or grouped frames.

Use --expect-priv OWNER=KEY=VALUE to make the script exit with status 1
when no tag holds a PRIV frame with that owner, a JSON object payload, and
that string value at that key of the object.

The script uses only the Python standard library.
"""

from __future__ import annotations

import argparse
import json
import sys
import urllib.request
from urllib.parse import urljoin

TS_PACKET = 188


def fetch(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "hls-id3-dump/1"})
    with urllib.request.urlopen(req, timeout=20) as resp:
        return resp.read()


def segment_urls(playlist_url: str) -> list[str]:
    text = fetch(playlist_url).decode("utf-8", "replace")
    lines = [ln.strip() for ln in text.splitlines() if ln.strip()]
    if any(ln.startswith("#EXT-X-STREAM-INF") for ln in lines):
        variant = next(ln for ln in lines if not ln.startswith("#"))
        return segment_urls(urljoin(playlist_url, variant))
    return [urljoin(playlist_url, ln) for ln in lines if not ln.startswith("#")]


def ts_payloads(data: bytes):
    """Yield (pid, payload_unit_start, payload) for each TS packet."""
    for off in range(0, len(data) - TS_PACKET + 1, TS_PACKET):
        pkt = data[off : off + TS_PACKET]
        if pkt[0] != 0x47:
            raise ValueError(f"lost TS sync at byte {off}")
        pusi = bool(pkt[1] & 0x40)
        pid = ((pkt[1] & 0x1F) << 8) | pkt[2]
        afc = (pkt[3] >> 4) & 0x3
        pos = 4
        if afc in (2, 3):
            pos += 1 + pkt[4]
        if afc in (1, 3) and pos < TS_PACKET:
            yield pid, pusi, pkt[pos:]


def psi_sections(data: bytes, want_pids):
    """Yield (pid, section) for each complete PSI section of the PIDs.

    want_pids is a function so that the caller can add PMT PIDs while it reads.
    A section can continue in the next TS packets of the same PID.
    """
    bufs: dict[int, bytearray] = {}
    for pid, pusi, payload in ts_payloads(data):
        if not want_pids(pid):
            continue
        if pusi:
            pointer = payload[0]
            bufs[pid] = bytearray(payload[1 + pointer :])
        elif pid in bufs:
            bufs[pid].extend(payload)
        else:
            continue
        buf = bufs[pid]
        if len(buf) >= 3:
            total = 3 + (((buf[1] & 0x0F) << 8) | buf[2])
            if len(buf) >= total:
                yield pid, bytes(buf[:total])
                del bufs[pid]


def find_id3_pid(data: bytes) -> int | None:
    pmt_pids: set[int] = set()
    for pid, sec in psi_sections(data, lambda p: p == 0 or p in pmt_pids):
        end = len(sec) - 4
        if pid == 0:
            for i in range(8, end - 3, 4):
                program = (sec[i] << 8) | sec[i + 1]
                if program != 0:
                    pmt_pids.add(((sec[i + 2] & 0x1F) << 8) | sec[i + 3])
            continue
        if len(sec) < 12:
            raise ValueError("PMT section too short")
        info_len = ((sec[10] & 0x0F) << 8) | sec[11]
        pos = 12 + info_len
        while pos + 5 <= end:
            stream_type = sec[pos]
            epid = ((sec[pos + 1] & 0x1F) << 8) | sec[pos + 2]
            es_len = ((sec[pos + 3] & 0x0F) << 8) | sec[pos + 4]
            if stream_type == 0x15:
                return epid
            pos += 5 + es_len
    return None


def pes_packets(data: bytes, want_pid: int):
    buf = bytearray()
    started = False
    for pid, pusi, payload in ts_payloads(data):
        if pid != want_pid:
            continue
        if pusi:
            if started and buf:
                yield bytes(buf)
            buf = bytearray(payload)
            started = True
        elif started:
            buf.extend(payload)
    if started and buf:
        yield bytes(buf)


def pes_body(pes: bytes) -> tuple[int | None, bytes]:
    if len(pes) < 9 or pes[:3] != b"\x00\x00\x01":
        raise ValueError("PES start code missing")
    pes_len = (pes[4] << 8) | pes[5]
    if pes_len:
        if len(pes) < 6 + pes_len:
            raise ValueError("PES packet truncated")
        pes = pes[: 6 + pes_len]
    flags = pes[7]
    header_len = pes[8]
    if len(pes) < 9 + header_len:
        raise ValueError("PES header truncated")
    pts = None
    if flags & 0x80 and header_len >= 5:
        p = pes[9:14]
        pts = (
            ((p[0] >> 1) & 0x07) << 30
            | p[1] << 22
            | (p[2] >> 1) << 15
            | p[3] << 7
            | p[4] >> 1
        )
    return pts, pes[9 + header_len :]


def syncsafe(b: bytes) -> int:
    return (b[0] << 21) | (b[1] << 14) | (b[2] << 7) | b[3]


def decode_text(enc: int, raw: bytes) -> str:
    if enc == 0:
        return raw.decode("latin-1")
    if enc == 1:
        return raw.decode("utf-16")
    if enc == 2:
        return raw.decode("utf-16-be")
    return raw.decode("utf-8", "replace")


def split_text(enc: int, raw: bytes) -> tuple[bytes, bytes]:
    """Split at the first terminator: one NUL byte, or two aligned NUL bytes for UTF-16."""
    if enc in (1, 2):
        for i in range(0, len(raw) - 1, 2):
            if raw[i] == 0 and raw[i + 1] == 0:
                return raw[:i], raw[i + 2 :]
        return raw, b""
    head, _, tail = raw.partition(b"\x00")
    return head, tail


def parse_id3(tag: bytes) -> dict:
    """Parse an ID3v2.3 or ID3v2.4 tag. Raise ValueError for a form that it does not read."""
    if len(tag) < 10 or tag[:3] != b"ID3":
        raise ValueError("ID3 header missing")
    version = tag[3]
    if version not in (3, 4):
        raise ValueError(f"ID3v2.{version} is not supported")
    if tag[5] & 0xC0:
        raise ValueError("ID3 unsynchronisation or extended header is not supported")
    size = syncsafe(tag[6:10])
    end = 10 + size
    if len(tag) < end:
        raise ValueError(f"ID3 tag truncated: header says {size} bytes, {len(tag) - 10} present")
    pos = 10
    frames = []
    while pos + 10 <= end and tag[pos : pos + 4] != b"\x00\x00\x00\x00":
        fid = tag[pos : pos + 4].decode("latin-1")
        fsize = syncsafe(tag[pos + 4 : pos + 8]) if version >= 4 else int.from_bytes(tag[pos + 4 : pos + 8], "big")
        fflags = tag[pos + 9]
        if pos + 10 + fsize > end:
            raise ValueError(f"frame {fid} runs past the end of the tag")
        if (version == 3 and fflags & 0xE0) or (version == 4 and fflags & 0x4F):
            raise ValueError(f"frame {fid} has format flags 0x{fflags:02x} that are not supported")
        data = tag[pos + 10 : pos + 10 + fsize]
        pos += 10 + fsize
        frame: dict = {"id": fid, "size": fsize}
        if fid == "TXXX":
            if fsize < 1:
                raise ValueError("TXXX frame is empty")
            frame["encoding"] = data[0]
            desc, value = split_text(data[0], data[1:])
            frame["description"] = decode_text(data[0], desc)
            frame["value"] = decode_text(data[0], value).rstrip("\x00")
        elif fid == "PRIV":
            owner, _, payload = data.partition(b"\x00")
            frame["owner"] = owner.decode("latin-1")
            if owner == b"com.apple.streaming.transportStreamTimestamp":
                frame["value"] = payload.hex()
            else:
                frame["value"] = payload.decode("utf-8", "replace")
        elif fid.startswith("T") and fsize >= 1:
            frame["encoding"] = data[0]
            frame["value"] = decode_text(data[0], data[1:]).rstrip("\x00")
        else:
            frame["bytes"] = len(data)
        frames.append(frame)
    return {"version": f"2.{version}", "tag_size": size, "frames": frames}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("playlist", help="URL of the HLS media playlist")
    ap.add_argument("--expect-txxx", action="append", default=[], metavar="DESCRIPTION=VALUE")
    ap.add_argument("--expect-priv", action="append", default=[], metavar="OWNER=KEY=VALUE")
    args = ap.parse_args()

    expected = [tuple(e.split("=", 1)) for e in args.expect_txxx]
    expected_priv = [tuple(e.split("=", 2)) for e in args.expect_priv]
    found: set[tuple[str, str]] = set()
    found_priv: set[tuple[str, str, str]] = set()
    errors = 0
    for url in segment_urls(args.playlist):
        seg = fetch(url)
        pid = find_id3_pid(seg)
        if pid is None:
            print(json.dumps({"segment": url.rsplit("/", 1)[-1], "id3_pid": None}))
            continue
        for pes in pes_packets(seg, pid):
            try:
                pts, body = pes_body(pes)
                tag = parse_id3(body)
            except ValueError as err:
                errors += 1
                print(json.dumps({"segment": url.rsplit("/", 1)[-1], "id3_pid": pid, "error": str(err)}))
                continue
            print(json.dumps({"segment": url.rsplit("/", 1)[-1], "id3_pid": pid, "pts": pts, **tag}, ensure_ascii=False))
            for fr in tag["frames"]:
                if fr["id"] == "TXXX":
                    found.add((fr["description"], fr["value"]))
                elif fr["id"] == "PRIV":
                    try:
                        obj = json.loads(fr["value"])
                    except ValueError:
                        continue
                    if isinstance(obj, dict):
                        for key, value in obj.items():
                            if isinstance(value, str):
                                found_priv.add((fr["owner"], key, value))
    missing = [e for e in expected if e not in found]
    for desc, value in missing:
        print(f"MISSING TXXX {desc}={value}", file=sys.stderr)
    missing_priv = [e for e in expected_priv if e not in found_priv]
    for owner, key, value in missing_priv:
        print(f"MISSING PRIV {owner} {key}={value}", file=sys.stderr)
    if errors:
        print(f"{errors} ID3 packet(s) could not be parsed", file=sys.stderr)
        return 2
    return 1 if missing or missing_priv else 0


if __name__ == "__main__":
    sys.exit(main())
