# Nano 3S per-chip tuning — hardware checklist

For Ryan, on the Avalon Nano 3S (12× Canaan A3197S), before this is merged
or sent upstream. Do not flash Canaan's own image onto a unit you want
to keep on Mujina. Keep a UART / kburn recovery path. A wrong PLL order
can hang or damage chips; this build follows Canaan's
`a3197s_set_pll_seq`, but the new `rtos_core` has to actually be the one
running.

This repo's CI (`just ci`) runs the Rust tests. It does not build a
flashable `.kdimg` or a new `rtos_core.elf`. The PLL sequence lives in
`rtos_core` and takes effect only after that ELF is rebuilt and
installed (the usual `rtos_core/tools/build_wsl.sh` path, then the
miner bundle that embeds `rtos_core/build/rtos_core.elf`).

## Before you start

1. Record a baseline with the current v5.1.0 firmware: AutoTune
   `efficient`, then `max_hash`, both with per-chip tuning off. Note
   TH/s, watts, fail %, and the hottest chip.
2. Confirm `tuning.conf` has no `chip_clock_policy` line. Missing means
   `clear`, which is the old behavior.

## Clocks

3. After installing this build's `rtos_core`, set a known board clock
   (for example 400/420/440/460 MHz) and confirm the chain hashes.
   PLL words for 174, 210, 396, 480, and 520 MHz must match
   `get_cpm_pll()` on the official table. 100–580 MHz ceiling lookups
   did not change versus v5.1.0.
4. `PATCH /api/v0/boards/nano3s-0/chip-clocks` with one offset inside
   −150..+50. The chain should keep hashing, the tuning table should
   show that chip's new frequency, and a reboot should restore it from
   `/data/userconfig/tuning.conf`.
5. Reject: an offset of +51 or −151, an absolute MHz that would push any
   domain outside 100–580, and a board voltage of 3600 mV. None of
   those may be applied.
6. While ChipTuner phase is `trial`, the same PATCH returns 409 and the
   table inputs stay disabled.

## Policy

7. Leave the policy on **Clear**. Switch AutoTune profile. All twelve
   offsets go to 0 and per-chip tuning turns off unless you explicitly
   enable it again. This matches v5.1.0.
8. Set **Keep**, apply a manual offset, switch profile and change the
   board clock. The offset remains and is added to the new base.
   ChipTuner, if it was mid-trial, returns to the last committed offset
   rather than leaving the trial in place.

## Telemetry and the tuner

9. The tuning table shows a distinct temperature and PVT millivolt
   reading per chip (on-die, hundreds of mV, not the ~3.5 V rail).
   Err % is the speed-log fail rate and GH/s is the speed-log pass
   hashrate (`pass * 0.256 / 1000`).
10. Run `efficient` and `max_hash` with `per_chip: true`. Offsets stay
    inside −150..+50. Board TH/s does not fall more than the tuner's
    total guard (about 2%). Neighbors on the series stack stay stable
    when one chip moves.
11. A chip at or above 95 °C is stepped down. A chip at or above 92 °C
    is not stepped up. Crossing 97 °C during a step-up reverts that
    step. The 97 °C backstop and the 137 W trip still drop the board
    clock / force the fan, including while the fan is manual.
12. A weak chip (high speed-log error) is clocked down, not up.

## Power loss

13. Pull power mid-ramp if you can do it safely. On the next boot the
    chain should come up at the saved board clock, not sit in a
    half-programmed PLL. The sequence either finishes or the boot path
    parks the chain again; do not resume a chip that was left between
    the PLL1 write and the PLL0 mux restore.

## Rollback

Take this path before the test build is installed, and again if the
chain hangs or you want v5.1.0 back. The new `mujina-minerd` copies its
embedded `rtos_core.elf` onto `/mntapp/release/rtt/rtos_core.elf` at
startup and reboots, so replacing only the Linux binary is not enough
unless that binary is the v5.1.0 miner (it embeds the v5.1.0 ELF).

**Back to Mujina v5.1.0, keeping `/data/userconfig` (pool, Wi-Fi, tuning).**
From a computer on the same network, upload the v5.1.0 OTA bundle.
The miner applies it and reboots. On the next boot the v5.1.0 miner
writes the v5.1.0 `rtos_core.elf` back.

```text
curl -X POST --data-binary @nano-mujina-v5.1.0-bundle.bin \
  -H "Content-Type: application/octet-stream" \
  http://<MINER_IP>/api/v0/firmware/bundle
```

That file is attached to
[IxTechCrypto/mujina v5.1.0](https://github.com/IxTechCrypto/mujina/releases/tag/v5.1.0)
(`nano-mujina-v5.1.0-bundle.bin`, 21,387,784 bytes). If the dashboard
still answers, the same upload is **Firmware → bundle** in the web UI.

**Back to Mujina v5.1.0 from USB, including an unbootable unit.**
Hold the recovery pinhole, apply 28 V, and flash
`nano-mujina-v5.1.0-release.kdimg` (129,232,128 bytes, same release)
with Torii or the K230 burning tool, as in
`docs/nano3s-install-guide.md`. This rewrites the app and data
partitions from the v5.1.0 image. Pool and Wi-Fi settings that lived
only in `/data` are replaced by whatever is in that image.

**Back to stock Canaan firmware.** The Mujina OTA endpoint cannot do
this. Use the same USB recovery burn with a Canaan image. That removes
Mujina. An unbootable board uses that same recovery button path.
