//! A mesh's open loops of edges and the fans that close them, shared by
//! cutting a limb off (`cut`) and shattering a prop (`shatter`).

use std::collections::BTreeMap;

pub(crate) fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}

pub(crate) fn cross(u: [f32; 3], v: [f32; 3]) -> [f32; 3] {
    [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ]
}

pub(crate) fn dot(u: [f32; 3], v: [f32; 3]) -> f32 {
    u[0] * v[0] + u[1] * v[1] + u[2] * v[2]
}

pub(crate) fn length(u: [f32; 3]) -> f32 {
    dot(u, u).sqrt()
}

/// The open loops of a mesh's edges (edges of one triangle only), each as
/// the triangles run it. Vertices at one place are one corner (meshes split
/// vertices along uv seams); a loop names each corner by its first vertex.
pub(crate) fn open_loops(positions: &[[f32; 3]], indices: &[u32]) -> Vec<Vec<u32>> {
    let (triangles, _) = indices.as_chunks::<3>();
    // Edges as the triangles run them; an edge whose reverse is not there
    // is open.
    let key = |v: u32| positions[v as usize].map(f32::to_bits);
    let mut first: BTreeMap<[u32; 3], u32> = BTreeMap::new();
    let mut runs = BTreeMap::new();
    for t in triangles {
        let t = t.map(|v| *first.entry(key(v)).or_insert(v));
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            if a != b {
                runs.insert((a, b), ());
            }
        }
    }
    // The open edges out of each corner: more than one where two loops
    // pinch through it.
    let mut next: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for &(a, b) in runs.keys().filter(|&&(a, b)| !runs.contains_key(&(b, a))) {
        next.entry(a).or_default().push(b);
    }
    // One open edge out of `at`, taken.
    fn take(next: &mut BTreeMap<u32, Vec<u32>>, at: u32) -> Option<u32> {
        let outs = next.get_mut(&at)?;
        let to = outs.pop();
        if outs.is_empty() {
            next.remove(&at);
        }
        to
    }
    // Walk the edges; coming back to a corner closes the loop since it. A
    // walk that leads nowhere is dropped.
    let mut laps = Vec::new();
    while let Some(start) = next.keys().next().copied() {
        let mut lap = vec![start];
        while let Some(to) = take(&mut next, *lap.last().unwrap_or(&start)) {
            if let Some(back) = lap.iter().position(|&v| v == to) {
                laps.push(lap.split_off(back));
                if lap.is_empty() {
                    break;
                }
            }
            lap.push(to);
        }
    }
    laps.retain(|lap| lap.len() >= 3);
    laps
}

/// The middle of a loop's corners.
pub(crate) fn middle(positions: &[[f32; 3]], lap: &[u32]) -> [f32; 3] {
    lap.iter()
        .fold([0.0f32; 3], |sum, &v| {
            std::array::from_fn(|i| sum[i] + positions[v as usize][i])
        })
        .map(|c| c / lap.len() as f32)
}

/// A fan closing `lap` from `middle`: its rim edges, each run back from the
/// way the mesh runs it (so the fan faces out of the hole), and the way it
/// faces (the loop's best-fit plane).
pub(crate) fn fan(
    positions: &[[f32; 3]],
    lap: &[u32],
    middle: [f32; 3],
) -> (Vec<(u32, u32)>, [f32; 3]) {
    let corner = |v: u32| positions[v as usize];
    let pairs: Vec<(u32, u32)> = lap
        .iter()
        .zip(lap.iter().cycle().skip(1))
        .map(|(&a, &b)| (b, a))
        .collect();
    let facing = pairs.iter().fold([0.0f32; 3], |sum, &(a, b)| {
        let n = cross(sub(corner(a), middle), sub(corner(b), middle));
        std::array::from_fn(|i| sum[i] + n[i])
    });
    let size = length(facing).max(1e-9);
    (pairs, facing.map(|c| c / size))
}
