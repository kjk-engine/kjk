//! KOTOR's humanoid skeleton (the player bodies and every humanoid
//! creature share it) as a ragdoll: pelvis, belly, chest and head, upper
//! and lower arms (the hand with the forearm), thighs, shins and feet.
//! Ranges are a person's, a little loose: a body thrown down should look
//! hurt, not broken.

use crate::body::{Group, JointPlan, PartPlan, Plan, Shape, Way};

/// A grown body's mass (kg).
pub const MASS: f32 = 75.0;

const fn arm(side: Group, names: [&'static str; 5]) -> [PartPlan; 2] {
    let [upper, lower, bicep, forearm, hand] = names;
    [
        PartPlan {
            name: upper,
            node: bicep,
            parent: Some("chest"),
            shape: Shape::To {
                node: forearm,
                past: 0.0,
            },
            // Hanging down and a little forward is the middle of a
            // shoulder's reach.
            joint: JointPlan::Ball {
                cone: 85.0,
                twist: 70.0,
                lean: Some([0.45, 0.35, -0.8]),
            },
            mass: 0.028,
            thickness: [0.035, 0.065],
            group: side,
        },
        PartPlan {
            name: lower,
            node: forearm,
            parent: Some(upper),
            shape: Shape::To {
                node: hand,
                past: 0.09,
            },
            joint: JointPlan::Hinge {
                toward: [0.0, 1.0, 0.0],
                max: 145.0,
            },
            mass: 0.022,
            thickness: [0.03, 0.05],
            group: side,
        },
    ]
}

const fn leg(side: Group, names: [&'static str; 6]) -> [PartPlan; 3] {
    let [upper, lower, end, thigh, shin, foot] = names;
    [
        PartPlan {
            name: upper,
            node: thigh,
            parent: Some("pelvis"),
            shape: Shape::To {
                node: shin,
                past: 0.0,
            },
            // Forward far, back a little, out some.
            joint: JointPlan::Ball {
                cone: 62.0,
                twist: 35.0,
                lean: Some([0.15, 0.55, -0.85]),
            },
            mass: 0.1,
            thickness: [0.06, 0.095],
            group: side,
        },
        PartPlan {
            name: lower,
            node: shin,
            parent: Some(upper),
            shape: Shape::To {
                node: foot,
                past: 0.0,
            },
            joint: JointPlan::Hinge {
                toward: [0.0, -1.0, 0.0],
                max: 150.0,
            },
            mass: 0.045,
            thickness: [0.045, 0.07],
            group: side,
        },
        PartPlan {
            name: end,
            node: foot,
            parent: Some(lower),
            shape: Shape::Along {
                way: Way::Forward,
                length: 0.14,
            },
            joint: JointPlan::Ball {
                cone: 30.0,
                twist: 15.0,
                lean: None,
            },
            mass: 0.015,
            thickness: [0.035, 0.05],
            group: side,
        },
    ]
}

const LEFT_ARM: [PartPlan; 2] = arm(
    Group::ArmLeft,
    [
        "upper arm left",
        "forearm left",
        "lbicep_g",
        "lforearm_g",
        "lhand_g",
    ],
);
const RIGHT_ARM: [PartPlan; 2] = arm(
    Group::ArmRight,
    [
        "upper arm right",
        "forearm right",
        "rbicep_g",
        "rforearm_g",
        "rhand_g",
    ],
);
const LEFT_LEG: [PartPlan; 3] = leg(
    Group::LegLeft,
    [
        "thigh left",
        "shin left",
        "foot left",
        "lthigh_g",
        "lshin_g",
        "lfoot_g",
    ],
);
const RIGHT_LEG: [PartPlan; 3] = leg(
    Group::LegRight,
    [
        "thigh right",
        "shin right",
        "foot right",
        "rthigh_g",
        "rshin_g",
        "rfoot_g",
    ],
);

const CORE: [PartPlan; 4] = [
    PartPlan {
        name: "pelvis",
        node: "pelvis_g",
        parent: None,
        shape: Shape::Between {
            ends: ("lthigh_g", "rthigh_g"),
            aim: "torso_g",
        },
        joint: JointPlan::Root,
        mass: 0.15,
        thickness: [0.08, 0.13],
        group: Group::Core,
    },
    PartPlan {
        name: "belly",
        node: "torso_g",
        parent: Some("pelvis"),
        shape: Shape::To {
            node: "torsoupr_g",
            past: 0.0,
        },
        joint: JointPlan::Ball {
            cone: 30.0,
            twist: 25.0,
            lean: None,
        },
        mass: 0.13,
        thickness: [0.09, 0.14],
        group: Group::Core,
    },
    PartPlan {
        name: "chest",
        node: "torsoupr_g",
        parent: Some("belly"),
        shape: Shape::Between {
            ends: ("lbicep_g", "rbicep_g"),
            aim: "neck_g",
        },
        joint: JointPlan::Ball {
            cone: 25.0,
            twist: 25.0,
            lean: None,
        },
        mass: 0.2,
        thickness: [0.09, 0.13],
        group: Group::Core,
    },
    PartPlan {
        name: "head",
        node: "head_g",
        parent: Some("chest"),
        shape: Shape::Along {
            way: Way::Up,
            length: 0.2,
        },
        joint: JointPlan::Ball {
            cone: 50.0,
            twist: 65.0,
            lean: None,
        },
        mass: 0.08,
        thickness: [0.1, 0.11],
        group: Group::Head,
    },
];

/// Parents first.
const PARTS: [PartPlan; 14] = [
    CORE[0],
    CORE[1],
    CORE[2],
    CORE[3],
    LEFT_ARM[0],
    LEFT_ARM[1],
    RIGHT_ARM[0],
    RIGHT_ARM[1],
    LEFT_LEG[0],
    LEFT_LEG[1],
    LEFT_LEG[2],
    RIGHT_LEG[0],
    RIGHT_LEG[1],
    RIGHT_LEG[2],
];

pub const PLAN: Plan = Plan {
    parts: &PARTS,
    mass: MASS,
    hips: ("lthigh_g", "rthigh_g"),
    // A body whose head is a model of its own (appearance model type B: the
    // commoners, the Republic's soldiers, the player's bodies) has no neck
    // or head nodes; its head hangs at `headhook`, at the neck's base.
    stand_ins: &[("head_g", "headhook"), ("neck_g", "headhook")],
};
