# Command runtimes

Exact arguments, environments and outputs are retained in [commands.json](commands.json). Times are measured wall seconds and include shared-lock waiting. Original sandbox attempts remain as history; their old target path was not used by this continuation. Read-only exploration and editing calls were not individually instrumented.

| Check | Process exit | Probe accepted | Seconds | Output |
| --- | ---: | --- | ---: | --- |
| disk | 0 |  | 0.003233 | [disk.log](disk.log) |
| touch-profile | 0 |  | 0.003067 | [touch-profile.log](touch-profile.log) |
| frame-tests | 101 |  | 0.583794 | [frame-tests.log](frame-tests.log) |
| canvas-build | 101 |  | 0.543868 | [canvas-build.log](canvas-build.log) |
| format-new-rust | 0 |  | 0.049556 | [format-new-rust.log](format-new-rust.log) |
| parse-page | 0 |  | 0.046651 | [record](commands.json) |
| parse-controls | 0 |  | 0.042771 | [record](commands.json) |
| parse-browser | 0 |  | 0.240266 | [record](commands.json) |
| parse-main | 0 |  | 0.037059 | [record](commands.json) |
| parse-actions | 0 |  | 0.034595 | [record](commands.json) |
| probe-js-syntax | 0 |  | 0.027637 | [probe-js-syntax.log](probe-js-syntax.log) |
| signin-js-syntax | 0 |  | 0.028271 | [signin-js-syntax.log](signin-js-syntax.log) |
| runner-python-syntax | 0 |  | 0.022518 | [runner-python-syntax.log](runner-python-syntax.log) |
| diff-whitespace | 0 |  | 0.029825 | [diff-whitespace.log](diff-whitespace.log) |
| lock-diff | 0 |  | 0.010151 | [lock-diff.log](lock-diff.log) |
| protected-path-diff | 0 |  | 0.007105 | [protected-path-diff.log](protected-path-diff.log) |
| dependency-update-repeat-for-timing | 0 |  | 1.846505 | [dependency-update.log](dependency-update.log) |
| wrapper-syntax | 0 |  | 0.018524 | [wrapper-syntax-continuation.log](wrapper-syntax-continuation.log) |
| rust-format | 0 |  | 0.141914 | [rust-format-continuation.log](rust-format-continuation.log) |
| fixture-probe-syntax | 0 |  | 0.043279 | [fixture-probe-syntax-continuation.log](fixture-probe-syntax-continuation.log) |
| diff-whitespace-continuation | 0 |  | 0.049638 | [diff-whitespace-continuation-continuation.log](diff-whitespace-continuation-continuation.log) |
| signin-index.html-syntax | 0 |  | 0.065740 | [signin-index.html-syntax-continuation.log](signin-index.html-syntax-continuation.log) |
| benchmark.html-syntax | 0 |  | 0.062383 | [benchmark.html-syntax-continuation.log](benchmark.html-syntax-continuation.log) |
| disk | 0 |  | 1.021982 | [disk-1793115804245986000.log](disk-1793115804245986000.log) |
| canvas-build | 0 |  | 1200.624678 | [canvas-build-1793115805269479000.log](canvas-build-1793115805269479000.log) |
| disk | 0 |  | 0.019644 | [disk-1793117068038680000.log](disk-1793117068038680000.log) |
| zoom | 1 |  | 4.989035 | [zoom-1793117068064513000.log](zoom-1793117068064513000.log) |
| signin | 1 |  | 0.367352 | [signin-1793117073054185000.log](signin-1793117073054185000.log) |
| frame-tests-full-access | 1 |  | 0.170483 | [frame-tests-full-access.log](frame-tests-full-access.log) |
| frame-tests-wrapper-fixed | 101 |  | 1499.930624 | [frame-tests-wrapper-fixed.log](frame-tests-wrapper-fixed.log) |
| frame-tests-retry | 0 |  | 451.345789 | [frame-tests-retry.log](frame-tests-retry.log) |
| disk | 0 |  | 0.005568 | [disk-1793117646419019000.log](disk-1793117646419019000.log) |
| zoom | 0 |  | 6.082786 | [zoom-1793117646432178000.log](zoom-1793117646432178000.log) |
| signin | -11 |  | 1.696275 | [signin-1793117652515496000.log](signin-1793117652515496000.log) |
| disk | 0 |  | 0.004258 | [disk-1793117747151497000.log](disk-1793117747151497000.log) |
| canvas-build | 0 |  | 495.138426 | [canvas-build-1793117747156222000.log](canvas-build-1793117747156222000.log) |
| disk | 0 |  | 0.003550 | [disk-1793118561233404000.log](disk-1793118561233404000.log) |
| zoom | 0 |  | 9.178998 | [zoom-1793118561242702000.log](zoom-1793118561242702000.log) |
| signin | -15 |  | 128.584959 | [signin-1793118570422308000.log](signin-1793118570422308000.log) |
| disk | 0 |  | 0.004109 | [disk-1793118796951293000.log](disk-1793118796951293000.log) |
| canvas-build | 101 |  | 11.657704 | [canvas-build-1793118796955840000.log](canvas-build-1793118796955840000.log) |
| disk | 0 |  | 0.033790 | [disk-1793118858710076000.log](disk-1793118858710076000.log) |
| canvas-build | 0 |  | 25.265005 | [canvas-build-1793118858744391000.log](canvas-build-1793118858744391000.log) |
| disk | 0 |  | 0.003774 | [disk-1793118903407438000.log](disk-1793118903407438000.log) |
| frame-tests | 0 |  | 14.915675 | [frame-tests-1793118903411621000.log](frame-tests-1793118903411621000.log) |
| canvas-build | 0 |  | 9.067835 | [canvas-build-1793118918327928000.log](canvas-build-1793118918327928000.log) |
| zoom | 0 |  | 3.723469 | [zoom-1793118927403782000.log](zoom-1793118927403782000.log) |
| signin | 0 |  | 1.518438 | [signin-1793118931127961000.log](signin-1793118931127961000.log) |
| measure | 0 |  | 1.463400 | [measure-1793118932647010000.log](measure-1793118932647010000.log) |
| disk | 0 |  | 0.004401 | [disk-1793119009255969000.log](disk-1793119009255969000.log) |
| canvas-build | 101 |  | 7.162390 | [canvas-build-1793119009260872000.log](canvas-build-1793119009260872000.log) |
| disk | 0 |  | 0.004359 | [disk-1793119088758031000.log](disk-1793119088758031000.log) |
| canvas-build | 0 |  | 9.429677 | [canvas-build-1793119088762845000.log](canvas-build-1793119088762845000.log) |
| disk | 0 |  | 0.003721 | [disk-1793119129776548000.log](disk-1793119129776548000.log) |
| zoom | 0 | False | 3.780718 | [zoom-1793119129785664000.log](zoom-1793119129785664000.log) |
| signin | 0 | False | 1.480136 | [signin-1793119133567534000.log](signin-1793119133567534000.log) |
| disk | 0 |  | 0.003851 | [disk-1793119152785314000.log](disk-1793119152785314000.log) |
| measure | 0 | False | 1.480289 | [measure-1793119152793490000.log](measure-1793119152793490000.log) |
| setup-media | 0 |  | 474.914795 | [setup-media.log](setup-media.log) |
| final-rust-format | 0 |  | 0.115055 | [final-rust-format.log](final-rust-format.log) |
| final-python-syntax | 0 |  | 0.039296 | [final-python-syntax.log](final-python-syntax.log) |
| final-probe-js | 0 |  | 0.036140 | [final-probe-js.log](final-probe-js.log) |
| final-diff-check | 0 |  | 0.135365 | [final-diff-check.log](final-diff-check.log) |
| final-protected-paths | 0 |  | 0.028788 | [final-protected-paths.log](final-protected-paths.log) |
| final-disk | 0 |  | 0.007472 | [final-disk.log](final-disk.log) |
| final-inline-signin-index | 0 |  | 0.044706 | [final-inline-signin-index.log](final-inline-signin-index.log) |
| final-inline-canvas-benchmark | 0 |  | 0.047577 | [final-inline-canvas-benchmark.log](final-inline-canvas-benchmark.log) |
| final-inline-canvas-zoom-probe | 0 |  | 0.045947 | [final-inline-canvas-zoom-probe.log](final-inline-canvas-zoom-probe.log) |

Unlocked-resume commands and runtimes are preserved separately in [resume/command-runtimes.md](resume/command-runtimes.md).

A2 diagnosis and final-pass commands: [first diagnosis](a2-repair/command-runtimes.md), [focus diagnosis](a2-focus-diagnosis/command-runtimes.md), [final pass](a2-final/command-runtimes.md).

Offline preparation: [commands](offline-ready/commands.json), [static checks and dry run](offline-ready/verification.json). No native windows opened in that continuation.
