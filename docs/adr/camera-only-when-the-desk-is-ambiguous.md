---
date: 2026-10-29
---

# Camera only when the desk is ambiguous

## Status

Accepted

## Context

AI agents often stop to wait for the owner while the owner is asleep or away.
The owner wants an agent to be able to ask whether they are physically at this Mac, keep working on safe choices when they are absent, and leave everything else waiting.
A camera answers that question directly, but it lights the camera indicator, needs camera permission, and is slower than other signals.
Screen lock, display sleep, and keyboard and mouse idle time are free and need no permission, but they cannot tell a reader who sits still from an empty chair.

## Decision

`yomydev` exposes one MCP tool, `presence`, that returns `present`, `absent`, or `unknown` with the basis and the idle seconds.

The decision is a pure two-step function in `yomydev-core`:
`without_camera` decides from the desk alone, and only its `NeedsCamera` result lets `with_camera` run.
A locked screen or a sleeping display is absent, and input within the last 60 seconds is present.
Only otherwise does the adapter open the built-in camera, analyse three frames after a one-second warm-up with Vision face and upper-body detection, and drop the frames.
Each frame also yields its mean luma and the percentage of a 16 by 12 grid of cell means whose second difference reaches 12 luma levels, which counts edges while ignoring smooth gradients such as vignetting.
A seen person is present.
An empty view is absent only when the camera has a clear view: the brightest frame reaches luma 40 and every frame shows edges in at least 5 percent of the grid.
A bright view without edges gives `featureless_view` and `unknown`, because a tissue over the lens gives a bright, edgeless frame that no person detector can see through.
A dark view counts as absent only after 300 seconds without input, because a covered lens and a dark room look the same.
A camera that is unauthorized, missing, busy, or slower than five seconds gives `unknown`.

`unknown` is treated like `present`.
Absence never grants authority.
The MCP `instructions` tell the agent to decide only low-risk, reversible, in-scope questions while the owner is absent, list those decisions, and still wait for commit, push, merge, pull request thread actions, deletion, and anything visible outside the machine.

The tool never prompts for camera access.
`yomydev grant` is the only path that asks macOS.

The decision thresholds live in `crates/yomydev-core/src/lib.rs`, and the edge measure lives in `crates/yomydev/src/frames.rs`.
The Kani harnesses in `crates/yomydev-core/src/proofs.rs` prove that the camera runs exactly in the ambiguous branch, that an unavailable camera is never absent, that an empty view is absent exactly when it is clear or dark after long idle, that a seen person is never absent, and that a locked or sleeping desk is absent without the camera.

## Consequences

The camera indicator lights only when the desk is ambiguous, and never while the owner is typing.
Someone sitting still in a dark room for under five minutes reads as `unknown`, so the agent asks as usual.
A lens covered by something opaque in a dark room still reads as absent after five minutes without input.
A camera that faces a plain wall reads as `unknown`, so an empty room needs some visible structure before it counts as absent.
The edge thresholds are first estimates and change only with measurements from covered, empty, and occupied views.
Camera permission belongs to the process macOS holds responsible, which is the terminal app when the MCP server runs under it.
The binary embeds an `Info.plist` with a camera usage description in case it ever becomes its own responsible process; its ad hoc signature then changes the grant on every rebuild.

Rejected alternatives:

- Opening the camera on every call lights the indicator during ordinary typing and adds latency for no extra certainty.
- An MCP SDK such as `rmcp` would add a dependency tree for five JSON-RPC methods.
- Shelling out to `imagesnap` or `ffmpeg` writes frames to disk and adds external tools.
- A Swift helper would split the project across two toolchains.
