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
| Talk definitions (`.talk`) and sprite sets (`.spriteset`) | [talk.md](talk.md), [actor.md](actor.md#spriteset) | Implemented in `luma-formats`; played in `luma-adventure` |
| Fonts (`.font`, bitmap and TrueType) and speech | [font.md](font.md) | Implemented in `luma-formats`; rendering and `Talk` in `luma-adventure` |
| Windows and controls (`.window`), tiled images, inventory (`.def`, `.items`) | [window.md](window.md) | Implemented in `luma-formats`; display, input and the inventory in `luma-adventure` |
| Scene geometry of 3D scenes (`.3ds`, `.geometry`) | [geometry.md](geometry.md) | Implemented in `luma-3d` (feature `3d`); the scene's 3D keys in `luma-formats` |
| 3D models (`.x`, DirectX text format) | [xfile.md](xfile.md) | Implemented in `luma-3d` (feature `3d`): frames, meshes, skin weights, animation sets; binary and compressed files rejected |
| 3D actors (`.act3d`) and animation sidecars (`.anim`) | [actor3d.md](actor3d.md) | Implemented in `luma-3d` (feature `3d`); the actor itself in a later task |
