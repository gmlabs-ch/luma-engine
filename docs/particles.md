# Particles

How Luma Engine runs the particle emitters of Wintermute games: the
`CreateParticleEmitter` of an entity or an actor, the `particle-emitter`
object scripts configure, its particles, forces and drawing. The rules
follow the original engine (`PartEmitter.cpp`, `PartParticle.cpp`,
`PartForce.cpp` and `AdObject.cpp` of Wintermute Engine 1.x, MIT) so
that a game's snow, sparks and smoke look as their authors tuned them.
Implemented in `crates/luma-adventure/src/particles.rs` (the emitter)
and `natives_particles.rs` (the script interface).

## An emitter and its owner

`object.CreateParticleEmitter(followParent, offsetX, offsetY)` gives an
entity or an actor its emitter (one per object: calling it again returns
the same emitter and only changes how it follows) and returns it;
`object.ParticleEmitter` reads it (`null` without one) and
`object.DeleteParticleEmitter()` removes it, which invalidates the
handle. With `followParent` the emitter's `X` and `Y` are set every
frame to the owner's position plus the offsets scaled by the owner's
scale (`CAdObject::UpdatePartEmitter`); otherwise the script places it
with `X` and `Y`.

The emitter goes with its owner: a scene entity's or a scene actor's
emitter is dropped at a scene change, a game actor's stays. Emitters of
the entities of windows (entity containers) can be created but are not
drawn yet.

## Parameters

Every parameter is a property of the emitter, with the original
engine's defaults in parentheses. Ranges are two values a particle's
value is drawn between at birth, uniformly.

| Property | Meaning |
|----------|---------|
| `X`, `Y`, `Width`, `Height` | The emission rectangle (a point at the origin): a particle is born at an integer position inside it, both edges included. |
| `Angle1`, `Angle2` (0, 0) | Direction of the initial velocity in degrees: `0` is up the screen, `90` right, `180` down, `270` left. The range goes clockwise from `Angle1` to `Angle2` (`350`, `10` is a cone around straight up). |
| `Velocity1`, `Velocity2` (0, 0), `VelocityZBased` (false) | Speed in pixels per second. With `VelocityZBased`, the speed is not random but follows the particle's depth: `Velocity1` at depth 0, `Velocity2` at depth 100. |
| `Scale1`, `Scale2` (100, 100), `ScaleZBased` (false) | Scale in percent, or by depth as above. |
| `LifeTime1`, `LifeTime2` (1000, 1000), `LifeTimeZBased` (false) | Lifetime in milliseconds; `0` for ever. By depth: `LifeTime2` at depth 0 down to `LifeTime1` at depth 100 (the original's formula, the reverse of the scale's and the speed's). |
| `Rotation1`, `Rotation2` (0, 0) | Initial rotation in degrees, a clockwise range like the angles. |
| `AngVelocity1`, `AngVelocity2` (0, 0) | Spin in degrees per second. |
| `GrowthRate1`, `GrowthRate2` (0, 0), `ExponentialGrowth` (false) | How the scale changes, in percent points per second, or in percent of the current scale per second when exponential. A particle whose scale reaches zero dies. |
| `Alpha1`, `Alpha2` (255, 255), `AlphaTimeBased` (false) | Opacity, `0` to `255` (a set value is clamped). One random value per particle, or, when time based, `Alpha1` at birth fading to `Alpha2` at the end of life. |
| `MaxParticles` (100) | How many particles may live at once. |
| `GenerationInterval` (0), `GenerationAmount` (1) | A batch of `GenerationAmount` particles is born when more than `GenerationInterval` milliseconds passed since the last batch and fewer than `MaxParticles` live; a batch never goes past the cap. `0` means a batch every frame. |
| `MaxBatches` (0) | Batches after which the emitter stops emitting; `0` for no limit. `Start` resets the count. |
| `FadeInTime`, `FadeOutTime` (0, 0) | A particle fades in from transparent over `FadeInTime` ms after birth; at the end of its life, or when it leaves its border, it fades out over `FadeOutTime` ms (with `0` it dies at once). |
| `UseRegion` (false) | Only the particles inside the owner's `REGION` are drawn (entities only). |
| `EmitEvent` (null) | The name of an event raised on the owner each frame a batch is born (not during the run-ahead of `Start`). |
| `NumLiveParticles` | Read only: the particles alive. |
| `Type` | `particle-emitter`. |

Every particle also gets a **depth** between 0 and 100 at birth. When
any of the `...ZBased` flags is on, the particles are sorted by depth
after each batch, so that the deepest are drawn first.

## Methods

| Method | Effect |
|--------|--------|
| `AddSprite(file)`, `RemoveSprite(file)` | The sprite files particles are drawn with, one picked at random per particle (a sprite file or an image). `AddSprite` returns `false` and adds nothing when the file does not exist; a file that exists but does not load leaves its particles dead at birth, as in the original. Particles already born keep their sprite when it is removed. Each particle plays its sprite's animation from the first frame. |
| `Start(overheadTime)` | Kills every particle, restarts the batch count and emits. With `overheadTime` (milliseconds), the emitter first runs that long ahead in steps of 500 ms, so that a scene opens with its snow already falling; no emit event for those steps. |
| `Stop()` | Removes every particle and stops. |
| `Pause()`, `Resume()` | Hold the emitter where it is (nothing moves, nothing is born) and go on. |
| `SetBorder(x, y, width, height)` | A rectangle (right and bottom edges excluded) a particle fades out on leaving; a rectangle without area means none. |
| `SetBorderThickness(left, right, top, bottom)` | The border shrinks by these amounts for the particles at depth 0, by nothing at depth 100, in between linearly: the nearest particles are kept further from the edges. |
| `AddGlobalForce(name, angle, strength)` | A push on every particle of `strength` pixels per second per second towards `angle` (as for the velocity: `180` is down, for gravity); a force with the same name (without case) is replaced. |
| `AddPointForce(name, x, y, angle, strength)` | In the original engine 1.x this creates the same global force as `AddGlobalForce` (`CPartEmitter::ScCallMethod` passes `FORCE_GLOBAL` for both), the point being kept but unused. Luma reproduces this, so that games tuned against the original look the same. The point force itself (a push towards `angle` scaled by `100 / distance` to the point) is implemented, but no script can create one. |
| `RemoveForce(name)` | Removes a force; `false` when there is none. |

The methods return `true`, or `false` when they failed (`AddSprite` of a
missing file, `RemoveSprite` and `RemoveForce` of an unknown name).

## A frame

Each frame, after the scene's entities and the actors updated
(`Runtime::tick`), every emitter follows its owner when asked and, when
running, updates on the game timer with the frame's duration
(`m_TimerDelta`); nothing happens while the game is frozen. For each
particle, in the original's order:

1. a fading-in particle only updates its opacity (the frame that ends
   the fade-in does nothing else); a fading-out one dies when the fade
   ends;
2. a particle past its lifetime starts fading out, or dies when there is
   no fade-out time; one outside its border starts fading out (with no
   fade-out time it dies the next frame);
3. a living particle's opacity follows its age when time based; every
   force adds its push times the frame's seconds to the velocity; the
   velocity times the seconds moves the particle; the spin turns it; the
   growth scales it.

Then, when fewer than `MaxParticles` live and the interval passed, a
batch is born: dead particles' slots are reused before new ones are
made; a particle gets, in order, a position in the rectangle, a depth,
a speed, a scale, a lifetime, a direction, a sprite, a rotation, a
spin, a growth, its border shrunk by depth, its opacity, and starts its
fade-in. The random values come from the world's generator, the one
scripts' `Random` uses, so a run is deterministic under a seed.

## Drawing

An emitter's particles are drawn right after their owner's sprite
(`CAdEntity::Display`, `CAdActor::Display`), in the owner's layer, each
live particle as its sprite's current frame placed by its hotspot at
the particle's position (truncated to pixels, as the original passes
integers), scaled by its scale and tinted white with its opacity. The
owner's `AlphaColor` does not apply. Rotation is not drawn yet: the
renderer has no per-quad rotation (see [rendering.md](rendering.md)).
The original engine batches the sprites of an emitter with a single
sprite file; Luma's renderer already merges consecutive quads of one
texture.

## Saved games

A saved game holds every emitter with its parameters, sprites, forces
and particles, live and dead, with their positions, velocities, ages
and fades, and how it follows its owner (`CPartEmitter::Persist`,
`CPartParticle::Persist`, `CPartForce::Persist`). The sprite files are
loaded again after a load, as the other sprites are (see
[saving.md](saving.md)).

## Not yet

- Rotation of the particles (`Rotation1`, `Rotation2`, `AngVelocity1`,
  `AngVelocity2` are kept and advanced, not drawn).
- The owner's blend mode (`BlendMode`) on the particles.
- Emitters of the entities of windows (entity containers) are not drawn.
- `CreateParticleEmitterBone` of 3D actors (see the 3D group of
  [compatibility.md](compatibility.md)).
