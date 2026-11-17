#!/usr/bin/env python3
"""Measure release launch to first frame, memory and 60s idle CPU.

The one-shot app metric is opt-in; it writes an elapsed time, never source data.
Processes are owned by this script and terminated after each sample.
"""
import argparse
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("binary", type=Path)
parser.add_argument("fixture", type=Path, nargs="?", help="omit to measure an empty welcome screen")
args = parser.parse_args()
command = [str(args.binary.resolve())]
if args.fixture is not None:
    command.append(str(args.fixture.resolve()))
samples = []
frame_samples = []


def cpu_seconds(pid):
    value = subprocess.check_output(["ps", "-p", str(pid), "-o", "time="], text=True).strip()
    minutes, seconds = value.split(":")
    return int(minutes) * 60 + float(seconds)


def footprint_mib(pid):
    summary = subprocess.check_output(["vmmap", "-summary", str(pid)], text=True)
    value, unit = re.search(r"^Physical footprint:\s+([\d.]+)\s*([KMG])", summary, re.M).groups()
    return float(value) * {"K": 1 / 1024, "M": 1, "G": 1024}[unit]


with tempfile.TemporaryDirectory(prefix="dotvar-metrics-") as directory:
    for index in range(10):
        metric = Path(directory) / f"{index}.txt"
        environment = dict(os.environ, DOTVAR_STARTUP_METRIC=str(metric),
                           DOTVAR_CONFIG_DIR=str(Path(directory) / "catalog"))
        started = time.monotonic()
        process = subprocess.Popen(command, env=environment,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            deadline = started + 20
            while not metric.exists() or not metric.read_text().strip():
                if process.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError("App exited or did not render its first ready frame within 20s")
                time.sleep(0.005)
            frame_samples.append(int(metric.read_text()) / 1000)
            samples.append((time.monotonic() - started) * 1000)
            if index == 9:
                time.sleep(2)
                rss = int(subprocess.check_output(["ps", "-p", str(process.pid), "-o", "rss="], text=True))
                footprint = footprint_mib(process.pid)
                before = cpu_seconds(process.pid)
                idle_start = time.monotonic()
                time.sleep(60)
                idle_cpu = (cpu_seconds(process.pid) - before) / (time.monotonic() - idle_start) * 100
        finally:
            process.terminate()
            process.wait(timeout=10)
print(json.dumps({
    "launch_to_metric_ms": samples,
    "launch_median_ms": statistics.median(samples),
    "main_to_ready_frame_ms": frame_samples,
    "render_median_ms": statistics.median(frame_samples),
    "idle_rss_mib": rss / 1024,
    "idle_physical_footprint_mib": footprint,
    "idle_cpu_percent_one_core_60s": idle_cpu,
}, indent=2))
