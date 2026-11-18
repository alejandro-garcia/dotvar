# Resource usage when editing `.env` files

This benchmark ran on November 17, 2026, with **Dotvar 0.1.0** and **Visual Studio Code 1.146.0**. Both apps ran as arm64 builds from their installed bundles in `/Applications`. The test machine was an M2 Mac mini (Mac14,3, 8 cores, 24 GiB) running macOS 27.0.1 (26A434) and connected to power.

## Results

Across both file sizes, Dotvar used **93% less memory at idle** and reached a **92% lower memory peak during editing**. For this comparison, VS Code ran with a clean profile, user extensions disabled, and built-in AI off.

| File | App | Idle memory | Peak memory during editing | Idle CPU | Total CPU time for three edits | Idle processes |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 200 variables | Dotvar | 56.2 MiB | 69.8 MiB | 0.0004% | 0.030 s | 1 |
| 200 variables | VS Code | 805.2 MiB | 891.0 MiB | 0.0039% | 0.065 s | 7 |
| 5,000 variables | Dotvar | 58.8 MiB | 73.8 MiB | 0.0005% | 0.033 s | 1 |
| 5,000 variables | VS Code | 849.6 MiB | 883.9 MiB | 0.0041% | 0.066 s | 7 |

Each measurement is the median of three independent runs. The peak for each run is the highest memory sample recorded during editing. Idle CPU is expressed as a percentage of one core. CPU time during editing is the sum of user and system time.

Across three search, edit, and save cycles, Dotvar used **54% less CPU time** with 200 variables and **50% less** with 5,000. Idle CPU usage was very low for both apps.

| Installed bundle disk usage | Dotvar | VS Code | Reduction |
| --- | ---: | ---: | ---: |
| Allocated disk space measured with `du -sk` | 29.9 MiB | 1,004.9 MiB | 97.0% |

### VS Code with built-in AI enabled

Across three runs with 200 variables and AI enabled, VS Code used 944.0 MiB at idle, peaked at 1,004.0 MiB during editing, used 0.0108% of one CPU core at idle, and accumulated 0.070 s of CPU time across the three edits. The median idle process count was 10. These runs used the same temporary setup and ran without signing in.

## Methodology

1. Open a single synthetic `.env` file with **200 variables** (6,624 bytes) or **5,000 variables** (165,024 bytes) in a 1,281 × 832 point window. Start a new process with a temporary profile for each run. Alternate apps and file sizes while leaving system caches in place.
2. Hide the app with the file open, wait 7 seconds, and sample for 12 seconds without interaction. Record median memory usage and the increase in CPU time over that interval.
3. Bring the app to the foreground. Use ⌘F to find the keys at the 25%, 50%, and 75% marks in the file, replace each value, and save with ⌘S after each change. In Dotvar, reveal only the matching value. In VS Code, close search and select the value in the text editor.
4. Compare the entire file with the expected bytes after each save. The three edits are cumulative. Sample the editing phase from the moment the window returns to the foreground until 350 ms after verifying the final save.
5. Sample every 250 ms. Read `ri_phys_footprint`, `ri_user_time`, and `ri_system_time` for each PID in the app's process tree using `proc_pid_rusage`. At each sample, sum the physical memory footprints of all processes and track their combined increase in CPU time. Include VS Code's helper processes. Report the total physical memory footprint in MiB.

Dotvar used a temporary catalog set through `DOTVAR_CONFIG_DIR` and kept values hidden by default. VS Code used temporary directories via `--user-data-dir`, `--extensions-dir`, and `--shared-data-dir`, along with `--disable-extensions`. Telemetry, updates, and experiments were disabled, and welcome, restore, and workspace trust prompts were skipped. The main comparison also set `"chat.disableAIFeatures": true`.

The memory and CPU figures cover the measured processes on this Mac with one window and one file open, while the other apps in the session remained open. Disk usage covers each installed app bundle. Editing measurements include searching, selecting, typing, saving, and pauses in the automation.

### Test file

To regenerate the initial file in a temporary directory, use `count = 200` or `count = 5000`.

```python
from pathlib import Path
from tempfile import mkdtemp

count = 200
fixture = "# Synthetic environment\n" + "".join(
    f"BENCH_{index:05d}=value_{index:05d}_baseline\n"
    for index in range(1, count + 1)
)
path = Path(mkdtemp(prefix="dotenv-benchmark-")) / ".env"
path.write_bytes(fixture.encode("utf-8"))
print(path)
```

After editing, the expected value for each of the three keys ends in `_changedx`.

### Individual runs

All 45 saves across the 15 valid runs matched the expected file contents. Interrupted runs and runs that failed verification were repeated.

| File | App | Run | Idle memory (MiB) | Peak memory during editing (MiB) | Idle CPU (% of one core) | CPU time during editing (s) | Idle processes |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 200 | Dotvar | 1 | 56.2 | 69.9 | 0.0004 | 0.029 | 1 |
| 200 | Dotvar | 2 | 51.3 | 69.8 | 0.0004 | 0.030 | 1 |
| 200 | Dotvar | 3 | 56.7 | 69.4 | 0.0005 | 0.031 | 1 |
| 200 | VS Code | 1 | 636.1 | 770.1 | 0.0053 | 0.062 | 7 |
| 200 | VS Code | 2 | 819.1 | 930.7 | 0.0039 | 0.067 | 7 |
| 200 | VS Code | 3 | 805.2 | 891.0 | 0.0039 | 0.065 | 7 |
| 200 | VS Code (AI enabled) | 1 | 820.5 | 946.8 | 0.0108 | 0.070 | 10 |
| 200 | VS Code (AI enabled) | 2 | 944.0 | 1,004.0 | 0.0107 | 0.081 | 10 |
| 200 | VS Code (AI enabled) | 3 | 989.0 | 1,053.9 | 0.0111 | 0.070 | 10 |
| 5,000 | Dotvar | 1 | 52.7 | 72.9 | 0.0006 | 0.032 | 1 |
| 5,000 | Dotvar | 2 | 58.9 | 73.8 | 0.0005 | 0.033 | 1 |
| 5,000 | Dotvar | 3 | 58.8 | 73.9 | 0.0004 | 0.033 | 1 |
| 5,000 | VS Code | 1 | 830.2 | 925.6 | 0.0041 | 0.079 | 7 |
| 5,000 | VS Code | 2 | 849.6 | 872.4 | 0.0041 | 0.066 | 7 |
| 5,000 | VS Code | 3 | 855.8 | 883.9 | 0.0041 | 0.066 | 7 |
