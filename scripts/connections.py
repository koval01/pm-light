"""Every connection the app and its webviews' processes made, from a trace by `strace -f`, and whom each belongs to.

Directly, the app may reach Proton, Quad9 and, when it is turned on, Gravatar, and nothing else. Through Tor it may
reach Tor's relays and nothing else: a connection to Proton or Quad9 then went around Tor. Tor's relays are those
Onionoo lists now, and the fallback directories arti carries built in (tor-dircommon's data/fallback_dirs.rs, given with
--fallbacks), which it asks for the network's directory at the start: one may have left the network since arti's
release, and still be the address and port it was built with. Any lookup on port 53 went
around Quad9's DNS over HTTPS. The owner of an address comes from RIPEstat, and Tor's relays from Onionoo, both open.
Prints a table by owner, and stops the run on anything not allowed.

On this machine the app may reach only itself: under Tor its clients and its webviews talk to its own SOCKS door on
the loopback interface. A port is the app's own when the trace shows one of its sockets holding it (`getsockname`,
which the app calls on the door it opens); any other local port is another program, and stops the run.

    python3 -I scripts/connections.py <trace> --way direct|tor [--gravatar] [--fallbacks <fallback_dirs.rs>]
    python3 -I scripts/connections.py --self-test

The self-test proves the rules on made-up traces, with no network, and runs before every real check.
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
# What a traced process's own socket holds, as `getsockname` answers, whole or resumed after another call.
OWN = re.compile(
    r'getsockname(?:\(\d+,\s*| resumed>\s*)\{sa_family=AF_INET6?,\s*sin6?_port=htons\((\d+)\).*?'
    r'(?:sin_addr=inet_addr\("([0-9.]+)"\)|inet_pton\(AF_INET6,\s*"([0-9a-fA-F:.]+)").*\)\s*=\s*0\s*$'
)
# Autonomous systems, by the words in their holders' names, and whom each stands for.
PROTON, QUAD9, GRAVATAR = "Proton", "Quad9", "Gravatar"
OWNERS = {PROTON: ("PROTON",), QUAD9: ("QUAD9",), GRAVATAR: ("AUTOMATTIC",)}
AGENT = {"User-Agent": "pm-light-connection-check"}
# The only places this asks, both open: RIPEstat for whom an address belongs to, Onionoo for Tor's relays.
ASKED = ("https://stat.ripe.net/data/", "https://onionoo.torproject.org/")


class NotAsked(Exception):
    """A place outside ASKED."""


def asks_only_known(url):
    return isinstance(url, str) and url.startswith(ASKED)


def fetch(url):
    if not asks_only_known(url):
        raise NotAsked(url)
    # Over HTTPS, and only to the two places in ASKED: the trace gives no more than an address, parsed as one.
    # nosemgrep: python.lang.security.audit.dynamic-urllib-use-detected.dynamic-urllib-use-detected
    with urllib.request.urlopen(urllib.request.Request(url, headers=AGENT), timeout=60) as answer:
        return json.load(answer)


def address_of(four, six):
    address = ipaddress.ip_address(four or six)
    return getattr(address, "ipv4_mapped", None) or address


def destinations(lines):
    """Where connections went, and the loopback ports the traced processes' own sockets held."""
    found, own = defaultdict(set), set()
    for line in lines:
        match = ADDRESS.search(line)
        if match:
            port, four, six = match.groups()
            found[address_of(four, six)].add(int(port))
        held = OWN.search(line)
        if held:
            port, four, six = held.groups()
            if address_of(four, six).is_loopback and int(port) != 0:
                own.add(int(port))
    return found, own


def as_number(value):
    """An autonomous system's number as RIPEstat gives it, or nothing for anything else."""
    text = str(value)
    return int(text) if text.isascii() and text.isdigit() and len(text) <= 10 else None


def holder(address, asns, get=None):
    get = get or fetch
    info = get("https://stat.ripe.net/data/network-info/data.json?resource=" + urllib.parse.quote(str(address)))
    numbers = info.get("data", {}).get("asns", [])
    number = as_number(numbers[0]) if numbers else None
    if number is None:
        return "no autonomous system"
    if number not in asns:
        overview = get(f"https://stat.ripe.net/data/as-overview/data.json?resource=AS{number}")
        asns[number] = f"AS{number} {overview.get('data', {}).get('holder', '')}"
    return asns[number]


FALLBACK = re.compile(r'"\[?([0-9a-fA-F:.]+?)\]?:(\d+)"')


def fallbacks_of_text(text):
    found = set()
    for host, port in FALLBACK.findall(text):
        try:
            found.add((ipaddress.ip_address(host), int(port)))
        except ValueError:
            continue
    return found


def fallbacks(path):
    """The address and port of each fallback directory arti was built with."""
    if not path:
        return set()
    with open(path, encoding="utf-8") as source:
        return fallbacks_of_text(source.read())


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


def judge(found, own, way, allowed, tor, name_of, built_in=frozenset()):
    """A table of whom the connections reached, by owner and verdict, and what is not allowed."""
    table, problems = defaultdict(lambda: [0, set()]), []
    for address, ports in sorted(found.items(), key=lambda item: (item[0].version, item[0])):
        if 53 in ports:
            problems.append(f"a DNS lookup to {address}, around Quad9's DNS over HTTPS")
        if address.is_loopback and ports <= own:
            name, verdict = "the app's own door on this machine", "own"
        elif address.is_loopback or address.is_private or address.is_link_local:
            name, verdict = "this machine or its network", "local"
            others = ports - own if address.is_loopback else ports
            if others - {53}:
                problems.append(f"a connection to {address} on this machine, ports {sorted(others)}, not the app's own")
        elif way == "tor" and address in tor:
            name, verdict = "a Tor relay", "allowed"
        elif way == "tor" and all((address, port) in built_in for port in ports):
            name, verdict = "a fallback directory built into arti", "allowed"
        elif way == "tor":
            name, verdict = name_of(address), "around Tor"
            problems.append(f"{address} ({name}), not a Tor relay: it went around Tor")
        else:
            name = name_of(address)
            owner = owner_of(name)
            verdict = "allowed" if owner in allowed else "not allowed"
            if owner not in allowed:
                problems.append(f"{address} ({name}), which is none of {sorted(allowed)}")
        row = table[(name, verdict)]
        row[0] += 1
        row[1] |= ports
    return table, problems


def held(port, address="127.0.0.1", answer="0"):
    return f'5169  getsockname(27, {{sa_family=AF_INET, sin_port=htons({port}), sin_addr=inet_addr("{address}")}}, [128 => 16]) = {answer}'


def reached(port, address="127.0.0.1", call="connect"):
    return f'5170  {call}(28, {{sa_family=AF_INET, sin_port=htons({port}), sin_addr=inet_addr("{address}")}}, 16) = -1 EINPROGRESS'


def reached_six(port, address):
    return (
        f"5170  connect(9, {{sa_family=AF_INET6, sin6_port=htons({port}), sin6_flowinfo=htonl(0), "
        f'inet_pton(AF_INET6, "{address}", &sin6_addr), sin6_scope_id=0}}, 28) = 0'
    )


def self_test():
    """Each made-up trace, the way, Gravatar allowed or not, and the verdict its one address must get."""
    six_held = (
        "5169  getsockname(5, {sa_family=AF_INET6, sin6_port=htons(4000), sin6_flowinfo=htonl(0), "
        'inet_pton(AF_INET6, "::1", &sin6_addr), sin6_scope_id=0}, [128 => 28]) = 0'
    )
    resumed = '5169  <... getsockname resumed>{sa_family=AF_INET, sin_port=htons(39071), sin_addr=inet_addr("127.0.0.1")}, [128 => 16]) = 0'
    relay, proton, google, gravatar, fallback = "95.216.10.20", "185.70.42.37", "142.250.0.1", "192.0.73.2", "91.39.70.41"
    owners = {proton: "AS62371 PROTON Proton AG", google: "AS15169 GOOGLE", gravatar: "AS2635 AUTOMATTIC"}
    cases = [
        ("the app's own door, held and reached", [held(39071), reached(39071)], "tor", False, "own"),
        ("a door held in a call resumed later", [resumed, reached(39071)], "tor", False, "own"),
        ("the own door over IPv6", [six_held, reached_six(4000, "::1")], "tor", False, "own"),
        ("a local port the app never held", [reached(39071)], "tor", False, "local"),
        ("a local port beside the one held", [held(39071), reached(39071), reached(9050)], "tor", False, "local"),
        ("a getsockname that failed", [held(39071, answer="-1 EBADF (Bad file descriptor)"), reached(39071)], "tor", False, "local"),
        ("a port held on every address, not loopback", [held(39071, "0.0.0.0"), reached(39071)], "tor", False, "local"),
        ("the system's resolver", [reached(53, "127.0.0.53", "sendto")], "direct", False, "local"),
        ("a lookup to a server's port 53", [reached(53, proton, "sendto")], "direct", False, "allowed"),
        ("the home network", [reached(80, "192.168.1.1")], "direct", False, "local"),
        ("a Tor relay", [reached(9001, relay)], "tor", False, "allowed"),
        ("a fallback directory built into arti", [reached(9001, fallback)], "tor", False, "allowed"),
        ("a fallback directory's address on another port", [reached(443, fallback)], "tor", False, "around Tor"),
        ("Proton reached around Tor", [reached(443, proton)], "tor", False, "around Tor"),
        ("Proton reached directly", [reached(443, proton)], "direct", False, "allowed"),
        ("someone else reached directly", [reached(443, google)], "direct", False, "not allowed"),
        ("Gravatar while it is off", [reached(443, gravatar)], "direct", False, "not allowed"),
        ("Gravatar while it is on", [reached(443, gravatar)], "direct", True, "allowed"),
    ]
    # Whether each must stop the run: all but these.
    passing = {
        "the app's own door, held and reached",
        "a door held in a call resumed later",
        "the own door over IPv6",
        "a Tor relay",
        "a fallback directory built into arti",
        "Proton reached directly",
        "Gravatar while it is on",
    }
    places = [
        ("https://stat.ripe.net/data/as-overview/data.json?resource=AS3", True),
        ("https://onionoo.torproject.org/details?type=relay", True),
        ("file:///etc/passwd", False),
        ("http://stat.ripe.net/data/", False),
        ("https://stat.ripe.net.example.com/data/", False),
        ("https://onionoo.torproject.org.example.com/", False),
        (b"https://stat.ripe.net/data/", False),
    ]
    for url, known in places:
        if asks_only_known(url) != known:
            print(f"::error::self-test: {url!r} was {'refused' if known else 'let through'}")
            return 1
    try:
        fetch("file:///etc/passwd")
    except NotAsked:
        pass
    except Exception as error:
        print(f"::error::self-test: a file was opened, and gave {type(error).__name__}")
        return 1
    else:
        print("::error::self-test: a file was read")
        return 1
    asked = []

    def ripe(answers):
        def get(url):
            asked.append(url)
            return answers.pop(0)
        return get

    numbers = [("62371", 62371), (62371, 62371), ("1/../x", None), ("1&resource=AS2", None), ("", None), ("12345678901", None), (None, None), ("٣", None)]
    for given, expected in numbers:
        if as_number(given) != expected:
            print(f"::error::self-test: the AS number {given!r} read as {as_number(given)!r}")
            return 1
    name = holder(ipaddress.ip_address(proton), {}, ripe([{"data": {"asns": ["1/../x"]}}]))
    if name != "no autonomous system" or len(asked) != 1:
        print(f"::error::self-test: an AS number that is no number was asked about: {asked}")
        return 1
    name = holder(ipaddress.ip_address(proton), {}, ripe([{"data": {"asns": [62371]}}, {"data": {"holder": "PROTON"}}]))
    if name != "AS62371 PROTON" or not asked[-1].endswith("resource=AS62371"):
        print(f"::error::self-test: a real AS number was misread: {name}, {asked}")
        return 1
    tor = {ipaddress.ip_address(relay)}
    built_in = fallbacks_of_text('"91.39.70.41:9001",\n"[2600:3c06::2000:c9ff:fe82:3b43]:443"')
    if (ipaddress.ip_address(fallback), 9001) not in built_in or len(built_in) != 2:
        print(f"::error::self-test: the fallback list was misread: {sorted(built_in)}")
        return 1
    failed = 0
    for title, lines, way, gravatar, verdict in cases:
        found, own = destinations(lines)
        allowed = {PROTON, QUAD9} | ({GRAVATAR} if gravatar else set())
        table, problems = judge(found, own, way, allowed, tor, lambda address: owners.get(str(address), "nobody"), built_in)
        verdicts = {given for _, given in table}
        if not found or bool(problems) == (title in passing) or verdicts != {verdict}:
            failed += 1
            print(f"::error::self-test, {title}: own {sorted(own)}, verdicts {sorted(verdicts)}, problems {problems}")
    print(f"self-test: {len(cases) - failed} of {len(cases)} cases hold")
    return 1 if failed else 0


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("trace", nargs="?")
    parser.add_argument("--way", choices=["direct", "tor"])
    parser.add_argument("--gravatar", action="store_true")
    parser.add_argument("--fallbacks")
    parser.add_argument("--self-test", action="store_true")
    arguments = parser.parse_args()
    if arguments.self_test:
        return self_test()
    if not arguments.trace or not arguments.way:
        parser.error("a trace and --way are needed")
    with open(arguments.trace, encoding="utf-8", errors="replace") as lines:
        found, own = destinations(lines)
    allowed = {PROTON, QUAD9} | ({GRAVATAR} if arguments.gravatar else set())
    tor = relays() if arguments.way == "tor" else set()
    asns = {}
    built_in = fallbacks(arguments.fallbacks) if arguments.way == "tor" else set()
    if arguments.way == "tor" and not built_in:
        print("::warning::no fallback directories of arti given: one of them may read as a way around Tor")
    table, problems = judge(found, own, arguments.way, allowed, tor, lambda address: holder(address, asns), built_in)
    print(f"{len(found)} addresses reached, the way {arguments.way}; the app's own ports here: {sorted(own)}")
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
