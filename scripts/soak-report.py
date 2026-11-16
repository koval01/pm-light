"""Judges what the long run wrote (app/src/soak.rs): memory that keeps growing, the program's own and with its
webviews, processes that are never let go, a flood of requests, the same request asked again within a second with no
change between, and connections of the app's pages stopped at the door to nowhere. Prints the figures and stops the run
on what is clearly wrong; what is only worth a look becomes a warning on the run's page. Numbers and paths without ids
only.

    python3 -I scripts/soak-report.py <file>.soak
"""

import sys

# Growth of the memory from the run's settled start to its end, in MB: a warning past the first, a failure past the
# second, which no cache explains.
GROWTH_WARNING = 50
GROWTH_FAILURE = 150
# More processes at the end than settled, on average: a webview that is never let go keeps its own.
PROCESSES_WARNING = 2
# Requests in the busiest minute: sync asks every 30 seconds, and a person's clicks a few more.
MINUTE_WARNING = 100
MINUTE_FAILURE = 300
# The same request again within a second, in the whole run.
REPEATS_FAILURE = 100


def read(path):
    values, lines = {}, []
    with open(path, encoding="utf-8") as report:
        for line in report:
            line = line.rstrip("\n")
            lines.append(line)
            key, _, value = line.rpartition("=")
            values.setdefault(key, []).append(value)
    return values, lines


def mean(numbers):
    return sum(numbers) / len(numbers) if numbers else 0.0


def figures(values, key):
    return [int(n) for n in values.get(key, [""])[0].split(",") if n]


def settled_and_last(numbers):
    """The first quarter fills caches and pages; the last quarter is where a leak shows."""
    quarter = max(len(numbers) // 4, 2)
    return mean(numbers[quarter : 2 * quarter]), mean(numbers[-quarter:])


def judge_memory(values, key, what, problems, warnings):
    memory = figures(values, key)
    if not memory:
        return
    if len(memory) < 8:
        warnings.append(f"too few minutes to judge {what}")
        return
    settled, last = settled_and_last(memory)
    growth = last - settled
    print(f"{what}: {settled:.0f} MB settled, {last:.0f} MB at the end, {growth:+.0f} MB")
    if growth > GROWTH_FAILURE:
        problems.append(f"{what} grew by {growth:.0f} MB")
    elif growth > GROWTH_WARNING:
        warnings.append(f"{what} grew by {growth:.0f} MB")


def main():
    values, lines = read(sys.argv[1])
    print("\n".join(lines))
    problems, warnings = [], []
    if not figures(values, "memory MB"):
        warnings.append("too few minutes to judge the memory")
    judge_memory(values, "memory MB", "memory", problems, warnings)
    # The program's own part of it: a leak in the program shows here, a webview's caches only in the whole.
    judge_memory(values, "own memory MB", "the program's own memory", problems, warnings)
    processes = figures(values, "processes")
    if len(processes) >= 8:
        settled, last = settled_and_last(processes)
        print(f"processes: {settled:.1f} settled, {last:.1f} at the end, {max(processes)} at most")
        if last - settled > PROCESSES_WARNING:
            warnings.append(f"the processes counted grew from {settled:.1f} to {last:.1f}")
    busiest = int(values.get("most in a minute", ["0"])[0])
    if busiest > MINUTE_FAILURE:
        problems.append(f"{busiest} requests in one minute")
    elif busiest > MINUTE_WARNING:
        warnings.append(f"{busiest} requests in one minute")
    repeats = sum(int(v) for k, vs in values.items() if k.startswith("repeated ") for v in vs)
    if repeats > REPEATS_FAILURE:
        problems.append(f"{repeats} requests asked again within a second")
    elif repeats:
        warnings.append(f"{repeats} requests asked again within a second")
    # A page of the app reached for the network on its own, and the door to nowhere stopped it (app/src/nowhere.rs).
    turned_away = int(values.get("turned away at the door to nowhere", ["0"])[0])
    if turned_away:
        warnings.append(f"{turned_away} connections of the app's pages were turned away at the door to nowhere")
    for warning in warnings:
        print(f"::warning::{warning}")
    for problem in problems:
        print(f"::error::{problem}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
