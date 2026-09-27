# prof.rs

`phifmpeg prof <binary> <samples> [--top N]`: the host half of the card
profiler (`card/phix/src/prof.rs`, `PHIX_PROF`).

Reads the binary's symbol table with the `object` crate, keeps symbols in
executable sections, and attributes each sample to the symbol with the
greatest address at or below it. A symbol with no size (NASM labels, which
the SIMD functions of both projects are) runs to the next symbol. C++
names are demangled (`cpp_demangle`). Prints self percentage, cumulative
percentage and sample count, most expensive first, plus how many samples
fell outside every symbol (those would point at a stripped or different
binary).

The binaries are static and not position-independent, so run-time
addresses equal file addresses. Use the unstripped `ffmpeg_g` or the x265
CLI from the build tree, the same build that ran.
