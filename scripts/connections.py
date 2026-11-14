"""Every connection the app and its webviews' processes made, from a trace by `strace -f`, and whom each belongs to.

Directly, the app may reach Proton, Quad9 and, when it is turned on, Gravatar, and nothing else. Through Tor it may
reach Tor's relays and nothing else: a connection to Proton or Quad9 then went around Tor. Any lookup on port 53 went
around Quad9's DNS over HTTPS. The owner of an address comes from RIPEstat, and Tor's relays from Onionoo, both open.
Prints a table by owner, and stops the run on anything not allowed.

    python3 -I scripts/connections.py <trace> --way direct|tor [--gravatar]
"""

import argparse
import ipaddress
import json
import re
import sys
import urllib.parse
import urllib.request
from collections import defaultdict

ADDRESS = re.compile(
    r'(?:connect|sendto|sendmsg)\(.*?sa_family=AF_INET6?,\s*sin6?_port=htons\((\d+)\).*?'
    r'(?:sin_addr=inet_addr\("([0-9.]+)"\)|inet_pton\(AF_INET6,\s*"([0-9a-fA-F:.]+)")'
)
# Autonomous systems, by the words in their holders' names, and whom each stands for.
PROTON, QUAD9, GRAVATAR = "Proton", "Quad9", "Gravatar"
OWNERS = {PROTON: ("PROTON",), QUAD9: ("QUAD9",), GRAVATAR: ("AUTOMATTIC",)}
AGENT = {"User-Agent": "pm-light-connection-check"}


def fetch(url):
    with urllib.request.urlopen(urllib.request.Request(url, headers=AGENT), timeout=60) as answer:
        return json.load(answer)


def destinations(trace):
    found = defaultdict(set)
    with open(trace, encoding="utf-8", errors="replace") as lines:
        for line in lines:
            match = ADDRESS.search(line)
            if match:
                port, four, six = match.groups()
                address = ipaddress.ip_address(four or six)
                if getattr(address, "ipv4_mapped", None):
                    address = address.ipv4_mapped
                found[address].add(int(port))
    return found


def holder(address, asns):
    info = fetch("https://stat.ripe.net/data/network-info/data.json?resource=" + urllib.parse.quote(str(address)))
    numbers = info.get("data", {}).get("asns", [])
    if not numbers:
        return "no autonomous system"
    number = numbers[0]
    if number not in asns:
        overview = fetch(f"https://stat.ripe.net/data/as-overview/data.json?resource=AS{number}")
        asns[number] = f"AS{number} {overview.get('data', {}).get('holder', '')}"
    return asns[number]


def relays():
    answer = fetch("https://onionoo.torproject.org/details?type=relay&fields=or_addresses,dir_address")
    known = set()
    for relay in answer.get("relays", []):
        for place in relay.get("or_addresses", []) + [relay.get("dir_address") or ""]:
            host = place.rsplit(":", 1)[0].strip("[]")
            if host:
                known.add(ipaddress.ip_address(host))
    return known


def owner_of(name):
    for owner, words in OWNERS.items():
        if any(word in name.upper() for word in words):
            return owner
    return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("trace")
    parser.add_argument("--way", choices=["direct", "tor"], required=True)
    parser.add_argument("--gravatar", action="store_true")
    arguments = parser.parse_args()
    found = destinations(arguments.trace)
    allowed = {PROTON, QUAD9} | ({GRAVATAR} if arguments.gravatar else set())
    tor = relays() if arguments.way == "tor" else set()
    asns, table, problems = {}, defaultdict(lambda: [0, set()]), []
    for address, ports in sorted(found.items(), key=lambda item: (item[0].version, item[0])):
        if 53 in ports:
            problems.append(f"a DNS lookup to {address}, around Quad9's DNS over HTTPS")
        if address.is_loopback or address.is_private or address.is_link_local:
            name, verdict = "this machine or its network", "local"
            if ports - {53}:
                problems.append(f"a connection to {address} on this machine, ports {sorted(ports)}")
        elif arguments.way == "tor":
            name = "a Tor relay" if address in tor else holder(address, asns)
            verdict = "allowed" if address in tor else "around Tor"
            if address not in tor:
                problems.append(f"{address} ({name}), not a Tor relay: it went around Tor")
        else:
            name = holder(address, asns)
            owner = owner_of(name)
            verdict = "allowed" if owner in allowed else "not allowed"
            if owner not in allowed:
                problems.append(f"{address} ({name}), which is none of {sorted(allowed)}")
        row = table[(name, verdict)]
        row[0] += 1
        row[1] |= ports
    print(f"{len(found)} addresses reached, the way {arguments.way}")
    for (name, verdict), (count, ports) in sorted(table.items()):
        print(f"  {verdict:<12} {name}: {count} addresses, ports {sorted(ports)}")
    for problem in problems:
        print(f"::error::{problem}")
    if not found:
        print("::error::no connection was traced at all: the trace saw nothing of the app")
        return 1
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
