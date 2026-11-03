---
title: codex-bridge shows a dead job as running when exit_code could not be written
kind: gotcha
trigger_case: >
  codex-bridge's run-job.sh records completion by writing exit_code after
  Codex exits, and wait-job.sh and codex_status treat its absence as
  "running". When the internal disk filled, Codex died and that write
  failed, so job cx-mvppmirt-z6gs showed as running for over three hours
  with no live process. codex_cancel replied "had already stopped" but
  wrote no exit_code, and codex_reply kept refusing with "still running".
  Confirming that the pid in the job directory was gone and writing
  exit_code by hand let codex_reply continue the same thread.
rule_short: >
  If a Codex job has been "running" with no new events, check
  ps -p $(cat <job>/pid); if the process is gone, read stderr.log, write
  <job>/exit_code, then continue it with codex_reply.
---
