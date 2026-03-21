# ATProto Localnet Lab

`deploy/localnet/` defines a second development path for `divine-sky`: a fuller ATProto lab for provisioning and protocol experiments that need more of the network surface than the fast bridge-focused stack in `config/docker-compose.yml`.

## Why This Exists

The default compose stack in `config/docker-compose.yml` stays the day-to-day option for bridge work because it is faster to boot, easier to reset, and already covers the bridge runtime, local PDS, storage, and relay needs for most development.

The localnet lab is additive. Use it when you need an isolated environment with:

- PLC
- PDS
- Jetstream
- DNS
- handle admin

## Contract

- Local handles use the `divine.test` suffix.
- Bridge and handle-gateway consume localnet-specific environment override files instead of growing separate code paths.
- Production and staging runtime contracts stay unchanged.

Each slice under `deploy/localnet/` documents its own compose file, placeholders, and operator steps.
