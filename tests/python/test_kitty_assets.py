"""Tests for the Python that claude-deck runs inside kitty (watcher, tab bar, kitten).

kitty's own modules aren't importable outside kitty, so small fakes stand in for
the boss, tabs and windows. Run with:  python3 -m unittest discover -s tests/python
"""
import importlib.util
import json
import os
import socket
import sys
import tempfile
import threading
import time
import types
import unittest

ASSETS = os.path.join(os.path.dirname(__file__), '..', '..', 'src', 'assets', 'kitty')

# ---- Stand-ins for kitty's modules ------------------------------------------------

_boss_holder = {'boss': None}


def _install_kitty_stubs():
    kitty = types.ModuleType('kitty')
    boss_mod = types.ModuleType('kitty.boss')
    boss_mod.get_boss = lambda: _boss_holder['boss']
    fdt = types.ModuleType('kitty.fast_data_types')
    fdt.timers = []
    fdt.add_timer = lambda fn, interval, repeat: fdt.timers.append((fn, interval, repeat))
    layout = types.ModuleType('kitty.layout')
    splits = types.ModuleType('kitty.layout.splits')

    class Pair:
        def __init__(self, horizontal=True):
            self.horizontal = horizontal
            self.one = self.two = None
            self.bias = 0.5

    splits.Pair = Pair
    kittens = types.ModuleType('kittens')
    tui = types.ModuleType('kittens.tui')
    handler = types.ModuleType('kittens.tui.handler')
    handler.result_handler = lambda **kw: (lambda fn: fn)
    for name, mod in {
        'kitty': kitty, 'kitty.boss': boss_mod, 'kitty.fast_data_types': fdt,
        'kitty.layout': layout, 'kitty.layout.splits': splits,
        'kittens': kittens, 'kittens.tui': tui, 'kittens.tui.handler': handler,
    }.items():
        sys.modules[name] = mod


_install_kitty_stubs()
RUNTIME = tempfile.mkdtemp(prefix='cdpy-', dir='/tmp')
os.environ['XDG_RUNTIME_DIR'] = RUNTIME
os.makedirs(os.path.join(RUNTIME, 'claude-deck'), exist_ok=True)


def load(name):
    spec = importlib.util.spec_from_file_location(f'deck_asset_{name}', os.path.join(ASSETS, f'{name}.py'))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


watcher = load('watcher')
tab_bar = load('tab_bar')
deck = load('deck')


class Geo:
    def __init__(self, left, top, right, bottom):
        self.left, self.top, self.right, self.bottom = left, top, right, bottom


class Group:
    def __init__(self, wid):
        self.main_window_id = wid
        self.id = wid


class Layout:
    def __init__(self, name):
        self.name = name
        self.pairs_root = None


class Windows:
    def __init__(self, tab):
        self.tab = tab

    def iter_all_layoutable_groups(self):
        return [Group(w.id) for w in self.tab.wins]


class Window:
    def __init__(self, wid, geo=None):
        self.id = wid
        self.geometry = geo or Geo(0, 0, 100, 100)
        self.tab = None

    def tabref(self):
        return self.tab


class Tab:
    def __init__(self, tid, wins, layout='splits'):
        self.id = tid
        self.wins = list(wins)
        for w in self.wins:
            w.tab = self
        self.windows = Windows(self)
        self.active_window = self.wins[0] if self.wins else None
        self.current_layout = Layout(layout)
        self.tm = None
        self.relayouts = 0

    def __iter__(self):
        return iter(self.wins)

    def tab_manager_ref(self):
        return self.tm

    def set_active_window(self, w):
        self.active_window = w

    def goto_layout(self, name):
        self.current_layout = Layout(name)

    def relayout(self):
        self.relayouts += 1

    def make_active(self):
        self.tm.active_tab = self


class TabManager:
    def __init__(self, tabs, os_window_id=1):
        self.tabs = tabs
        for t in tabs:
            t.tm = self
        self.active_tab = tabs[0]
        self.os_window_id = os_window_id

    def __iter__(self):
        return iter(self.tabs)

    def set_active_tab(self, t):
        self.active_tab = t

    def mark_tab_bar_dirty(self):
        pass


class Boss:
    def __init__(self, *tms):
        self.all_tab_managers = list(tms)
        self.active_tab_manager = tms[0]
        self.moves, self.inserts, self.closed, self.confirms = [], [], [], []
        self._next_tab = 1000

    @property
    def all_tabs(self):
        return [t for tm in self.all_tab_managers for t in tm]

    @property
    def window_id_map(self):
        return {w.id: w for t in self.all_tabs for w in t.wins}

    def tab_for_id(self, tid):
        return next((t for t in self.all_tabs if t.id == tid), None)

    def _move_window_to(self, window, target_tab_id):
        self.moves.append((window.id, target_tab_id))
        src = window.tab
        src.wins.remove(window)
        if target_tab_id == 'new':
            self._next_tab += 1
            dest = Tab(self._next_tab, [])
            src.tm.tabs.append(dest)
            dest.tm = src.tm
        else:
            dest = self.tab_for_id(target_tab_id)
        dest.wins.append(window)
        window.tab = dest

    def _insert_window_in_direction(self, w, dest, side):
        self.inserts.append((w.id, dest.id, side))

    def mark_window_for_close(self, w):
        self.closed.append(w.id)

    def confirm(self, msg, callback, *args, window=None, title=''):
        self.confirms.append((msg, callback, args, title))


def two_tabs():
    """Tab 10: windows 1 (left) and 2 (right). Tab 20: window 3."""
    t1 = Tab(10, [Window(1, Geo(0, 0, 50, 100)), Window(2, Geo(50, 0, 100, 100))])
    t2 = Tab(20, [Window(3)])
    boss = Boss(TabManager([t1, t2]))
    _boss_holder['boss'] = boss
    return boss, t1, t2


def kitten(boss, *args):
    return deck.handle_result(['deck.py', *[str(a) for a in args]], None, 0, boss)


# ---- deck.py: the kitten ------------------------------------------------------------

class KittenTest(unittest.TestCase):
    def test_peek_and_unpeek_restore_the_pane_you_were_in(self):
        boss, t1, t2 = two_tabs()
        self.assertEqual(kitten(boss, 'peek', 3), 'ok')
        self.assertIs(boss.active_tab_manager.active_tab, t2)
        # Peeking at another one keeps the original to go back to.
        self.assertEqual(kitten(boss, 'peek', 2), 'ok')
        self.assertIs(t1.active_window.id, 2)
        self.assertEqual(kitten(boss, 'unpeek'), 'ok')
        self.assertIs(boss.active_tab_manager.active_tab, t1)
        self.assertEqual(t1.active_window.id, 1)
        self.assertEqual(kitten(boss, 'peek', 99), 'missing')

    def test_focus_ends_a_peek(self):
        boss, t1, t2 = two_tabs()
        kitten(boss, 'peek', 3)
        self.assertEqual(kitten(boss, 'focus', 3), 'ok')
        kitten(boss, 'unpeek')
        self.assertIs(boss.active_tab_manager.active_tab, t2, 'unpeek after a focus stays put')
        self.assertEqual(kitten(boss, 'focus', 99), 'missing')

    def test_place_into_its_own_tab(self):
        boss, t1, t2 = two_tabs()
        self.assertEqual(kitten(boss, 'place', 3, 'tab'), 'ok')
        self.assertEqual(boss.moves, [], 'already alone in its tab')
        self.assertEqual(kitten(boss, 'place', 2, 'tab'), 'ok')
        self.assertEqual(boss.moves, [(2, 'new')])

    def test_place_beside_another_pane(self):
        boss, t1, t2 = two_tabs()
        t2.current_layout = Layout('tall')
        self.assertEqual(kitten(boss, 'place', 1, 3, 'left'), 'ok')
        self.assertEqual(boss.inserts, [(1, 3, 'left')])
        self.assertEqual(t2.current_layout.name, 'splits', 'only splits can put a pane on a side')
        self.assertEqual(kitten(boss, 'place', 1, 3, 'diagonal'), 'missing')
        self.assertEqual(kitten(boss, 'place', 1, 1, 'left'), 'missing', 'not beside itself')
        self.assertEqual(kitten(boss, 'place', 1, 77, 'left'), 'missing')

    def test_panes_are_normalised_to_the_tab(self):
        boss, t1, t2 = two_tabs()
        tabs = json.loads(kitten(boss, 'panes'))
        self.assertEqual([t['tab'] for t in tabs], [10, 20])
        left, right = tabs[0]['panes']
        self.assertEqual((left['window'], left['x'], left['w'], left['h']), (1, 0, 0.5, 1))
        self.assertEqual((right['window'], right['x']), (2, 0.5))
        self.assertEqual(tabs[1]['panes'], [{'window': 3, 'x': 0, 'y': 0, 'w': 1, 'h': 1}])

    def test_panes_carry_the_tabs_shape_and_split_tree(self):
        boss, t1, t2 = two_tabs()
        Pair = sys.modules['kitty.layout.splits'].Pair
        root = Pair(horizontal=True)
        root.one, root.two, root.bias = 1, Pair(horizontal=False), 0.3
        root.two.one, root.two.two = 2, 99  # 99: a group with no window any more
        t1.current_layout.pairs_root = root
        tab = json.loads(kitten(boss, 'panes'))[0]
        self.assertEqual(tab['tree'], {'h': True, 'bias': 0.3, 'one': 1, 'two': 2})
        self.assertGreater(tab['aspect'], 0)
        self.assertNotIn('tree', json.loads(kitten(boss, 'panes'))[1], 'one pane, no splits')

    def test_zoomed_tab_panes_become_columns(self):
        boss, t1, t2 = two_tabs()
        t1.current_layout = Layout('stack')
        for w in t1.wins:
            w.geometry = Geo(0, 0, 100, 100)
        panes = json.loads(kitten(boss, 'panes'))[0]['panes']
        self.assertEqual([(p['x'], p['w']) for p in panes], [(0, 0.5), (0.5, 0.5)])

    def test_untile_one_tab(self):
        boss, t1, t2 = two_tabs()
        self.assertEqual(kitten(boss, 'untile-tab', 10), 'ok')
        self.assertEqual(boss.moves, [(2, 'new')], 'the first pane keeps the tab')
        self.assertEqual(kitten(boss, 'untile-tab', 999), 'missing')

    def test_untile_everything_keeps_each_tabs_active_pane(self):
        boss, t1, t2 = two_tabs()
        t1.active_window = t1.wins[1]
        kitten(boss, 'untile', 1, 2, 3)
        self.assertEqual(boss.moves, [(1, 'new')])

    def test_tile_gathers_and_evens_out(self):
        boss, t1, t2 = two_tabs()
        self.assertEqual(kitten(boss, 'tile', 1, 2, 3), 'ok')
        self.assertEqual(boss.moves, [(3, 10)])
        self.assertEqual(t1.current_layout.name, 'splits')
        self.assertIsNotNone(t1.current_layout.pairs_root)

    def test_even_splits_gives_every_pane_the_same_share(self):
        tab = Tab(1, [Window(i) for i in range(1, 6)])
        deck._even_splits(tab)
        root = tab.current_layout.pairs_root
        # 5 panes: 3 columns of 2, 2 and 1.
        self.assertTrue(root.horizontal)
        self.assertAlmostEqual(root.bias, 1 / 3)
        first_col = root.one
        self.assertFalse(first_col.horizontal)
        self.assertEqual((first_col.one, first_col.two), (1, 2))
        self.assertAlmostEqual(root.two.bias, 1 / 2)
        self.assertEqual(root.two.two, 5)
        self.assertEqual(tab.relayouts, 1)
        single = Tab(2, [Window(9)])
        deck._even_splits(single)
        self.assertIsNone(single.current_layout.pairs_root)

    def test_close_now_or_after_asking(self):
        boss, t1, t2 = two_tabs()
        self.assertEqual(kitten(boss, 'close', 1, 'idle'), 'ok')
        self.assertEqual(boss.closed, [1])
        kitten(boss, 'close', 2, 'working', '#2 api')
        msg, cb, args, title = boss.confirms[-1]
        self.assertIn('still working in session #2 api', msg)
        self.assertEqual(title, 'Close session')
        cb(False, *args)
        self.assertEqual(boss.closed, [1], 'Esc keeps it')
        cb(True, *args)
        self.assertEqual(boss.closed, [1, 2])
        kitten(boss, 'close', 3, 'ask')
        self.assertTrue(boss.confirms[-1][0].startswith('Close this pane?'))
        self.assertEqual(kitten(boss, 'close', 42, 'idle'), 'missing')

    def test_layout_maps_windows_to_tabs(self):
        boss, t1, t2 = two_tabs()
        out = json.loads(kitten(boss, 'layout'))
        self.assertEqual(out['1']['tab'], 10)
        self.assertEqual(out['3'], {'tab': 20, 'os_window': 1, 'layout': 'splits'})

    def test_unknown_command(self):
        boss, _, _ = two_tabs()
        self.assertEqual(kitten(boss, 'dance'), 'unknown')
        self.assertEqual(deck.handle_result(['deck.py'], None, 0, boss), 'unknown')


# ---- watcher.py: focus, close and order reports -----------------------------------

class Listener:
    """A stand-in daemon socket collecting what the watcher sends."""

    def __init__(self, path):
        self.path = path
        self.lines = []
        if os.path.exists(path):
            os.unlink(path)
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.bind(path)
        self.sock.listen(8)
        self.sock.settimeout(0.05)
        self.stop = False
        self.thread = threading.Thread(target=self._run, daemon=True)
        self.thread.start()

    def _run(self):
        while not self.stop:
            try:
                conn, _ = self.sock.accept()
            except OSError:
                continue
            with conn:
                data = b''
                while chunk := conn.recv(4096):
                    data += chunk
                self.lines += [json.loads(line) for line in data.decode().splitlines() if line]

    def wait(self, n):
        deadline = time.time() + 3
        while len(self.lines) < n and time.time() < deadline:
            time.sleep(0.01)
        return self.lines

    def close(self):
        self.stop = True
        self.thread.join()
        self.sock.close()
        os.unlink(self.path)


class WatcherTest(unittest.TestCase):
    def setUp(self):
        watcher._last_order['ids'] = None
        self.daemon = Listener(watcher._SOCK)

    def tearDown(self):
        self.daemon.close()

    def test_focus_and_close_messages_match_the_daemon_protocol(self):
        # src/proto.rs has the matching test for the Rust side.
        boss, t1, _ = two_tabs()
        watcher.on_focus_change(boss, t1.wins[0], {'focused': True})
        watcher.on_focus_change(boss, t1.wins[0], {})
        watcher.on_close(boss, t1.wins[1], {})
        got = self.daemon.wait(3)
        self.assertIn({'op': 'focus', 'kitty_window': 1, 'focused': True}, got)
        self.assertIn({'op': 'focus', 'kitty_window': 1, 'focused': False}, got)
        self.assertIn({'op': 'closed', 'kitty_window': 2}, got)

    def test_order_is_tabs_then_reading_order_and_only_sent_on_change(self):
        top_right = Window(5, Geo(50, 0, 100, 50))
        top_left = Window(6, Geo(0, 3, 50, 50))     # a few pixels lower: still the same row
        bottom = Window(7, Geo(0, 50, 100, 100))
        t1 = Tab(1, [bottom, top_right, top_left])
        t2 = Tab(2, [Window(4)])
        boss = Boss(TabManager([t1, t2]))
        self.assertEqual(watcher._order(boss), [6, 5, 7, 4])
        watcher.on_tab_bar_dirty(boss, None, {})
        watcher.on_tab_bar_dirty(boss, None, {})
        self.assertEqual(self.daemon.wait(1), [{'op': 'order', 'windows': [6, 5, 7, 4]}])
        time.sleep(0.1)
        self.assertEqual(len(self.daemon.lines), 1, 'unchanged order is not resent')

    def test_tick_redraws_and_registers_once(self):
        boss, _, _ = two_tabs()
        from kitty import fast_data_types
        watcher._timer['started'] = False
        watcher.on_load(boss, {})
        watcher.on_load(boss, {})
        self.assertEqual(len(fast_data_types.timers), 1)
        watcher._tick(None)
        self.assertTrue(self.daemon.wait(1))


class WatcherWithoutDaemonTest(unittest.TestCase):
    def test_a_missing_daemon_is_not_an_error(self):
        boss, t1, _ = two_tabs()
        watcher.on_focus_change(boss, t1.wins[0], {'focused': True})
        watcher.on_close(boss, t1.wins[0], {})


# ---- tab_bar.py: tab titles -------------------------------------------------------

class Fg:
    tab = '<tab>'

    def __getattr__(self, name):
        return f'<{name}>'


class Fmt:
    fg = Fg()


def write_state(windows):
    with open(tab_bar._STATE, 'w') as f:
        json.dump({'rev': 1, 'windows': windows}, f)
    # Make sure the cache notices even within the same mtime tick.
    tab_bar._cache['mtime'] = -1.0


def title(tab_id, index=1, name='zsh'):
    return tab_bar.draw_title({'fmt': Fmt(), 'title': name, 'tab_id': tab_id, 'index': index})


class TabBarTest(unittest.TestCase):
    def setUp(self):
        self.boss, self.t1, self.t2 = two_tabs()

    def tearDown(self):
        if os.path.exists(tab_bar._STATE):
            os.unlink(tab_bar._STATE)

    def test_a_tab_without_sessions_keeps_its_title(self):
        write_state({})
        self.assertEqual(title(20, 2, 'vim'), '2 vim')

    def test_one_session(self):
        write_state({'3': {'status': 'working', 'seen': True, 'since': 0, 'index': 4, 'project': 'api', 'model': 'Sonnet 5.5'}})
        t = title(20)
        self.assertIn('<_5fbf7a>●', t)
        self.assertIn(' 4 api', t)
        self.assertIn('Sonnet 5.5', t)
        self.assertNotIn('m<tab>', t, 'no timer while working')

    def test_the_most_urgent_session_leads_and_the_wait_shows(self):
        old = int((time.time() - 300) * 1000)
        write_state({
            '1': {'status': 'working', 'seen': True, 'since': 0, 'index': 1, 'project': 'api', 'model': 'Opus 5.5'},
            '2': {'status': 'needs_you', 'seen': False, 'since': old, 'index': 2, 'project': 'web', 'model': 'Sonnet 5.5'},
        })
        t = title(10)
        self.assertTrue(t.startswith('<_f5a524>◆'), t)
        self.assertIn(' 2 web +1', t)
        self.assertIn(' 5m', t)
        self.assertNotIn('Opus', t, 'mixed models: none shown')

    def test_background_work_is_purple_and_below_working(self):
        write_state({
            '1': {'status': 'background', 'seen': True, 'since': 0, 'index': 1, 'project': 'api'},
            '2': {'status': 'your_turn', 'seen': True, 'since': 0, 'index': 2, 'project': 'web'},
        })
        t = title(10)
        self.assertTrue(t.startswith('<_b04cff>◐'), t)
        self.assertIn(' 1 api +1', t)
        self.assertNotIn('m<tab>', t, 'no timer: nothing waits on you')
        write_state({
            '1': {'status': 'background', 'seen': True, 'since': 0, 'index': 1, 'project': 'api'},
            '2': {'status': 'working', 'seen': True, 'since': 0, 'index': 2, 'project': 'web'},
        })
        self.assertTrue(title(10).startswith('<_5fbf7a>●'), 'working leads')

    def test_a_crowded_tab_shows_counts(self):
        wins = [Window(i) for i in range(1, 7)]
        tab = Tab(30, wins)
        _boss_holder['boss'] = Boss(TabManager([tab]))
        write_state({str(i): {'status': 'working' if i < 5 else 'your_turn', 'seen': False, 'since': 0, 'index': i, 'project': 'p'} for i in range(1, 7)})
        t = title(30)
        self.assertIn('●2', t)
        self.assertIn('●4', t)
        self.assertIn('+5', t)

    def test_unknown_status_and_broken_state_do_not_break_titles(self):
        write_state({'3': {'status': 'from_the_future', 'index': 1, 'project': 'x'}})
        self.assertIn('◌', title(20))
        with open(tab_bar._STATE, 'w') as f:
            f.write('{not json')
        tab_bar._cache['mtime'] = -1.0
        tab_bar._cache['data'] = {}
        self.assertEqual(title(20, 3, 'sh'), '3 sh')
        os.unlink(tab_bar._STATE)
        self.assertEqual(title(20, 3, 'sh'), '3 sh')

    def test_elapsed(self):
        now = time.time() * 1000
        self.assertEqual(tab_bar._elapsed(now - 30_000), '')
        self.assertEqual(tab_bar._elapsed(now - 125_000), ' 2m')
        self.assertEqual(tab_bar._elapsed(now - 3_900_000), ' 1h05')
        self.assertEqual(tab_bar._elapsed(now + 60_000), '', 'future timestamps are not negative')


def tearDownModule():
    import shutil
    shutil.rmtree(RUNTIME, ignore_errors=True)


if __name__ == '__main__':
    unittest.main()
