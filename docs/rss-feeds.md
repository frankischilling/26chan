# RSS feeds

Each board with RSS enabled serves `/{board}/index.rss`. Board and archive pages advertise the feed with an alternate link. The original board inventory enables RSS except on `/f/`, `/test/`, and the private staff board. Operators can change `content.boards.rss_enabled` with migration authority; public credentials cannot change it.

The feed is RSS 2.0 with an Atom self link and Dublin Core creators. It contains up to twenty live thread opening posts, newest post number first. Replies, deleted posts, removed threads, and archived threads are excluded. Sticky status and bump time do not change feed order. Selection and attachment metadata share a repeatable-read database snapshot.

Titles prefer the subject, then the source's first qualifying sentence, then a post-number fallback. The text projection preserves the source byte and word boundaries in `4chan-old/lib/rss.php`; synthetic reference cases are recorded in `fixtures/rss-reference.json`. Forced-anonymous boards omit creators. Dates use the reference's New York local time, including daylight-saving transitions.

Feed descriptions use the same typed comment renderer as board pages. Local quote and navigation links are absolute so they work in external readers. Attached images link only to approved media representations on the configured media origin. An unattached post has no image link. This differs from the source's unconditional image markup, which can produce a nonexistent file link. XML characters and embedded HTML are escaped independently; invalid XML scalars become replacement characters.

Successful responses use `application/rss+xml; charset=utf-8`, an ETag, and revalidation. `HEAD` returns the same headers without a body. A matching ETag returns 304. Post deletion, changed text, board visibility, and disabled feeds are checked before conditional responses; a cached feed cannot keep a private or disabled board accessible.

With the disposable database roles configured, run:

```sh
cargo test -p board-public --lib rss --locked
cargo test -p board-public --test rss --features database-tests --locked
```

The database test parses the emitted XML and checks empty feeds, ordering and limits, live visibility, summaries, creators, absolute links, escaping, daylight-saving dates, HEAD, cache invalidation, and configuration permissions. `scripts/extract-rss-reference.php` reads only the named pure source functions to regenerate the synthetic summary cases; it does not load the original application or its configuration.
