# Diagnostics, reports and metrics

Everything in this document is kept on the computer unless the user chooses otherwise. Nothing is sent without a
confirmation in the interface. The code is in `crates/sse-app/src/diagnostics.rs`, `metrics.rs` and `metrics_report.rs`,
and in `crates/sse-doctor`.

## Files on the computer

The diagnostics folder is `<application data folder>/logs` (`diagnostics::log_directory`). In test and example builds the
application data folder is a temporary folder; `STALKER_SAVE_EDITOR_DATA` is honoured there only when it points inside
that temporary folder.

| File | Contents |
|---|---|
| `save-editor.log` | The application log: start-up line with version, OS and architecture, info, warnings and errors |
| `last-crash.txt` | A crash marker, written by the crash reporter and removed when the user dismisses it |
| `automatic-error-report.txt` | An error report saved before the user decides whether to send it |
| `metrics.jsonl` | Local performance records. It rotates to `metrics.jsonl.1` at 1 MiB |

Reports are passed through a redaction step before they are saved or sent. It removes home-folder user names, Wine user
names, SteamID64 values, Steam userdata IDs and path-like tokens.

## Doctor

`stalker-save doctor` reads a save, a crash marker or a game installation and prints a report. It does not change
any file. The subcommands are `save`, `crash`, `game` and `quests`.

## Automatic error reports

When the program finds a crash marker or an error, it prepares a report. The report contains:

- the application version, operating system and architecture;
- the error text and the stack trace, both redacted;
- the last 16 KiB of `save-editor.log`, redacted;
- no save contents and no game logs. The code never reads them for the report.

The interface shows the report to the user. The report is first saved to `automatic-error-report.txt`. It is sent only
after the user confirms. The upload is a single HTTPS POST of the report compressed as gzip, limited to 2 MiB. The
default receiver is the project's download worker (`/diagnostics`), which stores the upload under `diagnostics/`. A
build can point elsewhere through the compile-time variable `SSE_REPORT_ENDPOINT`, which must also be HTTPS. There is
no automatic retry; a rate-limited response is reported to the user.

## Performance metrics

Metrics are measured and written to `metrics.jsonl` on every run. This local file is not consent-gated and is never
sent by itself. Each session records:

- the operating system, when the session starts;
- the time to the first frame and the save discovery time;
- screen-switch and scroll timings;
- save reads and writes, each with its format, a size bucket and its duration;
- the peak resident memory, where the platform reports it;
- the window size and the scale factor.

### Sending

Sending is off by default (`send_metrics` in the settings is `false`). When the user turns it on, the Metrics dialog
shows the exact JSON body that would be sent. The body is built from counts, timings, size buckets, format names,
the operating-system category (`linux`, `windows`, `macos` or `other`) and the application version. It has no field
for file paths or save names. The send button in the dialog starts one HTTPS POST to the metrics endpoint. The request
is never retried, and the response is limited to 4 KiB. The body must pass the metrics schema check before it is sent.

The receiver (`infra/downloads-worker/worker.mjs`, route `/metrics`) accepts a body of up to 16 KiB and stores it under
`metrics/`. It keeps the body as sent.

`sse-ui-dev --metrics-report <dir>` reads downloaded `metrics/*.json` files and prints a report by operating system.
From the version of this document that includes PR #531, it also lists the application versions in each group. Percentiles are not merged exactly across uploads; the report says how each figure is combined.

## Game-folder checks

`stalker-save lint <game-folder>` runs static checks on the game scripts (`crates/sse-lint`). It reads files and prints
findings. `--json` prints them in JSON.

## What the program does not do

- It does not read saves or game logs for reports.
- It does not send metrics unless `send_metrics` is on and the user presses send for that upload.
- It does not change the save when the doctor or the lint runs.
