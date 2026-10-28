# Ghoul2 playback: which frames a bone shows

What `asset_ja::g2` reproduces of Ghoul2's timing. Units: frames and ms.

## The frames at a moment (`BoneAnim::timing`)
The detailed specification of which frames a bone shows, and of the
reference's first backward wrap (QUIRKS.md Q-0028), was removed. The
behaviour is specified in the clean-room specifications kept by the
maintainer and implemented in `crates/asset_ja/src/g2.rs` and
`crates/sim/src/skeleton/playback.rs`.

## The ranges (`Player::set`)
Forward: `first` to `first + count`. Backward, without quirks: `first +
count − 1` down to `first − 1` (its own frames, last to first); with quirks
the reference's, one frame higher (Q-0003). Looping when the sequence has a
loop frame. Speed: `50 / frame_ms` times the style's scale, negative for a
backward sequence.

## Picking up the other part's frame (`Player::set_part`)
The multiplayer cgame's "wobbly spine" rule,
legs (`model_root`) set before the torso (`lower_lumbar`). When, after the
set, both parts play the same animation, a part that switches to it (not a
restart of its own) begins on the frame the other part's bone shows (place
and fraction), if that lies within the sequence's forward range, `first`
to `first + count` inclusive; never a backward sequence. Beginning on frame
`b` times the animation as if set `(b − start) × 50 / speed` ms earlier,
truncated to a whole millisecond as Ghoul2 keeps it; the blend is
unchanged. `Motion` is set with each part that sets the animation the
torso plays (the legs' set too, with the legs' begin and blend). With
`quirks` only, a legs restart
picks up the torso's frame as well (QUIRKS.md Q-0029).

## The pose a blend starts from (`Player::set`, `Player::pose`)
When a new animation blends in, the old one's two frames and mix at that
moment are kept (forward ones recomputed from its place: the next frame, or
`start` or `last` at the end) and mixed into the new one over the blend
time. Only with quirks: the weights the wrong way round (Q-0004), a backward
one's snapped to a whole frame (Q-0010), and a change mid-blend starting
from the frames rather than the pose shown (Q-0017).

Divergence: frames and blends turn along an arc at full size (see
`g2`'s module docs), where Ghoul2 lerps matrices.
