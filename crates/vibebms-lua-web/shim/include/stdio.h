#pragma once
#include <stddef.h>
#include <stdarg.h>
typedef struct FILE FILE;
extern FILE *stdin, *stdout, *stderr;
#define EOF (-1)
#define BUFSIZ 1024
#define L_tmpnam 32
#define SEEK_SET 0
#define SEEK_CUR 1
#define SEEK_END 2
#define _IOFBF 0
#define _IOLBF 1
#define _IONBF 2
FILE *fopen(const char *, const char *); FILE *freopen(const char *, const char *, FILE *);
int fclose(FILE *); size_t fread(void *, size_t, size_t, FILE *); size_t fwrite(const void *, size_t, size_t, FILE *);
int fflush(FILE *); int fprintf(FILE *, const char *, ...); int fputs(const char *, FILE *); int fputc(int, FILE *);
char *fgets(char *, int, FILE *); int getc(FILE *); int ungetc(int, FILE *); int ferror(FILE *);
int feof(FILE *); void clearerr(FILE *); int fseek(FILE *, long, int); long ftell(FILE *);
int setvbuf(FILE *, char *, int, size_t); FILE *tmpfile(void); char *tmpnam(char *);
int remove(const char *); int rename(const char *, const char *);
int snprintf(char *, size_t, const char *, ...); int sprintf(char *, const char *, ...);
int vsnprintf(char *, size_t, const char *, va_list);
