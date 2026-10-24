# Third-party notices

DoD Studio's own code is MIT (`LICENSE`); `dem-patch/` is LGPL-3.0. Almost every library it builds with is MIT or Apache-2.0 and asks for nothing beyond its licence file. This page lists the ones whose terms ask for more, or that aren't open source.

## UnRAR (RARLAB)

**Used for:** reading the `.dem` files inside `.rar` archives in Demo Auditor › Duplicates, with "Show demos backed up in archives" ticked (#929). Read only: entries are decompressed in UnRAR's test mode and never written to disk, and no archive is ever created or changed.

**How it gets in:** the `unrar_sys` crate (MIT or Apache-2.0) vendors RARLAB's UnRAR C++ source, with its own small patches, and builds it into a static library; `hl-demo-auditor/src/archive/rar.rs` calls its C API. Only on Windows, and only with `hl-demo-auditor`'s `rar` feature (on by default). `cargo build --no-default-features -p hl-demo-auditor` leaves it out, and `.rar` files are then not looked inside.

**Licence:** UnRAR is freeware, not open source. Its licence lets any software use it to read RAR archives and ship it, and forbids using it to make a RAR-compatible archiver or to re-create the RAR compression algorithm; DoD Studio does neither. It asks that this paragraph be quoted in full:

> UnRAR source code may be used in any software to handle RAR archives without limitations free of charge, but cannot be used to develop RAR (WinRAR) compatible archiver and to re-create RAR compression algorithm, which is proprietary. Distribution of modified UnRAR source code in separate form or as a part of other software is permitted, provided that full text of this paragraph, starting from "UnRAR source code" words, is included in license, or in documentation if license is not available, and in source code comments of resulting package.

The rest of its licence (copyright to Alexander Roshal; the UnRAR utility may be freely distributed, including inside other software packages; no warranty) is in `vendor/unrar/license.txt` of the `unrar_sys` crate. Its acknowledgements credit Intel's Slicing-by-8 CRC-32 code, under a BSD licence:

> Copyright (c) 2004-2006 Intel Corporation. All Rights Reserved.
>
> Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met: Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution.
>
> THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

**For anyone changing it:** the test archive `hl-demo-auditor/src/archive/testdata/pack.rar` is a stored (uncompressed) RAR 4 file written by hand from RARLAB's published format notes, not with RAR, and the repo holds no RAR writer. Keep it that way: nothing here may compress into RAR.

## sevenz-rust2

**Used for:** the `.dem` files inside `.7z` archives, same feature. Pure Rust, Apache-2.0, no NOTICE file; listed here only because it sits beside UnRAR.
