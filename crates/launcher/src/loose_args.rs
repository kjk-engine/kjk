//! The flags of `kjk view` and `kjk rig` that show a body's loose parts
//! (`game::loose_view`).

use kjk_game::loose_view::Loose;

#[derive(clap::Args, Clone, Default)]
pub(crate) struct LooseArgs {
    /// Draw the 14 capsules that the ragdoll fits to a humanoid body, as
    /// wire frames that move with the skeleton. They are green for the
    /// right arm, blue for the left arm, yellow for the right leg, orange
    /// for the left leg, pink for the head and white for the trunk
    #[arg(long)]
    capsules: bool,
    /// Put a red dot on every garment vertex that starts inside a capsule
    /// when the body stands at rest, and print how many there are. The
    /// dots follow the animation. A garment vertex is one that a robe,
    /// cape or cloth bone moves most, or one of a KOTOR 1 Jedi robe
    #[arg(long)]
    marks: bool,
    /// Draw each loose bone (robe, cape, cloth, flap, hood, sleeve, belt
    /// and sash bones) as a line from its parent to it, with a square at
    /// its joint
    #[arg(long)]
    bones: bool,
    /// Move the robe, cape and cloth bones with the robe and cape solver,
    /// which collides them with the body's capsules. The bones are picked
    /// by their names for now (`frobe1_g`, `cape01_g`, `cloth_1_g` and the
    /// like), and every point has the same plain values. The game does not
    /// use the solver yet
    #[arg(long)]
    sim: bool,
    /// Draw the body's surfaces with this opacity, from 0 (invisible) to 1,
    /// so that the capsules and bones show through a robe. 0.25 is a good
    /// first try. The faint surfaces are not sorted by depth, so a cape
    /// behind the body can be drawn over the chest. Leave this flag out to
    /// judge how a garment really hangs
    #[arg(long, value_name = "OPACITY")]
    ghost: Option<f32>,
    /// Frame the body with an orbit camera, which the left mouse button
    /// turns and the wheel zooms. The other flags of this group turn it on
    /// too. A `camera` command in `--cmds` takes the camera back
    #[arg(long)]
    orbit: bool,
    /// The orbit camera's first angle around the body, in degrees from the
    /// front. 90 looks at the body's left side, 180 at its back
    #[arg(long, default_value_t = 0.0)]
    angle: f32,
    /// How many times closer than the whole-body framing the orbit camera
    /// starts
    #[arg(long, default_value_t = 1.0)]
    zoom: f32,
}

impl LooseArgs {
    /// What to draw over the first model, if any flag asks for it.
    /// `slow` is the share of its speed that the clip plays at.
    pub(crate) fn loose(&self, slow: f32) -> Option<Loose> {
        let asked = self.capsules
            || self.marks
            || self.bones
            || self.sim
            || self.ghost.is_some()
            || self.orbit
            || (slow - 1.0).abs() > f32::EPSILON;
        asked.then(|| Loose {
            capsules: self.capsules,
            marks: self.marks,
            bones: self.bones,
            sim: self.sim,
            ghost: self.ghost.map(|opacity| opacity.clamp(0.0, 1.0)),
            slow,
            angle: self.angle,
            zoom: self.zoom,
        })
    }
}
