"""Window chrome for the in-game windows (`crates/dusk_client/src/windows.rs`, `journal.rs`),
in the "iron & oak" look of `ui.py`:

- `ui_close_{idle,hover,press}.png` (26x26) and `ui_close_small_*` (22x22): the close box
  every window carries in its title bar.
- `ui_journal.png` (580x460): quest journal. Quest list well inner (24, 74) 186x318, detail
  well inner (226, 74) 330x318, Track button at (226, 406) 120x30, close box at (530, 23).
- `ui_btn_{track,untrack}_{idle,hover,press}.png` (120x30): the journal's Track button.
- `ui_micro_bar.png` (180x44) with `ui_micro_{idle,hover,press}.png` (32x32) plates at
  (6 + 34 i, 6) and `ui_micro_icon_<name>.png` (22x22) icons: the micro-menu next to the
  action bar (character, inventory, abilities, journal, menu).

Output: `assets/content/ui/` (our own names, always available).

    python -I tools/artgen/windows_ui.py
"""

from __future__ import annotations

import math
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).parent))
import ui  # noqa: E402
from ui import (  # noqa: E402
    Canvas,
    _ell,
    _poly,
    _rect,
    band,
    button_plate,
    close_x,
    ic_head,
    icon_mask,
    metal,
    panel,
    rivet,
    stamp_icon,
    title_bar,
    well,
)

ui.OUT = ui.ROOT / "assets" / "content" / "ui"

STATES = ("idle", "hover", "press")


def close_buttons():
    for size, name in ((26, "ui_close"), (22, "ui_close_small")):
        for i, st in enumerate(STATES):
            c = Canvas(size, size)
            close_x(c, 0, 0, size, st, 300 + i)
            c.save(f"{name}_{st}")


def journal():
    c = Canvas(580, 460)
    panel(c, 8, 10, 572, 452, 311)
    title_bar(c, 14, 14, 566, 58, "JOURNAL", 312)
    well(c, 24, 74, 186, 318, rim=2)
    well(c, 226, 74, 330, 318, rim=2)
    # a thin iron rule under the detail title area
    rule = c.rect(236, 120, 310, 2)
    c.paint(rule, "iron", metal(c, 313, 0.35))
    rivet(c, 233, 121, 1.3)
    rivet(c, 549, 121, 1.3)
    c.save("ui_journal")


def track_buttons():
    for label, name in (("TRACK", "track"), ("UNTRACK", "untrack")):
        for i, st in enumerate(STATES):
            c = Canvas(120, 30)
            button_plate(c, 0, 0, 120, 30, label, st, 320 + i)
            c.save(f"ui_btn_{name}_{st}")


# --- micro-menu ---------------------------------------------------------------------------


def ic_bag(d, k):
    _poly(d, k, [(30, 30), (70, 30), (88, 62), (84, 92), (16, 92), (12, 62)])
    _rect(d, k, (36, 16, 64, 30))
    _rect(d, k, (30, 22, 70, 30), 0)
    d.line([(30 * k, 24 * k), (70 * k, 24 * k)], fill=255, width=int(6 * k))
    _ell(d, k, (42, 52, 58, 68), 0)


def ic_book(d, k):
    _poly(d, k, [(4, 24), (44, 32), (44, 94), (4, 86)])
    _poly(d, k, [(96, 24), (56, 32), (56, 94), (96, 86)])
    for y in (46, 64):
        d.line([(12 * k, y * k), (36 * k, (y + 5) * k)], fill=0, width=int(6 * k))
        d.line([(64 * k, (y + 5) * k), (88 * k, y * k)], fill=0, width=int(6 * k))
    # a spark over the pages
    _poly(d, k, [(50, 0), (55, 10), (66, 13), (55, 16), (50, 26), (45, 16), (34, 13), (45, 10)])


def ic_scroll(d, k):
    _rect(d, k, (22, 14, 78, 86))
    _ell(d, k, (12, 6, 34, 26))
    _ell(d, k, (66, 74, 88, 94))
    for y in (32, 46, 60):
        d.line([(32 * k, y * k), (68 * k, y * k)], fill=0, width=int(4 * k))


def ic_cog(d, k):
    cx, cy = 50, 50
    for i in range(8):
        a = i * math.pi / 4
        pts = []
        for da, r in ((-0.2, 30), (-0.14, 46), (0.14, 46), (0.2, 30)):
            pts.append((cx + r * math.cos(a + da), cy + r * math.sin(a + da)))
        _poly(d, k, pts)
    _ell(d, k, (18, 18, 82, 82))
    _ell(d, k, (38, 38, 62, 62), 0)


MICRO_ICONS = {
    "character": ic_head,
    "inventory": ic_bag,
    "abilities": ic_book,
    "journal": ic_scroll,
    "menu": ic_cog,
}


def micro_menu():
    n = len(MICRO_ICONS)
    w = 6 + 34 * n + 4
    c = Canvas(w, 44)
    band(c, 0, 0, w, 44, 4, "iron", 330)
    inner = c.rect(4, 4, w - 8, 36)
    c.paint(inner, "oak", 0.22 + 0.1 * (ui.vnoise(c.h, c.w, 4, 4, 331) - 0.5))
    for x in (2.5, w - 3.5):
        rivet(c, x, 22, 1.2)
    c.save("ui_micro_bar")
    for i, st in enumerate(STATES):
        c = Canvas(32, 32)
        button_plate(c, 0, 0, 32, 32, None, st, 340 + i, ramp="iron")
        c.save(f"ui_micro_{st}")
    for name, fn in MICRO_ICONS.items():
        c = Canvas(22, 22)
        stamp_icon(c, icon_mask(18, fn), 2, 2, ramp="brass", base=0.5)
        c.save(f"ui_micro_icon_{name}")
    # keep `ic_head` imported for the character icon
    assert ic_head is MICRO_ICONS["character"]


def main():
    close_buttons()
    journal()
    track_buttons()
    micro_menu()
    print(f"{len(ui.SAVED)} images -> {ui.OUT}")


if __name__ == "__main__":
    main()
