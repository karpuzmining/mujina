# Nano 3S multi-pool hardware checklist

Test build `v5.1.0-multipool-test.2`. It is v5.1.0 plus in-process multi-pool, and the dashboard menus open their own pages. The RT-Smart core embedded in the miner is the v5.1.0 release `rtos_core.elf` (`2e88ff5f2b58acbe3761a1ab083ad3c46b89b296f7491f05ca1e0970717299d1`). Do not use this bundle to test the per-chip PLL changes; that is a different image.

SHA-256 of `nano-mujina-v5.1.0-multipool-test.2-bundle.bin` is recorded on the `v5.1.0-multipool-test.2` release.

## Install

1. Boot the unit into the normal dashboard, not ROM boot mode. Boot mode only accepts a `.kdimg`.
2. Open **Firmware** and upload `nano-mujina-v5.1.0-multipool-test.2-bundle.bin`.
3. Wait until the dashboard is back. Confirm the build string on **Info**.

## Menus

Each item opens its own page. Old `/dashboard#...` links redirect there. There is no in-page sheet.

- **Power** `/power` — pause, reboot, restart, and the power-profile ceiling (50–140 W). The 112 W and 130 W profiles and the overclock warning are unchanged.
- **Tuning** `/tuning` — auto tuner, live clock and voltage (3600 mV is still rejected), plus the older PD / hashrate / fan-thermostat sections.
- **Fan** and **Fan Curve** `/fan` — mode, duty 0–100, and the outlet-temperature curve (2–8 points).
- **Pool** `/pool` — the multi-pool editor. The old single-pool form is not repeated.
- **Display** `/display` — the LCD theme.

## Pool page

1. The main menu **Pool** item opens `/pool`, not the old single-pool sheet.
2. Add a second pool. Set weights with the sliders. They must show 100 before Save is accepted.
3. Save. The page says the change was applied with no reboot. Mining must keep running; hashrate must not fall to 0 and stay there (that is the stale Core 1 IPC failure mode of a process restart).
4. Each pool row shows connected or down, difficulty, accepted, rejected, and effective hashrate for 10 min and 1 h.
5. Set one weight to 0. That pool stays in the list as a backup and should not take hash time while the others are up.
6. Remove a pool and save again. The removed pool drops off the live list without a reboot.
7. `GET /api/v0/pool` still returns pool 1. `GET /api/v0/pools` returns every pool and never includes a `password` field.

## Failover

1. Point one pool at a closed port and give it a non-zero weight.
2. Within about a minute its row shows down, and the live pools' effective hashrate rises to absorb its share.
3. Shares found just before a switch still show up as accepted or rejected on the pool that issued the job, not on the pool that received the next slot.

## Rollback

Upload the v5.1.0 OTA bundle from the `v5.1.0` release, or USB-flash `nano-mujina-v5.1.0-release.kdimg` if the dashboard will not boot. `pools.conf` is only read by this build. v5.1.0 reads `pool.conf`, which this build keeps in sync with pool 1.
