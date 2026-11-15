"""A port on this machine that counts the connections it gets and closes each unread, for the long run's link check
(.github/workflows/long-run.yml, app/src/link_check.rs): a clicked link in a mail points here, and nothing may arrive.

    python3 -I scripts/count-connections.py <port file> <count file>

Writes the port it listens on, then the count after every connection. Runs until it is stopped.
"""

import socket
import sys


def main():
    port_file, count_file = sys.argv[1], sys.argv[2]
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.bind(("127.0.0.1", 0))
    listener.listen(16)
    with open(count_file, "w", encoding="utf-8") as count:
        count.write("0\n")
    with open(port_file, "w", encoding="utf-8") as port:
        port.write(f"{listener.getsockname()[1]}\n")
    received = 0
    while True:
        connection, _ = listener.accept()
        connection.close()
        received += 1
        with open(count_file, "w", encoding="utf-8") as count:
            count.write(f"{received}\n")


if __name__ == "__main__":
    sys.exit(main())
