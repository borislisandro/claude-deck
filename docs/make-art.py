#!/usr/bin/env python3
"""Draws the README's pictures from the widget's own sprites.

The critters are rendered with the same rows and drawing steps as
plasmoid/package/contents/ui/Critter.qml (one frame of each mood), so the pictures
match what the panel shows. Re-run after changing the critter:

    python3 docs/make-art.py
"""
import colorsys
import os

HERE = os.path.dirname(os.path.abspath(__file__))
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


def island_svg():
    px = 4
    panel_h = 64
    pill_h = px * 11 + 12
    sessions = [('running', 'working', 1, False), ('thinking', 'needs_you', 24, False), ('resting', 'your_turn', 5, False)]
    gap = px * 3
    pad = px * 4
    parts, x = [], 0
    for mood, status, frame, seen in sessions:
        svg, w = critter_with_light(mood, status, px, 0, 0, frame, seen)
        parts.append((svg, x, w))
        x += w + gap
    timer_w = 26
    chevron_w = px * 5 + px * 2 + 1
    pill_w = pad * 2 + x + timer_w + chevron_w
    pill_x = 360
    monitor_w = 200
    width = int(pill_x + pill_w + 14 + monitor_w + 16)
    pill_y = (panel_h - pill_h) / 2
    crit_y = pill_y + (pill_h - px * 11) / 2
    body = []
    for svg, ox, _ in parts:
        body.append(f'<g transform="translate({pill_x + pad + ox},{crit_y})">{svg}</g>')
    # The red session waited a while: its timer, in its color.
    last_end = pill_x + pad + parts[-1][1] + parts[-1][2]
    body.append(f'<text x="{last_end + 5}" y="{crit_y + 8.6 * px + 6}" fill="{COLORS["your_turn"]}" font-size="17" font-weight="600" font-family="Inter, Noto Sans, sans-serif">4m</text>')
    # Divider and the popup's chevron (a top panel: it points down).
    cx = pill_x + pill_w - pad - px * 5
    body.append(f'<rect x="{cx - px * 2 - 1}" y="{panel_h / 2 - px * 3}" width="1" height="{px * 6}" fill="#ffffff" fill-opacity="0.18"/>')
    for (gx, gy) in [(0, 0), (4, 0), (1, 1), (3, 1), (2, 2)]:
        body.append(f'<rect x="{cx + gx * px}" y="{panel_h / 2 - px * 1.5 + gy * px}" width="{px}" height="{px}" fill="{COLORS["needs_you"]}"/>')
    text = 'font-family="Inter, Noto Sans, sans-serif" font-size="17"'
    return f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {panel_h}" width="{width}" height="{panel_h}" shape-rendering="crispEdges" role="img" aria-label="The Claude Deck island on a Plasma panel: one session working, one needing you, one waiting for your next prompt">
<rect width="{width}" height="{panel_h}" rx="14" fill="#1d1719"/>
<text x="24" y="{panel_h / 2 + 6}" fill="#e8dfe0" {text}>⌘  Chrome</text>
<text x="170" y="{panel_h / 2 + 6}" fill="#e8dfe0" {text}>Sat 7 Nov  13:33</text>
<rect x="{pill_x}" y="{pill_y}" width="{pill_w}" height="{pill_h}" rx="{pill_h / 2}" fill="#3a2f31" fill-opacity="0.9" stroke="{COLORS["needs_you"]}" stroke-opacity="0.75" stroke-width="1.5" shape-rendering="geometricPrecision"/>
<rect x="{pill_x}" y="{pill_y}" width="{pill_w}" height="{pill_h}" rx="{pill_h / 2}" fill="{COLORS["needs_you"]}" fill-opacity="0.08" shape-rendering="geometricPrecision"/>
{chr(10).join(body)}
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


if __name__ == '__main__':
    for name, svg in [('island.svg', island_svg()), ('moods.svg', moods_svg()), ('running.svg', frames_svg())]:
        with open(os.path.join(HERE, name), 'w') as f:
            f.write(svg)
        print('wrote', os.path.join('docs', name))
