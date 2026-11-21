# Nano 3S input power

Canaan's smart-PSU path is `apps/miner/hal/platform/linux/psu_dcdc_pd.c`
in [canaan_sdk_for_linux](https://github.com/Canaan-Creative/canaan_sdk_for_linux)
(MIT). The board table is `apps/miner/hal/board/board_nano3s.h`.

The input meter is an INA226 at I2C address `0x40` on `/dev/i2c-2`
(`I2C_PD_DEV`). Canaan does not use `i2cget` and does not use a kernel
hwmon driver. Before trusting current they write configuration `0x4727`
and calibration `0x0AEC`. Bus voltage (register `0x02`) is 1.25 mV per
count. Current (register `0x04`) is 0.1831 mA per count. That calibration
constant is a 10 mΩ shunt: `0.00512 / (0.0001831 * 0x0AEC) = 0.010 Ω`.
A negative current word (sign bit set) is treated as no current. Input
power is bus voltage times that current. Supply is treated as absent
below 8 V (`POWER_VALID_VOLT_MIN`).

The PD controller is a separate chip at `0x42` on the same bus. Canaan's
`pd_pout_limit` writes register `0x02` = `0x1B` once, before GPIO34
enables the hashboard rail, and then reads source capabilities at
`0x6A`–`0x6F`. This miner does only that one write. It does not read
`0x01` and it does not shell out to `i2cget`. `MUJINA_NANO3S_NO_I2C2=1` (or `MUJINA_NANO3S_I2C2=off`)
skips the write, the INA226, and bus recovery.

The core rail is not a second current sensor. The DC-DC setpoint is
register `0xF8` on address `0x48` of `/dev/i2c-3`, 26 mV steps around
3600 mV. Canaan's `hal_psu_reading` names (`vout`, `iout`, `pout`) are
the PD input meter, plus `voutcmd` for that setpoint. There is no core
current or core watt reading in the SDK.

Our earlier reader shelled out to `i2cget` and never wrote the
calibration register. On the test.3 image that call fails, so
`GET /api/v0/boards/nano3s-0/psu` omitted `bus_v` and reported only
`effective_ceiling_w: 134`. That 134 is `137 − 3` (`POWER_TARGET_SAFETY_W`
minus `PD_CEILING_MARGIN_W`) used when no bus voltage is available. The
power-target loop then compared the trip with 0 W, so the trip never
fired. A missing meter now holds the voltage and frequency instead of
looking like zero load. The 137 W trip still runs when the meter
answers, including while PSU bypass is on. Bypass only drops the
USB-C PD wattage tier.

When `/dev/i2c-2` does not answer, the dashboard shows an estimate and
prefixes it with `~`. The estimate is `V² × f` per die plus a leakage
term, summed over 12 chips and divided by an input efficiency of 0.90.
Die voltage is the on-die PVT reading when it is between 20 and 1500 mV,
otherwise the commanded rail divided by 12. The two fit points are the
stock LOW baseline (50.6 W at 210 MHz / 3392 mV) and the 122 W point at
460 MHz / 3860 mV. Active-core fraction stays 1 because the status word
does not publish a core-enable mask. `power_cal.conf` (`factor=`, 0.5 to
2.0, fsync and rename) scales both the meter and the estimate so a wall
meter can correct them. Check the meter against a wall meter on hardware
before treating the watts as calibrated.
