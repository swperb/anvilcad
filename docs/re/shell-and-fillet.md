# Shell and fillet behaviour

What AnvilCAD's shell feature and its fillets on round edges should do, written before the
exact-kernel implementation.

## How this was studied

This specification comes from the publicly documented, user-visible behaviour of shell and
fillet features in mainstream parametric CAD (the SOLIDWORKS online help for Shell and
Fillet among them), from AnvilCAD's existing faceted implementation, and from general solid
modelling practice. It is written in AnvilCAD's own words. No SOLIDWORKS binaries,
libraries or files were analysed: the SOLIDWORKS licence agreement forbids reverse
engineering it, and [INTEROPERABILITY.md](../../INTEROPERABILITY.md) says the REA toolkit is
never pointed at Dassault software. These are generic modelling operations, so their
behaviour is fully described by what a user sees.

## Shell

Inputs: a solid, a wall thickness `t` > 0, and a set of faces to remove (may be empty).

- The result is the solid with its inside hollowed out to a wall of thickness `t`,
  measured along each face's normal. The wall follows every face: flat faces stay flat,
  cylinders and spheres stay cylinders and spheres with their radius changed by `t`, a
  fillet of radius `r` becomes a concentric round of radius `r - t` on the inside (`r + t`
  for a concave one).
- Removed faces are opened: the cavity breaks through them, so the walls end at the
  removed faces' former position.
- The wall grows inwards; the outside of the part does not move.
- With no faces removed the result is a closed hollow part (a sealed void inside).
- It fails, saying why, when the wall is thicker than the part allows: when an inner
  surface would turn inside out (a fillet or round of radius below `t`, on its convex
  side), or when the hollow would change shape (a feature thinner than `2t`).
- Face names: the outer faces keep their names; inner faces and the open edges are new.

AnvilCAD's exact kernel builds the shell when every face has an exact offset (planes,
cylinders, cones, spheres, tori) and the hollow keeps the part's shape; otherwise the
faceted kernel builds it, as before. A sealed hollow (no faces removed) needs a solid with
two shells, which the exact kernel does not represent yet, so it is built faceted.

## Fillets and chamfers on round edges

Inputs: an edge, and a radius (fillet) or distance (chamfer).

- A fillet replaces the edge with a round of the given radius tangent to both faces; a
  chamfer with a flat (or conical) bevel set back the given distance along each face.
  Convex edges lose material, concave edges gain it.
- On a circular edge between faces that are round about the same axis as the edge (a
  flat face square to the axis, a cylinder or a cone on it) the round is part of a torus
  and the bevel part of a cone; for example the rim of a boss, the edge of a hole, or an
  edge of a revolved part.
- The blend must fit on both faces: it fails, saying so, when it would run off either
  face, or when it would reach the axis.
- Face names: the round or bevel face is new and named after the feature and the edge;
  the faces next to it keep their names.

AnvilCAD's exact kernel blends straight edges between flat faces (already) and whole
circular edges between coaxial planes, cylinders and cones. Other curved edges still go to
the faceted kernel.
