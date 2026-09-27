# stack.rs

Finds Intel-Phi-3120A (the stack) in the family's order:

1. `PHI_STACK_ROOT` (an error if set to something that is not a stack);
2. `phi` on PATH, which the stack's `phi install-cli` makes a symlink to
   `<stack>/scripts/phi.sh`;
3. a checkout next to this one, as `Intel-Phi-3120A` or `Intel Phi 3120A`;
4. the same two names in `$HOME`.

A directory counts as the stack when it has `toolchain/env.sh` and
`scripts/phi.sh`.

`toolchain_env` sources the stack's `toolchain/env.sh` in a `bash` and
imports the result with `env -0`. That script is the stack's own interface
for card builds: it puts `knc-cc` and the patched LLVM first on PATH and
creates `phi_root`, a space-free alias of the stack (the checkout path has a
space, and configure scripts expand paths unquoted). Re-implementing it
here would copy the stack's knowledge; sourcing it keeps one owner.

`isa_audit` returns the stack's `phi-isa-audit` from its `host/target`
(debug first, then release).
