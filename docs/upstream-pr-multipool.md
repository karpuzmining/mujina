# Draft upstream note: multi-pool time-slicing

Not opened. This is a draft for IxTechCrypto/mujina when the hardware checklist has been run. Do not open a pull request from this file.

## Summary

The Nano 3S has a single hash chain, so more than one pool cannot be given chips. This change gives each pool a percent of hash *time*.

- Up to four pools, each with a url, user, password and weight. Weight 0 is a failover-only backup.
- Config is stored at `/data/userconfig/pools.conf` with fsync and rename. An existing `pool.conf` is pool 1 at 100%. `MUJINA_POOL_*` is still the fallback when no file is present. No pool url, wallet, worker or password is baked into the build.
- A 60 second slot is given to one pool. Over time the slot counts match the weights. A down pool's time is redistributed across the live positive-weight pools. If none of those are up, the first pool that is up, in list order, takes the chain.
- The scheduler switches the chain's job at most once per slot. A share is submitted to the pool whose job produced it. The previous pool's in-flight shares are not dropped on a switch.
- `allocated_hashrate()` returns that pool's share of the chain, so each stratum client suggests difficulty and reports hashrate for its own share. A pool with a zero share is told 1 H/s so its session stays up without claiming the chain.
- `GET` and `PUT /api/v0/pools` apply in-process. `GET` and `PATCH /api/v0/pool` remain pool 1. `GET` returns `password_set` and never the password.
- The dashboard Pool item links to `/pool`, which edits the set and shows live status per pool.

## Why it is in-process

Restarting `mujina-minerd` leaves the Core 1 IPC handle stale and hashrate stuck at 0. Adding or removing a stratum task has to happen inside the running process.

## Not in this change

No RT-Smart changes. No per-chip PLL or SmartSpeed changes. Those stay on their own branch.
