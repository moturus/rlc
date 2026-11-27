# RLC

This is a clone of [Tokei](https://github.com/XAMPPRocky/tokei/) that uses only
the Rust standard library and builds on Motor OS. It follows the Tokei 15.0.0
command-line interface.

`rlc` shows the number of files, lines, lines of code, comments, and blank
lines grouped by language.

## Example
```console
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
 Language            Files        Lines         Code     Comments       Blanks
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
 BASH                    4           49           30           10            9
 JSON                    1         1332         1332            0            0
 Shell                   1           49           38            1           10
 TOML                    2           77           64            4            9
───────────────────────────────────────────────────────────────────────────────
 Markdown                5         1355            0         1074          281
 |- JSON                 1           41           41            0            0
 |- Rust                 2           53           42            6            5
 |- Shell                1           22           18            0            4
 (Total)                           1471          101         1080          290
───────────────────────────────────────────────────────────────────────────────
 Rust                   19         3416         2840          116          460
 |- Markdown            12          351            5          295           51
 (Total)                           3767         2845          411          511
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
 Total                  32         6745         4410         1506          829
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

## Options

```text
rlc [OPTIONS] [--] [input]...
```

Pass one or more files or directories to scan. With no paths, `rlc` scans the
current directory. Use `--` before a path that starts with a dash.

| Option | Behavior |
| --- | --- |
| `-c`, `--columns <columns>` | Set the statistics table width, with a minimum of 81 columns. |
| `-e`, `--exclude <pattern>` | Exclude files and directories matching a Git ignore pattern. Repeat for multiple patterns. |
| `-f`, `--files` | Show individual file statistics. |
| `-i`, `--input <file_input>` | Validate a previous JSON report from a file, `stdin`, or a literal JSON argument. Printed totals come from the current scan. |
| `--hidden` | Include hidden files and directories. |
| `-l`, `--languages` | List supported languages and their file extensions. |
| `--no-ignore` | Skip all ignore files. |
| `--no-ignore-parent` | Skip ignore files in directories above the input directory. |
| `--no-ignore-dot` | Skip `.ignore` and `.rlcignore` files. |
| `--no-ignore-vcs` | Skip Git ignore and exclude rules. |
| `-o`, `--output json` | Write a single JSON report. |
| `--streaming <format>` | Write one record per file in `simple` or `json` format. |
| `-s`, `--sort <sort>` | Sort languages by `files`, `lines`, `blanks`, `code`, or `comments`. |
| `-r`, `--rsort <rsort>` | Reverse the language sort using the same keys as `--sort`. |
| `-t`, `--types <types>` | Count only the listed languages. Use comma-separated names, for example `--types Rust,Markdown`. |
| `-C`, `--compact` | Hide embedded-language rows. The grand total still includes their counts. |
| `-n`, `--num-format <style>` | Format table numbers with `plain`, `commas`, `dots`, or `underscores`. |
| `-v`, `--verbose` | Warn about unknown extensions and interpreters. Use `-vv` for ignore-file diagnostics and `-vvv` for line traces. |
| `-h`, `--help` | Print the command-line help. |
| `-V`, `--version` | Print the version and supported serialization format. |

`--columns` and `--num-format` apply to the statistics table. The `--files`
and `--languages` displays use `COLUMNS` for automatic width when it is set in
a terminal. JSON reports and streams emit per-file records in a stable order.

## Supported Languages

Run `rlc --languages` for the current list of languages and extensions.
Definitions live in [src/language_data.rs](./src/language_data.rs).

## Copyright and License

The original Tokei (C) Copyright 2015 by XAMPPRocky and contributors

RLC is distributed under either the MIT License or the Apache License, Version 2.0.
