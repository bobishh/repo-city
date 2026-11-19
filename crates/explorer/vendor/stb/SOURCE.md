# stb_textedit

`stb_textedit.h` is copied unchanged from `nothings/stb` revision
`9600522de53973336fa033a289858a4354cbb04c`, version 1.16.

Source: https://github.com/nothings/stb/blob/9600522de53973336fa033a289858a4354cbb04c/stb_textedit.h

The project uses the MIT option in the header. See `licenses/stb-textedit-MIT.txt`.

`adapter.c` is local integration code. It supplies a bounded Unicode scalar buffer,
monospace wrapped row layout, key constants, and a libc-free memory move for native
and `wasm32-unknown-unknown` builds. It exposes an opaque state through a small C ABI.
A selected-text replacement records one upstream replacement undo operation instead
of separate deletion/insertion records. Rust checks byte limits before mutation.
The upstream header is not modified.

Each editor has 256 undo records and storage for 262144 Unicode scalars in history.
History is bounded and older operations can be evicted. Text rendering, input events,
clipboard, IME commits, highlighting, draft persistence, and explicit worktree apply
are provided by Repo City. Caret movement operates on Unicode scalars, not grapheme
clusters; this widget does not implement full bidirectional editing.

A C11 compiler with WebAssembly support is required for browser builds. Apple Clang
omits that target; install Homebrew LLVM or set `CC_wasm32_unknown_unknown` explicitly.
The adapter uses no C system headers or runtime allocation.
