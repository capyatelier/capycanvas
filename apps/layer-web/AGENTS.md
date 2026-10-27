# Web client

Start with the [Web guide](../../docs/development/web.md).

- The pure `.test.mjs` suites listed in the [Web guide](../../docs/development/web.md#tests)
  run under `node --test`. The other `.test.mjs` files are browser journeys that
  `test.mjs` and `device.test.mjs` import.
- `icons/`, `brush-previews/` and `filter-previews/` are shared by every client.
- Tablet Chrome sessions follow the [device rules](../../docs/development/devices.md).
