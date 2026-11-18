# Contributing

Pull requests are welcome. The full guide is
[CONTRIBUTING.md](https://github.com/RustedMC/Rusted/blob/main/CONTRIBUTING.md).

## How ported code is written

- Same rules, numbers and random-number order as PNX. Where Java needed objects and virtual
  calls, plain Rust data does the same job.
- Credit the source with a `// from PNX <Class>` comment.
- Leave a `// TODO:` where something isn't ported properly yet.
- Add a test for the behaviour you ported.

## Before opening a PR

```bash
cargo test
cargo clippy --all-targets
```

Both should pass with no warnings.

## Editing these docs

The docs are plain Markdown in the [`docs/`](https://github.com/RustedMC/Rusted/tree/main/docs)
folder, built with [mdBook](https://rust-lang.github.io/mdBook/). Every page has an edit button
in the top right. To preview locally:

```bash
cargo install mdbook
mdbook serve docs --open
```

## Talk to us

[Discord](https://discord.gg/HBegxHxREc) · [Issues](https://github.com/RustedMC/Rusted/issues)
