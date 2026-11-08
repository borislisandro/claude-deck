#!/usr/bin/env python3
"""Draws the README's pictures from the widget's own sprites.

The critters are rendered with the same rows and drawing steps as
plasmoid/package/contents/ui/Critter.qml (one frame of each mood), so the pictures
match what the panel shows. Re-run after changing the critter:

    python3 docs/make-art.py
"""
import colorsys
import os
import shutil
import subprocess

HERE = os.path.dirname(os.path.abspath(__file__))
SANS = 'Inter, Noto Sans, sans-serif'
MONO = 'JetBrains Mono, Noto Sans Mono, DejaVu Sans Mono, monospace'
BODY = '#d97757'
EYE = '#1b1112'
COLORS = {'working': '#5fbf7a', 'needs_you': '#f5a524', 'your_turn': '#ff5a6a', 'exited': '#8a7a7c'}
BODY_ROWS = ['.HHBBBBBBB.', '.BBBBBBBBB.', 'BBBBBBBBBBB', '.BBBBBBBBB.', '.MMMMMMMMM.']
LEGS = ['..D.D.D.D..', '..D.D.D.D..']


def hex_rgb(c):
    c = c.lstrip('#')
    return tuple(int(c[i:i + 2], 16) for i in (0, 2, 4))


def rgb_hex(r, g, b):
    return '#%02x%02x%02x' % (round(r), round(g), round(b))


def qt_lighter(c, f):
    """QColor::lighter: scale HSV value; past full brightness, take it from saturation."""
    h, s, v = colorsys.rgb_to_hsv(*[x / 255 for x in hex_rgb(c)])
    s, v = s * 255, v * 255 * f
    if v > 255:
        s, v = max(0, s - (v - 255)), 255
    return rgb_hex(*[x * 255 for x in colorsys.hsv_to_rgb(h, s / 255, v / 255)])


def qt_darker(c, f):
    h, s, v = colorsys.rgb_to_hsv(*[x / 255 for x in hex_rgb(c)])
    return rgb_hex(*[x * 255 for x in colorsys.hsv_to_rgb(h, s, v / f)])


class Canvas:
    """Screen pixels of one critter, drawn like the QML Canvas: later wins."""

    def __init__(self, px):
        self.px = px
        self.pixels = {}

    def rect(self, color, x, y, w, h, alpha=1.0):
        for yy in range(y, y + h):
            for xx in range(x, x + w):
                self.pixels[(xx, yy)] = (color, alpha)

    def cell(self, color, x, y, w=1, h=1, alpha=1.0):
        p = self.px
        self.rect(color, round(x * p), round(y * p), max(1, round(w * p)), max(1, round(h * p)), alpha)

    def clear_cell(self, x, y):
        p = self.px
        for yy in range(round(y * p), round(y * p) + p):
            for xx in range(round(x * p), round(x * p) + p):
                self.pixels.pop((xx, yy), None)

    def sprite(self, rows, ox, oy, pal):
        for r, row in enumerate(rows):
            for c, ch in enumerate(row):
                if ch != '.' and ch in pal:
                    self.cell(pal[ch], ox + c, oy + r)

    def svg(self, ox, oy):
        """Horizontal runs of equal pixels, as rects."""
        out = []
        for y in sorted({p[1] for p in self.pixels}):
            xs = sorted(x for (x, yy) in self.pixels if yy == y)
            i = 0
            while i < len(xs):
                x0, (col, a) = xs[i], self.pixels[(xs[i], y)]
                j = i
                while j + 1 < len(xs) and xs[j + 1] == xs[j] + 1 and self.pixels[(xs[j + 1], y)] == (col, a):
                    j += 1
                op = '' if a >= 1 else f' fill-opacity="{a:.2f}"'
                out.append(f'<rect x="{ox + x0}" y="{oy + y}" width="{xs[j] - x0 + 1}" height="1" fill="{col}"{op}/>')
                i = j + 1
        return '\n'.join(out)


def palette(accent, grey=False):
    if grey:
        return {'B': '#7d6f70', 'H': '#8e8081', 'M': '#6b5f60', 'D': '#564b4c', 'S': '#564b4c', 'A': '#564b4c'}
    return {'B': BODY, 'H': qt_lighter(BODY, 1.2), 'M': qt_darker(BODY, 1.15),
            'D': qt_darker(BODY, 1.5), 'S': qt_darker(BODY, 1.5), 'A': accent}


def eyes_open(cv, oy, dx, upper):
    for x in (4, 8):
        cv.cell(EYE, x + dx, oy + 3, 1, 0.6 if upper else 1)


def eyes_closed(cv, oy, curved):
    for x in (4, 8):
        if curved:
            cv.cell(EYE, x - 0.5, oy + 3.2, 0.5, 0.5)
            cv.cell(EYE, x, oy + 3.55, 1, 0.5)
            cv.cell(EYE, x + 1, oy + 3.2, 0.5, 0.5)
        else:
            cv.cell(EYE, x, oy + 3.55, 1, 0.45)


def draw(mood, px, accent, frame):
    """One frame of Critter.qml's drawStanding / drawRunning / drawThinking / drawResting."""
    cv = Canvas(px)
    grey = mood == 'exited'
    pal = palette(accent, grey)
    f = frame
    if mood == 'running':
        phase = f % 4
        air = phase % 2 == 1
        oy = 0 if air else 1
        cv.sprite(['...A.......' if phase == 2 else '....A......', '.....S.....'], 1, oy, pal)
        cv.sprite(BODY_ROWS, 1, oy + 2, pal)
        cv.clear_cell(1, oy + 4)
        cv.clear_cell(11, oy + 4)
        cv.cell(pal['B'], 1, oy + (3 if phase == 0 else 5 if phase == 2 else 4))
        cv.cell(pal['B'], 11, oy + (3 if phase == 2 else 5 if phase == 0 else 4))
        legs = (['..D.D.D.D..', '.D.D...D.D.'] if phase == 0 else
                ['..D.D.D.D..', '...D.D.D...'] if phase == 2 else ['..D.D.D.D..'])
        cv.sprite(legs, 1, oy + 7, pal)
        eyes_open(cv, oy, 1, False)
        cv.cell('#ffffff', 0, oy + (3 if phase < 2 else 5), 0.8, 0.5, 0.30 if air else 0.18)
        cv.cell('#ffffff', 0.2, oy + (5.5 if phase < 2 else 3.5), 0.5, 0.5, 0.30 if air else 0.18)
        if not air:
            cv.cell('#ffffff', 0.3, oy + 8.4, 0.7, 0.6, 0.32)
            cv.cell('#ffffff', 0, oy + 7.6, 0.5, 0.5, 0.18)
    elif mood == 'thinking':
        oy = 1
        tip_on = (f // 3) % 2 == 0
        cv.cell(accent, 6, oy, 1, 1, 1.0 if tip_on else 0.4)
        cv.sprite(['.....S.....'], 1, oy + 1, pal)
        cv.sprite(BODY_ROWS, 1, oy + 2, pal)
        cv.clear_cell(11, oy + 4)
        cv.cell(pal['B'], 11, oy + 3)
        cv.cell(pal['M'], 10.5, oy + 2.5, 0.5, 0.5)
        cv.sprite(LEGS, 1, oy + 7, pal)
        eyes_open(cv, oy, 1 if (f // 14) % 2 == 0 else 0, True)
        t = f % 34
        cloud = '#f3ead9'
        if t >= 2:
            cv.cell(cloud, 4.4, 2.3, 0.5, 0.5, 0.95)
        if t >= 5:
            cv.cell(cloud, 3.5, 1.6, 0.7, 0.7, 0.95)
        if t >= 8:
            cv.cell(cloud, 0.5, 0, 2, 0.5, 0.95)
            cv.cell(cloud, 0, 0.5, 3, 1.5, 0.95)
            cv.cell(cloud, 0.5, 2, 2, 0.5, 0.95)
            n = 1 + ((t - 8) // 3) % 3
            for i in range(3):
                cv.cell(accent, 0.35 + i * 0.8, 1.0, 0.5, 0.5, 1.0 if i < n else 0.25)
    elif mood == 'resting':
        top = 6 if (f // 12) % 2 == 1 else 7
        cv.cell(pal['S'], 7, top - 1)
        cv.cell(pal['A'], 8, top - 0.6)
        rows = ['.HHBBBBBBB.'] + ['.BBBBBBBBB.' for _ in range(top + 1, 8)] + ['BBBBBBBBBBB', '.MMMMMMMMM.']
        cv.sprite(rows, 1, top, pal)
        eyes_closed(cv, top - 2, True)
        # Z's: the QML draws them in screen pixels; scale them with the sprite here.
        unit = max(1, px // 3)
        for offset in (0, 14):
            q = ((f + offset) % 28) / 28
            x = round((9 + q * 5) * px)
            y = round((top - 1.5 - q * 5.5) * px)
            s = round(3 + q * 3)
            a = q / 0.15 if q < 0.15 else (1 - q) / 0.3 if q > 0.7 else 1
            for i in range(s):
                cv.rect(accent, x + i * unit, y, unit, unit, 0.95 * a)
                cv.rect(accent, x + i * unit, y + (s - 1) * unit, unit, unit, 0.95 * a)
            for i in range(1, s - 1):
                cv.rect(accent, x + (s - 1 - i) * unit, y + i * unit, unit, unit, 0.95 * a)
    else:
        oy = 1
        cv.sprite(['.....A.....', '.....S.....'], 1, oy, pal)
        cv.sprite(BODY_ROWS, 1, oy + 2, pal)
        cv.sprite(LEGS, 1, oy + 7, pal)
        if grey:
            eyes_closed(cv, oy, False)
        else:
            eyes_open(cv, oy, 0, False)
    return cv


def light(cx, cy, size, color, status, seen=False):
    """StatusDot: glow, a ripple caught mid-way for states that want you, the core."""
    out = []
    wants = status == 'needs_you' or (status == 'your_turn' and not seen)
    glow = 0.22 if wants else 0.12 if status == 'working' else 0
    if glow:
        out.append(f'<circle cx="{cx}" cy="{cy}" r="{size * 0.95:.1f}" fill="{color}" fill-opacity="{glow}"/>')
    if wants:
        out.append(f'<circle cx="{cx}" cy="{cy}" r="{size * 0.8:.1f}" fill="none" stroke="{color}" stroke-width="{max(1, size * 0.18):.1f}" stroke-opacity="0.45"/>')
    if status == 'exited':
        out.append(f'<circle cx="{cx}" cy="{cy}" r="{size / 2 - 1:.1f}" fill="none" stroke="{color}" stroke-width="{max(1, size * 0.2):.1f}"/>')
    else:
        out.append(f'<circle cx="{cx}" cy="{cy}" r="{size / 2:.1f}" fill="{color}"/>')
    return '\n'.join(out)


def critter_with_light(mood, status, px, x, y, frame, seen=False):
    """A session on the island: SessionIcon's critter and light. Returns (svg, width)."""
    color = COLORS[status] if status != 'your_turn' or not seen else qt_darker(COLORS['your_turn'], 1.45)
    cv = draw(mood, px, color, frame)
    size = max(6, round(px * 3.4))
    light_cx = x + 12 * px + px * 3 + size / 2
    light_cy = y + (8.6 if mood == 'resting' else 5.5) * px
    svg = cv.svg(x, y) + '\n' + light(light_cx, light_cy, size, color, status, seen)
    return svg, light_cx + size / 2 - x


def island_pill(sessions, px=4, panel_h=64, pill_x=0):
    """The island's pill: critters, lights, wait times, divider and chevron.

    `sessions` is a list of (mood, status, frame, seen, timer). Returns
    (svg, pill_w, pill_y, pill_h, centers) where centers are the critters' x."""
    pill_h = px * 11 + 12
    gap = px * 3
    pad = px * 4
    timer_w = 26
    parts, x = [], 0
    for mood, status, frame, seen, timer in sessions:
        svg, w = critter_with_light(mood, status, px, 0, 0, frame, seen)
        parts.append((svg, x, w, mood, status, seen, timer))
        x += w + gap + (timer_w if timer else 0)
    chevron_w = px * 5 + px * 2 + 1
    pill_w = pad * 2 + x + chevron_w
    pill_y = (panel_h - pill_h) / 2
    crit_y = pill_y + (pill_h - px * 11) / 2
    body, centers = [], []
    for svg, ox, w, mood, status, seen, timer in parts:
        body.append(f'<g transform="translate({pill_x + pad + ox},{crit_y})">{svg}</g>')
        centers.append(pill_x + pad + ox + 6.5 * px)
        if timer:
            color = COLORS[status] if not seen else qt_darker(COLORS[status], 1.45)
            ty = crit_y + (8.6 if mood == 'resting' else 5.5) * px + 6
            body.append(f'<text x="{pill_x + pad + ox + w + 5}" y="{ty}" fill="{color}" font-size="17" font-weight="600" font-family="{SANS}">{timer}</text>')
    # Divider and the popup's chevron (a top panel: it points down).
    cx = pill_x + pill_w - pad - px * 5
    body.append(f'<rect x="{cx - px * 2 - 1}" y="{panel_h / 2 - px * 3}" width="1" height="{px * 6}" fill="#ffffff" fill-opacity="0.18"/>')
    for (gx, gy) in [(0, 0), (4, 0), (1, 1), (3, 1), (2, 2)]:
        body.append(f'<rect x="{cx + gx * px}" y="{panel_h / 2 - px * 1.5 + gy * px}" width="{px}" height="{px}" fill="{COLORS["needs_you"]}"/>')
    attention = any(st == 'needs_you' or (st == 'your_turn' and not se) for _, _, _, _, st, se, _ in parts)
    ring = (f' stroke="{COLORS["needs_you"]}" stroke-opacity="0.75" stroke-width="1.5"' if attention else '')
    bg = [f'<rect x="{pill_x}" y="{pill_y}" width="{pill_w}" height="{pill_h}" rx="{pill_h / 2}" fill="#3a2f31" fill-opacity="0.9"{ring} shape-rendering="geometricPrecision"/>']
    if attention:
        bg.append(f'<rect x="{pill_x}" y="{pill_y}" width="{pill_w}" height="{pill_h}" rx="{pill_h / 2}" fill="{COLORS["needs_you"]}" fill-opacity="0.08" shape-rendering="geometricPrecision"/>')
    return '\n'.join(bg + body), pill_w, pill_y, pill_h, centers


def island_svg():
    panel_h = 64
    pill_x = 360
    monitor_w = 200
    sessions = [('running', 'working', 1, False, None), ('thinking', 'needs_you', 24, False, None),
                ('resting', 'your_turn', 5, False, '4m')]
    pill, pill_w, pill_y, pill_h, _ = island_pill(sessions, 4, panel_h, pill_x)
    width = int(pill_x + pill_w + 14 + monitor_w + 16)
    text = f'font-family="{SANS}" font-size="17"'
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {panel_h}" width="{width}" height="{panel_h}" shape-rendering="crispEdges" role="img" aria-label="The Claude Deck island on a Plasma panel: one session working, one needing you, one waiting for your next prompt">
<rect width="{width}" height="{panel_h}" rx="14" fill="#1d1719"/>
<text x="24" y="{panel_h / 2 + 6}" fill="#e8dfe0" {text}>⌘  Chrome</text>
<text x="170" y="{panel_h / 2 + 6}" fill="#e8dfe0" {text}>Sat 7 Nov  13:33</text>
{pill}
<rect x="{pill_x + pill_w + 14}" y="{pill_y}" width="{monitor_w}" height="{pill_h}" rx="{pill_h / 2}" fill="#3a2f31" fill-opacity="0.9" shape-rendering="geometricPrecision"/>
<text x="{pill_x + pill_w + 34}" y="{panel_h / 2 + 6}" fill="#e8dfe0" {text}>CPU 52°C   GPU 44°C</text>
</svg>
'''


def moods_svg():
    px = 7
    cell_w, height = 214, 168
    moods = [
        ('running', 'working', 0, 'Working', 'thinking or running tools'),
        ('thinking', 'needs_you', 24, 'Needs you', 'a permission, a question, a plan'),
        ('resting', 'your_turn', 5, 'Your turn', 'the turn is over: your move'),
        ('exited', 'exited', 0, 'Exited', 'Claude quit; [r] resumes'),
    ]
    width = cell_w * len(moods)
    out = []
    for i, (mood, status, frame, title, sub) in enumerate(moods):
        svg, w = critter_with_light(mood, status, px, 0, 0, frame)
        ox = i * cell_w + (cell_w - w) / 2
        out.append(f'<g transform="translate({ox:.0f},16)">{svg}</g>')
        tx = i * cell_w + cell_w / 2
        out.append(f'<text x="{tx}" y="122" text-anchor="middle" fill="{COLORS[status]}" font-size="17" font-weight="700">{title}</text>')
        out.append(f'<text x="{tx}" y="146" text-anchor="middle" fill="#bfb2b4" font-size="12.5">{sub}</text>')
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}" shape-rendering="crispEdges" font-family="Inter, Noto Sans, sans-serif" role="img" aria-label="The four critter moods: running while working, thinking when Claude needs you, resting when it is your turn, grey when exited">
<rect width="{width}" height="{height}" rx="16" fill="#1d1719"/>
{chr(10).join(out)}
</svg>
'''


def frames_svg():
    """The running cycle, frame by frame."""
    px = 6
    gap = 26
    w1 = 16 * px
    width = 4 * w1 + 5 * gap
    height = 11 * px + 28
    out = []
    for f in range(4):
        cv = draw('running', px, COLORS['working'], f)
        out.append(f'<g transform="translate({gap + f * (w1 + gap)},14)">{cv.svg(0, 0)}</g>')
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}" shape-rendering="crispEdges" role="img" aria-label="The four frames of the running critter">
<rect width="{width}" height="{height}" rx="14" fill="#1d1719"/>
{chr(10).join(out)}
</svg>
'''


# ---- Scenes for the README ------------------------------------------------
# Made-up sessions only: generic projects under ~/code, nothing from a real machine.

FG = '#e8dfe0'
DIM = '#9a8a8c'
FAINT = '#6b5f60'
ORANGE = '#d97757'
SEEN = qt_darker(COLORS['your_turn'], 1.45)


def esc(t):
    return t.replace('&', '&amp;').replace('<', '&lt;').replace('>', '&gt;')


def icon(x, y, size):
    """The app icon (packaging/claude-deck.svg), placed once per picture (it has ids)."""
    with open(os.path.join(HERE, '..', 'packaging', 'claude-deck.svg')) as f:
        src = f.read()
    inner = src[src.index('>', src.index('<svg')) + 1:src.rindex('</svg>')]
    return f'<svg x="{x}" y="{y}" width="{size}" height="{size}" viewBox="0 0 128 128">{inner}</svg>'


def spans(segments):
    """[(text, color, extra-attrs)] as tspans of one text line."""
    out = []
    for seg in segments:
        t, c = seg[0], seg[1]
        extra = seg[2] if len(seg) > 2 else ''
        out.append(f'<tspan fill="{c}"{extra}>{esc(t)}</tspan>')
    return ''.join(out)


def mono_lines(x, y, lines, size=12.5, lh=19):
    out = []
    for i, line in enumerate(lines):
        if not line:
            continue
        if isinstance(line, str):
            line = [(line, FG)]
        out.append(f'<text x="{x}" y="{y + i * lh}" font-family="{MONO}" font-size="{size}" xml:space="preserve">{spans(line)}</text>')
    return '\n'.join(out)


def deck_svg():
    """The deck: a kitty window, its tab bar, and a tab split into two sessions."""
    W, H = 980, 520
    title_h, bar_h = 36, 28
    cw = 12.5 * 0.6  # a monospace cell
    out = [f'<rect width="{W}" height="{H}" rx="12" fill="#1a1415"/>',
           f'<path d="M0 12a12 12 0 0 1 12-12h{W - 24}a12 12 0 0 1 12 12v{title_h - 12}H0z" fill="#2a2224"/>',
           icon(12, 7, 22),
           f'<text x="{W / 2}" y="23" text-anchor="middle" fill="{FG}" font-family="{SANS}" font-size="13.5" font-weight="600">Claude Deck</text>']
    for i, glyph in enumerate(['M0 0h10', 'M0 0h10v10h-10z', 'M0 0l10 10M10 0l-10 10']):
        bx = W - 76 + i * 24
        out.append(f'<path transform="translate({bx},{13 if i else 18})" d="{glyph}" stroke="{DIM}" stroke-width="1.5" fill="none"/>')
    out.append(f'<rect y="{title_h}" width="{W}" height="{bar_h}" fill="#150f10"/>')
    Y, G, R = COLORS['needs_you'], COLORS['working'], COLORS['your_turn']
    tabs = [
        (True, None, [('◆', Y), ('●', G), (' 1 api +1', '#ffffff', ' font-weight="700"'), (' · Opus 5.5', DIM), (' 2m', Y)]),
        (False, '#4a141b', [('●', R), (' 3 docs', FG), (' · Sonnet 5.5', DIM), (' 4m', R)]),
        (False, None, [('○', SEEN), (' 4 infra', FG), (' · Opus 5.5', DIM)]),
        (False, None, [('●', G), (' 5 ml-pipeline', FG), (' · Sonnet 5.5', DIM)]),
    ]
    tx = 0
    for active, tint, segs in tabs:
        n = sum(len(t[0]) for t in segs)
        w = n * cw + 28
        bg = '#3a2c2f' if active else tint
        if bg:
            out.append(f'<rect x="{tx}" y="{title_h}" width="{w}" height="{bar_h}" fill="{bg}"/>')
        out.append(f'<text x="{tx + 14}" y="{title_h + 18.5}" font-family="{MONO}" font-size="12.5" xml:space="preserve">{spans(segs)}</text>')
        tx += w
        out.append(f'<rect x="{tx}" y="{title_h + 7}" width="1" height="{bar_h - 14}" fill="#ffffff" fill-opacity="0.12"/>')
    top = title_h + bar_h
    mid = W / 2
    out.append(f'<rect x="{mid}" y="{top}" width="1" height="{H - top}" fill="#3a2f31"/>')
    # Left: blocked on a permission prompt.
    lx, ly = 18, top + 30
    out.append(mono_lines(lx, ly, [
        [('> ', DIM), ('Put the new rate limiter behind a feature flag', FG)],
        '',
        [('● ', G), ('Update', FG, ' font-weight="700"'), ('(src/limiter.rs)', FG)],
        [('  ⎿  Updated src/limiter.rs with 42 additions', DIM)],
        [('● ', G), ('Bash', FG, ' font-weight="700"'), ('(cargo test limiter)', FG)],
        [('  ⎿  test result: ok. 18 passed; 0 failed', DIM)],
        [('● ', Y), ('Bash', FG, ' font-weight="700"'), ('(make deploy-staging)', FG)],
    ]))
    by = ly + 7 * 19 - 4
    out.append(f'<rect x="{lx - 6}" y="{by}" width="{mid - 2 * lx + 12}" height="{8 * 19 + 14}" rx="6" fill="none" stroke="{Y}" stroke-opacity="0.7" shape-rendering="geometricPrecision"/>')
    out.append(mono_lines(lx + 6, by + 24, [
        [('Bash command', Y, ' font-weight="700"')],
        [('  make deploy-staging', FG)],
        [('  Deploy the flagged build to staging', DIM)],
        '',
        [('Do you want to proceed?', FG)],
        [('> 1. Yes', Y, ' font-weight="700"')],
        [('  2. Yes, and don’t ask again this session', FG)],
        [('  3. No, and tell Claude what to do', FG)],
    ]))
    # Right: still working, its own model and branch.
    rx = mid + 18
    out.append(mono_lines(rx, ly, [
        [('> ', DIM), ('Wire the checkout page to the pricing API', FG)],
        '',
        [('● ', G), ('Read', FG, ' font-weight="700"'), ('(src/pages/Checkout.tsx)', FG)],
        [('  ⎿  Read 214 lines', DIM)],
        [('● ', G), ('Update', FG, ' font-weight="700"'), ('(src/pages/Checkout.tsx)', FG)],
        [('  ⎿  Updated with 31 additions and 12 removals', DIM)],
        [('● ', G), ('Bash', FG, ' font-weight="700"'), ('(npm test -- checkout)', FG)],
        [('  ⎿  Running…', DIM)],
        '',
        [('✻ Wiring… ', ORANGE), ('(esc to interrupt)', DIM)],
    ]))
    py = H - 78
    out.append(f'<rect x="{rx - 6}" y="{py}" width="{W - rx - 12}" height="30" rx="6" fill="none" stroke="{FAINT}" shape-rendering="geometricPrecision"/>')
    out.append(mono_lines(rx + 6, py + 20, [[('> ', DIM)]]))
    out.append(f'<rect x="{rx + 6 + 2 * cw}" y="{py + 8}" width="{cw}" height="15" fill="{FG}" fill-opacity="0.8"/>')
    for x in (lx, rx):
        out.append(mono_lines(x, H - 22, [[('5h 23% (2h 10m) · 7d 41% (3d 2h)', DIM)]], 11.5))
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" role="img" aria-label="The deck: a kitty window with tabs whose titles show each tab's sessions. The first tab is split: on the left Claude waits for permission to deploy, on the right another session is still working">
{chr(10).join(out)}
</svg>
'''


def card(x, y, w, h, fill='#2a2224', stroke='#ffffff', stroke_op=0.08, rx=10):
    return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{rx}" fill="{fill}" stroke="{stroke}" stroke-opacity="{stroke_op}" shape-rendering="geometricPrecision"/>'


def label(x, y, t, color=FG, size=13, weight=400, anchor='start', italic=False, family=None):
    st = ' font-style="italic"' if italic else ''
    return (f'<text x="{x}" y="{y}" fill="{color}" font-family="{family or SANS}" font-size="{size}" '
            f'font-weight="{weight}" text-anchor="{anchor}"{st}>{esc(t)}</text>')


def dot(cx, cy, status, size=8, seen=False):
    color = SEEN if seen else COLORS[status]
    return light(cx, cy, size, color, status, seen)


def away_svg():
    """You're in your editor; a session needs you: island, hover card, notification."""
    W, H = 1200, 600
    out = [f'<defs><linearGradient id="wall" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#2b1e22"/><stop offset="1" stop-color="#14181f"/></linearGradient>'
           f'<filter id="shadow" x="-10%" y="-10%" width="120%" height="130%"><feDropShadow dx="0" dy="6" stdDeviation="9" flood-opacity="0.45"/></filter></defs>',
           f'<rect width="{W}" height="{H}" rx="16" fill="url(#wall)"/>']
    # The editor you're actually in: abstract code, nothing to read.
    ex, ey, ew, eh = 40, 96, W - 80, H - 126
    out.append(f'<g filter="url(#shadow)">{card(ex, ey, ew, eh, "#191b22", "#ffffff", 0.06, 10)}</g>')
    out.append(f'<path d="M{ex} {ey + 10}a10 10 0 0 1 10-10h{ew - 20}a10 10 0 0 1 10 10v20H{ex}z" fill="#22252e"/>')
    out.append(label(ex + ew / 2, ey + 20, 'checkout.tsx — editor', DIM, 12.5, 500, 'middle'))
    out.append(f'<rect x="{ex}" y="{ey + 30}" width="180" height="{eh - 30}" fill="#1d2028"/>')
    for i in range(14):
        out.append(f'<rect x="{ex + 20 + (i % 3) * 12}" y="{ey + 52 + i * 22}" width="{70 + (i * 37) % 70}" height="8" rx="4" fill="#ffffff" fill-opacity="0.07"/>')
    palette_ = ['#c792ea', '#82aaff', '#c3e88d', '#f78c6c', '#89ddff', '#ffcb6b']
    for i in range(18):
        indent = [0, 1, 2, 2, 3, 2, 1, 1, 2, 3, 3, 2, 1, 0, 0, 1, 2, 1][i]
        x = ex + 210 + indent * 24
        out.append(label(ex + 200, ey + 58 + i * 22, str(i + 1), FAINT, 11, 400, 'end', family=MONO))
        for j in range(1 + (i * 7) % 3):
            wj = 30 + (i * 13 + j * 29) % 110
            out.append(f'<rect x="{x}" y="{ey + 50 + i * 22}" width="{wj}" height="9" rx="4" fill="{palette_[(i + j) % 6]}" fill-opacity="0.55"/>')
            x += wj + 10
    # The panel, with the island in it.
    panel_h = 64
    px0, py0 = 12, 10
    out.append(f'<rect x="{px0}" y="{py0}" width="{W - 24}" height="{panel_h}" rx="14" fill="#1d1719"/>')
    text = f'font-family="{SANS}" font-size="17" fill="{FG}"'
    out.append(f'<text x="{px0 + 24}" y="{py0 + 38}" {text}>⌘   Editor</text>')
    sessions = [('thinking', 'needs_you', 24, False, '2m'), ('running', 'working', 1, False, None),
                ('resting', 'your_turn', 5, False, '4m'), ('resting', 'your_turn', 17, True, None),
                ('running', 'working', 3, False, None)]
    pill_x = 330
    pill, pill_w, pill_y, pill_h, centers = island_pill(sessions, 4, panel_h, pill_x)
    out.append(f'<g transform="translate({px0},{py0})" shape-rendering="crispEdges">{pill}</g>')
    out.append(f'<text x="{W - 40}" y="{py0 + 38}" text-anchor="end" {text}>Sat 7 Nov  13:33</text>')
    # The hover card under the yellow critter.
    hx, hy, hw = px0 + centers[0] - 70, py0 + panel_h + 12, 440
    hh = 226
    out.append(f'<g filter="url(#shadow)">{card(hx, hy, hw, hh, "#2a2224", "#ffffff", 0.1, 12)}</g>')
    out.append(f'<path d="M{px0 + centers[0] - 8} {hy + 0.5}l8 -8l8 8z" fill="#2a2224"/>')
    cv = draw('thinking', 2, COLORS['needs_you'], 24)
    out.append(f'<g transform="translate({hx + 16},{hy + 14})" shape-rendering="crispEdges">{cv.svg(0, 0)}</g>')
    out.append(dot(hx + 50, hy + 25, 'needs_you'))
    out.append(label(hx + 64, hy + 30, '1  api', FG, 15, 700))
    out.append(label(hx + hw - 16, hy + 30, 'Needs you · 2m', COLORS['needs_you'], 13, 600, 'end'))
    out.append(label(hx + 16, hy + 58, 'Opus 5.5   ⎇ feat/rate-limit   ~/code/api', DIM, 12))
    out.append(label(hx + 16, hy + 82, 'Wants to: Running make deploy-staging', FG, 13.5))
    out.append(label(hx + 16, hy + 104, '“Put the new rate limiter behind a feature flag”', DIM, 13, italic=True))
    out.append(f'<rect x="{hx + 16}" y="{hy + 118}" width="{hw - 32}" height="92" rx="6" fill="#ffffff" fill-opacity="0.04" stroke="{COLORS["needs_you"]}" stroke-opacity="0.35" shape-rendering="geometricPrecision"/>')
    for i, t in enumerate(['The limiter is behind `RATE_LIMIT_V2` and off by', 'default. All 18 limiter tests pass.', '',
                           'Deploying to staging so we can flip it there first.']):
        if t:
            out.append(label(hx + 28, hy + 140 + i * 18, t, FG, 12, family=MONO if False else SANS))
    # The notification.
    nx, ny, nw, nh = W - 40 - 380, py0 + panel_h + 12, 380, 150
    out.append(f'<g filter="url(#shadow)">{card(nx, ny, nw, nh, "#2a2224", "#ffffff", 0.1, 12)}</g>')
    out.append(icon(nx + 14, ny + 12, 22))
    out.append(label(nx + 42, ny + 28, 'Claude Deck', DIM, 12, 500))
    out.append(label(nx + nw - 16, ny + 28, 'now', DIM, 12, 400, 'end'))
    out.append(label(nx + 16, ny + 56, 'api needs you', FG, 15, 700))
    out.append(label(nx + 16, ny + 78, 'Wants to: Running make deploy-staging', FG, 13))
    out.append(label(nx + 16, ny + 97, 'Put the new rate limiter behind a feature flag', DIM, 12.5))
    out.append(f'<rect x="{nx + 16}" y="{ny + 110}" width="{nw - 32}" height="28" rx="6" fill="#ffffff" fill-opacity="0.07" shape-rendering="geometricPrecision"/>')
    out.append(label(nx + nw / 2, ny + 129, 'Open', FG, 13, 600, 'middle'))
    # A pointer resting on the yellow critter.
    cx, cy = px0 + centers[0] + 10, py0 + 44
    out.append(f'<path transform="translate({cx},{cy})" d="M0 0v17l4.5-4.2 3 6.6 3-1.4-3-6.4h6.2z" fill="#ffffff" stroke="#000" stroke-width="1" shape-rendering="geometricPrecision"/>')
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" role="img" aria-label="Working in an editor while the island on the panel shows five sessions. Hovering the yellow one shows what it wants, and a notification offers to open it">
{chr(10).join(out)}
</svg>
'''


def popup_svg():
    """The island's popup: sessions grouped the way the deck has them."""
    W, H = 520, 612
    Y, G, R = COLORS['needs_you'], COLORS['working'], COLORS['your_turn']
    out = [f'<defs><filter id="shadow" x="-10%" y="-10%" width="120%" height="130%"><feDropShadow dx="0" dy="6" stdDeviation="8" flood-opacity="0.4"/></filter></defs>',
           f'<rect width="{W}" height="{H}" rx="16" fill="#1d1719"/>']
    out.append(label(20, 38, 'Claude Deck', FG, 17, 700))
    out.append(label(20, 60, '2 working · 1 needs you · 2 your turn', DIM, 12.5))
    for i, glyph in enumerate(['M5 0v10M0 5h10', 'M0 0h4v10h-4zM6 0h4v10h-4z', 'M0 3h10M7 0l3 3-3 3M10 8H0M3 5l-3 3 3 3', 'M0 0h10v10h-10zM0 3h10']):
        bx = W - 128 + i * 28
        out.append(f'<rect x="{bx - 7}" y="23" width="24" height="24" rx="5" fill="#ffffff" fill-opacity="0.05"/>')
        out.append(f'<path transform="translate({bx},30)" d="{glyph}" stroke="{FG}" stroke-width="1.4" fill="none" stroke-linecap="round" stroke-linejoin="round"/>')
    x, w = 14, W - 28
    # Tab 1: two sessions, opened up.
    y = 80
    out.append(card(x, y, w, 132, '#2a2224', Y, 0.4))
    out.append(dot(x + 22, y + 24, 'needs_you'))
    out.append(label(x + 38, y + 29, 'Tab 1', FG, 14, 700))
    out.append(label(x + 88, y + 29, 'api, web', DIM, 13))
    out.append(label(x + w - 128, y + 29, '◆ 1', Y, 12.5, 600))
    out.append(label(x + w - 96, y + 29, '● 1', G, 12.5, 600))
    out.append(label(x + w - 16, y + 29, 'Opus 5.5', DIM, 12, 400, 'end'))
    out.append(f'<rect x="{x + 12}" y="{y + 44}" width="{w - 24}" height="1" fill="#ffffff" fill-opacity="0.08"/>')
    rows = [('needs_you', False, '1  api', 'Wants to: Running make deploy-staging', 'Needs you · 2m'),
            ('working', False, '2  web', 'Running npm test -- checkout', 'Working')]
    for i, (st, seen, name, act, state) in enumerate(rows):
        ry = y + 70 + i * 36
        if i == 0:
            out.append(f'<rect x="{x + 8}" y="{ry - 18}" width="{w - 16}" height="32" rx="6" fill="#ffffff" fill-opacity="0.05"/>')
        out.append(dot(x + 26, ry - 3, st, 7, seen))
        out.append(label(x + 40, ry + 2, name, FG, 13, 600))
        out.append(label(x + 100, ry + 2, act, DIM, 12.5))
        out.append(label(x + w - 16, ry + 2, state, COLORS[st], 12, 600, 'end'))
    # Tab 2: one session, opened to its latest message and prompt box.
    y += 146
    out.append(card(x, y, w, 236, '#2a2224', R, 0.3))
    cv = draw('resting', 2, R, 5)
    out.append(f'<g transform="translate({x + 14},{y + 8})" shape-rendering="crispEdges">{cv.svg(0, 0)}</g>')
    out.append(label(x + 48, y + 29, 'Tab 2', DIM, 12.5))
    out.append(label(x + 92, y + 29, 'docs', FG, 14, 700))
    out.append(label(x + w - 16, y + 29, 'Your turn  4m', R, 13, 600, 'end'))
    out.append(label(x + 48, y + 51, 'Updated README.md, docs/install.md', DIM, 12.5))
    out.append(label(x + 48, y + 70, '⎇ main     Sonnet 5.5', FAINT, 12))
    out.append(f'<rect x="{x + 14}" y="{y + 84}" width="{w - 28}" height="96" rx="6" fill="#ffffff" fill-opacity="0.04"/>')
    for i, t in enumerate(['Both guides now cover the new install script.', '',
                           '• Added a Fedora and an Arch section', '• Moved troubleshooting to its own page']):
        if t:
            out.append(label(x + 26, y + 106 + i * 19, t, FG, 12.5))
    out.append(f'<rect x="{x + 14}" y="{y + 192}" width="{w - 28}" height="32" rx="6" fill="#000" fill-opacity="0.25" stroke="#ffffff" stroke-opacity="0.12" shape-rendering="geometricPrecision"/>')
    out.append(label(x + 26, y + 213, 'Prompt docs…  (Enter sends)', FAINT, 12.5))
    # Tab 3: seen, so dimmed. Tab 4: working.
    y += 250
    for i, (st, seen, tab, name, act, state, model) in enumerate([
            ('your_turn', True, 'Tab 3', 'infra', 'Ran terraform plan', 'Your turn', 'Opus 5.5'),
            ('working', False, 'Tab 4', 'ml-pipeline', 'Running pytest tests/test_features.py', 'Working', 'Sonnet 5.5')]):
        cy = y + i * 64
        out.append(card(x, cy, w, 56))
        mood = 'resting' if st == 'your_turn' else 'running'
        cv = draw(mood, 2, SEEN if seen else COLORS[st], 3)
        out.append(f'<g transform="translate({x + 14},{cy + 8})" shape-rendering="crispEdges">{cv.svg(0, 0)}</g>')
        out.append(label(x + 48, cy + 24, tab, DIM, 12.5))
        out.append(label(x + 92, cy + 24, name, FG, 14, 700))
        out.append(label(x + w - 16, cy + 24, state, SEEN if seen else COLORS[st], 13, 600, 'end'))
        out.append(label(x + 48, cy + 44, act, DIM, 12.5))
        out.append(label(x + w - 16, cy + 44, model, FAINT, 12, 400, 'end'))
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" role="img" aria-label="The island's popup: sessions grouped by deck tab. Tab 1 holds two sessions, one needing you; tab 2's latest message is open with a box to prompt it">
{chr(10).join(out)}
</svg>
'''


def social_svg():
    """GitHub's social preview (1280x640): upload docs/social-preview.png in the repo settings."""
    W, H = 1280, 640
    out = [f'<defs><radialGradient id="glow" cx="0.22" cy="0.45" r="0.6"><stop offset="0" stop-color="{ORANGE}" stop-opacity="0.28"/><stop offset="1" stop-color="{ORANGE}" stop-opacity="0"/></radialGradient></defs>',
           f'<rect width="{W}" height="{H}" fill="#1d1719"/>', f'<rect width="{W}" height="{H}" fill="url(#glow)"/>',
           icon(70, 120, 360),
           label(470, 260, 'claude-deck', '#faf9f5', 92, 800),
           label(474, 320, 'Many Claude Code sessions, one terminal deck,', FG, 30, 500),
           label(474, 362, 'and a live status island on your KDE Plasma panel.', FG, 30, 500)]
    moods = [('running', 'working', 0, 'Working'), ('thinking', 'needs_you', 24, 'Needs you'),
             ('resting', 'your_turn', 5, 'Your turn'), ('exited', 'exited', 0, 'Exited')]
    for i, (mood, status, frame, name) in enumerate(moods):
        svg, w = critter_with_light(mood, status, 5, 0, 0, frame)
        ox = 474 + i * 190
        out.append(f'<g transform="translate({ox},430)" shape-rendering="crispEdges">{svg}</g>')
        out.append(label(ox, 520, name, COLORS[status], 20, 700))
    out.append(label(474, 590, 'kitty  ·  Plasma 6  ·  Wayland & X11  ·  Rust', DIM, 20, 500))
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}">
{chr(10).join(out)}
</svg>
'''


if __name__ == '__main__':
    for name, svg in [('island.svg', island_svg()), ('moods.svg', moods_svg()), ('running.svg', frames_svg()),
                      ('deck.svg', deck_svg()), ('away.svg', away_svg()), ('popup.svg', popup_svg()),
                      ('social-preview.svg', social_svg())]:
        with open(os.path.join(HERE, name), 'w') as f:
            f.write(svg)
        print('wrote', os.path.join('docs', name))
    # GitHub takes the social preview as a PNG only; rasterize it when ImageMagick is around.
    if shutil.which('magick'):
        subprocess.run(['magick', '-density', '96', os.path.join(HERE, 'social-preview.svg'),
                        os.path.join(HERE, 'social-preview.png')], check=True)
        print('wrote', os.path.join('docs', 'social-preview.png'))
