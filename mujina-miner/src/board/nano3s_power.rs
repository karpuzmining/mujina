//! Input-power math for the Avalon Nano 3S.
//!
//! The measured path follows Canaan's `psu_dcdc_pd.c`: INA226 bus register
//! `0x02` is 1.25 mV/LSB and current register `0x04` is 0.1831 mA/LSB after
//! calibration `0x0AEC` (a 10 mΩ shunt). The estimate is only used when that
//! meter does not answer. See `docs/nano3s-power.md`.

/// `MUJINA_NANO3S_I2C2=off` (also `0`, `disable`, `disabled`) skips every
/// transaction on `/dev/i2c-2`. An unset or empty value leaves the bus on.
pub(crate) fn i2c2_bus_enabled(value: Option<&str>) -> bool {
    match value.map(str::trim).filter(|s| !s.is_empty()) {
        None => true,
        Some(v) => !matches!(
            v.to_ascii_lowercase().as_str(),
            "0" | "off" | "disable" | "disabled"
        ),
    }
}

/// Bus volts, amps, watts from the two INA226 words Canaan reads.
pub(crate) fn ina226_sdk_reading(bus_raw: u16, cur_raw: u16) -> (f64, f64, f64) {
    const CUR_LSB_MA: f64 = 0.1831;
    let bus_v = bus_raw as f64 * 1.25 / 1000.0;
    let current_a = if cur_raw & 0x8000 != 0 {
        0.0
    } else {
        cur_raw as f64 * CUR_LSB_MA / 1000.0
    };
    (bus_v, current_a, bus_v * current_a)
}

/// Leakage and dynamic terms, watts at the ASIC, at the LOW reference
/// (210 MHz, rail 3392 mV shared by 12 dies). Dividing by
/// [`INPUT_EFFICIENCY`] turns that into input watts.
const POWER_LEAK_W: f64 = 0.880;
const POWER_DYN_W: f64 = 2.915;
const POWER_VREF_V: f64 = 3.392 / 12.0;
const POWER_FREF_MHZ: f64 = 210.0;
const INPUT_EFFICIENCY: f64 = 0.90;

/// One chip's share of input watts. `die_mv` is the on-die PVT reading,
/// or the series rail divided by 12 when that reading is missing.
pub(crate) fn chip_input_watts(die_mv: f64, freq_mhz: f64) -> f64 {
    let v = if die_mv > 20.0 && die_mv < 1500.0 {
        die_mv / 1000.0
    } else {
        POWER_VREF_V
    };
    let f = if freq_mhz > 1.0 { freq_mhz } else { POWER_FREF_MHZ };
    let asic = POWER_LEAK_W + POWER_DYN_W * (v / POWER_VREF_V).powi(2) * (f / POWER_FREF_MHZ);
    asic / INPUT_EFFICIENCY
}

/// IOMUX pad at `0x91105000`. SEL is bits[13:11]. Alt3 (`0x19CF` on
/// IO11/IO12) is I2C2. Clearing SEL leaves the same pull and drive
/// bits with the pad in GPIO mode (`0x01CF`).
pub(crate) const I2C2_PAD_ALT3: u32 = 0x19CF;
pub(crate) const I2C2_PAD_SEL_MASK: u32 = 0x7 << 11;
pub(crate) const I2C2_PAD_INPUT_BIT: u32 = 1 << 31;

pub(crate) fn i2c2_pad_gpio(pad: u32) -> u32 {
    pad & !I2C2_PAD_SEL_MASK
}

/// Bit 31 is the pad's live input. SDA stuck low is that bit clear.
pub(crate) fn i2c2_sda_stuck_low(sda_pad: u32) -> bool {
    sda_pad & I2C2_PAD_INPUT_BIT == 0
}

/// Sum of twelve chips. Missing die voltage uses `rail_mv / 12`.
pub(crate) fn estimate_input_watts(dies_mv: &[f64], freq_mhz: &[f64], rail_mv: f64) -> f64 {
    let n = 12usize;
    let mut sum = 0.0;
    for i in 0..n {
        let die = dies_mv
            .get(i)
            .copied()
            .filter(|v| *v > 20.0 && *v < 1500.0)
            .unwrap_or_else(|| {
                if rail_mv > 500.0 {
                    rail_mv / n as f64
                } else {
                    POWER_VREF_V * 1000.0
                }
            });
        let f = freq_mhz.get(i).copied().filter(|f| *f > 1.0).unwrap_or(0.0);
        sum += chip_input_watts(die, f);
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i2c2_off_skips_the_bus() {
        assert!(i2c2_bus_enabled(None));
        assert!(i2c2_bus_enabled(Some("")));
        assert!(i2c2_bus_enabled(Some("on")));
        assert!(!i2c2_bus_enabled(Some("off")));
        assert!(!i2c2_bus_enabled(Some("0")));
        assert!(!i2c2_bus_enabled(Some(" Disable ")));
    }

    #[test]
    fn ina226_scale_matches_canaan() {
        let (v, i, p) = ina226_sdk_reading(22112, 16658);
        assert!((v - 27.64).abs() < 0.02, "bus {v}");
        assert!((i - 3.05).abs() < 0.02, "amps {i}");
        assert!((p - 84.3).abs() < 0.5, "watts {p}");
        let (_, amps, _) = ina226_sdk_reading(22112, 0x8001);
        assert_eq!(amps, 0.0);
    }

    #[test]
    fn estimate_matches_the_published_points() {
        let low = estimate_input_watts(&[], &[210.0; 12], 3392.0);
        assert!((low - 50.6).abs() < 0.2, "low {low}");
        let high = estimate_input_watts(&[], &[460.0; 12], 3860.0);
        assert!((high - 122.0).abs() < 0.3, "high {high}");
        let die = 3392.0 / 12.0;
        let from_die = estimate_input_watts(&[die; 12], &[210.0; 12], 0.0);
        assert!((from_die - low).abs() < 0.2, "die {from_die}");
    }

    #[test]
    fn i2c2_gpio_mode_clears_only_the_sel_field() {
        assert_eq!(i2c2_pad_gpio(I2C2_PAD_ALT3), 0x01CF);
        assert_eq!(i2c2_pad_gpio(0x8000_19CF) & !I2C2_PAD_INPUT_BIT, 0x01CF);
    }

    #[test]
    fn sda_input_bit_is_stuck_only_when_low() {
        assert!(i2c2_sda_stuck_low(0x0000_19CF));
        assert!(!i2c2_sda_stuck_low(0x8000_19CF));
    }
}
