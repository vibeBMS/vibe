#pragma once
/* setjmp / longjmp are lowered by LLVM (-mllvm -wasm-enable-sjlj) to Wasm
   exceptions; libc.c has the runtime they call. */
typedef unsigned long long jmp_buf[4];
int setjmp(jmp_buf);
_Noreturn void longjmp(jmp_buf, int);
