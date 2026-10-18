# Janitor request file effects

The source file-policy model is checked against 1,728 executed PHP cases from
`admin.php:adminBanReq` and `reports/ReportQueue.php`. The extractor verifies both
complete source-file hashes before retaining the bounded branches. The oracle
intercepts every file and SQL call and uses synthetic posts, a fixed synthetic
salt and owned paths. It never reads source keys or copies real uploads.

## Source behavior

Request creation attempts both original and thumbnail copies only for an
`everything` template without an illegal-category report. The source ignores
copy errors and attempts these paths even when the post has no extension. In
the other branch it puts the raw digest into captured post JSON. A failed copy
does not switch branches or expose that digest.

Amending a request with an extension to a `json_only` template removes both
captured files and restores the raw digest from the serialized post. The source
checks an undefined `has_approved` variable on this path; its false value causes
the thumbnail removal. The oracle records that warning instead of inventing a
successful approval state.

A non-warning review inserts an image rule only when the captured raw digest is
nonempty and the selected policy is `image` or `rejectimage`. `image` requests an
indefinite automatic ban; `rejectimage` rejects the upload. The caller's warning
branch controls this decision, separately from the resulting stored duration.

Clearing a request always deletes its original file. Denial also deletes its
thumbnail; approved cleanup leaves the thumbnail. The JSON-only amendment can
have removed that thumbnail before approved cleanup runs.

## Native storage requirements

These are effect decisions, not file custody or authorization. Native storage
must require approved media, bind illegal-report checks to the actual locked
request target, keep copied material private, and commit deletion/capture/review
without partial effects. Browser fields cannot supply a digest or retained path.
Unknown retained digest evidence must remain distinct from known absence.

The original creation query uses the submitted post number for its illegal-report
check. Native authorization must use the captured target instead. A source
negative control removes that exclusion and confirms that the isolated fixture
observes both copy calls; the unchanged branch performs neither.

The model and source oracle pass locally. Private custody, authenticated media
reads and the transactional image-rule writer still need implementation and
integration qualification. The model alone does not complete issue #223.
