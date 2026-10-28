# Data formats

Documentation of the Wintermute Engine data formats, as implemented by Luma
Engine. Written from the MIT-licensed WME sources and from analysis of game
files only (see the clean-room rule in [CONTRIBUTING.md](../../CONTRIBUTING.md)).

| Format | Document | Status |
|--------|----------|--------|
| Game packages (`.dcp`) | [dcp.md](dcp.md) | Implemented in `luma-archive` |
| Definition files (`.game`, `.scene`, `.sprite`, `.window`, `.font`, ...) | [definition-files.md](definition-files.md) | Syntax implemented in `luma-formats` |
| Sprites (`.sprite`) and images | [sprite.md](sprite.md) | Implemented in `luma-formats` |
| Scenes (`.scene`) and entities (`.entity`) | [scene.md](scene.md) | Implemented in `luma-formats` |
| Compiled scripts (`.script`) | [script.md](script.md) | Loader and disassembler in `luma-script` |
| Game definition (`default.game`) and string tables (`string.tab`) | [game.md](game.md) | Implemented in `luma-formats` |
| Actors (`.actor`) | [actor.md](actor.md) | Implemented in `luma-formats`; walking in `luma-adventure` |
| Fonts (`.font`, bitmap and TrueType) and speech | [font.md](font.md) | Implemented in `luma-formats`; rendering and `Talk` in `luma-adventure` |
