# CAM roadmap

What `anvil-cam` does today is in [DESIGN.md](DESIGN.md). This is what remains, roughly in
order. Items move to the README's "What works today" when they are in the code and tested.

## Near term

1. **Application integration.** A CAM tab: setups, stock, the operation list from the
   automatic plan, tool library editing, tool path display, simulation playback with
   findings shown on the part, and posting with the certificate shown.
2. **Adaptive clearing performance and quality.** Parallel field measurement; recompute
   only what changed; fewer and shorter links (lifted links, conventional passes where
   allowed); trochoidal passes for channels barely wider than the tool (Held and Pfeiffer
   2025; Otkur and Lazoglu 2007); arcs fitted to the output to shorten programs.
3. **Rest machining.** Clear what a larger tool left with a smaller one, using the
   material model the larger tool's plan ends with.
4. **More feature recognition.** Open slots and steps as features with their own entry
   from the side; counterbores and countersinks; floor fillets and chamfers; threaded and
   tapped holes from the hole feature once the CAD side has it; features on faces whose
   normal is not +Z (for indexed setups).
5. **More drilling.** Spot drilling, tapping (G84) and boring cycles, hole-making chains
   (spot, drill, ream, tap) from hole types.
6. **Posts.** More example posts (Haas-style, Heidenhain conversational is out of scope for
   the RS274 verifier), arc output in other planes, inverse time, and the verifier rules
   for each; a post editor with the diff and certificate shown.
7. **The registry service.** The public repository, signed index releases, client-side
   pinning and update prompts, and the reporting workflow described in
   [POSTS.md](POSTS.md).

## Later

8. **3-axis surfacing.** Parallel, waterline, scallop and pencil finishing on the exact
   B-rep (or its tessellation), with a drop-cutter on the faceted body; the height-field
   simulator already models ball and bull-nose tools.
9. **Holder-aware tool selection.** Choose stick-out and holder from the simulator's
   collision checks.
10. **Feeds and speeds.** Tool-maker data import, per-tool overrides, and force or power
    limits from the material's properties.
11. **4- and 5-axis.** Indexed (3+2) setups first, then simultaneous machining with a
    dexel or voxel simulator and machine kinematics in the verifier.
12. **Turning and mill-turn.**

## Known limits of the first version

- The automatic plan machines closed pockets with vertical walls, holes and the top face.
  Open floors and drafted walls are reported but not machined.
- The material raster limits resolution to a cell (1/80 of the tool diameter by default);
  the margins and verifier are built around that, but very small features may be left as
  rest material.
- Adaptive planning takes seconds for a small pocket in a release build.
- Height-field simulation cannot represent undercuts (which 2.5D and 3-axis milling from
  +Z cannot make anyway) and can miss features narrower than a cell.
- The verifier accepts a deliberately small dialect; a post needing more must wait for
  verifier support, not bypass it.
- The AI features need network access and an API key, and are advisory only.
