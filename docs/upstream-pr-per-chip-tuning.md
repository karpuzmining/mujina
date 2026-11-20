# Draft pull request for IxTechCrypto/mujina

Do not open this until Ryan has hardware-tested it. Target
`IxTechCrypto/mujina` `main`. This fork's PR is the hardware-test
vehicle and is not that upstream PR.

## Title

nano3s: adopt Canaan's A3197S PLL sequence and finish per-chip tuning

## Body

Ports the MIT-licensed A3197S PLL programming order, CPM table, and PVT
decode from Canaan Creative `canaan_sdk_for_linux` v1.1.2-alpha
(`07fe73f`) into the Nano 3S path, and finishes the manual per-chip
surface that v5.1.0 already started. No third tuner. No per-chip
voltage: the twelve dies share one series rail.

### PLL

`a3197s_set_pll_seq` replaces the previous PRE → PLL1–3 → MID → PLL0 →
COMMIT → ROLLTIME order. The official order is PRE, PLL0 lock check,
PLL1–3, ROLLTIME, switch onto PLL1, PLL1 lock check, mux to PLL1,
program PLL0, mux back, enable SmartSpeed. Each step selects the chip.
Canaan's docs warn that a wrong order can hang or damage the chips.

The SS_CTRL words were already the official defaults (`0x1429`,
`0x142A`, `0x1C2A`). What differed was the order, the missing mux
writes around PLL0, ROLLTIME being last instead of with PLL1–3, and
the missing lock checks.

The CPM table's live 99–600 MHz rows already matched the official
349-row table. This replaces the file with that table (adding the
official 15–98 MHz rows and removing an unscanned extrapolated tail).
Ceiling lookup from 100 to 580 MHz is unchanged. `correct_freq()` is
ported and unit-tested but not used on the programming path: it snaps
some saved integers down by 1 MHz (`230 → 229`, `100 → 99`) and would
change clocks on upgrade. Every domain is clamped to 100–580 MHz
before the lookup so a request cannot write a zero PLL word.

PVT temperature is Canaan's degree-4 polynomial (invalid outside
−50..200 °C, reported as −273). PVT voltage is the official linear
decode, zero below 14 mV.

MIT copyright and the permission notice are in `NOTICE` and on the
ported files. The combined work stays GPL-3.0-or-later. Nothing from
Avalon_mm, Canaan's debug logins, or the OTA public key is included.

### Tuning

`PATCH /api/v0/boards/{name}/chip-clocks` still takes `offset_mhz`. It
also takes absolute `freq_mhz` (converted to an offset from PLL0) and
`policy`:

- `clear` (default, and what an existing `tuning.conf` does): a profile
  change, AutoTune off, or a manual board VF write still zeroes the
  twelve offsets and turns ChipTuner off.
- `keep`: those events leave the offsets in place. An in-flight
  ChipTuner trial is still dropped back to the last committed offset.

ChipTuner's default window is now the same −150..+50 MHz as the API.
It consumes each die's PVT temperature: ≥95 °C steps that die down,
≥92 °C blocks a step up, and 97 °C reverts a step-up immediately. The
post-lock two-layer design is unchanged (board rail and clock first,
then one chip at a time). 3600 mV stays forbidden. Power and
temperature trips stay 137 W and 97 °C.

The tuning page grows a per-chip table: temperature, on-die millivolts,
frequency, speed-log error rate, and speed-log GH/s. Those last two are
the official `0x42c` / `0x430` counters reduced the same way rtos
already reduces them (`fail/(pass+fail)`, `pass*0.256/1000`). Raw
counter words are not added to the IPC status struct, so an older
`rtos_core.elf` still parses.

`/tuning?demo=1` draws the table from labeled simulated rows.

### Tests

Unit tests cover the 349-row table (including a byte match against
`pll_cpm_table.c`), the update and park sequence order, a scan of the
C update-path writes, PVT decode, the shared bounds, absolute-MHz
conversion, and the keep/clear policy. `cargo test` is the CI suite.
`nano3s_chain_sim` still drives the ChipTuner tests.

Hardware still has to confirm the sequence on a real Nano 3S. See
`docs/nano3s-per-chip-hardware-test.md`.
