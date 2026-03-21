# PLC Slice

This slice runs a dedicated PLC directory for the ATProto localnet lab.

## Intended Hostname

Expose the TLS proxy on `plc.<tailnet>.ts.net`, where `<tailnet>` is your Tailscale tailnet name.

## Services

- `postgres` stores PLC data
- `tailscale` provides the tailnet network namespace
- `app` runs the PLC server
- `nginx` terminates TLS and proxies to the PLC app

## Notes

- Replace the placeholder Tailscale auth key before bringing the slice up.
- Mount Tailscale-issued certificates into the `plc-certs` volume or adjust the nginx paths to match your cert workflow.
