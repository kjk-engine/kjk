#define_import_path kjk::sway

// The places are arrays of three floats, not `vec3`s, which would be padded
// to 16 bytes. A slot is then `dangly::SLOT_BYTES` long.
struct SwayVertex {
    now: array<f32, 3>,
    was: array<f32, 3>,
}

fn last_slot(length: u32) -> u32 {
    return max(length, 1u) - 1u;
}
