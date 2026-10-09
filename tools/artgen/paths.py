"""Where the generators read and write: the asset root `assets/` (see docs/content.md)."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT / "assets"

DATA = ASSETS / "data"  # data/*.txt
MAPS = ASSETS / "maps"  # <name>.map + .spawns / .cover / .markers
SCRIPTS = ASSETS / "scripts"
NPC_SCRIPTS = SCRIPTS / "npc"  # NPC sprite scripts (<model>.txt)
PLAYER_SCRIPTS = SCRIPTS / "player"  # paper-doll sprite scripts (<model>.txt)
ANIMATION = SCRIPTS / "animation"  # .sa flipbooks
CONTENT = ASSETS / "content"  # everything the client resolves by bare file name
SPRITES = CONTENT / "sprites"  # unit and gear sprite sheets
ENV = CONTENT / "env"
VALE = CONTENT / "vale"
UI = CONTENT / "ui"
ICONS = CONTENT / "icons"  # icons/items, icons/spells
PORTRAITS = CONTENT / "portraits"
SPELLFX = CONTENT / "spellfx"
FX = CONTENT / "fx"
SFX = CONTENT / "sfx"
PREVIEW = ASSETS / "preview"  # contact sheets and overviews (gitignored)
