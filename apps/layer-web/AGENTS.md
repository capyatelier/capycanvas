# Web client

Start with the [Web guide](../../docs/development/web.md).

- Only `run`, `package`, `frame`, `pointer`, `workspace-client`, `canvas-bar`,
  `notice`, `zoom-readout`, `export-controls`, `size-dialog` and `text-input`
  `.test.mjs` files run under
  `node --test`. The other `.test.mjs` files are browser journeys that
  `test.mjs` and `device.test.mjs` import.
- `icons/` and `brush-previews/` are shared by every client.
- Tablet Chrome sessions follow the [device rules](../../docs/development/devices.md).
