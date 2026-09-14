# Windows visual failure evidence

[Issue #142](https://github.com/frankischilling/26chan/issues/142) tracks two
unexplained failures: a post-number click did not open Quick Reply in job
103940095407, and an in-place catalog screenshot differed from the server-mode
screenshot in job 103940049323 even though image sources and dimensions matched.
Both runs had zero downloadable artifacts. The same Quick Reply case passed
20 local Windows repetitions. That result does not establish a cause or fix.
The separate spoiler-reveal investigation remains open in #139.

The Quick Reply visual suite and in-place catalog comparison now retain bounded
failure state for up to four synthetic pages: script response paths/statuses,
eight script errors, eight recent click/change events, form/dialog presence,
finite catalog switches and a few element dimensions. These observers do not
change events, requests, responses, waits, retries or production code. The state
collector does not read form values, cookies, storage, URL queries or fragments.
A browser test checks that sentinel values in those locations stay out of its
serialized output. Script error text is limited to 512 characters and is only
collected from these synthetic visual fixtures; this helper is not for real
posting sessions or staff authentication tests.

On a catalog comparison failure, the test attaches the two PNG buffers it
actually compared, before retaining the exact byte-equality assertion. It does
not take replacement screenshots or accept new baselines. The Windows failure
step uploads only `test-results/**/*.png`, including those attachments and the
existing automatic failure screenshots. It does not upload traces, videos,
HTML reports, network logs, credentials or environment files. Files expire after
three days. Artifact collection does not convert a failed test into success.

The official upload-artifact v7.0.1 tag resolves to
`043fb46d1a93c77aae656e7c1c64a875d1fc6a0a`; its manifest uses Node 24. The
[pinned manifest](https://github.com/actions/upload-artifact/blob/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/action.yml)
defines the path, retention and hidden-file controls used here. The workflow
retains read-only repository permissions and does not run `pull_request_target`.
Local syntax/workflow checks and fixture execution do not verify a hosted
artifact upload. Inspect the next actual failure artifact before attributing
these failures to initialization, layout, screenshot encoding or other causes.

Local checks passed all 119 theme cases, including the diagnostic privacy test.
A separate ignored failure probe exercised the actual failure hook and reporter.
It first exposed that body-only attachments did not leave PNG files for upload.
The helper now writes the original compared buffer to an output path before
attaching it. The repeated probe exited 1 as intended and retained the original
PNG, its reporter attachment copy and the automatic failure screenshot. This
checks local capture, not hosted upload or a fix for the original failures.
