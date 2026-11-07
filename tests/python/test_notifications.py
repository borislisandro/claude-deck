"""End-to-end tests of the notifications: the real daemon and real hooks, talking to a
mock Plasma notification server (mock_notifyd.py) on a private D-Bus.

Needs dbus-daemon, gdbus, and python3-gi; skipped without them. The binary is
target/debug/claude-deck unless CLAUDE_DECK_BIN says otherwise (`cargo build` first).
Run with:  python3 -m unittest discover -s tests/python
"""
import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
BIN = os.environ.get('CLAUDE_DECK_BIN') or os.path.join(HERE, '..', '..', 'target', 'debug', 'claude-deck')

try:
    import gi  # noqa: F401
    HAVE_GI = True
except ImportError:
    HAVE_GI = False
HAVE_TOOLS = HAVE_GI and all(shutil.which(t) for t in ('dbus-daemon', 'gdbus')) and os.path.exists(BIN)


def wait_for(pred, timeout=8.0, what='condition'):
    end = time.time() + timeout
    while time.time() < end:
        v = pred()
        if v:
            return v
        time.sleep(0.05)
    raise AssertionError(f'timed out waiting for {what}')


class Base(unittest.TestCase):
    config = {'notify': True, 'sounds': False, 'remind_after_min': 0}
    mock_delay = 0.0

    def setUp(self):
        self.dir = tempfile.mkdtemp(prefix='cdn-', dir='/tmp')
        for d in ('home', 'rt', 'config/claude-deck', 'state', 'data'):
            os.makedirs(os.path.join(self.dir, d))
        with open(os.path.join(self.dir, 'config/claude-deck/config.json'), 'w') as f:
            json.dump(self.config, f)
        self.log = os.path.join(self.dir, 'notifications.log')
        self.bus = subprocess.Popen(['dbus-daemon', '--session', '--nofork', '--print-address'],
                                    stdout=subprocess.PIPE, text=True)
        self.addr = self.bus.stdout.readline().strip()
        self.env = dict(os.environ, HOME=self.dir + '/home', XDG_RUNTIME_DIR=self.dir + '/rt',
                        XDG_CONFIG_HOME=self.dir + '/config', XDG_STATE_HOME=self.dir + '/state',
                        XDG_DATA_HOME=self.dir + '/data', DBUS_SESSION_BUS_ADDRESS=self.addr)
        for v in ('CLAUDE_DECK_SID', 'KITTY_WINDOW_ID'):
            self.env.pop(v, None)
        self.mock = subprocess.Popen([sys.executable, os.path.join(HERE, 'mock_notifyd.py'), self.log, str(self.mock_delay)],
                                     env=self.env)
        wait_for(lambda: self.events('ready'), what='the mock notification server')
        self.daemon = None
        self.start_daemon()

    def tearDown(self):
        for p in (self.daemon, self.mock, self.bus):
            if p and p.poll() is None:
                p.kill()
                p.wait()
        self.bus.stdout.close()
        shutil.rmtree(self.dir, ignore_errors=True)

    # -- plumbing ---------------------------------------------------------------
    def start_daemon(self):
        self.daemon = subprocess.Popen([BIN, 'daemon', '--demo'], env=self.env,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        sock = self.dir + '/rt/claude-deck/daemon.sock'

        def up():
            try:
                with socket.socket(socket.AF_UNIX) as s:
                    s.connect(sock)
                return True
            except OSError:
                return False
        wait_for(up, what='the daemon')

    def call(self, req):
        with socket.socket(socket.AF_UNIX) as s:
            s.connect(self.dir + '/rt/claude-deck/daemon.sock')
            s.sendall((json.dumps(req) + '\n').encode())
            with s.makefile() as f:
                return json.loads(f.readline())

    def hook(self, sid, win, event, **extra):
        payload = dict(hook_event_name=event, session_id='c-' + sid, cwd='/work/proj', **extra)
        subprocess.run([BIN, 'hook'], input=json.dumps(payload).encode(), check=True, timeout=5,
                       env=dict(self.env, CLAUDE_DECK_SID=sid, KITTY_WINDOW_ID=str(win)))

    def events(self, kind=None):
        try:
            with open(self.log) as f:
                evs = [json.loads(line) for line in f if line.strip()]
        except (FileNotFoundError, ValueError):
            return []
        return [e for e in evs if kind is None or e['ev'] == kind]

    def mine(self):
        """Notifications about the session under test (the demo sessions chatter too)."""
        return [e for e in self.events('notify') if 'proj' in e['summary']]

    def on_screen(self, project='proj'):
        """Ids of this session's notifications that are up right now."""
        up = {}
        for e in self.events():
            if e['ev'] == 'notify' and project in e['summary']:
                up[e['id']] = True
            elif e['ev'] == 'close':
                up.pop(e['id'], None)
        return list(up)

    def quiet_for(self, seconds, pred):
        time.sleep(seconds)
        return not pred()


@unittest.skipUnless(HAVE_TOOLS, 'needs dbus-daemon, gdbus, python3-gi and a built binary')
class NotificationTests(Base):
    def test_a_blocked_session_notifies_and_is_withdrawn_when_answered(self):
        self.hook('a', 7, 'UserPromptSubmit', prompt='ship it\nnow')
        self.hook('a', 7, 'PermissionRequest', tool_name='Bash', tool_input={'command': 'make deploy'})
        n = wait_for(self.mine, what='the needs-you notification')[0]
        self.assertEqual(n['summary'], 'proj needs you')
        self.assertEqual(n['body'], 'Wants to: Running make deploy\n“ship it”')
        self.assertEqual((n['urgency'], n['timeout']), (2, 0), 'critical, and it stays until handled')
        self.assertIn('default', n['actions'])
        self.hook('a', 7, 'PostToolUse', tool_name='Bash')
        wait_for(lambda: not self.on_screen(), what='the notification being withdrawn')

    def test_done_says_what_claude_said(self):
        transcript = os.path.join(self.dir, 't.jsonl')
        with open(transcript, 'w') as f:
            f.write(json.dumps({'type': 'assistant', 'message': {'id': 'm1', 'model': 'claude-x', 'content': [
                {'type': 'text', 'text': '## Fixed the flaky test\n\nIt raced on the clock.'}]}}) + '\n')
        self.hook('a', 7, 'UserPromptSubmit', prompt='fix the test', transcript_path=transcript)
        self.hook('a', 7, 'Stop', transcript_path=transcript)
        n = wait_for(self.mine, what='the done notification')[0]
        self.assertEqual(n['summary'], 'proj is done')
        self.assertEqual(n['body'], 'Fixed the flaky test\n“fix the test”')
        self.assertEqual((n['urgency'], n['timeout']), (1, 15000))

    def test_a_question_is_quoted(self):
        self.hook('a', 7, 'UserPromptSubmit', prompt='plan the db')
        self.hook('a', 7, 'PreToolUse', tool_name='AskUserQuestion',
                  tool_input={'questions': [{'question': 'Postgres or SQLite?', 'options': []}]})
        n = wait_for(self.mine, what='the question notification')[0]
        self.assertTrue(n['body'].startswith('Asking: Postgres or SQLite?'), n['body'])

    def test_nothing_is_said_while_you_are_in_the_deck(self):
        self.call({'op': 'focus', 'kitty_window': 9, 'focused': True})  # you are in the deck, on another pane
        self.hook('a', 7, 'UserPromptSubmit', prompt='go')
        self.hook('a', 7, 'PermissionRequest', tool_name='Bash', tool_input={'command': 'ls'})
        self.hook('a', 7, 'PostToolUse', tool_name='Bash')
        self.hook('a', 7, 'Stop')
        self.assertTrue(self.quiet_for(1.5, self.mine), 'the tab bar and the island already say it')
        # Out of the deck, the same event is news.
        self.call({'op': 'focus', 'kitty_window': 9, 'focused': False})
        self.hook('a', 7, 'UserPromptSubmit', prompt='again')
        self.hook('a', 7, 'Stop')
        wait_for(self.mine, what='the notification once you left the deck')

    def test_looking_at_the_session_withdraws_its_notification(self):
        self.hook('a', 7, 'UserPromptSubmit', prompt='go')
        self.hook('a', 7, 'Stop')
        wait_for(self.mine, what='a notification')
        self.call({'op': 'focus', 'kitty_window': 7, 'focused': True})
        wait_for(lambda: not self.on_screen(), what='the withdrawal')

    def test_a_crash_while_you_were_away_is_reported(self):
        self.hook('a', 7, 'UserPromptSubmit', prompt='long task')
        self.hook('a', 7, 'SessionEnd', reason='other')
        n = wait_for(lambda: [e for e in self.events('notify') if 'exited unexpectedly' in e['summary']], 6, 'the crash notification')[0]
        self.assertEqual(n['summary'], 'proj: Claude exited unexpectedly')
        self.assertIn('long task', n['body'])

    def test_a_normal_exit_is_silent(self):
        self.hook('a', 7, 'UserPromptSubmit', prompt='x')
        self.hook('a', 7, 'Stop')
        wait_for(self.mine, what='the done notification')
        self.hook('a', 7, 'SessionEnd', reason='prompt_input_exit')
        self.assertTrue(self.quiet_for(3.2, lambda: [e for e in self.events('notify') if 'exited' in e['summary']]))

    def test_closing_the_pane_or_the_deck_is_not_a_crash(self):
        self.hook('a', 7, 'UserPromptSubmit', prompt='x')
        self.hook('b', 8, 'UserPromptSubmit', prompt='y')
        self.call({'op': 'closed', 'kitty_window': 7})          # you closed one pane...
        self.hook('a', 7, 'SessionEnd', reason='other')
        self.call({'op': 'quit'})                               # ...then quit the deck
        self.hook('b', 8, 'SessionEnd', reason='other')
        self.assertTrue(self.quiet_for(3.2, lambda: [e for e in self.events('notify') if 'exited' in e['summary']]))

    def test_the_digest_is_for_what_began_waiting_while_away(self):
        self.hook('a', 7, 'UserPromptSubmit', prompt='x')
        self.hook('a', 7, 'Stop')
        wait_for(self.mine, what='the done notification')
        now = int(time.time() * 1000)
        # A fullscreen app that began after the session finished: nothing new to tell.
        self.call({'op': 'digest', 'since': now + 60_000})
        self.assertTrue(self.quiet_for(1.0, lambda: [e for e in self.events('notify') if 'While you were away' in e['summary']]))
        # One that began before: tell, once, replacing itself on the next ask.
        self.call({'op': 'digest', 'since': 0})
        first = wait_for(lambda: [e for e in self.events('notify') if 'While you were away' in e['summary']], what='the digest')[0]
        self.assertIn('proj — done', first['body'])
        self.call({'op': 'digest', 'since': 0})
        wait_for(lambda: len([e for e in self.events('notify') if 'While you were away' in e['summary']]) == 2, what='the second digest')
        second = [e for e in self.events('notify') if 'While you were away' in e['summary']][1]
        self.assertEqual(second['replaces'], first['id'], 'one digest on screen, not a stack')

    def test_a_daemon_that_died_does_not_leave_a_notification_nobody_can_withdraw(self):
        self.hook('a', 7, 'UserPromptSubmit', prompt='x')
        self.hook('a', 7, 'PermissionRequest', tool_name='Bash', tool_input={'command': 'ls'})
        n = wait_for(self.mine, what='the notification')[0]
        self.daemon.send_signal(signal.SIGKILL)
        self.daemon.wait()
        self.start_daemon()
        wait_for(lambda: any(e['id'] == n['id'] for e in self.events('close')), what='the new daemon closing the old notification')

    def test_a_background_agent_does_not_withdraw_a_pending_prompt(self):
        self.hook('a', 7, 'UserPromptSubmit', prompt='go')
        self.hook('a', 7, 'PermissionRequest', tool_name='Edit', tool_input={'file_path': '/a.rs'})
        wait_for(self.mine, what='the notification')
        self.hook('a', 7, 'PreToolUse', agent_id='sub-1', tool_name='Read', tool_input={'file_path': '/b'})
        self.hook('a', 7, 'PostToolUse', agent_id='sub-1', tool_name='Read')
        self.assertTrue(self.quiet_for(0.8, lambda: self.events('close')), 'the prompt is still open')
        self.assertEqual(len(self.on_screen()), 1)


@unittest.skipUnless(HAVE_TOOLS, 'needs dbus-daemon, gdbus, python3-gi and a built binary')
class SlowServerTests(Base):
    """The same world, but the notification server takes its time to answer Notify."""
    mock_delay = 0.5

    def test_answering_instantly_leaves_nothing_behind(self):
        self.hook('a', 7, 'UserPromptSubmit', prompt='go')
        self.hook('a', 7, 'PermissionRequest', tool_name='Bash', tool_input={'command': 'ls'})
        self.hook('a', 7, 'PostToolUse', tool_name='Bash')   # auto-approved a moment later
        wait_for(self.mine, what='the notification being shown')
        time.sleep(1.5)
        self.assertEqual(self.on_screen(), [], 'it was withdrawn once it was up, not left sticky')

    def test_two_quick_changes_never_stack(self):
        self.hook('a', 7, 'UserPromptSubmit', prompt='go')
        self.hook('a', 7, 'Stop')
        self.hook('a', 7, 'UserPromptSubmit', prompt='again')
        self.hook('a', 7, 'PermissionRequest', tool_name='Bash', tool_input={'command': 'ls'})
        time.sleep(2.0)
        shown = self.on_screen()
        self.assertEqual(len(shown), 1, f'one notification for one session, got {shown}')


if __name__ == '__main__':
    unittest.main()
