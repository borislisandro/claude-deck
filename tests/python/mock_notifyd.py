"""A stand-in for Plasma's notification server, for the end-to-end notification test.

Usage: mock_notifyd.py LOGFILE [NOTIFY_DELAY_SECONDS]
Owns org.freedesktop.Notifications on the session bus and appends one JSON line to
LOGFILE for every Notify / CloseNotification it gets. A delay makes Notify slow, to
reproduce a notification being dismissed before its id has come back.
"""
import json
import sys
import time
import warnings

warnings.simplefilter('ignore')
import gi

gi.require_version('Gio', '2.0')
from gi.repository import Gio, GLib  # noqa: E402

LOG = sys.argv[1]
DELAY = float(sys.argv[2]) if len(sys.argv) > 2 else 0.0
XML = """<node><interface name="org.freedesktop.Notifications">
<method name="GetCapabilities"><arg direction="out" type="as"/></method>
<method name="Notify">
 <arg direction="in" type="s"/><arg direction="in" type="u"/><arg direction="in" type="s"/>
 <arg direction="in" type="s"/><arg direction="in" type="s"/><arg direction="in" type="as"/>
 <arg direction="in" type="a{sv}"/><arg direction="in" type="i"/><arg direction="out" type="u"/></method>
<method name="CloseNotification"><arg direction="in" type="u"/></method>
<method name="GetServerInformation"><arg direction="out" type="s"/><arg direction="out" type="s"/>
 <arg direction="out" type="s"/><arg direction="out" type="s"/></method>
<signal name="NotificationClosed"><arg type="u"/><arg type="u"/></signal>
<signal name="ActionInvoked"><arg type="u"/><arg type="s"/></signal>
</interface></node>"""

next_id = [100]
conn_ref = [None]


def log(**kw):
    with open(LOG, 'a') as f:
        f.write(json.dumps(kw) + '\n')


def on_call(conn, sender, path, iface, method, params, inv):
    if method == 'GetCapabilities':
        inv.return_value(GLib.Variant('(as)', (['actions', 'body'],)))
    elif method == 'GetServerInformation':
        inv.return_value(GLib.Variant('(ssss)', ('mock', 'test', '1', '1.2')))
    elif method == 'Notify':
        app, replaces, icon, summary, body, actions, hints, timeout = params.unpack()
        if DELAY:
            time.sleep(DELAY)
        nid = replaces or next_id[0]
        if not replaces:
            next_id[0] += 1
        urgency = hints.get('urgency')
        log(ev='notify', id=nid, replaces=replaces, summary=summary, body=body,
            urgency=int(urgency) if urgency is not None else None, timeout=timeout, actions=actions)
        inv.return_value(GLib.Variant('(u)', (nid,)))
    elif method == 'CloseNotification':
        (nid,) = params.unpack()
        log(ev='close', id=nid)
        inv.return_value(None)
        conn.emit_signal(None, path, iface, 'NotificationClosed', GLib.Variant('(uu)', (nid, 3)))


def on_bus(conn, name):
    info = Gio.DBusNodeInfo.new_for_xml(XML)
    conn.register_object('/org/freedesktop/Notifications', info.interfaces[0], on_call, None, None)


Gio.bus_own_name(Gio.BusType.SESSION, 'org.freedesktop.Notifications', Gio.BusNameOwnerFlags.NONE,
                 on_bus, None, None)
log(ev='ready')
GLib.MainLoop().run()
