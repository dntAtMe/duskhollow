"""Item and spell icons (40x40) for our own data.

Usage (from repo root):  python -I tools/artgen/icons.py [--force] [--only <substring>] [--no-preview]

Reads `assets/data/spells.txt`, `items.txt` and `item_bases.txt` (whichever exist; the
`[id]` / `[kind id]` + `key=value` text format) and collects every `icon=` name. An icon that
already exists anywhere under `assets/content/` (bare-name lookup, as in the client) is kept;
a missing one is generated into `assets/content/icons/{items,spells}/`. `--force`
regenerates every referenced icon that lives in those two folders (plus missing ones), `--only`
limits the run to icon names containing the substring. Contact sheets go to `assets/preview/`.

- Items (`icon_items.py`): voxel models (weapons, armour by family and slot, flasks, rings, gems,
  scrolls, food, junk ...) on a dark dithered card tinted by item quality, with a bevelled frame in
  the quality colour. The motif comes from the item's `model`, then the icon name, then the data
  (`equip_type`, `weapon_type`, `armor_type`) and name keywords. Item bases (generated gear) are
  drawn at their `quality=` (default 2: the plain card).
- Spells (`icon_spells.py`): symbolic motifs chosen from the spell's name / icon name / description
  by keyword rules (`DUSK_RULES` first), painted in the palette of its school.
"""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).parent))
import icon_items  # noqa: E402
import icon_spells  # noqa: E402
from iconlib import Canvas  # noqa: E402

from paths import CONTENT, DATA, PREVIEW, ROOT  # noqa: E402,F401
from paths import ICONS as OUT  # noqa: E402
SIZE = 40

# quality -> (background ramp, frame light, frame dark)
QUALITY_FRAME = {
    1: ("q1", (110, 110, 110), (44, 44, 44)),
    2: ("q2", (150, 132, 100), (58, 50, 38)),
    3: ("q3", (86, 170, 64), (26, 64, 22)),
    4: ("q4", (84, 136, 214), (24, 46, 90)),
    5: ("q5", (226, 176, 58), (96, 66, 16)),
    6: ("q6", (170, 104, 226), (62, 30, 96)),
}


# --- data ---------------------------------------------------------------------------------------------


def sections(path: Path) -> list[tuple[str | None, str, dict]]:
    """(kind, id, {key: [values]}) per `[id]` / `[kind id]` section; '#' comments."""
    if not path.exists():
        return []
    out = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("[") and line.endswith("]"):
            head = line[1:-1].split()
            kind, sid = (head[0], head[1]) if len(head) > 1 else (None, head[0])
            out.append((kind, sid, {}))
            continue
        if not out or "=" not in line:
            continue
        k, v = (s.strip() for s in line.split("=", 1))
        out[-1][2].setdefault(k, []).append(v)
    return out


def first(d: dict, k: str, default=""):
    return d.get(k, [default])[-1]


def as_int(v, default=0):
    try:
        return int(str(v).strip())
    except ValueError:
        return default


def spell_jobs() -> list[dict]:
    jobs = []
    for _, sid, d in sections(DATA / "spells.txt"):
        icon = first(d, "icon")
        if not icon:
            continue
        effects = [as_int(first(d, f"effect{i}")) for i in (1, 2, 3)]
        jobs.append({
            "kind": "spells", "icon": icon, "id": sid,
            "info": {"name": first(d, "name"), "description": first(d, "description"),
                     "school": as_int(first(d, "school", "1"), 1), "effects": [e for e in effects if e],
                     "positive": first(d, "effect1_positive") == "1"},
        })
    return jobs


def item_jobs() -> list[dict]:
    jobs = []
    for path, base in ((DATA / "items.txt", False), (DATA / "item_bases.txt", True)):
        for _, sid, d in sections(path):
            # Item bases list one icon (and name) per quality, 2 (plain) .. 6 (purple).
            icons = [i.strip() for i in first(d, "icon").split(",")] if base else [first(d, "icon")]
            names = [n.strip() for n in first(d, "name").split(",")] if base else [first(d, "name")]
            for k, icon in enumerate(icons):
                if not icon:
                    continue
                jobs.append({
                    "kind": "items", "icon": icon, "id": sid,
                    "info": {"name": names[min(k, len(names) - 1)], "model": first(d, "model"),
                             "quality": 2 + k if base else as_int(first(d, "quality", "2"), 2),
                             "equip_type": first(d, "equip_type"), "weapon_type": first(d, "weapon_type"),
                             "armor_type": first(d, "armor_type"), "base": base},
                })
    return jobs


def existing_icons() -> dict[str, Path]:
    """Lowercase bare name -> file, for every PNG under our content."""
    return {p.name.lower(): p for p in CONTENT.rglob("*.png")}


# --- drawing ------------------------------------------------------------------------------------------


def item_image(stem: str, info: dict, n: int, spells: dict) -> np.ndarray:
    q = info.get("quality") or 2
    bg, light, dark = QUALITY_FRAME.get(q, QUALITY_FRAME[2])
    cv = Canvas(n, bg)
    if stem.lower().startswith("scroll_"):
        spell_stem = stem[len("scroll_"):]
        prims, rot, fit = icon_items.scroll("redmark", "redmark")
        cv.obj(prims, rot, fit=0.78, off=(0.12, -0.1))
        inset = spell_image(spell_stem, spells.get(spell_stem.lower(), {"name": spell_stem}), 19)
        cv.layers.append(_place(inset, n, 2, 2))
    else:
        prims, rot, fit = icon_items.item_icon(stem, info)
        cv.obj(prims, rot, fit=fit)
    return cv.finish(light, dark)


def _place(img, n, x, y):
    out = np.zeros((n, n, 4), np.uint8)
    h, w = img.shape[:2]
    out[y : y + h, x : x + w] = img
    return out


def spell_image(stem: str, info: dict, n: int) -> np.ndarray:
    return icon_spells.spell_icon(stem, info, n)


def contact_sheet(images: list[tuple[str, np.ndarray]], path: Path, cols=16, zoom=2):
    if not images:
        return
    n = images[0][1].shape[0]
    cell = n * zoom + 4
    rows = (len(images) + cols - 1) // cols
    sheet = Image.new("RGB", (cols * cell + 4, rows * cell + 4), (24, 22, 20))
    for i, (_, img) in enumerate(images):
        im = Image.fromarray(img).resize((img.shape[1] * zoom, img.shape[0] * zoom), Image.NEAREST)
        sheet.paste(im, (4 + (i % cols) * cell, 4 + (i // cols) * cell))
    path.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(path)


def main(argv):
    force = "--force" in argv
    only = argv[argv.index("--only") + 1].lower() if "--only" in argv else None
    spells, items = spell_jobs(), item_jobs()
    spell_info = {j["icon"][:-4].lower(): j["info"] for j in spells}
    have = existing_icons()
    done = {"items": [], "spells": []}
    seen = set()
    kept = 0
    for job in spells + items:
        icon, kind = job["icon"], job["kind"]
        key = (kind, icon.lower())
        if key in seen or (only and only not in icon.lower()):
            continue
        seen.add(key)
        out = OUT / kind / icon
        current = have.get(icon.lower())
        ours = current is not None and current.parent == out.parent
        if current is not None and not (force and ours):
            kept += 1
            continue
        stem = icon[:-4]
        if kind == "items":
            img = item_image(stem, job["info"], SIZE, spell_info)
        else:
            img = spell_image(stem, job["info"], SIZE)
        out.parent.mkdir(parents=True, exist_ok=True)
        Image.fromarray(img).save(out, optimize=True)
        detail = ""
        if kind == "spells":
            detail = f" ({icon_spells.pick_recipe(stem, job['info'])[1]})"
        print(f"{kind} {job['id']} {job['info']['name']}: {icon}{detail}")
        done[kind].append((icon, img))
    print(f"generated items: {len(done['items'])}, spells: {len(done['spells'])}; kept existing: {kept}")
    if "--no-preview" not in argv:
        contact_sheet(done["items"], PREVIEW / "icons_new_items.png", cols=10, zoom=4)
        contact_sheet(done["spells"], PREVIEW / "icons_new_spells.png", cols=10, zoom=4)


if __name__ == "__main__":
    main(sys.argv[1:])
