# ASN lookup fixture

`crates/domain/tests/fixtures/GeoLite2-ASN-Test.mmdb` is the producer's public
synthetic database, pinned to MaxMind-DB revision
`276926d23b4109ca5452709bfb5931c338afb34c`.

- [Binary fixture](https://github.com/maxmind/MaxMind-DB/blob/276926d23b4109ca5452709bfb5931c338afb34c/test-data/GeoLite2-ASN-Test.mmdb)
- [Producer source data](https://github.com/maxmind/MaxMind-DB/blob/276926d23b4109ca5452709bfb5931c338afb34c/source-data/GeoLite2-ASN-Test.json)
- [Apache license](https://github.com/maxmind/MaxMind-DB/blob/276926d23b4109ca5452709bfb5931c338afb34c/LICENSE-APACHE), already retained as `crates/domain/tests/fixtures/MAXMIND-LICENSE-APACHE.txt`

Size: 12,653 bytes. SHA-256:
`75901b98ed6e58d3bd41af9985044b747a7ec0be1369f930c24f5e044427181a`.

The fixture is test data, not a production ASN feed. It verifies local lookup
and address canonicalization without a network request or a client-supplied
ASN. A deployment must supply its own supported database. Configure `ASN_DATABASE` for public posting and `STAFF_ASN_DATABASE` for staff.
See [network policy](network-policy.md) for provider failure semantics and the
intentional IPv6 admission difference.
