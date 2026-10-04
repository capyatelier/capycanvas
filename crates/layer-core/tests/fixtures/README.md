# Package fixtures

The JSON records in `capy/` exercise the [portable package grammar](../../../../docs/reference/capy-package.md):

- `empty.json` and `paint-and-mask.json` describe editable artwork.
- `ancillary.json` retains unknown ancillary fields.
- `retained-future.json` retains unsupported unplaced content.
- `reused-group.json` exercises indirect sharing outside the editable subset.

Package codec tests assemble these records into archives. Independent sample,
resource, choice-value and metadata assertions live in the codec and effect-record tests.
