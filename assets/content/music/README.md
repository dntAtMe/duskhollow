# Soundtrack

Background music by the project owner. A map plays the tracks named by `music=` in
`data/maps.txt`, or the whole soundtrack when that is empty; tracks rotate randomly with
crossfades (see `crates/dusk_client/src/audio.rs`, `docs/audio.md`).

| File | Title |
|---|---|
| `tenebrae.mp3` | Tenebrae |
| `sous_les_voutes.mp3` | Sous les voûtes |
| `minor_modal_motif.mp3` | Minor Modal Motif |

Adding a track: drop an `.mp3` (or `.wav`) into this folder; file names must be unique under
`assets/content/`.
