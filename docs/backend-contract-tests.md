# Backend contract tests

GroundStation26 records real backend replies and websocket frames (produced by
its actual handlers and types) in `backend/tests/contract-fixtures/`. This
repository keeps a copy in `tests/fixtures/backend-contract/` and decodes every
file with the UI's own serde types:

- `src/backend_contract.rs`: fixture loader and the list of covered fixtures
  (fails when a fixture has no decoding test).
- `src/telemetry_dashboard/backend_contract_tests.rs`: station mode review,
  confirmation and switch (`station_mode_contract`), `StationModeChanged`,
  `/api/station_mode`, board recovery status, action policy with `network_note`,
  alerts and `history_cleared_through_ms`, radio modem, `radio_tx_budget`.
- Inline `backend_contract_tests` modules in `src/auth.rs` (session, login
  challenge), `errors_tab.rs` (history clear reply) and `recovery_status.rs`
  (`/api/rrls/status`), where the types are private.

The tests check the fields the UI acts on, not only that decoding succeeds.
Example: a station mode challenge without `"version": 2` fails with
"Unsupported mode challenge", as it did in the app.

## Run

```bash
cargo test backend_contract -- --test-threads=1
```

Against a backend checkout instead of the copy:

```bash
GS_CONTRACT_FIXTURES_DIR=../GroundStation26/backend/tests/contract-fixtures \
  cargo test backend_contract -- --test-threads=1
```

## Keep the copy current

```bash
python3 scripts/check_backend_contract_fixtures.py            # compare, exit 1 on difference
python3 scripts/check_backend_contract_fixtures.py --update   # copy the backend's files
```

The backend directory is `--backend`, else `$GS_BACKEND_CONTRACT_FIXTURES`,
else `../GroundStation26/backend/tests/contract-fixtures` next to this checkout.

When the backend changes a shape on purpose it regenerates its fixtures
(`GS_UPDATE_CONTRACT_FIXTURES=1 cargo test -p groundstation_backend contract_fixtures`,
see GroundStation26 `docs/backend/contract-fixtures.md`). Refresh the copy here,
run the tests, and update the UI types or the assertions in the same change.
A new fixture also needs an entry in `COVERED` and a decoding test.
