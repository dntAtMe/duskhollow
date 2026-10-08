"""Our own item and spell icons (40x40), replacing the originals with `--art custom`.

Usage (from repo root):  python -I tools/artgen/icons.py [--only <substring>] [--no-preview]

Reads the ORIGINAL data only for names and numbers: `assets/game.db` (`item_template.icon`, name,
quality, model; `spell_template.icon`, name, description, cast_school, effects) and the file
listings / pixel sizes of `assets/content/item_icons_new` and `spell_icons_new` (found via
DUSK_ASSETS or an `assets/` folder above this repo). No original pixels are read or reused.

Writes one PNG per original icon file name (same size) into
`custom_assets/content/override/icons/{items,spells}/` -- the client resolves images by bare file
name, so these replace the originals -- plus contact sheets in `custom_assets/preview/`.

- Items (`icon_items.py`): real voxel models (weapons, armour by family and slot, flasks, rings,
  gems, orbs, scrolls, food, junk ...) on a dark dithered card tinted by item quality, with a
  bevelled frame in the quality colour. `scroll_<spell>.png` items are a parchment scroll with that
  spell's icon inset in the top-left corner.
- Spells (`icon_spells.py`): symbolic motifs (flames, ice shards, holy light, skulls, arrows,
  swords, shields, hands, wings ...) chosen from the spell's name / icon name / description by
  keyword rules, painted in the palette of its school.
"""

from __future__ import annotations

import os
import sqlite3
import sys
from collections import Counter
from pathlib import Path

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).parent))
import icon_items  # noqa: E402
import icon_spells  # noqa: E402
from iconlib import Canvas, rgb  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "custom_assets" / "content" / "override" / "icons"
PREVIEW = ROOT / "custom_assets" / "preview"

# quality -> (background ramp, frame light, frame dark)
QUALITY_FRAME = {
    1: ("q1", (110, 110, 110), (44, 44, 44)),
    2: ("q2", (150, 132, 100), (58, 50, 38)),
    3: ("q3", (86, 170, 64), (26, 64, 22)),
    4: ("q4", (84, 136, 214), (24, 46, 90)),
    5: ("q5", (226, 176, 58), (96, 66, 16)),
    6: ("q6", (170, 104, 226), (62, 30, 96)),
}


def find_assets() -> Path:
    cands = [Path(os.environ["DUSK_ASSETS"])] if os.environ.get("DUSK_ASSETS") else []
    p = ROOT
    for _ in range(6):
        cands.append(p / "assets")
        p = p.parent
    for c in cands:
        if (c / "game.db").exists():
            return c
    raise SystemExit("original assets (game.db) not found; set DUSK_ASSETS")


def load(assets: Path):
    """{icon lower: info} for items and spells, plus the original file names and sizes."""
    db = sqlite3.connect(assets / "game.db")
    items = {}
    for icon, name, quality, model, equip in db.execute("select icon, name, quality, model, equip_type from item_template where icon is not null and icon != ''"):
        e = items.setdefault(icon.lower(), {"names": Counter(), "qualities": Counter(), "models": Counter(), "equip": equip})
        e["names"][name] += 1
        if isinstance(quality, int):
            e["qualities"][quality] += 1
        if model:
            e["models"][model] += 1
    spells = {}
    cols = "icon, name, description, cast_school, effect1, effect2, effect3, effect1_data1, effect2_data1, effect3_data1, effect1_positive"
    for row in db.execute(f"select {cols} from spell_template where icon is not null and icon != '' order by entry"):
        icon = row[0].lower()
        if icon in spells:
            continue
        spells[icon] = {
            "name": row[1] or "", "description": row[2] or "", "school": row[3] if isinstance(row[3], int) else None,
            "effects": [e for e in row[4:7] if isinstance(e, int) and e], "auras": [a for a in row[7:10] if isinstance(a, int) and a],
            "positive": row[10] == 1,
        }
    files = {}
    for kind, folder in (("items", "item_icons_new"), ("spells", "spell_icons_new")):
        for f in sorted(os.listdir(assets / "content" / folder)):
            if f.lower().endswith(".png"):
                with Image.open(assets / "content" / folder / f) as im:
                    files[f] = (kind, im.size)
    return items, spells, files


def item_info(e: dict | None) -> dict:
    if not e:
        return {"quality": 2}
    q = max(e["qualities"]) if e["qualities"] else 2
    return {"quality": q, "model": e["models"].most_common(1)[0][0] if e["models"] else "",
            "name": e["names"].most_common(1)[0][0], "equip_type": e["equip"]}


def item_image(stem: str, info: dict, n: int, spells: dict) -> np.ndarray:
    q = info.get("quality") or 2
    bg, light, dark = QUALITY_FRAME.get(q, QUALITY_FRAME[2])
    cv = Canvas(n, bg)
    if stem.lower().startswith("scroll_"):
        spell_stem = stem[len("scroll_"):]
        prims, rot, fit = icon_items.scroll("redmark", "redmark")
        cv.obj(prims, rot, fit=0.78, off=(0.12, -0.1))
        inset = spell_image(spell_stem, spells.get(spell_stem.lower() + ".png", {"name": spell_stem}), 19)
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
    only = argv[argv.index("--only") + 1].lower() if "--only" in argv else None
    assets = find_assets()
    items, spells, files = load(assets)
    referenced = set(items) | set(spells)
    missing = sorted(i for i in referenced if i not in {f.lower() for f in files})
    done = {"items": [], "spells": []}
    for f, (kind, (w, h)) in files.items():
        if only and only not in f.lower():
            continue
        stem = f[:-4]
        if kind == "items":
            img = item_image(stem, item_info(items.get(f.lower())), w, spells)
        else:
            img = spell_image(stem, spells.get(f.lower(), {"name": stem}), w)
        if img.shape[:2] != (h, w):
            img = np.array(Image.fromarray(img).resize((w, h), Image.NEAREST))
        out = OUT / kind / f
        out.parent.mkdir(parents=True, exist_ok=True)
        Image.fromarray(img).save(out, optimize=True)
        done[kind].append((f, img))
    print(f"items: {len(done['items'])}, spells: {len(done['spells'])}")
    unref = [f for f in files if f.lower() not in referenced]
    print(f"generated but unreferenced by the DB: {len(unref)}")
    print(f"referenced but missing from the original set: {missing}")
    if "--no-preview" not in argv:
        contact_sheet(done["items"], PREVIEW / "icons_items.png")
        contact_sheet(done["spells"], PREVIEW / "icons_spells.png")


if __name__ == "__main__":
    main(sys.argv[1:])
