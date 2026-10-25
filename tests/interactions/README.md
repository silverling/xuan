# Size-dialog interaction checks

These development-only scenarios drive the real Canvas Size and Image Size windows
through egui pointer events and inspect the real document and undo history. They cover
unchanged Apply, Cancel, and a changed Apply followed by Undo/Redo. The size setter
updates the dialog model; these are not tests of numeric keyboard entry or GPU output.

Build the opt-in adapter with `cargo build --locked --features interaction-probe`.
Normal builds do not expose the `--interaction-probe` argument or compile the adapter.

The CI workflows use [interaction-check](https://github.com/PerchAnton/interaction-check)
at commit `67a280537977bcc3dc14f5be0637c8431bb310be` (experimental alpha.3).
This independent tool is maintained by the author of this contribution. It is a
development/CI dependency, and introduces no runtime network or Python dependency in Xuan.
Updating the pin should be reviewed like any other executable test dependency.

With Python 3.10+ and a checkout of that runner:

```sh
python scripts/check-interactions.py /path/to/interaction_check.py --report /tmp/xuan-interactions.json
```

The adapter reads JSONL from stdin and writes only JSONL to stdout. Each request contains
`protocol: 1`, an integer `id`, and an `event`. Supported event types are `new`,
`rename`, `undo`, `redo`, `open_size`, `set_size`, `apply`, and `cancel`.
`new` creates a small synthetic document. It never opens a user's files.
The `open_size` kind is `canvas` or `image`; `set_size` takes width, height and resolution.
Each response echoes protocol/id and reports the observed state. Expected states belong
only to the runner, not the adapter. Errors terminate the adapter with diagnostics.

The ordinary Rust regression tests run without this feature or Python. The adapter and
these scenarios also run on Linux and Windows in CI.

