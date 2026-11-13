---
title: On macOS, std reports ENOTSUP as Uncategorized, not Unsupported
kind: gotcha
trigger_case: >
  Theorem's note-move recovery treated an owner-marker xattr failure as
  "marker unavailable" only for ErrorKind::PermissionDenied,
  ReadOnlyFilesystem and Unsupported. On macOS a volume without xattrs
  (exFAT, FAT) returns ENOTSUP, errno 45, which Rust 1.97.1's std maps to
  ErrorKind::Uncategorized; EOPNOTSUPP (102) is the one that maps to
  Unsupported. On Linux both are errno 95 and map to Unsupported, so the
  gap only shows on macOS. The red test
  filesystems_without_xattrs_count_as_unavailable_markers failed with
  "45", and moves on such volumes would have refused instead of falling
  back to identity and hashes. The fix matches raw_os_error() against
  libc::ENOTSUP and libc::EOPNOTSUPP as well as the kinds.
rule_short: >
  When classifying "operation not supported" io errors on macOS, match
  raw_os_error() against libc::ENOTSUP and libc::EOPNOTSUPP; ErrorKind
  alone misses errno 45.
---
