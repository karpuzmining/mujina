// SPDX-License-Identifier: MIT
//
// Copyright (c) 2026, Canaan Creative Co., Ltd.
//
// Port of the A3197S CPM table lookup, PLL programming order, and PVT
// decode from Canaan Creative's canaan_sdk_for_linux v1.1.2-alpha
// (commit 07fe73f), apps/miner/hal/asic/a3197s/{a3197s_pll.c,
// a3197s_pll_seq.c, a3197s_pvt.c}. The original files are MIT licensed.
// See NOTICE. The rest of Mujina is GPL-3.0-or-later.
//
// NO WARRANTY. The Software is provided "AS IS", without warranty of any
// kind.

//! A3197S clock and PVT helpers shared by the Nano 3S API and ChipTuner.
//!
//! The PLL programming order is the official `a3197s_set_pll_seq`. Canaan's
//! miner manual warns that a wrong order or chip select can hang or damage
//! the chips. `rtos_core` executes that order; this module is the tested
//! description of it.
//!
//! `correct_freq()` is ported and tested, but the running lookup is
//! [`cpm_pll`] (the ceiling search `a3197s_set_pll_seq` itself calls).
//! Canaan's `a3197s_set_freq` snaps some integer requests down by 1 MHz
//! before that (`230 → 229`, `100 → 99`). Applying that snap would move
//! clocks operators and AutoTune have already saved, including under the
//! 100 MHz floor, so the firmware does not.

// Used by unit tests and by the `nano3s` feature. A host build without
// that feature only calls the offset and temperature constants.
#![cfg_attr(not(any(test, feature = "nano3s")), allow(dead_code))]

include!("a3197s_cpm_table.inc.rs");

/// Big cores per A3197S. PLLCNT1 is this, or this shifted by 8, when a
/// domain has locked.
pub const CORE_COUNT: u32 = 152;
/// ROLLTIME = this / PLL3 frequency in MHz.
pub const ROLLTIME_DIVIDEND: u32 = 335_000_000;
/// Broadcast chip address (`A3197S_CHIP_BROADCAST`).
pub const CHIP_BROADCAST: u16 = 0x3ff;
/// Nano 3S `INIT_FREQ`. `correct_freq` will not return a lower clock.
pub const INIT_FREQ_MHZ: u32 = 100;

pub const REG_SS_CTRL: u32 = 0x400;
pub const REG_SPD_SYNC: u32 = 0x21c;
pub const REG_ROLLTIME: u32 = 0x214;
pub const REG_PLL0: u32 = 0x3004;
pub const REG_PLL1: u32 = 0x300c;
pub const REG_PLL2: u32 = 0x3014;
pub const REG_PLL3: u32 = 0x301c;
pub const REG_MUX: u32 = 0x303c;
pub const REG_PLLCNT1: u32 = 0x420;

pub const SPD_SYNC_CFG: u32 = 0x8003_9700;
pub const DEFAULT_MUX: u32 = 0x108c_000f;

/// Manual API and ChipTuner share this offset window, MHz.
pub const CHIP_OFFSET_MIN_MHZ: i16 = -150;
pub const CHIP_OFFSET_MAX_MHZ: i16 = 50;
/// Each PLL domain, after the offset is applied, stays in this window.
pub const DOMAIN_MIN_MHZ: u32 = 100;
pub const DOMAIN_MAX_MHZ: u32 = 580;
/// Series rail. 3600 mV is the core-domain migration collapse.
pub const RAIL_MIN_MV: u32 = 3300;
pub const RAIL_MAX_MV: u32 = 3912;
pub const RAIL_FORBIDDEN_MV: u32 = 3600;
/// Emergency clock drop. ChipTuner will not hold a step-up through this.
pub const BACKSTOP_TEMP_C: f64 = 97.0;
/// Board power trip used by the manual API.
pub const POWER_TRIP_W: f64 = 137.0;

/// What one step of [`pll_sequence`] does. The firmware performs these in
/// order and does not stop on a lock-count mismatch (it logs, as Canaan does).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PllStep {
    Select(u16),
    Write {
        addr: u32,
        value: u32,
    },
    DelayMs(u32),
    ExpectLock {
        reg: u32,
        name: &'static str,
        count: u32,
    },
}

/// Inputs to [`pll_sequence`]. SS_CTRL fields are Canaan's Nano 3S defaults
/// (`ssdn_pro=0`, `spdlow=1`, `spdhigh=4`, `ss_sel=1`, `pll_select=2`,
/// `ss_enable=1`).
#[derive(Clone, Debug)]
pub struct PllProgram {
    pub freq: [u32; 4],
    pub init_freq: u32,
    /// When false, PLL0 is parked at `init_freq` and not retargeted.
    pub pll0_update: bool,
    pub asic_id: u16,
    /// Chip select to restore when the sequence finishes.
    pub saved_select: u16,
}

/// Ceiling search. Same rule as Canaan `get_cpm_pll`: the first row whose
/// labeled MHz is at least `freq`. Returns 0 above the table.
pub fn cpm_pll(freq: u32) -> u32 {
    CPM_TABLE
        .iter()
        .find(|&&(f, _)| freq <= f)
        .map(|&(_, w)| w)
        .unwrap_or(0)
}

/// Inverse of [`cpm_pll`]. Lock and lock-ip bits are ignored, matching
/// Canaan `get_cpm_freq`.
pub fn cpm_freq(pll: u32) -> u32 {
    let pll = (pll & 0x9fff_ffff) | 1;
    CPM_TABLE
        .iter()
        .find(|&&(_, w)| w == pll)
        .map(|&(f, _)| f)
        .unwrap_or(0)
}

/// Canaan `correct_freq`. Floors at [`INIT_FREQ_MHZ`], then snaps to the
/// table row whose following word's FPLL/2 still covers `freq`.
///
/// Not used when programming. See the module docs.
pub fn correct_freq(mut freq: u32) -> u32 {
    if freq < INIT_FREQ_MHZ {
        freq = INIT_FREQ_MHZ;
    }
    for i in 0..CPM_TABLE.len() - 1 {
        let pll = CPM_TABLE[i + 1].1;
        let fpll = fpll_mhz(pll);
        if freq >= CPM_TABLE[i].0 && freq < (fpll + 1) / 2 {
            return CPM_TABLE[i].0;
        }
    }
    CPM_TABLE[CPM_TABLE.len() - 1].0
}

fn fpll_mhz(pll: u32) -> u32 {
    let fb = (pll >> 7) & 0x3ff;
    let refdiv = (pll >> 1) & 0x3f;
    let post = (pll >> 17) & 0x7;
    let den = refdiv << post;
    if den == 0 {
        return 0;
    }
    fb * 24 / den
}

/// Mux divider field from `calc_mux_div_mode`: `<400 → 4`, `<600 → 6`,
/// `<800 → 8`, else 10.
pub fn mux_div_mode(freq: u32) -> u32 {
    if freq < 400 {
        4
    } else if freq < 600 {
        6
    } else if freq < 800 {
        8
    } else {
        10
    }
}

pub fn mux_word(freq: u32) -> u32 {
    (DEFAULT_MUX & !(0x0f << 21)) | (mux_div_mode(freq) << 21)
}

pub fn clamp_domain(freq: u32) -> u32 {
    freq.clamp(DOMAIN_MIN_MHZ, DOMAIN_MAX_MHZ)
}

/// Official SS_CTRL word for the Nano 3S defaults at this `glbset` / `spd_en`.
pub fn ss_ctrl(glbset: u32, spd_en: u32) -> u32 {
    let mut w = glbset & 7;
    w |= 1 << 3; // glbsetvld
    w |= 1 << 5; // spdlow = 1
    w |= 4 << 8; // spdhigh = 4
    w |= (spd_en & 1) << 11;
    w |= 1 << 12; // ss_sel = 1
    w
}

/// Ordered steps for one chip (or the broadcast address).
pub fn pll_sequence(prog: &PllProgram) -> Vec<PllStep> {
    let freq = prog.freq.map(clamp_domain);
    let mut steps = Vec::new();
    if !prog.pll0_update {
        steps.push(PllStep::Select(prog.asic_id));
        let park = cpm_pll(clamp_domain(prog.init_freq)) & 0xf7ff_fffe;
        steps.push(PllStep::Write {
            addr: REG_PLL0,
            value: park,
        });
        steps.push(PllStep::DelayMs(1));
    }
    push_ss(&mut steps, prog.asic_id, ss_ctrl(1, 0));
    push_lock(&mut steps, prog.asic_id, "PLL0", CORE_COUNT);
    steps.push(PllStep::Select(prog.asic_id));
    steps.push(PllStep::Write {
        addr: REG_PLL1,
        value: cpm_pll(freq[1]),
    });
    steps.push(PllStep::Write {
        addr: REG_PLL2,
        value: cpm_pll(freq[2]),
    });
    steps.push(PllStep::Write {
        addr: REG_PLL3,
        value: cpm_pll(freq[3]),
    });
    steps.push(PllStep::Write {
        addr: REG_ROLLTIME,
        value: ROLLTIME_DIVIDEND / freq[3].max(1),
    });
    steps.push(PllStep::DelayMs(1));
    if prog.pll0_update {
        push_ss(&mut steps, prog.asic_id, ss_ctrl(2, 0));
        push_lock(&mut steps, prog.asic_id, "PLL1", CORE_COUNT << 8);
        steps.push(PllStep::Select(prog.asic_id));
        steps.push(PllStep::Write {
            addr: REG_MUX,
            value: mux_word(freq[1]) | (1 << 4),
        });
        steps.push(PllStep::Write {
            addr: REG_PLL0,
            value: cpm_pll(freq[0]),
        });
        steps.push(PllStep::DelayMs(1));
        steps.push(PllStep::Write {
            addr: REG_MUX,
            value: mux_word(freq[0]),
        });
        steps.push(PllStep::DelayMs(1));
    }
    // Final SmartSpeed enable: spd_en=1, glbset=pll_select (2).
    push_ss(&mut steps, prog.asic_id, ss_ctrl(2, 1));
    steps.push(PllStep::Select(prog.saved_select));
    steps
}

fn push_ss(steps: &mut Vec<PllStep>, asic: u16, word: u32) {
    steps.push(PllStep::Select(asic));
    steps.push(PllStep::Write {
        addr: REG_SS_CTRL,
        value: word,
    });
    steps.push(PllStep::Write {
        addr: REG_SPD_SYNC,
        value: SPD_SYNC_CFG,
    });
}

fn push_lock(steps: &mut Vec<PllStep>, asic: u16, name: &'static str, count: u32) {
    steps.push(PllStep::DelayMs(5));
    if asic == CHIP_BROADCAST {
        steps.push(PllStep::Select(0));
    }
    steps.push(PllStep::ExpectLock {
        reg: REG_PLLCNT1,
        name,
        count,
    });
}

/// Degree-4 PVT temperature. `None` outside −50..200 °C, which is Canaan's
/// `ASIC_TEMP_NONE`. rtos_core reports that case as −273 so existing
/// callers can keep their sentinel.
pub fn decode_pvt_temp_c(code: u16) -> Option<f64> {
    let c = f64::from(code);
    let t = (((2.28746e-10 * c) + -2.84236e-6) * c + 1.31753e-2) * c + -26.70380;
    let t = t * c + 19725.61;
    if !(-50.0..=200.0).contains(&t) {
        None
    } else {
        Some(t)
    }
}

/// PVT voltage in mV. 0 when the decoded value is under 14 mV.
pub fn decode_pvt_volt_mv(code: u16) -> u16 {
    let mv = (f64::from(code) * 1000.0 + 19144.2) / 3434.4847;
    if mv < 14.0 { 0 } else { (mv + 0.5) as u16 }
}

pub fn rail_mv_allowed(mv: u32) -> bool {
    (RAIL_MIN_MV..=RAIL_MAX_MV).contains(&mv) && mv != RAIL_FORBIDDEN_MV
}

/// Whether manual chip clocks survive a profile change or a board VF write.
///
/// `Clear` is the v5.1.0 behavior and the upgrade default: those events
/// zero the twelve offsets and turn ChipTuner off. `Keep` is opt-in
/// (`chip_clock_policy=keep`): the offsets stay on top of the new base.
/// ChipTuner still drops an unjudged trial. Per-chip voltage is not a
/// setting; the twelve dies share one rail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ChipClockPolicy {
    #[default]
    Clear,
    Keep,
}

impl ChipClockPolicy {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "clear" => Some(Self::Clear),
            "keep" => Some(Self::Keep),
            _ => None,
        }
    }

    #[cfg_attr(not(feature = "nano3s"), allow(dead_code))]
    pub fn as_conf(self) -> &'static str {
        match self {
            Self::Clear => "clear",
            Self::Keep => "keep",
        }
    }

    pub fn clears_on_board_change(self) -> bool {
        matches!(self, Self::Clear)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChipClocks {
    Offsets([i16; 12]),
    Absolute([u32; 12]),
}

/// Turns a chip-clock request into offsets on `base` PLL0.
///
/// Absolute MHz is `offset = freq - base[0]`. Every domain of every chip
/// must stay in [`DOMAIN_MIN_MHZ`]..=[`DOMAIN_MAX_MHZ`], and the offset
/// itself must stay in the shared ChipTuner window. When `base` is still
/// unknown (a zero domain), only the offset window is checked.
pub fn resolve_chip_clocks(base: [u32; 4], clocks: ChipClocks) -> Result<[i16; 12], &'static str> {
    let offsets = match clocks {
        ChipClocks::Offsets(o) => o,
        ChipClocks::Absolute(freq) => {
            if base
                .iter()
                .any(|&d| !(DOMAIN_MIN_MHZ..=DOMAIN_MAX_MHZ).contains(&d))
            {
                return Err("board PLL is not known yet");
            }
            let mut o = [0i16; 12];
            for (i, f) in freq.iter().enumerate() {
                let delta = *f as i32 - base[0] as i32;
                let Ok(delta) = i16::try_from(delta) else {
                    return Err("absolute frequency is outside the offset window");
                };
                o[i] = delta;
            }
            o
        }
    };
    let know_base = base
        .iter()
        .all(|&d| (DOMAIN_MIN_MHZ..=DOMAIN_MAX_MHZ).contains(&d));
    for off in offsets {
        if !(CHIP_OFFSET_MIN_MHZ..=CHIP_OFFSET_MAX_MHZ).contains(&off) {
            return Err("offset is outside -150..=50 MHz");
        }
        if know_base {
            for d in base {
                let f = d as i32 + i32::from(off);
                if !(DOMAIN_MIN_MHZ as i32..=DOMAIN_MAX_MHZ as i32).contains(&f) {
                    return Err("a PLL domain would leave 100..=580 MHz");
                }
            }
        }
    }
    Ok(offsets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::nano3s_chip_tuner::ChipTunerConfig;

    fn writes(steps: &[PllStep]) -> Vec<(u32, u32)> {
        steps
            .iter()
            .filter_map(|s| match s {
                PllStep::Write { addr, value } => Some((*addr, *value)),
                _ => None,
            })
            .collect()
    }

    fn sample(update: bool) -> PllProgram {
        PllProgram {
            freq: [210, 230, 250, 270],
            init_freq: 100,
            pll0_update: update,
            asic_id: 4,
            saved_select: 7,
        }
    }

    #[test]
    fn table_is_the_official_349_rows() {
        assert_eq!(CPM_TABLE.len(), 349);
        assert_eq!(CPM_TABLE[0], (15, 0x900c_2803));
        assert_eq!(*CPM_TABLE.last().unwrap(), (600, 0x9002_3203));
        for w in CPM_TABLE.windows(2) {
            assert!(w[0].0 <= w[1].0, "{} then {}", w[0].0, w[1].0);
        }
        assert_eq!(cpm_pll(100), 0x9008_4303);
        assert_eq!(cpm_pll(174), 0x9006_3a03);
        assert_eq!(cpm_pll(210), 0x9006_4603);
        assert_eq!(cpm_freq(0x9004_4203), 396);
        // Lock bits are stripped before the reverse lookup.
        assert_eq!(cpm_freq(0x9004_4203 | 0x6000_0000), 396);
        assert_eq!(cpm_pll(700), 0);
        assert_eq!(cpm_pll(520) >> 28, 0x9);
    }

    #[test]
    fn rust_table_matches_the_rtos_array() {
        let c = include_str!("../../../rtos_core/src/pll_cpm_table.c");
        let mut parsed = Vec::new();
        for line in c.lines() {
            let line = line.trim().trim_end_matches(',');
            if !line.starts_with('{') || !line.contains("0x") {
                continue;
            }
            let inside = line.trim_matches(|ch: char| ch == '{' || ch == '}' || ch == ' ');
            let mut it = inside.split(',');
            let freq: u32 = it.next().unwrap().trim().parse().unwrap();
            let hex = it.next().unwrap().trim().trim_start_matches("0x");
            parsed.push((freq, u32::from_str_radix(hex, 16).unwrap()));
        }
        assert_eq!(parsed.len(), CPM_TABLE.len());
        assert_eq!(parsed, CPM_TABLE.to_vec());
    }

    #[test]
    fn operating_range_words_match_a_ceiling_search() {
        for freq in [174u32, 302, 396, 480, 520, 580] {
            let word = cpm_pll(freq);
            assert_ne!(word, 0, "{freq}");
            let labeled = cpm_freq(word);
            assert!(labeled >= freq, "{freq} labeled {labeled}");
            assert!(labeled - freq <= 3, "{freq} jumped to {labeled}");
        }
    }

    #[test]
    fn correct_freq_is_ported_but_snaps_some_saved_clocks() {
        assert_eq!(correct_freq(50), 99);
        assert_eq!(correct_freq(100), 99);
        assert_eq!(correct_freq(210), 210);
        assert_eq!(correct_freq(230), 229);
        assert_eq!(correct_freq(270), 270);
        assert_eq!(correct_freq(900), 600);
        // The programming path keeps the ceiling word for a saved 230 MHz.
        assert_ne!(cpm_pll(230), cpm_pll(correct_freq(230)));
    }

    #[test]
    fn update_sequence_parks_pll0_last_and_checks_lock() {
        let steps = pll_sequence(&sample(true));
        let addrs: Vec<u32> = writes(&steps).into_iter().map(|(a, _)| a).collect();
        let expected = [
            REG_SS_CTRL,
            REG_SPD_SYNC,
            REG_PLL1,
            REG_PLL2,
            REG_PLL3,
            REG_ROLLTIME,
            REG_SS_CTRL,
            REG_SPD_SYNC,
            REG_MUX,
            REG_PLL0,
            REG_MUX,
            REG_SS_CTRL,
            REG_SPD_SYNC,
        ];
        assert_eq!(addrs, expected);
        let words: Vec<u32> = writes(&steps).into_iter().map(|(_, w)| w).collect();
        assert_eq!(words[0], 0x0000_1429);
        assert_eq!(words[6], 0x0000_142a);
        assert_eq!(words[11], 0x0000_1c2a);
        assert_eq!(words[5], ROLLTIME_DIVIDEND / 270);
        assert_eq!(words[8], mux_word(230) | (1 << 4));
        assert_eq!(words[9], cpm_pll(210));
        assert_ne!(words[9], 0);
        assert!(steps.contains(&PllStep::ExpectLock {
            reg: REG_PLLCNT1,
            name: "PLL0",
            count: 152
        }));
        assert!(steps.contains(&PllStep::ExpectLock {
            reg: REG_PLLCNT1,
            name: "PLL1",
            count: 152 << 8
        }));
        assert_eq!(steps.last(), Some(&PllStep::Select(7)));
        // ROLLTIME is written before PLL0 moves.
        let roll = addrs.iter().position(|a| *a == REG_ROLLTIME).unwrap();
        let pll0 = addrs.iter().position(|a| *a == REG_PLL0).unwrap();
        assert!(roll < pll0);
    }

    #[test]
    fn park_sequence_does_not_retarget_pll0_or_touch_the_mux() {
        let steps = pll_sequence(&sample(false));
        let addrs: Vec<u32> = writes(&steps).into_iter().map(|(a, _)| a).collect();
        assert_eq!(addrs[0], REG_PLL0);
        assert!(!addrs.iter().skip(1).any(|a| *a == REG_PLL0));
        assert!(!addrs.contains(&REG_MUX));
        let park = writes(&steps)[0].1;
        assert_eq!(park, cpm_pll(100) & 0xf7ff_fffe);
        assert_eq!(park & 1, 0);
    }

    #[test]
    fn broadcast_lock_check_samples_chip_zero() {
        let mut prog = sample(true);
        prog.asic_id = CHIP_BROADCAST;
        let steps = pll_sequence(&prog);
        let lock = steps
            .iter()
            .position(|s| matches!(s, PllStep::ExpectLock { name: "PLL0", .. }))
            .unwrap();
        assert_eq!(steps[lock - 1], PllStep::Select(0));
        assert_eq!(steps[lock - 2], PllStep::DelayMs(5));
    }

    #[test]
    fn c_sequence_writes_registers_in_the_official_update_order() {
        let c = include_str!("../../../rtos_core/src/asic_pll.c");
        let body = c
            .split("SEQ_UPDATE_BEGIN")
            .nth(1)
            .and_then(|s| s.split("SEQ_UPDATE_END").next())
            .expect("sequence markers");
        let mut pos = 0;
        for marker in [
            "SS_CTRL_RAMP_PRE",
            "REG_PLL1_FREQ",
            "REG_PLL2_FREQ",
            "REG_PLL3_FREQ",
            "REG_ROLLTIME",
            "SS_CTRL_RAMP_MID",
            "REG_PLL_COMMIT",
            "REG_PLL0_FREQ",
            "REG_PLL_COMMIT",
            "SS_CTRL_RAMP_COMMIT",
        ] {
            let found = body[pos..]
                .find(marker)
                .unwrap_or_else(|| panic!("missing {marker}"));
            pos += found + marker.len();
        }
    }

    #[test]
    fn pvt_decode_matches_the_official_polynomial() {
        assert_eq!(decode_pvt_temp_c(0), None);
        assert_eq!(decode_pvt_temp_c(2000), None); // about −60 °C
        let code = 1857u16;
        let c = f64::from(code);
        let expanded = 2.28746e-10 * c.powi(4) - 2.84236e-6 * c.powi(3) + 1.31753e-2 * c.powi(2)
            - 26.70380 * c
            + 19725.61;
        let t = decode_pvt_temp_c(code).unwrap();
        assert!((t - expanded).abs() < 1e-6, "{t} vs {expanded}");
        assert!((60.0..=90.0).contains(&t), "{t}");
        let warm = decode_pvt_temp_c(1900).unwrap();
        assert!((30.0..=40.0).contains(&warm), "{warm}");
        assert_eq!(decode_pvt_volt_mv(0), 0);
        assert_eq!(decode_pvt_volt_mv(14), 0); // decoded ~9.7 mV
        assert_eq!(decode_pvt_volt_mv(29), 14);
        assert_eq!(decode_pvt_volt_mv(800), 239);
    }

    #[test]
    fn bounds_are_shared_and_3600_mv_is_forbidden() {
        assert!(!rail_mv_allowed(3600));
        assert!(!rail_mv_allowed(3299));
        assert!(!rail_mv_allowed(3913));
        assert!(rail_mv_allowed(3300));
        assert!(rail_mv_allowed(3599));
        assert!(rail_mv_allowed(3626));
        assert!(rail_mv_allowed(3912));
        assert_eq!(BACKSTOP_TEMP_C, 97.0);
        assert_eq!(POWER_TRIP_W, 137.0);
        assert_eq!(
            DOMAIN_MAX_MHZ,
            crate::board::nano3s_auto_tune::MAX_DOMAIN_MHZ
        );
        assert_eq!(
            BACKSTOP_TEMP_C,
            crate::board::nano3s_auto_tune::BACKSTOP_TEMP_C
        );
        assert!(
            crate::board::nano3s_auto_tune::NANO3S_ENVELOPE
                .forbidden_voltages_mv
                .contains(&(RAIL_FORBIDDEN_MV as u16))
        );
        let cfg = ChipTunerConfig::default();
        assert_eq!(cfg.min_offset_mhz, CHIP_OFFSET_MIN_MHZ);
        assert_eq!(cfg.max_offset_mhz, CHIP_OFFSET_MAX_MHZ);
        assert!(ChipClockPolicy::Clear.clears_on_board_change());
        assert!(!ChipClockPolicy::Keep.clears_on_board_change());
        assert_eq!(ChipClockPolicy::parse("keep"), Some(ChipClockPolicy::Keep));
        assert_eq!(
            ChipClockPolicy::parse("clear"),
            Some(ChipClockPolicy::Clear)
        );
        assert_eq!(ChipClockPolicy::parse("KEEP"), None);
    }

    #[test]
    fn absolute_mhz_converts_to_offsets_inside_both_windows() {
        let base = [400, 420, 440, 460];
        assert_eq!(
            resolve_chip_clocks(base, ChipClocks::Absolute([400; 12])).unwrap(),
            [0; 12]
        );
        let mut abs = [400u32; 12];
        abs[3] = 450;
        let off = resolve_chip_clocks(base, ChipClocks::Absolute(abs)).unwrap();
        assert_eq!(off[3], 50);
        abs[3] = 451;
        assert!(resolve_chip_clocks(base, ChipClocks::Absolute(abs)).is_err());
        abs[3] = 250;
        assert_eq!(
            resolve_chip_clocks(base, ChipClocks::Absolute(abs)).unwrap()[3],
            -150
        );
        abs[3] = 249;
        assert!(resolve_chip_clocks(base, ChipClocks::Absolute(abs)).is_err());
        // +50 on a 560 MHz domain would program 610 and a zero PLL word.
        let high = [520, 540, 560, 580];
        assert!(resolve_chip_clocks(high, ChipClocks::Offsets([50; 12])).is_err());
        assert!(resolve_chip_clocks(base, ChipClocks::Offsets([-151; 12])).is_err());
        assert!(resolve_chip_clocks([0, 0, 0, 0], ChipClocks::Absolute([400; 12])).is_err());
        assert_eq!(
            resolve_chip_clocks([0, 0, 0, 0], ChipClocks::Offsets([10; 12])).unwrap(),
            [10; 12]
        );
    }

    #[test]
    fn clamp_keeps_a_request_above_the_table_on_a_real_word() {
        assert_eq!(clamp_domain(610), 580);
        assert_ne!(cpm_pll(clamp_domain(610)), 0);
        assert_eq!(clamp_domain(50), 100);
        assert_ne!(cpm_pll(clamp_domain(50)), 0);
    }
}
