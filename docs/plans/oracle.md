# Frame nightly acceptance oracle

Frame's answer to "is the pinned browser still the browser we accepted?" It
runs nightly (and on demand) on a hosted macOS runner, and it is adapted from
Theorem's `theorem-browser nightly acceptance oracle` workflow
(`.github/workflows/browser-oracle.yml` in Travis-Gilbert/Theorem), which is
itself downstream of the Zen/Firefox gap research in Theorem's
`docs/plans/theorem-browser-zen/`. The engine work this oracle guards is
recorded in [youtube.md](youtube.md).

| Piece | Path |
|---|---|
| Workflow | `.github/workflows/browser-oracle.yml` |
| Rows and pins (machine) | `docs/plans/oracle-spec.json` |
| Pin unity checker | `script/check-pins.py` |
| Runner and validator | `script/oracle-run.py` |

## What the nightly proves

| Row | Check |
|---|---|
| P1 | Pin unity: bootstrap scripts, `patches/README.md`, the workflow, the spec, `Cargo.toml` and the `deps/` checkouts name one Theorem (`8c5d22bd…`), one Servo (`efdf7370…`) and one GStreamer (`1b2a95a1…`) |
| P2 | The archival `patches/servo-mse.patch` covers exactly the fork's `ebc89e68..efdf7370` diff and reproduces the pinned tree byte-for-byte |
| B1 | The patched GStreamer MSE runtime loads: `gst-inspect-1.0 mse` sees `msesrc` through `script/media-env` |
| B2 | `cargo build --locked` of `frame` and every example against the pinned engine, substrate and runtime |
| B3 | `script/setup-media` resolves every GStreamer plugin the pinned Servo expects beside the built binaries |
| T1 | Native MSE backend regression: buffered-range removal and refill for VP8 and Vorbis against the pinned WPT fixtures |
| T2 | The chrome UI integration tests in headless windows (`src/browser/tests.rs`) |

Each row runs as one logged subprocess; the receipt records command, exit
code, duration and log hash. `oracle-run.py validate` rules the receipt
against the spec and writes `readiness.md`; the workflow uploads everything
as a 90-day artifact.

## What the nightly cannot prove, and never claims

A hosted runner has no window server session, no GPU and no human. Frame's
rendering probes (`examples/media_check.rs`) create a real GPU context, so
every playback, pixel and page acceptance stays a **live row**: the runner
records it `not-run`, and `validate` refuses any CI-mode receipt that reports
one demonstrated. Live rows run on a development Mac:

| Row | Acceptance | Local command |
|---|---|---|
| L1 | Ordinary MP4/WebM playback (muted) | fixture server + `media_check -- http://127.0.0.1:8766/media/` |
| L2 | MSE browser fixture: chunked appends, EOS, seek/resume | fixture server + `media_check -- --mse http://127.0.0.1:8766/media-source/` |
| L3 | Timer GC regression (256 callbacks, checksum 32640) | `FRAME_MEDIA_SCRIPT=tests/media/timer-gc.js … media_check` |
| L4 | Inline SVG paint pixels, before/after restyle (needs Pillow) | `tests/rendering/check_svg_paint.py <target>/debug/examples/media_check` |
| L5 | Public YouTube watch: identity, no ad, pause, seek, 1.5x | `FRAME_MEDIA_SCRIPT=tests/media/youtube.js … media_check -- --youtube …aqz-KE-bpKQ` |
| L6 | `media-source` WPT conformance suite | open implementation work (youtube.md step 4) |
| L7 | Audible output, full visible-window playback | human |
| L8 | Chrome parity measurements | human, same machine/network |

`script/oracle-run.py run --live --output DIR` executes the runnable ones
(starting the fixture server itself) into the same receipt shape, so local
acceptance and nightly CI speak one vocabulary. The authoritative live
evidence remains [youtube.md](youtube.md) and `docs/plans/evidence/`.

## One-time setup: the Theorem read token

Frame's RustyWeb crates are path dependencies on the **private**
`Travis-Gilbert/Theorem` repository, so the workflow checks the pinned
substrate out with a token. Create a fine-grained personal access token
(repository access: `Travis-Gilbert/Theorem` only; permissions: Contents,
read-only) and store it as an Actions secret of this repository:

```bash
gh secret set THEOREM_READONLY_TOKEN --repo Travis-Gilbert/Platinum
```

The workflow refuses as its first step, with this document as the pointer,
when the secret is absent. The token belongs in the secret store only —
never in a file in this repository.

## Schedule and dispatch

- `schedule: 17 5 * * *` UTC. GitHub fires scheduled workflows only on the
  default branch, so the nightly becomes active when this workflow is merged
  to `main`.
- `workflow_dispatch` works from any pushed branch — including this one, to
  trial the workflow before merging. The `preflight_only` input verifies pins
  and patch identity (minutes) without the multi-hour build.

## Reproducing locally

```bash
script/bootstrap                 # pins the Theorem substrate under deps/
script/bootstrap-media           # pins the Servo fork, patches and builds GStreamer MSE
python3 script/check-pins.py --require-deps
python3 script/oracle-run.py run --check --output /tmp/frame-oracle   # preflight
python3 script/oracle-run.py run --output /tmp/frame-oracle           # CI rows (includes the full build)
python3 script/oracle-run.py run --live --output /tmp/frame-oracle    # + runnable live rows
python3 script/oracle-run.py validate --receipt /tmp/frame-oracle/receipt.json \
  --report /tmp/frame-oracle/readiness.md
```

`run --only ID,ID` executes a subset; its receipt records the rest as
not-selected, so `validate` rules it BLOCKED by design: a partial run is not
acceptance. Budget: a Servo build needs about 25 GB of target disk; the job
cap is 360 minutes and per-row timeouts live in the spec.

## Changing a pin

Every named source must move together, and `check-pins.py` (row P1) refuses
until they do: `script/bootstrap`, `script/bootstrap-media`,
`patches/README.md`, `docs/plans/oracle-spec.json`,
`.github/workflows/browser-oracle.yml` (the substrate `ref:`), and the
`deps/` checkouts themselves. Refusal codes are documented in each script's
docstring; they follow the naming of Theorem's `check-servo-pin-unity.py`.

## Rules

1. A row is `demonstrated` only when its evidence class meets its oracle
   class. Compilation demonstrates a build row and nothing else.
2. A CI-mode receipt never claims a live row; `validate` treats the claim as
   a refusal (`oracle_live_row_claimed`), not as a pass.
3. No stand-in is silently promoted: L6–L8 stay `not-run` with their reason
   until a real runner path or a real measurement exists.
4. The receipt is a record of one invocation; pins, fixtures and expectations
   live in the spec, and `oracle_spec_drift` blocks a receipt produced
   against different spec text.
