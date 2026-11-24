MOCK data, written by hand from the flight-predict CONTRACT.md before the
GroundStation26 backend served these routes. The backend's real fixtures are
in `tests/fixtures/backend-contract/flight-plans/` and `.../los-prediction/`;
these mocks remain for states those do not cover (a live `predicting` and a
`resolved` LOS state, a plan with burn curves). Nothing here is a real flight.
