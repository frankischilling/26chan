# Synthetic country database

`GeoIP2-Country-Test.mmdb` comes from the public MaxMind-DB test-data repository
at commit `276926d23b4109ca5452709bfb5931c338afb34c`:

- [Binary fixture](https://github.com/maxmind/MaxMind-DB/blob/276926d23b4109ca5452709bfb5931c338afb34c/test-data/GeoIP2-Country-Test.mmdb)
- [Producer source data](https://github.com/maxmind/MaxMind-DB/blob/276926d23b4109ca5452709bfb5931c338afb34c/source-data/GeoIP2-Country-Test.json)
- [Apache license](https://github.com/maxmind/MaxMind-DB/blob/276926d23b4109ca5452709bfb5931c338afb34c/LICENSE-APACHE), retained as `MAXMIND-LICENSE-APACHE.txt`

Collected September 30, 2026. The 19,492-byte binary has SHA-256
`b37601903448683d241af52893c8cbf0fed461e0cdebe0bfaca01891fdeb6db9`.
The retained license has SHA-256
`aac73b3148f6d1d7111dbca32099f68d26c644c6813ae1e4f05f6579aa2663fe`.

The producer data maps `81.2.69.142` to GB / United Kingdom, with a different
registered-country value, and `2001:218::` to JP / Japan. Tests also use mapped
IPv4 and a documentation-range address absent from the country data. These
addresses are lookup inputs only; tests never contact them. This synthetic
database is for offline qualification, not production geolocation.
