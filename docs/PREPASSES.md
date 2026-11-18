# The scene camera's prepasses

A prepass draws the scene before the main pass. Bevy has three, and the
scene camera can have any of them:

- The **depth prepass** writes each pixel's depth. Its **coverage** says
  which meshes it draws: none (there is no depth prepass), only the rooms'
  meshes (rooms), or every mesh (full).
- The **normal prepass** also writes each pixel's normal.
- The **motion prepass** also writes how far each pixel moved since the
  frame before. Bevy calls it the motion vector prepass.

No player setting chooses them. Each feature that reads a prepass declares
what it needs every frame, and one system gives the scene camera the union
of those needs (`crates/game/src/prepasses.rs`).

## Why the depth prepass exists

The depth prepass came with
[2db5506b](https://github.com/Superd22/kjk/commit/2db5506b) ("Depth pass
before the main pass",
[#342](https://github.com/Superd22/kjk/pull/342)), under every
anti-aliasing choice. Its purpose is to save the GPU's shading. Without it,
the main pass shades every surface drawn at a pixel, including the hidden
ones, and each hidden fragment traces the characters' capsule shadows. With
it, the main pass shades only the surface that shows. On an M4 Pro with
Soft shadows, the arena fight against 8 foes took 47.5 ms a frame without
the depth prepass and 19.2 ms with it.

The depth prepass also costs the CPU, because each mesh is drawn twice.
Under multisampling, which the default, Low and High presets use, nothing
reads a prepass texture. On an RTX 4080 at the default preset, the bench
gave these frame rates over 2 runs each. The render thread's frame time is
in brackets.

| scene | full depth prepass | rooms only | no depth prepass |
|---|---|---|---|
| Anchorhead (`anchorhead_brawl`) | 86.3 fps (10.81 ms) | 91.9 fps (10.03 ms) | 99.2 fps (8.90 ms) |
| Refugee Quad (`refugee_brawl`) | 95.2 fps | 101.4 fps | 102.2 fps |
| Jekk'Jekk Tarr (`crowd_brawl`) | 106.4 fps | 113.9 fps | 114.1 fps |

On the 4080, dropping the depth prepass cost at most about 0.2 ms of GPU
time. The pictures matched within run-to-run noise. So a fast GPU is
better off without the depth prepass under multisampling, and a slow GPU
is better off with it. The rooms coverage sits between the two. The rooms
hide most of what is behind them, so drawing only their meshes first keeps
most of the GPU's saving for far fewer draws.

## Who reads which prepass

| reader | what it reads | what its feature declares |
|---|---|---|
| TAA (Bevy's `TemporalAntiAliasing`) | depth and motion of every mesh | a full depth prepass and the motion prepass |
| kjk's upscaler (`upscale.rs`) | depth and motion of every mesh | a full depth prepass and the motion prepass |
| DLAA and DLSS (Bevy's `Dlss`) | depth and motion of every mesh | a full depth prepass and the motion prepass |
| Ambient occlusion (`ambient_occlusion.rs`, `odyssey.wgsl`) | depth and normals of every mesh | a full depth prepass and the normal prepass |
| The motion debug view (`KJK_MOTION_DEBUG=1`, `motion_debug.wgsl`) | motion | a full depth prepass and the motion prepass, while it is shown |
| Grass's motion twins (`grass.rs`) and the motion proxies (`motion_proxy.rs`) | nothing; they write into the motion prepass | nothing; they draw only while the camera has the motion prepass, so they never cause it |
| The GPU load check (`prepasses.rs`) | nothing | a rooms depth prepass while the GPU is close to holding the frame rate back |

Ambient occlusion works only under TAA, kjk's upscaler, DLAA or DLSS, and
only under Enhanced lighting. Soft particle edges read the main pass's
depth, not the prepass's, so they need no prepass.

The approved test `prepass_readers_declared` reads every `.rs` and `.wgsl`
file of `crates/game/src`. It fails when a file that reads a prepass's
texture (`depth_prepass_texture`, `normal_prepass_texture`,
`motion_vector_prepass_texture`, `prepass_utils::` or
`ViewPrepassTextures`) is missing from its table of declared readers, or
when a listed file no longer reads one. A debug build also checks every
frame, in the render world, that a view with TAA, the upscaler, DLSS or
ambient occlusion has a full depth prepass and the prepass its feature
needs.

## How the prepasses are derived

Every frame, each feature declares its needs in `PrepassNeeds`. After the
declarations, `derive` takes their union. A full coverage wins over a
rooms coverage, and a rooms coverage wins over none. A prepass that writes
normals or motion must draw every mesh, so either of them makes the
coverage full. `derive` then inserts or removes the scene camera's
`DepthPrepass`, `NormalPrepass` and `MotionVectorPrepass`, and sets its
coverage. It is the only system that does.

Under a rooms coverage, every mesh that is not a room's is taken out of the
prepass after Bevy queues the frame's meshes. A room's mesh is a mesh whose
tag is its room's index plus 1. Characters, placeables, doors, grass and
effects are drawn in the main pass alone.

Bevy keeps its prepass phases from frame to frame. It queues a mesh again
only when the mesh becomes visible, or when its pipeline is specialized
anew. So when a view's coverage changes, all of the view's pipelines are
specialized anew in that frame, and Bevy queues every visible mesh again.
A switch from rooms to full, such as a change from multisampling to TAA,
therefore draws the full prepass in the very next rendered frame.

## The GPU load check

The check compares the GPU's time for a frame with the render thread's
frame time.

- The **GPU's time** is measured with two timestamps a frame. Each is
  taken at the end of a compute pass of one thread that does nothing. The
  first pass runs before the frame's rendering and the second after it, so
  every pass of every camera and every shadow view lies between them. The
  CPU reads the two values back one to three frames later. This costs two
  passes of one thread, a 16-byte copy and one read back a frame. Bevy's
  per-pass diagnostics (`KJK_GPU_TIMES=1`) stay off.
- The **render thread's frame time** is the time from the start of one
  frame's rendering to the start of the next. It includes the time the
  render thread waits for the main thread or for the GPU.

The check averages both over half a second. It then works out the GPU's
share, which is the GPU's time divided by the render thread's frame time.

- When the share is 0.8 or more, the GPU is close to holding the frame rate
  back, and the check asks for the rooms' depth prepass.
- When the share is 0.5 or less, the GPU has room to spare, and the check
  lets the prepass go. Dropping the prepass raises the GPU's time, so this
  threshold is well below the first. The GPU's time can grow by 60% before
  the check asks for the prepass again.
- After each change, the check keeps its answer for at least 2 seconds.
- If the check asks for the prepass again within 10 seconds of letting it
  go, the prepass bounced. The check then keeps it for 15 seconds before it
  may let it go again. Each further bounce doubles that time, up to 5
  minutes. On the M4 Pro fight above, dropping the prepass made the GPU's
  time 2.5 times longer, so the prepass would bounce there and then stay.

A device without timestamp queries (`TIMESTAMP_QUERY`) measures nothing.
There, and until the first half second has been measured, the check asks
for the rooms' depth prepass.

Apple's GPUs take timestamps only at the boundaries of a pass that does
some work. A timestamp written between passes reads 0 there, so Bevy's
per-pass timings (`kjk bench --gpu`) read 0 on a Mac. This is why the
check's passes each run one thread. Some of a Mac's samples are still not
written, and the slot then holds the value of an earlier frame. A reading
is kept only when both of its timestamps changed since the slot's last
reading and the second is after the first. About a quarter of a Mac's
frames are kept, which is plenty for an average over half a second.

## The override and the reports

`KJK_DEPTH_PREPASS=none`, `rooms` or `full` takes the place of the GPU load
check for a run, for tests and benchmarks. The features' needs still win
over it: under TAA, `KJK_DEPTH_PREPASS=none` still draws the full depth
prepass. No menu offers it.

The log has one `display` line whenever the prepasses change. It names the
coverage, the normal and motion prepasses, and why. The reason is the
features that declared them, `KJK_DEPTH_PREPASS`, or the GPU load check's
GPU time against the render thread's frame time.

A dump's `depth_prepass` fact gives the coverage now and why, whether the
normal and motion prepasses are drawn, the frames at each coverage and the
number of changes since the run began, and the GPU load check's last
measurement. The vitals give the draws of the main opaque pass
(`opaque_draws`), of its alpha-masked pass (`mask_draws`) and of the depth
prepass (`prepass_draws`). `kjk bench` adds a line under each scene with the
share of the measured frames at each coverage, the number of changes and
the prepass's draws. Its `results.jsonl` holds the same in
`depth_prepass`, `opaque_draws`, `mask_draws` and `prepass_draws`.
