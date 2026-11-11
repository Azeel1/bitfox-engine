#[allow(dead_code)]
#[path = "src/eval/contract.rs"]
mod contract;

use contract::{FEATURES, HL, NETWORK_BYTES, OUTPUT_BUCKETS, QA, QB, SCALE};

fn word(bytes: &[u8], index: usize) -> i64 {
    i64::from(i16::from_le_bytes([bytes[2 * index], bytes[2 * index + 1]]))
}

fn validate(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() != NETWORK_BYTES {
        return Err(format!(
            "network size {}, expected {NETWORK_BYTES}",
            bytes.len()
        ));
    }
    if QA <= 0 || QA > i32::from(i16::MAX) || QB <= 0 || SCALE <= 0 {
        return Err("QA must fit positive i16; QB and SCALE must be positive".into());
    }
    if i64::from(QA) * i64::from(QB) > i64::from(i32::MAX) {
        return Err("QA * QB exceeds the i32 denominator".into());
    }
    // worst case: all 32 pieces land on a neuron with its biggest weight
    let input_end = FEATURES * HL;
    let mut largest = vec![0i64; HL];
    for (i, entry) in largest.iter_mut().enumerate() {
        for feature in 0..FEATURES {
            *entry = (*entry).max(word(bytes, feature * HL + i).abs());
        }
        let bias = word(bytes, input_end + i).abs();
        let bound = bias + 32 * *entry;
        if bound > i64::from(i16::MAX) {
            return Err(format!(
                "input accumulator neuron {i} bound {bound} exceeds i16"
            ));
        }
    }
    let output_start = input_end + HL;
    let bias_start = output_start + OUTPUT_BUCKETS * 2 * HL;
    let qa = i64::from(QA);
    for bucket in 0..OUTPUT_BUCKETS {
        let mut sum = 0i64;
        for i in 0..2 * HL {
            // the SIMD output layer computes v * w in i16, with v up to QA
            let w = word(bytes, output_start + bucket * 2 * HL + i).abs();
            if w * qa > i64::from(i16::MAX) {
                return Err(format!(
                    "output weight at bucket {bucket}, index {i} exceeds fused i16 limit"
                ));
            }
            sum += w;
        }
        let accumulated = sum * qa * qa;
        if accumulated > i64::from(i32::MAX) {
            return Err(format!(
                "output accumulator bucket {bucket} bound {accumulated} exceeds i32"
            ));
        }
        let biased = accumulated / qa + word(bytes, bias_start + bucket).abs();
        if biased > i64::from(i32::MAX) {
            return Err(format!("biased output bucket {bucket} exceeds i32"));
        }
        if biased * i64::from(SCALE) / (qa * i64::from(QB)) > i64::from(i32::MAX) {
            return Err(format!("scaled output bucket {bucket} exceeds i32"));
        }
    }
    Ok(())
}

fn main() {
    println!("cargo:rerun-if-changed=networks/bitfox.nnue");
    println!("cargo:rerun-if-changed=src/eval/contract.rs");
    let bytes = std::fs::read("networks/bitfox.nnue").expect("cannot read embedded network");
    validate(&bytes).unwrap_or_else(|e| panic!("invalid embedded network: {e}"));
}

// cargo doesn't test build scripts, run these by hand from engine/:
// rustc --edition 2021 --test build.rs -o guard-tests && ./guard-tests
#[cfg(test)]
mod tests {
    use super::*;

    fn set_word(bytes: &mut [u8], index: usize, value: i16) {
        bytes[index * 2..index * 2 + 2].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn embedded_network_passes() {
        let bytes = std::fs::read("networks/bitfox.nnue").unwrap();
        assert_eq!(validate(&bytes), Ok(()));
    }

    #[test]
    fn rejects_incorrect_length() {
        assert!(validate(&[]).unwrap_err().contains("network size"));
    }

    #[test]
    fn rejects_both_signs_beyond_output_product_limit() {
        let mut bytes = vec![0; NETWORK_BYTES];
        let start = FEATURES * HL + HL;
        for weight in [128, -128] {
            set_word(&mut bytes, start, weight);
            assert!(validate(&bytes).is_ok(), "weight {weight}");
        }
        for weight in [129, -129, i16::MIN, i16::MAX] {
            set_word(&mut bytes, start, weight);
            assert!(validate(&bytes).unwrap_err().contains("output weight"));
        }
    }

    #[test]
    fn rejects_input_accumulator_overflow() {
        let mut bytes = vec![0; NETWORK_BYTES];
        set_word(&mut bytes, 0, 1023);
        assert!(validate(&bytes).is_ok());
        for weight in [1024, -1024] {
            set_word(&mut bytes, 0, weight);
            assert!(validate(&bytes).unwrap_err().contains("input accumulator"));
        }
    }

    #[test]
    fn rejects_output_sum_overflow_even_if_each_product_fits() {
        let mut bytes = vec![0; NETWORK_BYTES];
        let start = FEATURES * HL + HL;
        for i in 0..2 * HL {
            set_word(&mut bytes, start + i, 128);
        }
        assert!(validate(&bytes).unwrap_err().contains("output accumulator"));
    }
}
