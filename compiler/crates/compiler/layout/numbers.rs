//! Bit widths, quantization steps and event id codes.

pub fn bits_for(max: u64) -> u32 {
    (64 - max.leading_zeros()).max(1)
}

pub fn float_steps(min: f64, max: f64, step: f64) -> u64 {
    ((max - min) / step - 1e-9).ceil() as u64
}

pub fn rotation_steps(degrees: f64) -> u32 {
    let radians = degrees.to_radians();
    let needed = (3.0 * std::f64::consts::SQRT_2 / radians).ceil() as u64 + 1;
    let bits = (64 - needed.leading_zeros()).clamp(4, 16);
    (1 << bits) - 2
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdCodes {
    pub bits: u32,
    pub short: u32,
}

impl IdCodes {
    pub fn new(events: u64) -> IdCodes {
        let bits = bits_for(events);
        let symbols = events + 1;
        if events == 0 || symbols.is_power_of_two() {
            return IdCodes { bits, short: 0 };
        }
        IdCodes { bits, short: ((1u64 << bits) - symbols) as u32 }
    }

    pub fn first_bits(self) -> u32 {
        if self.short > 0 { self.bits - 1 } else { self.bits }
    }

    pub fn code(self, symbol: u32) -> (u32, u32) {
        if self.short == 0 {
            return (symbol, self.bits);
        }
        if symbol < self.short {
            return (symbol, self.bits - 1);
        }
        let rest = symbol - self.short;
        (self.short + rest / 2 + ((rest % 2) << (self.bits - 1)), self.bits)
    }
}

pub fn mask(bits: u32) -> u64 {
    if bits >= 32 { u64::from(u32::MAX) } else { (1 << bits) - 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_cover_their_maximum() {
        assert_eq!(bits_for(0), 1);
        assert_eq!(bits_for(1), 1);
        assert_eq!(bits_for(255), 8);
        assert_eq!(bits_for(256), 9);
        assert_eq!(bits_for(u64::from(u32::MAX)), 32);
    }

    #[test]
    fn whole_step_ranges_gain_no_step() {
        assert_eq!(float_steps(-100.0, 100.0, 0.01), 20000);
        assert_eq!(float_steps(0.0, 1.0, 0.3), 4);
    }

    #[test]
    fn rotation_steps_are_even_and_tighten_with_precision() {
        for degrees in [45.0, 2.0, 0.5, 0.25, 0.001] {
            assert_eq!(rotation_steps(degrees) % 2, 0);
        }
        assert!(rotation_steps(0.25) > rotation_steps(0.5));
    }

    #[test]
    fn id_codes_shorten_the_first_symbols() {
        let ids = IdCodes::new(4);
        assert_eq!(ids, IdCodes { bits: 3, short: 3 });
        let codes: Vec<(u32, u32)> = (0..=4).map(|symbol| ids.code(symbol)).collect();
        assert_eq!(codes, [(0, 2), (1, 2), (2, 2), (3, 3), (7, 3)]);
        assert_eq!(IdCodes::new(3), IdCodes { bits: 2, short: 0 });
        assert_eq!(IdCodes::new(0), IdCodes { bits: 1, short: 0 });
        assert_eq!(IdCodes::new(16).short, 15);
    }
}
