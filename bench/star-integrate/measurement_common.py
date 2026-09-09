"""Small, fail-closed helpers shared by host-only STAR measurement runners."""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time


STAR_FLAGS = ("STAR_INTEGRATE", "STAR_INTEGRATE_STRICT", "STAR_INTEGRATE_SIDECAR",
              "STAR_INTEGRATE_THP", "STAR_INTEGRATE_DROP_INDEX_CACHE",
              "STAR_INTEGRATE_COLLAPSE_INDEX")


def identity(path, hash_file=False):
    path = Path(path)
    result = {"path": str(path), "bytes": path.stat().st_size}
    if hash_file:
        digest = hashlib.sha256()
        with path.open("rb") as stream:
            for block in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(block)
        result["sha256"] = digest.hexdigest()
    return result


def executable_identity(path):
    """Executable identity is never optional; input hashing is a separate choice."""
    return identity(path, True)


def fresh_dir(path):
    path = Path(path)
    if path.exists():
        raise ValueError("output already exists; refusing to overwrite evidence: " + str(path))
    path.mkdir(parents=True)
    return path


def load_base_argv(path):
    value = json.loads(Path(path).read_text())
    if not isinstance(value, list) or not all(isinstance(x, str) for x in value):
        raise ValueError("base argv must be a JSON array of strings")
    return value


def replace_option(argv, option, values, required=True):
    positions = [i for i, value in enumerate(argv) if value == option]
    if len(positions) != 1:
        if not positions and not required:
            return list(argv)
        raise ValueError("expected exactly one " + option)
    i = positions[0]
    if i + len(values) >= len(argv):
        raise ValueError("truncated " + option)
    result = list(argv)
    result[i + 1:i + 1 + len(values)] = [str(x) for x in values]
    return result


def remove_option(argv, option, values=1):
    positions = [i for i, value in enumerate(argv) if value == option]
    if len(positions) > 1:
        raise ValueError("ambiguous " + option)
    result = list(argv)
    if positions:
        i = positions[0]
        if i + values >= len(argv):
            raise ValueError("truncated " + option)
        del result[i:i + values + 1]
    return result


def star_argv(base, executable, mate1, mate2, prefix):
    argv = list(base)
    if argv and not argv[0].startswith("-"):
        argv[0] = str(executable)
    else:
        argv.insert(0, str(executable))
    argv = replace_option(argv, "--readFilesIn", (mate1, mate2))
    argv = replace_option(argv, "--outFileNamePrefix", (str(prefix) + "/",))
    return argv


def clean_env(extra=None):
    env = dict(os.environ)
    # New integration knobs must not silently leak into an arm as the C++ code grows.
    for name in tuple(env):
        if name.startswith("STAR_INTEGRATE"):
            env.pop(name, None)
    if extra:
        env.update({key: str(value) for key, value in extra.items()})
    return env


def verify_lock(lock_path):
    """Acquire the shared flock, accepting only a genuine inherited fd 9."""
    lock_path = Path(lock_path)
    try:
        inherited = os.fstat(9)
        expected = lock_path.stat()
        if inherited.st_dev == expected.st_dev and inherited.st_ino == expected.st_ino:
            fcntl.flock(9, fcntl.LOCK_EX | fcntl.LOCK_NB)
            return None
    except (OSError, FileNotFoundError):
        pass
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    handle = lock_path.open("a+")
    fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
    return handle


def run_stage(root, label, argv, env, timeout_s, cwd=None):
    """Run one tree under external timeout and retain all evidence."""
    stage = Path(root) / "stages"
    stage.mkdir(exist_ok=True)
    (stage / (label + ".argv.json")).write_text(json.dumps(argv, indent=2) + "\n")
    (stage / (label + ".env.json")).write_text(json.dumps({k: env[k] for k in STAR_FLAGS if k in env}, indent=2) + "\n")
    time_file = stage / (label + ".time.tsv")
    if timeout_s <= 0:
        raise ValueError("timeout must be positive")
    timeout_bin = shutil.which("timeout")
    if timeout_bin is None:
        started = time.monotonic()
        try:
            with (stage / (label + ".stdout")).open("w") as stdout, (stage / (label + ".stderr")).open("w") as stderr:
                completed = subprocess.run(argv, cwd=cwd, env=env, stdout=stdout, stderr=stderr, timeout=timeout_s)
        except subprocess.TimeoutExpired:
            completed = subprocess.CompletedProcess(argv, 124)
        time_file.write_text(f"{time.monotonic() - started:.6f}\tNA\tNA\tNA\t{completed.returncode}\n")
        (stage / (label + ".exit")).write_text(str(completed.returncode) + "\n")
        if completed.returncode:
            raise RuntimeError(label + " failed (exit " + str(completed.returncode) + ")")
        return completed
    time_bin = shutil.which("gtime") or "/usr/bin/time"
    try:
        subprocess.run([time_bin, "--version"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
        time_args = ["-f", "%e\t%U\t%S\t%M\t%x", "-o", str(time_file)]
    except (OSError, subprocess.CalledProcessError):
        # macOS ships BSD time; retain a real wall measurement there, while marking
        # unavailable GNU fields instead of pretending they were measured.
        time_args = ["-p", "-o", str(time_file)]
    command = [time_bin, *time_args, timeout_bin, "--signal=TERM", "--kill-after=30s", str(timeout_s) + "s", *argv]
    with (stage / (label + ".stdout")).open("w") as stdout, (stage / (label + ".stderr")).open("w") as stderr:
        completed = subprocess.run(command, cwd=cwd, env=env, stdout=stdout, stderr=stderr)
    (stage / (label + ".exit")).write_text(str(completed.returncode) + "\n")
    if completed.returncode:
        raise RuntimeError(label + " failed (exit " + str(completed.returncode) + ")")
    return completed
