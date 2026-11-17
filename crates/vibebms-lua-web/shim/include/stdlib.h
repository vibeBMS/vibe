#pragma once
#include <stddef.h>
void *malloc(size_t); void *calloc(size_t, size_t); void *realloc(void *, size_t); void free(void *);
_Noreturn void abort(void); _Noreturn void exit(int); char *getenv(const char *); int system(const char *);
double strtod(const char *, char **); int abs(int);
#define EXIT_SUCCESS 0
#define EXIT_FAILURE 1
