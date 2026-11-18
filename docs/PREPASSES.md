# The scene camera's prepasses

A prepass draws the scene before the main pass. Bevy has three, and the
scene camera can have any of them:

- The **depth prepass** writes each pixel's depth. Its **coverage** says
  which meshes it draws. It draws no mesh (none) or every mesh (full).
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
better off without the depth prepass under multisampling, and a GPU near
its limit is better off with it.

An M4 Max is such a GPU. Headless at the default preset, #563 at 5b849d9a
gave these frame rates in one run each. The render thread's frame time is
in brackets.

| scene | full depth prepass | rooms only | no depth prepass |
|---|---|---|---|
| Anchorhead (`anchorhead_brawl`) | 182.1 fps (5.23 ms) | 104.2 fps (9.32 ms) | 112.4 fps (8.62 ms) |
| Refugee Quad (`refugee_brawl`) | 106.1 fps (8.09 ms) | 61.0 fps (16.00 ms) | 53.9 fps (18.08 ms) |
| Proving grounds (`proving_grounds`) | 394.9 fps (2.31 ms) | 180.7 fps (5.31 ms) | 181.8 fps (5.27 ms) |

A first version of this module also had a third coverage, which drew only
the rooms' meshes. It was meant for a GPU near its limit. On the Mac, it
was as slow as no depth prepass. On the 4080 it was slower than no depth
prepass (88.1 fps against 95.1 fps in Anchorhead). So it was removed. The
depth prepass now draws every mesh or none.

## Who reads which prepass

| reader | what it reads | what its feature declares |
|---|---|---|
| TAA (Bevy's `TemporalAntiAliasing`) | depth and motion of every mesh | a full depth prepass and the motion prepass |
| kjk's upscaler (`upscale.rs`) | depth and motion of every mesh | a full depth prepass and the motion prepass |
| DLAA and DLSS (Bevy's `Dlss`) | depth and motion of every mesh | a full depth prepass and the motion prepass |
| Ambient occlusion (`ambient_occlusion.rs`, `odyssey.wgsl`) | depth and normals of every mesh | a full depth prepass and the normal prepass |
| The motion debug view (`KJK_MOTION_DEBUG=1`, `motion_debug.wgsl`) | motion | a full depth prepass and the motion prepass, while it is shown |
| Grass's motion twins (`grass.rs`) and the motion proxies (`motion_proxy.rs`) | nothing, as they only write into the motion prepass | nothing, as they draw only while the camera has the motion prepass and so never cause it |
| The GPU load check (`prepasses.rs`) | nothing | a full depth prepass while the GPU is close to holding the frame rate back, or while the check cannot time the GPU |

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
declarations, `derive` takes their union. A full coverage wins over none.
A prepass that writes
normals or motion must draw every mesh, so either of them makes the
coverage full. `derive` then inserts or removes the scene camera's
`NormalPrepass` and `MotionVectorPrepass`, and sets its coverage. It is the
only system that does.

The scene camera keeps Bevy's `DepthPrepass` at all times, and a coverage
of none is an empty depth prepass. The reason is that the main pass's
pipelines depend on whether the view has a depth prepass. Taking
`DepthPrepass` away or giving it back would compile every main pass
pipeline anew in the middle of play. On macOS, Bevy compiles pipelines on
the render thread, and meshes are missing until their pipelines are
ready. An empty depth prepass still clears the depth and copies it to the
prepass's depth texture. That costs the GPU little and the CPU no draw.
Warming the main pass pipelines both with and without a depth prepass was
the other way. It would need a second camera that matches the scene
camera's view in every other respect, through every change of settings,
so it was not done.

Under a coverage of none, every mesh is taken out of the prepass after
Bevy queues the frame's meshes.

Bevy keeps its prepass phases from frame to frame. It queues a mesh again
only when the mesh becomes visible, or when its pipeline is specialized
anew. So when a view's coverage changes, all of the view's pipelines are
specialized anew in that frame, and Bevy queues every visible mesh again.
The pipelines are found in Bevy's cache, so nothing is compiled. A switch
from none to full, such as a change from multisampling to TAA, therefore
draws the full prepass in the very next rendered frame. So does a switch
from none to full by the GPU load check.

## The GPU load check

The check compares the GPU's time for a frame with the render thread's
frame time.

- The **GPU's time** is measured with two timestamps a frame. Each is
  taken at the end of a compute pass of one thread. The first pass does
  nothing and runs before the frame's rendering. The second pass runs
  after the rendering and reads one texel of each view's main texture.
  That read makes the GPU wait for every pass that draws into the main
  texture before it runs the second pass. So every pass of every camera
  and every shadow view lies between the two timestamps. If the GPU is
  still busy with the frame before when the first pass runs, the frame's
  time starts at the end of the frame before instead. The CPU reads the
  two values back one to three frames later. This costs two passes of one
  thread, a 16-byte copy and one read back a frame. Bevy's per-pass
  diagnostics (`KJK_GPU_TIMES=1`) stay off.
- The **render thread's frame time** is the time from the start of one
  frame's rendering to the start of the next. It includes the time the
  render thread waits for the main thread or for the GPU.

The GPU's share is the GPU's time divided by the render thread's frame
time. The check works it out over windows of at least a quarter of a
second. A window also waits for at least 3 GPU readings, for up to one
second, and the check takes their median. A device that drops three
readings in four still gathers about 4 in a quarter of a second at 60
frames a second. The check measures only while
its answer decides the coverage, so not under TAA, ambient occlusion or
`KJK_DEPTH_PREPASS`. It also waits while Bevy is still compiling
pipelines, since a compile stalls the frame and spoils the window.

- **Asking for the prepass.** When a window's share is 0.8 or more, the GPU
  is close to holding the frame rate back. The check then asks for the
  full depth prepass at once. Nothing ever delays this.
- **Letting the prepass go.** Dropping the prepass makes the GPU's time
  longer, by a factor the check calls the growth. The check predicts the
  share without the prepass as the share with it times the growth. It lets
  the prepass go only when that predicted share is under 0.7, and only 2
  seconds or more after it last asked for the prepass.
- **Learning the growth.** The growth starts at a guess of 1.3. After each
  drop, the check compares the GPU's time in the second window after the
  drop with its time in the last window before it, and keeps that ratio,
  between 1 and 4. The first window after a drop still holds frames drawn
  with the prepass, since the GPU's readings arrive one to three frames
  late, so it is not used.
- **A bounce.** If a window after a drop already shows a share of 0.8 or
  more, the drop bounced. The check asks for the prepass again at once. It
  then sets the growth to at least the factor that would have predicted
  that share. So the same load cannot drop the prepass again.

On the M4 Pro fight above, a frame took about 2.5 times longer without the
depth prepass. If the GPU's time grows as much, a first drop at a share between 0.32 and 0.54 would
bounce once, for about half a second. The check then learns a growth of
about 2.5, and it lets the prepass go again only below a share of 0.28. On
the RTX 4080, dropping the prepass cost at most about 0.2 ms, so the
learned growth stays close to 1. The check then lets the prepass go at any
share under about 0.7.

The check chooses the full depth prepass whenever it cannot time the GPU.
This is the safe choice, and it is what the scene camera always drew
before the check existed. A device without timestamp queries
(`TIMESTAMP_QUERY`) measures nothing, so it always draws the full depth
prepass. The check also asks for it before its first window, and after
any window of one second that kept no GPU reading.

### Timing Apple's GPUs

Apple's GPUs (Metal) take timestamps only at the boundaries of a pass
that does some work. A timestamp written between passes or inside a pass
reads 0 there. wgpu offers no `TIMESTAMP_QUERY_INSIDE_PASSES` or
`TIMESTAMP_QUERY_INSIDE_ENCODERS` on them, so Bevy's per-pass timings
(`kjk bench --gpu`) read 0 on a Mac. For the same reason,
`console::gpu_times` and Bevy's `RenderDiagnosticsPlugin` are not reused
for the check. A timestamp at the end of a compute pass does work there.

A first version of the check ended the frame with a pass that read
nothing. A Mac's readings then did not measure the frame. Apple's GPUs run
work from successive command buffers side by side when no resource ties
them together, so that pass often ran before the frame's rendering had
ended. Headless at 3024x1964 in an 8-foe arena fight, the middle reading
of each quarter second ranged from 0.8 ms to 29 ms within one fight. That
version treated every Mac as untimed.

The second pass now reads a texel of each view's main texture. Metal
tracks every texture wgpu creates, so the GPU starts that pass only after
the passes that draw into the texture have ended. With this tie, the
Mac's readings follow the frame. These are the means of the check's
windows over 20 seconds on an M4 Max, headless, at the default preset,
with `KJK_DEPTH_PREPASS` forcing the coverage. The GPU's time is the
check's reading. The render frame is the render thread's frame time.

| scene | full: GPU, render frame, fps | none: GPU, render frame, fps |
|---|---|---|
| Anchorhead (`anchorhead_brawl`) | 4.73 ms, 5.51 ms, 183.4 fps | 9.50 ms, 8.75 ms, 118.9 fps |
| Refugee Quad (`refugee_brawl`) | 10.03 ms, 9.49 ms, 108.3 fps | 18.91 ms, 15.70 ms, 60.0 fps |
| Proving grounds (`proving_grounds`) | 1.69 ms, 2.65 ms, 387.6 fps | 5.81 ms, 5.86 ms, 171.8 fps |

When the GPU holds the frame back, its time reads at or a little above the
render frame. A reading can pass the render frame because the check takes
the median of the GPU's readings and the mean of the render frames. In
the Proving grounds under the full depth prepass, the GPU is not what
holds the frame back, and its time reads 64% of the render frame. So the
check now times Apple's GPUs as it times the others.

On this Mac, the check keeps the full depth prepass on most frames of
every fight. In 10 automatic runs of the Proving grounds, it let the
prepass go on 0% to 28% of the measured frames. Each drop lasted one to
three seconds. The next windows then showed a share of 0.8 or more, and
the check took the prepass back.

Every device drops a reading in three cases. A reading is dropped when one
of its timestamps did not change since the slot's last reading, or when
the second timestamp is not after the first. It is also dropped when it is
longer than two of the render thread's frames, since it then spans
several frames. The check takes the median of a window's readings.

## The override and the reports

`KJK_DEPTH_PREPASS=none` or `full` takes the place of the GPU load check
for a run, for tests and benchmarks. The features' needs still win over
it. Under TAA, `KJK_DEPTH_PREPASS=none` still draws the full depth
prepass. No menu offers it. The rooms coverage was removed, so
`KJK_DEPTH_PREPASS=rooms` now draws the full depth prepass, and the log
warns about it.

The log has one `display` line whenever the prepasses change. It names the
coverage, the normal and motion prepasses, and why. The reason is the
features that declared them, `KJK_DEPTH_PREPASS`, or the GPU load check's
GPU time against the render thread's frame time.

A dump's `depth_prepass` fact gives the coverage now and why, whether the
normal and motion prepasses are drawn, the frames at each coverage and the
number of changes since the run began, and the GPU load check's last
measurement and learned growth. It also adds up the GPU and render times
of every timed window since the run began. The check keeps measuring
while `KJK_DEPTH_PREPASS` or a feature decides the coverage, so these sums
are there in a forced run too. The vitals give the draws of the main opaque pass
(`opaque_draws`), of its alpha-masked pass (`mask_draws`) and of the depth
prepass (`prepass_draws`). `kjk bench` adds a line under each scene with the
share of the measured frames at each coverage, the number of changes, the
check's mean GPU time and render frame over the run, and the prepass's
draws. Its `results.jsonl` holds the same in
`depth_prepass`, `opaque_draws`, `mask_draws` and `prepass_draws`.
