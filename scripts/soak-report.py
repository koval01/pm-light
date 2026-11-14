"""Judges what the long run wrote (app/src/soak.rs): memory that keeps growing, a flood of requests, and the same
request asked again within a second. Prints the figures and stops the run on what is clearly wrong; what is only
worth a look becomes a warning on the run's page. Numbers and paths without ids only.

    python3 -I scripts/soak-report.py <file>.soak
"""

import sys

# Growth of the memory from the run's settled start to its end, in MB: a warning past the first, a failure past the
# second, which no cache explains.
GROWTH_WARNING = 50
GROWTH_FAILURE = 150
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


def main():
    values, lines = read(sys.argv[1])
    print("\n".join(lines))
    memory = [int(n) for n in values.get("memory MB", [""])[0].split(",") if n]
    problems, warnings = [], []
    if len(memory) >= 8:
        # The first quarter fills caches and pages; the last quarter is where a leak shows.
        quarter = max(len(memory) // 4, 2)
        settled, last = mean(memory[quarter : 2 * quarter]), mean(memory[-quarter:])
        growth = last - settled
        print(f"memory: {settled:.0f} MB settled, {last:.0f} MB at the end, {growth:+.0f} MB")
        if growth > GROWTH_FAILURE:
            problems.append(f"memory grew by {growth:.0f} MB")
        elif growth > GROWTH_WARNING:
            warnings.append(f"memory grew by {growth:.0f} MB")
    else:
        warnings.append("too few minutes to judge the memory")
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
    for warning in warnings:
        print(f"::warning::{warning}")
    for problem in problems:
        print(f"::error::{problem}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
