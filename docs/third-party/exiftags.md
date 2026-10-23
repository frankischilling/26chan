# exiftags runtime

The isolated still-image guest uses exiftags 1.01 with Debian's 1.01-8 patch set. The supplied application invokes `exiftags` but does not record its deployed version. This is a reproducible compatibility baseline, not proof of that deployment's binary identity. The Debian GPS precision correction is included.

Source archives:

- [Original source](https://deb.debian.org/debian/pool/main/e/exiftags/exiftags_1.01.orig.tar.gz), SHA-256 `d95744de5f609f1562045f1c2aae610e8f694a4c9042897a51a22f0f0d7591a4`.
- [Debian patch set](https://deb.debian.org/debian/pool/main/e/exiftags/exiftags_1.01-8.debian.tar.xz), SHA-256 `ab45f09f55c43caae9ede45d137f1a9236c72690cefc0fe37ae863c8a71745f3`.

The builder checks both hashes, applies the recorded patch series, enables compiler/linker hardening and retains Debian's copyright file with the resulting executable. Only the guest invokes this parser. The public, staff and publication services accept bounded text, not native EXIF structures.

The upstream license includes an advertising acknowledgement and redistribution conditions. This product includes software developed by Eric M. Johnston. The [complete notice](exiftags-copyright.txt) is packaged as `/usr/share/doc/26chan-exiftags/copyright` in the still initramfs. This paragraph does not replace it.
