//! Packs fields that waste bits into one number, so they share the waste.

use super::numbers::bits_for;

pub const MAX_PRODUCT: u64 = 1 << 52;

pub const MAX_CHUNK_BITS: u32 = 21;

fn candidate(max: u64) -> bool {
    let held = 1u64 << bits_for(max);
    max > 0 && (held - (max + 1)) * 64 >= held
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    pub size: u32,
    pub base: u64,
    pub bits: u32,
}

impl Chunk {
    pub fn new(max: u64) -> Option<Chunk> {
        if !candidate(max) {
            return None;
        }
        let base = max + 1;
        let (mut size, mut product) = (0u32, 1u64);
        while product * base <= 1 << MAX_CHUNK_BITS {
            product *= base;
            size += 1;
        }
        let bits = group_bits(product);
        (size >= 2 && bits < size * bits_for(max)).then_some(Chunk { size, base, bits })
    }

    pub fn partial_bits(self, count: u32) -> u32 {
        if count == 0 { 0 } else { group_bits(self.base.pow(count)) }
    }

    pub fn bits_for_count(self, count: u64) -> u64 {
        let size = u64::from(self.size);
        count / size * u64::from(self.bits) + u64::from(self.partial_bits((count % size) as u32))
    }
}

pub fn group_bits(product: u64) -> u32 {
    bits_for(product - 1)
}

pub fn groups(maxes: &[u64]) -> Vec<Vec<usize>> {
    let mut found: Vec<(Vec<usize>, u64)> = Vec::new();
    let mut open: (Vec<usize>, u64) = (Vec::new(), 1);
    for (index, &max) in maxes.iter().enumerate() {
        if !candidate(max) {
            continue;
        }
        let base = max + 1;
        if u128::from(open.1) * u128::from(base) > u128::from(MAX_PRODUCT) {
            found.push(std::mem::replace(&mut open, (Vec::new(), 1)));
        }
        open.0.push(index);
        open.1 *= base;
    }
    found.push(open);
    found
        .into_iter()
        .filter(|(members, product)| {
            let apart: u32 = members.iter().map(|&index| bits_for(maxes[index])).sum();
            members.len() >= 2 && group_bits(*product) < apart
        })
        .map(|(members, _)| members)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_fractional_fields() {
        assert_eq!(groups(&[1050, 2]), [vec![0, 1]]);
        assert_eq!(group_bits(1051 * 3), 12);
    }

    #[test]
    fn leaves_powers_of_two_and_single_values_alone() {
        assert!(groups(&[255, 1, 0, 0, 65535]).is_empty());
    }

    #[test]
    fn leaves_nearly_full_ranges_alone() {
        assert!(groups(&[510, 510, 510, 62, 62]).is_empty());
        assert_eq!(groups(&[40, 40]), [vec![0, 1]]);
    }

    #[test]
    fn keeps_only_groups_that_save_bits() {
        assert_eq!(groups(&[2, 4]), [vec![0, 1]]);
        assert!(groups(&[2, 2]).is_empty());
    }

    #[test]
    fn chunks_pack_small_ranges() {
        assert_eq!(Chunk::new(2), Some(Chunk { size: 13, base: 3, bits: 21 }));
        assert_eq!(Chunk::new(2).map(|chunk| chunk.partial_bits(4)), Some(7));
        assert_eq!(Chunk::new(2).map(|chunk| chunk.bits_for_count(30)), Some(21 + 21 + 7));
        assert_eq!(Chunk::new(1050), Some(Chunk { size: 2, base: 1051, bits: 21 }));
        assert_eq!(Chunk::new(1800), None);
        assert_eq!(Chunk::new(255), None);
        assert_eq!(Chunk::new(510), None);
    }

    #[test]
    fn splits_at_the_product_limit() {
        assert_eq!(groups(&[102_400, 102_400, 102_400]), [vec![0, 1, 2]]);
        assert!(groups(&[409_600, 409_600, 409_600]).is_empty());
        assert_eq!(groups(&[409_600, 409_600, 2]), [vec![0, 1, 2]]);
    }
}
