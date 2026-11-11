pub const HL: usize = 768;
pub const INPUT_BUCKETS: usize = 10;
pub const OUTPUT_BUCKETS: usize = 8;
pub const SCALE: i32 = 400;
pub const QA: i32 = 255;
pub const QB: i32 = 64;
pub const PIECE_FEATURES: usize = 2 * 6 * 64;

#[rustfmt::skip]
pub const BUCKET_LAYOUT: [usize; 32] = [
    0, 1, 2, 3,
    4, 5, 6, 7,
    8, 8, 8, 8,
    9, 9, 9, 9,
    9, 9, 9, 9,
    9, 9, 9, 9,
    9, 9, 9, 9,
    9, 9, 9, 9,
];

pub const FEATURES: usize = PIECE_FEATURES * INPUT_BUCKETS;
pub const PARAMETER_BYTES: usize =
    (FEATURES * HL + HL + OUTPUT_BUCKETS * 2 * HL + OUTPUT_BUCKETS) * 2;
pub const NETWORK_BYTES: usize = (PARAMETER_BYTES + 63) & !63;
