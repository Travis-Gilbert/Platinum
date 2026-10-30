# Theorem live check, 2026-10-30

The Agent place, driven in the production window against a running Theorem
authority, through connecting, live answers, Stop, and the breaks a reader
meets. The check is `src/browser/theorem_live_check.rs`.

- Platinum: `90b84f8` with this round's changes (the driver, the transcript,
  and the check itself), uncommitted when the runs were made.
- Authority: `rustyred-thg-server` built from Theorem `08495b1`, the pin in
  `Cargo.toml`, with one line added to `router.rs` so it compiles (the
  `media` field `web_search_graph_job` leaves out). Local, embedded store,
  auth required, tenant `local-review`, chat model `minimax/MiniMax-M3`, on
  127.0.0.1:18082 behind a logging proxy on 18083.
- Platform: Darwin 27.0.0 arm64, rustc 1.97.1.
- Not covered: the hosted authority. Both tokens on this machine are refused
  at theoremweb.com (401).

## Running it

```
FRAME_THEOREM_LIVE_CHECK=<output> FRAME_THEOREM_LIVE_STORE=<store script> \
THEOREMWEB_AUTHORITY_URL=http://127.0.0.1:18083 \
THEOREMWEB_AUTH_TOKEN_FILE=<token file> \
script/media-env <frame built with --features snapshot>
```

The store script takes `start`, `stop` (SIGKILL), `start-refusing` (up,
refusing the check's credential) and `start-bad-provider` (up, with a model
key the provider rejects). The check sends messages and kills the
authority, so it never points at a shared one. Every step lands in
`report.json`, passed or not, and the process exits nonzero if any failed.

## Run 8

[report.json](run8/report.json). The binary predates three changes made
after it: the check waiting for the second window's settlement, the
provider step requiring Failed, and the program label on settled turns.

| Step | Result | What the card did |
| --- | --- | --- |
| down-at-launch | pass | Refused, with the authority's sentence, inside 30 s ([capture](run8/down-at-launch.png)) |
| retry-connects | pass | Ready once the authority was up |
| new-conversation | pass | An empty account got its General project |
| live-answer | pass | First part 5.1 s after sending, three part counts drawn before the run settled at 23.3 s ([streaming](run8/live-streaming.png), [answer](run8/live-answer.png)) |
| stop-mid-answer | pass | Aborted inside 30 s, the turn reads "Stopped." ([capture](run8/stop-mid-answer.png)) |
| outage-mid-answer | fail, Theorem | The card kept the run's work and Stop while the authority was down ([capture](run8/live-outage.png)); the restarted authority still names the run live and nothing settles it |
| stop-clears-orphaned-run | pass | Stop marked that run Aborted |
| answers-after-outage | fail, Theorem | The authority refuses every new turn in that conversation |
| stuck-conversation-is-explained | pass | Interrupted, with "Start a new conversation to continue", 2.3 s after sending; the draft kept ([capture](run8/stuck-conversation.png)) |
| new-conversation-after-outage | pass | |
| second-window-follows-live-run | pass | The second window adopted the live run, drew four part counts, and settled with the answer at 42.9 s ([capture](run8/second-window-follows-live-run.png)) |
| provider-failure-is-shown | false pass | The conversation was the second window's, whose run the check orphaned by killing the authority before it wrote the answer; the check now waits for that and requires Failed |
| refused-credential-signs-out | pass | Signed out, "nothing has been lost", the authority's sign-in page offered ([capture](run8/refused-credential-signs-out.png)) |
| reconnects-after-sign-in | pass | |

## Runs 9 to 13

Each run's binary carries the fixes the run before it led to:
[run 9](run9/report.json) the three changes after run 8,
[run 10](run10/report.json) the session renewal (break 6),
[run 11](run11/report.json) the check following each send until the
written turn arrives (break 8), [run 12](run12/report.json) the failed
turn kept failed (break 7), [run 13](run13/report.json) the cut-off answer
said to be incomplete (break 9).

| Step | Run 9 | Run 10 | Run 11 | Run 12 | Run 13 |
| --- | --- | --- | --- | --- | --- |
| down-at-launch | pass | pass | pass | pass | pass |
| retry-connects | pass | pass | pass | pass | pass |
| new-conversation | pass | pass | pass | pass | pass |
| live-answer | pass | fail, Theorem | pass | pass | pass |
| stop-mid-answer | pass | pass | pass | pass | pass |
| outage-mid-answer | fail, Theorem | fail, Theorem | fail, Theorem | fail, Theorem | fail, Theorem |
| stop-clears-orphaned-run | pass | pass | pass | pass | pass |
| answers-after-outage | fail, Theorem | fail, Theorem | fail, Theorem | fail, Theorem | fail, Theorem |
| stuck-conversation-is-explained | pass, 2.1 s | pass, 2.2 s | pass, 2.2 s | pass, 2.0 s | pass, 2.8 s |
| new-conversation-after-outage | pass | pass | pass | pass | pass |
| second-window-follows-live-run | pass, 64.2 s | pass, 13.2 s | pass, 74.4 s | pass, 81.5 s | pass, 50.0 s |
| provider-failure-is-shown | fail, break 6 | pass | fail, break 7 | pass | pass |
| refused-credential-signs-out | pass | pass | pass | pass | pass |
| reconnects-after-sign-in | pass | pass, break 8 | pass | pass, break 9 | pass |

- Live answers: the first part drew 4.7 to 5.2 s after sending, and the
  card showed three or four part counts before the run settled (24.5,
  26.2, 19.7 and 74.3 s in runs 9, 11, 12 and 13, as long as the model
  took; the authority wrote the turn 1.2 to 2.8 s after that,
  [capture](run12/live-answer.png)). Run 10's failed
  because Theorem's agent ran out of iterations ("max_iterations 8
  reached without final reply") after 18 parts, though the message asks
  it to use no tools; the card showed that run Failed.
- Run 9's provider step: the authority had just restarted, and the first
  message to it was refused for a CSRF session it had forgotten (break 6).
- Run 10's provider step passed (Failed 6.6 s after sending), but the
  check stopped the authority before it wrote that turn, so the last step
  found the conversation linked to an orphaned run and showed it
  Interrupted (break 8).
- Run 11's provider step: Failed at 20.7 s, then "Worked · 1 step" with no
  error once the authority wrote the turn at 23.8 s (break 7).
- Run 12's provider step: Failed through the write (settled 19.8 s,
  written 21.7 s); the header, the navigator row and the turn all say so
  ([capture](run12/provider-failure-is-shown.png)). Signed in again, the
  same window reread the conversation as the authority wrote it and showed
  the turn as "Worked · 1 step" with nothing under it
  ([capture](run12/reconnects-after-sign-in.png), break 9).
- Run 13's provider step read Failed through the write again (settled
  16.8 s, written 17.6 s), and after signing in again the reread turn says
  "This answer is incomplete." ([capture](run13/reconnects-after-sign-in.png)).
  The rivers answer above it is missing from that capture, though the
  conversation holds it: the answer is 6103 characters, GPUI Kit's `TextView`
  parses Markdown over 4 KiB off the UI thread
  (`MAX_SYNC_FULL_REPLACE_BYTES` in gpui-base 0.8.0), and the check
  captures the reread's first frame. Rebuilding the stored conversation
  through Theorem's `runtime_from_snapshot` and `project_thread` gives its
  progress, program and text parts; run 12's 2606-character answer showed
  after its reread.
- The outage steps fail the same way in every run: the restarted authority
  never closes the run it lost, Stop clears it, and the conversation then
  refuses every turn, which the card explains in about 2 s.

## Breaks found and fixed in Platinum

Runs 1 to 7 found the first five, runs 9 to 12 the rest. Each fix has a
test; the third's, the seventh's and the ninth's were also run without
the fix, and fail there.

1. A message sent between a run's last event and the authority writing its
   answer into the conversation named a revision the authority had moved
   past, and was refused as a conflict. The composer now reads as sending
   until the settled conversation arrives
   (`the_next_message_waits_until_the_authority_settles_the_last`).
2. After a refused turn the window held a revision or a CSRF session the
   authority had moved past, so every later send was refused too. It now
   reads the session and the conversation again and keeps the draft
   (`a_refused_turn_leaves_the_conversation_at_the_authority_revision`,
   `a_send_after_the_authority_restarts_uses_its_new_session`).
3. A conversation whose run the authority lost was followed again on every
   open and send. Run 7 spent about 33 s resuming before showing "Theorem
   could not recover that conversation's run" with the run's internal id.
   The driver now remembers a run it saw end that the authority still names
   as live, and shows that conversation as Interrupted with the notice
   above; run 8 shows it in 2.3 s (`a_run_the_authority_lost_is_not_followed_again`).
4. A settled turn kept saying it was working: a failed run leaves its tool
   calls pending, and the authority leaves a program part streaming after
   the run ends ("Writing a program" in the stop capture). Once a message
   settles, an unfinished part reads as unfinished, and a progress label is
   the group's summary on its own
   (`a_progress_label_is_the_summary_and_a_settled_turn_stops_working`,
   `a_settled_turn_is_not_still_writing_its_program`).
5. The check itself: App Nap coalesced its tenth-of-a-second timers into
   minutes while its window sat behind others, and the authority's agent
   reached for host tools on a simple question until it ran out of
   iterations (run 7's live answer failed after 23 parts), so messages that
   need an answer ask it not to.
6. The first message after the authority restarted was refused with "The
   chat CSRF token is invalid or expired": the authority keeps one CSRF
   session per account and forgets them all when it restarts (run 9, whose
   provider step ended with "Say hello." refused and kept in the composer,
   [capture](run9/provider-failure-is-shown.png)). A send or a new
   conversation refused that way now reads the session again and, when the
   authority hands out a different token and the conversation is at the
   same revision, sends once more with it; refused a second time, it is
   shown as refused
   (`a_send_after_the_authority_restarts_uses_its_new_session`,
   `a_send_is_renewed_once_and_then_refused_as_it_is`,
   `a_new_conversation_after_the_authority_restarts_uses_its_new_session`).
   Run 10's provider step, sent right after a restart, went through.
7. A failed turn read as finished once the authority wrote it. The
   authority writes a failed run's answer as its first progress part,
   still running, and names no run, so the card first showed Failed and
   then, when the written conversation arrived, "Worked · 1 step" with no
   error (run 11, [capture](run11/provider-failure-is-shown.png)). When the
   run's stream ended it failed, stopped or interrupted, that ending is
   kept over a conversation that names no run
   (`a_failed_run_stays_failed_once_the_authority_writes_its_turn`).
8. The check itself: it stopped the authority as soon as a run's stream
   ended, before the authority wrote the turn, which orphaned that run and
   left the last step's conversation Interrupted (run 10's
   reconnects-after-sign-in). Each send now follows until the written turn
   arrives, and the live and provider steps require it.
9. A failed turn read as an answer that finished with nothing to say once
   any window read the conversation again (run 12's
   reconnects-after-sign-in): what the authority wrote holds one progress
   part, still running, and no status, so the transcript showed "Worked ·
   1 step" alone. An answer holding only activity, some of it never
   finished, now says "This answer is incomplete."; the reason is not in
   what was written, so it cannot say "error"
   (`an_answer_cut_off_in_its_activity_reads_as_incomplete`).

## Still Theorem's

- An authority that restarts mid-run, or is killed between a run's stream
  ending and its settlement, leaves the conversation linked to the run.
  Nothing settles it, cancelling records the cancel without unlinking it,
  and every later turn there is refused as `chat_revision_conflict` whose
  message names the same revision as expected and current (seen again in
  runs 9 to 12: "expected 12, current 12").
- A failed run's turn is written without its failure: the answer holds only
  the first progress part, status running, and the conversation names no
  run and no error, so `reported_run_state` reads it as Idle. Theorem's own
  GPUI host (`gpui/host/src/chat_driver.rs`) uses the same reading and
  loses the failure too. Platinum keeps the ending its stream saw (break 7);
  a window that reads the conversation later can only say the answer is
  incomplete (break 9), not that it failed or why.
- The agent's own notes to the operator ("Note to Theorem: ...", about the
  harness prompt and the room context) are published in the answer (run 9,
  [capture](run9/second-window-follows-live-run.png)).
- The "Claims JSON:" trailer is published as part of the answer (stop and
  stuck captures).
- A program part stays `streaming` after its run settles.
- The agent reaches for host tools, terminal commands included, on
  questions that need none, and can exhaust its iterations doing so.
- `rustyred-thg-server` at `08495b1` does not compile without the
  `router.rs` line above.
- The native `ClientError` carries no code, so a refusal is recognized by
  reading the authority again rather than by its kind.

## Still Platinum's

- A message the authority refused shows both in the transcript and, kept,
  in the composer (stuck capture).
- "This answer is incomplete." sits under the user's message when the
  authority never wrote an answer (stuck capture).
