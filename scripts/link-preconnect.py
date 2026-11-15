"""What WebKitGTK sends over the network when a link in a page is pressed, while the page refuses to go anywhere.

The mail view refuses every navigation, hands a link to the browser, and sets `target="_blank"` on every link of a
mail (core/src/html.rs). This puts a page with one link, filling the window, to the same test: the link points at a
port on this machine that counts the connections it gets, and the page's navigations are all refused, as the app's
are. A real pointer then hovers, presses or clicks it (xdotool, through XTEST), and the script prints what reached the
port. Nothing leaves the machine.

    python3 -I scripts/link-preconnect.py <variant>

Variants: hover, press (held, let go elsewhere), click (target _blank), click-self (no target), click-no-preconnect
(target _blank, WebKit's LinkPreconnect feature off). Needs a display (Xvfb), xdotool and WebKitGTK's introspection
(gir1.2-webkit2-4.1, python3-gi). Prints one line: the variant, the navigations refused, the connections received.
"""

import socket
import subprocess
import sys
import threading

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import GLib, Gtk, WebKit2  # noqa: E402

VARIANTS = ("hover", "press", "click", "click-self", "click-no-preconnect")
WIDTH, HEIGHT = 600, 400
SETTLE_MS = 3000


class Counter:
    """A port on this machine that accepts and closes, and counts."""

    def __init__(self):
        self.listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(16)
        self.port = self.listener.getsockname()[1]
        self.count = 0
        threading.Thread(target=self.serve, daemon=True).start()

    def serve(self):
        while True:
            try:
                connection, _ = self.listener.accept()
            except OSError:
                return
            self.count += 1
            connection.close()


def xdotool(*arguments):
    subprocess.run(["xdotool", *arguments], check=True)


def main():
    variant = sys.argv[1] if len(sys.argv) > 1 else ""
    if variant not in VARIANTS:
        print(f"one of {', '.join(VARIANTS)}", file=sys.stderr)
        return 2
    counter = Counter()
    refused = []
    window = Gtk.Window()
    window.set_default_size(WIDTH, HEIGHT)
    window.move(0, 0)
    view = WebKit2.WebView()
    settings = view.get_settings()
    turned_off = "no feature to turn off"
    if variant == "click-no-preconnect":
        features = WebKit2.Settings.get_all_features()
        for index in range(features.get_length()):
            feature = features.get(index)
            if feature.get_identifier() == "LinkPreconnect":
                settings.set_feature_enabled(feature, False)
                turned_off = "LinkPreconnect off"
        if turned_off != "LinkPreconnect off":
            print(f"{variant}: WebKitGTK {webkit_version()} has no LinkPreconnect feature")
            return 1

    def decide(_view, decision, kind):
        if kind in (WebKit2.PolicyDecisionType.NAVIGATION_ACTION, WebKit2.PolicyDecisionType.NEW_WINDOW_ACTION):
            uri = decision.get_navigation_action().get_request().get_uri()
            if uri.startswith("http"):
                refused.append(uri)
                decision.ignore()
                return True
        return False

    view.connect("decide-policy", decide)
    target = "" if variant == "click-self" else ' target="_blank"'
    page = (
        "<!doctype html><html><body style='margin:0'>"
        f"<a href='http://127.0.0.1:{counter.port}/x'{target} "
        "style='display:block;width:100vw;height:100vh;background:#36c'>a link</a></body></html>"
    )
    window.add(view)
    window.show_all()

    def act():
        x, y = WIDTH // 2, HEIGHT // 2
        if variant == "hover":
            xdotool("mousemove", str(x), str(y))
        elif variant == "press":
            xdotool("mousemove", str(x), str(y), "mousedown", "1")
            GLib.timeout_add(SETTLE_MS, lambda: xdotool("mousemove", str(WIDTH + 50), str(HEIGHT + 50), "mouseup", "1"))
        else:
            xdotool("mousemove", str(x), str(y), "click", "1")
        GLib.timeout_add(SETTLE_MS * 2, finish)
        return False

    def finish():
        print(
            f"{variant}: WebKitGTK {webkit_version()}, {turned_off}, "
            f"navigations refused {len(refused)}, connections to the link's port {counter.count}"
        )
        Gtk.main_quit()
        return False

    def loaded(_view, event):
        if event == WebKit2.LoadEvent.FINISHED:
            GLib.timeout_add(1000, act)

    view.connect("load-changed", loaded)
    view.load_html(page, "about:blank")
    Gtk.main()
    return 0


def webkit_version():
    return f"{WebKit2.get_major_version()}.{WebKit2.get_minor_version()}.{WebKit2.get_micro_version()}"


if __name__ == "__main__":
    sys.exit(main())
