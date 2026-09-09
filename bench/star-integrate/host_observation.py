"""Low-overhead, fail-closed Linux observations for a measured target process."""

import json
import os
from pathlib import Path
import threading
import time

PROCESS_FIELDS = ("VmRSS", "VmHWM", "VmSwap")
HOST_MEM_FIELDS = ("MemAvailable", "Cached", "SwapFree")
VMSTAT_FIELDS = ("pswpin", "pswpout", "pgmajfault")


def parse_key_values(text, wanted):
    """Parse procfs-style ``key: value`` or ``key value`` rows without coercion."""
    result = {}
    for line in text.splitlines():
        key, separator, rest = line.partition(":")
        if not separator:
            key, _, rest = line.partition(" ")
        key = key.strip()
        fields = rest.split()
        if key in wanted and fields:
            try:
                result[key] = int(fields[0])
            except ValueError:
                continue
    return result


def proc_snapshot(proc_root, pid):
    proc = Path(proc_root) / str(pid)
    return {
        "pid": pid,
        "status": parse_key_values((proc / "status").read_text(), PROCESS_FIELDS),
        "io": parse_key_values(
            (proc / "io").read_text(), ("read_bytes", "write_bytes", "syscr", "syscw")
        ),
    }


def host_snapshot(proc_root):
    root = Path(proc_root)
    return {
        "meminfo": parse_key_values((root / "meminfo").read_text(), HOST_MEM_FIELDS),
        "vmstat": parse_key_values((root / "vmstat").read_text(), VMSTAT_FIELDS),
        "scope": "system-wide",
    }


class HostObservation:
    """Watch for an actual executable, writing raw JSONL as samples arrive."""

    def __init__(self, directory, executable, interval_s=1.0, proc_root="/proc"):
        self.directory = Path(directory)
        self.executable = Path(executable).resolve()
        self.interval_s = interval_s
        self.proc_root = Path(proc_root)
        self.path = self.directory / "host-observation.jsonl"
        self.pid = None
        self.samples_written = 0
        self._stop = threading.Event()
        self._thread = None
        self._unavailable = not (self.proc_root / "self" / "exe").exists()

    def _write(self, row):
        row["timestamp_unix_s"] = time.time()
        with self.path.open("a") as stream:
            stream.write(json.dumps(row, sort_keys=True) + "\n")
            stream.flush()

    def _find_target(self):
        for candidate in self.proc_root.iterdir():
            if not candidate.name.isdigit():
                continue
            try:
                if (candidate / "exe").resolve() == self.executable:
                    return int(candidate.name)
            except OSError:
                pass
        return None

    def start(self):
        if self._unavailable:
            self._write(
                {"phase": "before", "available": False, "reason": "procfs unavailable"}
            )
            return
        self._write(
            {
                "phase": "before",
                "available": True,
                "host": host_snapshot(self.proc_root),
            }
        )
        self._thread = threading.Thread(target=self._sample, daemon=True)
        self._thread.start()

    def _sample(self):
        while not self._stop.is_set():
            if self.pid is None:
                self.pid = self._find_target()
            if self.pid is not None:
                try:
                    if (self.proc_root / str(self.pid) / "exe").resolve() != self.executable:
                        self._stop.wait(self.interval_s)
                        continue
                    self._write(
                        {
                            "phase": "during",
                            "available": True,
                            "target_exe": str(self.executable),
                            "process": proc_snapshot(self.proc_root, self.pid),
                            "host": host_snapshot(self.proc_root),
                        }
                    )
                    self.samples_written += 1
                except OSError:
                    pass
            self._stop.wait(self.interval_s)

    def finish(self):
        self._stop.set()
        if self._thread is not None:
            self._thread.join(timeout=self.interval_s + 1)
        if self._unavailable:
            self._write(
                {"phase": "after", "available": False, "reason": "procfs unavailable"}
            )
            return {"available": False, "target_sampled": False}
        self._write(
            {"phase": "after", "available": True, "host": host_snapshot(self.proc_root)}
        )
        if self.samples_written == 0:
            raise RuntimeError(
                "target process was never sampled; refusing to assert host memory state"
            )
        return {"available": True, "target_sampled": True, "pid": self.pid,
                "samples_written": self.samples_written}
