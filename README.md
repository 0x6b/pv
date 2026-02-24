# pv

View the markdown file in `~/.claude/plans/` with the given command. By default, it opens the most recent file.

## Usage

```
$ pv --help
Usage: pv [OPTIONS] [PATH]

Arguments:
  [PATH]  File path to open directly

Options:
  -c, --command <COMMAND>  Command to open the file (receives absolute path as argument) [default: "gh mdp"]
  -i, --interactive        Interactive mode: list files with fzf-style selection
  -h, --help               Print help
  -V, --version            Print version
```

See [0x6b/gh-mdp: A GitHub Flavored Markdown live preview server](https://github.com/0x6b/gh-mdp) for what `gh mdp` is.

## LICENSE

MIT. See [LICENSE](LICENSE) for details.
