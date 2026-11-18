# Contributing to Rusted

Thanks for helping! A few ground rules keep the project fast and faithful.

## Before you start
- Check [PORTING.md](PORTING.md) and [ROADMAP.md](ROADMAP.md) to see what's being worked on.
- For anything big, open an issue first so we can agree on the approach.

## Porting from PowerNukkitX
- Port behaviour faithfully: same rules, numbers and random-number order as PNX.
- Put `// Ported from PNX <Class>` at the top of ported code.
- If something can't be ported properly yet, leave a `// TODO:` saying what's missing.
- Don't copy Java's structure where plain Rust data does the same job faster.

## Pull requests
- `cargo build --release` must pass with no new warnings in the files you touched.
- Add a test for new logic (`cargo test --release`). Some tests need `data/structures.nbt`,
  which isn't in the repository; ask in your PR if you need it.
- Keep changes focused: one feature or fix per pull request.
- Never commit `*.pem` files, worlds, player data or `server.properties`.

## License
By contributing you agree your work is released under the [LGPL-3.0](LICENSE), like the rest
of Rusted.
