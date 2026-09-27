# prof.rs

The card profiler. `PHIX_PROF=<file>[,<microseconds>]` in the environment
of an FFmpeg or x265 process (with phix linked) arms `ITIMER_PROF`.

- The timer counts CPU time of the whole process, all threads, so busy
  threads are sampled in proportion to their work; the kernel sends the
  `SIGPROF` to the process and delivers it on a thread that is running.
- The handler reads the interrupted `RIP` from the signal frame's
  `ucontext_t` (`uc_mcontext.gregs[REG_RIP]`), takes a slot with one atomic
  increment (`lock xadd`, which Knights Corner has), and stores it. Nothing
  else: no allocation, no locks, no libc calls.
- The buffer is 4 Mi samples in an anonymous `MAP_NORESERVE` mapping, only
  touched as it fills; samples past it are counted and dropped.
- At exit (`atexit`) the timer is stopped and the samples are written as
  little-endian `u64`s. A process that dies by signal writes nothing.
- Default interval 10 ms of process CPU time; `,<us>` changes it (100 us
  to 1 s).

`SA_RESTART` keeps interrupted system calls restarting. The same approach
(`setitimer` + `SIGPROF` + `REG_RIP`, symbols resolved on the host) was
used on this card for FastLanes in the stack's history; on a static,
non-PIE binary an address needs no load offset.

Read the result with `phifmpeg prof <binary> <file>` on the host, against
the unstripped binary that ran (`ffmpeg_g`, the x265 CLI).
