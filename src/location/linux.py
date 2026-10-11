"""GeoClue2 on one persistent system-bus connection (python3-gi required)."""
import json
import sys
import time

try:
    from gi.repository import Gio, GLib
    bus = Gio.bus_get_sync(Gio.BusType.SYSTEM, None)
    destination = "org.freedesktop.GeoClue2"

    def call(path, interface, method, args=None):
        return bus.call_sync(destination, path, interface, method, args, None,
                             Gio.DBusCallFlags.NONE, 5000, None).unpack()

    client = call("/org/freedesktop/GeoClue2/Manager",
                  "org.freedesktop.GeoClue2.Manager", "GetClient")[0]
    props = "org.freedesktop.DBus.Properties"
    for key, value in [("DesktopId", GLib.Variant("s", "org.nekosportsworld.tool")),
                       ("RequestedAccuracyLevel", GLib.Variant("u", 8))]:
        call(client, props, "Set", GLib.Variant("(ssv)",
             ("org.freedesktop.GeoClue2.Client", key, value)))
    try:
        call(client, "org.freedesktop.GeoClue2.Client", "Start")
        deadline = time.monotonic() + 50
        while time.monotonic() < deadline:
            path = call(client, props, "Get", GLib.Variant("(ss)",
                        ("org.freedesktop.GeoClue2.Client", "Location")))[0]
            if path != "/":
                data = call(path, props, "GetAll", GLib.Variant("(s)",
                            ("org.freedesktop.GeoClue2.Location",)))[0]
                timestamp = data.get("Timestamp", (0, 0))[0]
                if time.time() - timestamp <= 60:
                    print(json.dumps(dict(latitude=data["Latitude"], longitude=data["Longitude"],
                                          accuracy=data["Accuracy"], city=None)))
                    break
            time.sleep(0.25)
        else:
            raise RuntimeError("Location timed out")
    finally:
        call(client, "org.freedesktop.GeoClue2.Client", "Stop")
except Exception as error:
    print("GeoClue location unavailable. Install geoclue-2.0 and python3-gi, enable location "
          "and allow org.nekosportsworld.tool in the desktop location agent. " + str(error), file=sys.stderr)
    sys.exit(1)
