//! `kjk join`: its flags and its run.

use std::process::ExitCode;

use crate::{JoinSpec, discover, join_session, launch, netem_conditions};

#[derive(clap::Args)]
pub(crate) struct JoinArgs {
    /// The host: `address[:port]`, `steam:<id>`, or a Steam friend hosting
    /// now, by name
    host: String,
    /// The name the host sees
    #[arg(long, default_value = "player")]
    name: String,
    /// Network conditions on what this side sends (overrides KJK_NETEM)
    #[arg(long)]
    netem: Option<String>,
    /// Scripted input instead of the keyboard (see `moves.rs`)
    #[arg(long)]
    moves: Option<String>,
    /// Commands to run, e.g. 'wait 10s; quit'
    #[arg(long)]
    cmds: Option<String>,
    #[arg(long)]
    headless: bool,
    /// Draw the sim's blades (green) beside the drawn ones, and the
    /// gaps between their tips (yellow), measured into the dumps and
    /// `blade-gap.csv`
    #[arg(long)]
    sim_blades: bool,
    /// Seconds each of the host's modules takes longer to plan here
    /// (to try the host's load barrier and the late-loader path)
    #[arg(long, default_value_t = 0.0)]
    plan_delay: f64,
    /// Write the own body's pelvis, head, hands and feet every frame
    /// (CSV), as `kjk play --trace`
    #[arg(long)]
    trace: Option<std::path::PathBuf>,
    /// The host's join password (`kjk host --password`)
    #[arg(long)]
    password: Option<String>,
}

/// `kjk join`: joins the host, loads its stage, and plays in it.
pub(crate) fn run(args: JoinArgs, size: (u32, u32)) -> ExitCode {
    let conditions = match netem_conditions(args.netem.as_deref()) {
        Ok(conditions) => conditions,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let spec = JoinSpec {
        host: &args.host,
        name: &args.name,
        password: args.password.as_deref(),
        conditions,
        moves: args.moves.as_deref(),
        sim_blades: args.sim_blades,
        plan_delay: std::time::Duration::from_secs_f64(args.plan_delay.clamp(0.0, 600.0)),
        trace: args.trace.as_deref(),
    };
    // The host's module is known once it answers: the screen shows none.
    launch(
        args.cmds.as_deref().unwrap_or(""),
        args.headless,
        size,
        None,
        || join_session(&discover(), spec),
    )
}
