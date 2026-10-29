#!/bin/bash
SP="$(cd "$(dirname "$0")" && pwd)"
run() { python3 "$SP/r2run.py" "$@"; rc=$?; if [ $rc -ne 0 ]; then echo "RUNNER STOP rc=$rc at $1"; exit $rc; fi; }
for t in test_fetch_upload_after_load test_xhr_upload_after_load test_abort_fetch_upload test_fetch_network_error test_subresource_redirect test_document_redirect test_navigate_away_during_upload test_drop_webview_finishes_upload test_data_url_fetch; do
  run "request_activity-$t" cargo test --locked -p servo --test request_activity "$t" -- --exact
done
for t in test_negative_resize_to_request test_page_zoom test_pinch_zoom_update_dom_visual_viewport test_resize_webview_zero test_set_viewport_css_resize test_set_viewport_scale_only test_set_viewport_then_legacy_resize_and_scale test_viewport_meta_tag_initial_scale test_webview_resize_interactivity; do
  run "webview-$t" cargo test --locked -p servo --test webview "$t" -- --exact
done
run "servo-lib-webview_delegate" cargo test --locked -p servo --lib webview_delegate
echo ALL DONE
