/* The C library Lua needs on wasm32-unknown-unknown, which has none.
   Allocation, math, strtod, float formatting, the clock and the console
   come from Rust (src/lib.rs); there are no files (skin scripts get `io`
   and `require` from vibeBMS, in Lua). */
#include <stddef.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <locale.h>
#include <time.h>

int errno;

/* ---- string ---- */
size_t strlen(const char *s) { const char *p = s; while (*p) p++; return (size_t)(p - s); }
int strcmp(const char *a, const char *b) {
  while (*a && *a == *b) { a++; b++; }
  return (unsigned char)*a - (unsigned char)*b;
}
int strncmp(const char *a, const char *b, size_t n) {
  for (; n; n--, a++, b++)
    if (*a != *b || !*a) return (unsigned char)*a - (unsigned char)*b;
  return 0;
}
int strcoll(const char *a, const char *b) { return strcmp(a, b); }
char *strchr(const char *s, int c) {
  for (;; s++) { if (*s == (char)c) return (char *)s; if (!*s) return NULL; }
}
void *memchr(const void *s, int c, size_t n) {
  const unsigned char *p = s;
  for (; n; n--, p++) if (*p == (unsigned char)c) return (void *)p;
  return NULL;
}
char *strstr(const char *h, const char *n) {
  size_t l = strlen(n);
  if (!l) return (char *)h;
  for (; *h; h++) if (*h == *n && !memcmp(h, n, l)) return (char *)h;
  return NULL;
}
char *strpbrk(const char *s, const char *a) {
  for (; *s; s++) if (strchr(a, *s)) return (char *)s;
  return NULL;
}
size_t strspn(const char *s, const char *a) {
  size_t n = 0;
  while (s[n] && strchr(a, s[n])) n++;
  return n;
}
char *strcpy(char *d, const char *s) { char *r = d; while ((*d++ = *s++)); return r; }
char *strncpy(char *d, const char *s, size_t n) {
  size_t i = 0;
  for (; i < n && s[i]; i++) d[i] = s[i];
  for (; i < n; i++) d[i] = 0;
  return d;
}
char *strerror(int e) { return e == 2 ? "No such file or directory" : "error"; }

/* ---- ctype ("C" locale) ---- */
int isdigit(int c) { return c >= '0' && c <= '9'; }
int islower(int c) { return c >= 'a' && c <= 'z'; }
int isupper(int c) { return c >= 'A' && c <= 'Z'; }
int isalpha(int c) { return islower(c) || isupper(c); }
int isalnum(int c) { return isalpha(c) || isdigit(c); }
int isspace(int c) { return c == ' ' || (c >= '\t' && c <= '\r'); }
int iscntrl(int c) { return (c >= 0 && c < 32) || c == 127; }
int isprint(int c) { return c >= 32 && c < 127; }
int isgraph(int c) { return c > 32 && c < 127; }
int ispunct(int c) { return isgraph(c) && !isalnum(c); }
int isxdigit(int c) { return isdigit(c) || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F'); }
int tolower(int c) { return isupper(c) ? c + 32 : c; }
int toupper(int c) { return islower(c) ? c - 32 : c; }

/* ---- locale ---- */
static struct lconv lc = { "." };
struct lconv *localeconv(void) { return &lc; }
char *setlocale(int c, const char *l) { (void)c; return (!l || !*l || !strcmp(l, "C")) ? "C" : NULL; }

/* ---- stdlib ---- */
_Noreturn void abort(void) { __builtin_trap(); }
_Noreturn void exit(int c) { (void)c; __builtin_trap(); }
char *getenv(const char *n) { (void)n; return NULL; }
int system(const char *c) { return c ? -1 : 0; }
int abs(int x) { return x < 0 ? -x : x; }
void *calloc(size_t n, size_t s) {
  if (s && n > (size_t)-1 / s) return NULL;
  void *p = malloc(n * s);
  if (p) memset(p, 0, n * s);
  return p;
}

/* ---- stdio: no files; stdout and stderr go to the log ---- */
struct FILE { int fd; };
static FILE f_in = {0}, f_out = {1}, f_err = {2};
FILE *stdin = &f_in, *stdout = &f_out, *stderr = &f_err;
extern void vblua_write(int fd, const char *s, size_t n);
FILE *fopen(const char *p, const char *m) { (void)p; (void)m; errno = 2; return NULL; }
FILE *freopen(const char *p, const char *m, FILE *f) { (void)p; (void)m; (void)f; errno = 2; return NULL; }
int fclose(FILE *f) { (void)f; return 0; }
size_t fread(void *b, size_t s, size_t n, FILE *f) { (void)b; (void)s; (void)n; (void)f; return 0; }
size_t fwrite(const void *b, size_t s, size_t n, FILE *f) { vblua_write(f->fd, b, s * n); return n; }
int fflush(FILE *f) { (void)f; return 0; }
int fputs(const char *s, FILE *f) { vblua_write(f->fd, s, strlen(s)); return 0; }
int fputc(int c, FILE *f) { char ch = (char)c; vblua_write(f->fd, &ch, 1); return c; }
char *fgets(char *b, int n, FILE *f) { (void)b; (void)n; (void)f; return NULL; }
int getc(FILE *f) { (void)f; return EOF; }
int ungetc(int c, FILE *f) { (void)c; (void)f; return EOF; }
int ferror(FILE *f) { (void)f; return 0; }
int feof(FILE *f) { (void)f; return 1; }
void clearerr(FILE *f) { (void)f; }
int fseek(FILE *f, long o, int w) { (void)f; (void)o; (void)w; return -1; }
long ftell(FILE *f) { (void)f; return -1; }
int setvbuf(FILE *f, char *b, int m, size_t s) { (void)f; (void)b; (void)m; (void)s; return 0; }
FILE *tmpfile(void) { return NULL; }
char *tmpnam(char *s) { (void)s; return NULL; }
int remove(const char *p) { (void)p; errno = 2; return -1; }
int rename(const char *a, const char *b) { (void)a; (void)b; errno = 2; return -1; }

/* ---- printf: integers and strings here, floats in Rust (exact) ---- */
extern int vblua_fmt_float(char *out, int cap, int conv, int flags, int width, int prec, double v);
enum { F_MINUS = 1, F_PLUS = 2, F_SPACE = 4, F_HASH = 8, F_ZERO = 16 };

typedef struct { char *buf; size_t cap, len; } Out;
static void put(Out *o, char c) { if (o->len + 1 < o->cap) o->buf[o->len] = c; o->len++; }
static void put_n(Out *o, const char *s, size_t n) { while (n--) put(o, *s++); }
static void pad(Out *o, char c, int n) { while (n-- > 0) put(o, c); }

static void fmt_int(Out *o, unsigned long long u, int neg, int base, int upper, int flags,
                    int width, int prec) {
  char tmp[32];
  int n = 0;
  const char *dig = upper ? "0123456789ABCDEF" : "0123456789abcdef";
  if (!(u == 0 && prec == 0)) { do { tmp[n++] = dig[u % base]; u /= base; } while (u); }
  char sign = neg ? '-' : (flags & F_PLUS) ? '+' : (flags & F_SPACE) ? ' ' : 0;
  const char *pre = "";
  int pl = 0;
  if ((flags & F_HASH) && base == 16 && n) { pre = upper ? "0X" : "0x"; pl = 2; }
  if ((flags & F_HASH) && base == 8 && prec <= n) prec = n + 1;
  int digits = prec > n ? prec : n;
  int total = digits + (sign != 0) + pl;
  int zero = (flags & F_ZERO) && !(flags & F_MINUS) && prec < 0;
  if (!(flags & F_MINUS) && !zero) pad(o, ' ', width - total);
  if (sign) put(o, sign);
  put_n(o, pre, (size_t)pl);
  if (zero) pad(o, '0', width - total);
  pad(o, '0', digits - n);
  while (n) put(o, tmp[--n]);
  if (flags & F_MINUS) pad(o, ' ', width - total);
}

int vsnprintf(char *buf, size_t cap, const char *f, va_list ap) {
  Out o = { buf, cap, 0 };
  for (; *f; f++) {
    if (*f != '%') { put(&o, *f); continue; }
    f++;
    int flags = 0, width = 0, prec = -1, lng = 0;
    for (;; f++) {
      if (*f == '-') flags |= F_MINUS;
      else if (*f == '+') flags |= F_PLUS;
      else if (*f == ' ') flags |= F_SPACE;
      else if (*f == '#') flags |= F_HASH;
      else if (*f == '0') flags |= F_ZERO;
      else break;
    }
    if (*f == '*') {
      width = va_arg(ap, int);
      if (width < 0) { flags |= F_MINUS; width = -width; }
      f++;
    } else {
      while (isdigit(*f)) width = width * 10 + (*f++ - '0');
    }
    if (*f == '.') {
      f++;
      prec = 0;
      if (*f == '*') { prec = va_arg(ap, int); f++; }
      else while (isdigit(*f)) prec = prec * 10 + (*f++ - '0');
    }
    for (;; f++) {
      if (*f == 'l') lng++;
      else if (*f == 'h' || *f == 'L') {}
      else if (*f == 'z' || *f == 'j' || *f == 't') lng = 2;
      else break;
    }
    switch (*f) {
    case 'd': case 'i': {
      long long v = lng >= 2 ? va_arg(ap, long long) : lng == 1 ? va_arg(ap, long) : va_arg(ap, int);
      unsigned long long u = v < 0 ? 0ULL - (unsigned long long)v : (unsigned long long)v;
      fmt_int(&o, u, v < 0, 10, 0, flags, width, prec);
      break;
    }
    case 'u': case 'x': case 'X': case 'o': {
      unsigned long long u = lng >= 2 ? va_arg(ap, unsigned long long)
                           : lng == 1 ? va_arg(ap, unsigned long) : va_arg(ap, unsigned int);
      fmt_int(&o, u, 0, *f == 'o' ? 8 : *f == 'u' ? 10 : 16, *f == 'X',
              flags & ~(F_PLUS | F_SPACE), width, prec);
      break;
    }
    case 'p': fmt_int(&o, (uintptr_t)va_arg(ap, void *), 0, 16, 0, F_HASH, width, -1); break;
    case 'c': {
      char c = (char)va_arg(ap, int);
      if (!(flags & F_MINUS)) pad(&o, ' ', width - 1);
      put(&o, c);
      if (flags & F_MINUS) pad(&o, ' ', width - 1);
      break;
    }
    case 's': {
      const char *s = va_arg(ap, const char *);
      if (!s) s = "(null)";
      size_t n = 0;
      while (s[n] && (prec < 0 || n < (size_t)prec)) n++;
      if (!(flags & F_MINUS)) pad(&o, ' ', width - (int)n);
      put_n(&o, s, n);
      if (flags & F_MINUS) pad(&o, ' ', width - (int)n);
      break;
    }
    case 'f': case 'F': case 'e': case 'E': case 'g': case 'G': case 'a': case 'A': {
      double v = va_arg(ap, double);
      char tmp[512];
      int n = vblua_fmt_float(tmp, (int)sizeof tmp, *f, flags, width, prec, v);
      put_n(&o, tmp, (size_t)(n < (int)sizeof tmp ? n : (int)sizeof tmp));
      break;
    }
    case '%': put(&o, '%'); break;
    default:
      put(&o, '%');
      if (*f) put(&o, *f); else f--;
      break;
    }
  }
  if (cap) buf[o.len < cap ? o.len : cap - 1] = 0;
  return (int)o.len;
}
int snprintf(char *b, size_t n, const char *f, ...) {
  va_list ap;
  va_start(ap, f);
  int r = vsnprintf(b, n, f, ap);
  va_end(ap);
  return r;
}
int sprintf(char *b, const char *f, ...) {
  va_list ap;
  va_start(ap, f);
  int r = vsnprintf(b, (size_t)-1 >> 1, f, ap);
  va_end(ap);
  return r;
}
int fprintf(FILE *fp, const char *f, ...) {
  char tmp[1024];
  va_list ap;
  va_start(ap, f);
  int r = vsnprintf(tmp, sizeof tmp, f, ap);
  va_end(ap);
  vblua_write(fp->fd, tmp, (size_t)(r < (int)sizeof tmp ? r : (int)sizeof tmp - 1));
  return r;
}

/* ---- time: the clock and the time zone come from the browser ---- */
extern double vblua_now_ms(void);           /* wall clock, ms since the epoch */
extern double vblua_clock_ms(void);         /* ms since the page started */
extern int vblua_tz_offset_s(double t);     /* local minus UTC at t, in s */

time_t time(time_t *t) {
  time_t v = (time_t)(vblua_now_ms() / 1000.0);
  if (t) *t = v;
  return v;
}
clock_t clock(void) { return (clock_t)(vblua_clock_ms() * 1000.0); }
double difftime(time_t a, time_t b) { return (double)(a - b); }

static const int CUM[] = {0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334};

/* fields of t (seconds, UTC) */
static void civil(time_t t, struct tm *r) {
  long long days = t / 86400, rem = t % 86400;
  if (rem < 0) { rem += 86400; days--; }
  r->tm_hour = (int)(rem / 3600);
  r->tm_min = (int)(rem % 3600 / 60);
  r->tm_sec = (int)(rem % 60);
  r->tm_wday = (int)(((days + 4) % 7 + 7) % 7);
  long long z = days + 719468, era = (z >= 0 ? z : z - 146096) / 146097;
  unsigned doe = (unsigned)(z - era * 146097);
  unsigned yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
  long long y = (long long)yoe + era * 400;
  unsigned doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
  unsigned mp = (5 * doy + 2) / 153, d = doy - (153 * mp + 2) / 5 + 1;
  unsigned m = mp < 10 ? mp + 3 : mp - 9;
  y += m <= 2;
  int leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
  r->tm_year = (int)(y - 1900);
  r->tm_mon = (int)m - 1;
  r->tm_mday = (int)d;
  r->tm_yday = CUM[m - 1] + (int)d - 1 + (leap && m > 2);
  r->tm_isdst = 0;
}

/* seconds (UTC) of the fields of r, which may be out of range */
static time_t timegm_(const struct tm *r) {
  long long y = r->tm_year + 1900LL, mon = r->tm_mon;
  y += mon / 12;
  mon %= 12;
  if (mon < 0) { mon += 12; y--; }
  long long m = mon + 1;
  y -= m <= 2;
  long long era = (y >= 0 ? y : y - 399) / 400, yoe = y - era * 400;
  long long doy = (153 * (m + (m > 2 ? -3 : 9)) + 2) / 5 + r->tm_mday - 1;
  long long doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
  long long days = era * 146097 + doe - 719468;
  return days * 86400 + r->tm_hour * 3600LL + r->tm_min * 60LL + r->tm_sec;
}

static struct tm tm_buf;
struct tm *gmtime(const time_t *t) { civil(*t, &tm_buf); return &tm_buf; }
struct tm *localtime(const time_t *t) {
  civil(*t + vblua_tz_offset_s((double)*t), &tm_buf);
  return &tm_buf;
}
time_t mktime(struct tm *r) {
  time_t local = timegm_(r);
  time_t t = local - vblua_tz_offset_s((double)local);
  t = local - vblua_tz_offset_s((double)t); /* the offset at t itself */
  civil(t + vblua_tz_offset_s((double)t), r);
  return t;
}

static const char *const WDAY[] = {"Sunday", "Monday", "Tuesday", "Wednesday",
                                   "Thursday", "Friday", "Saturday"};
static const char *const MON[] = {"January", "February", "March", "April", "May", "June", "July",
                                  "August", "September", "October", "November", "December"};

static int leap_year(int y) { return (y % 4 == 0 && y % 100 != 0) || y % 400 == 0; }
/* ISO weeks in a year whose 1 January falls on `jan1` (Monday = 0) */
static int iso_weeks(int jan1, int leap) { return jan1 == 3 || (leap && jan1 == 2) ? 53 : 52; }

/* the "C" locale's conversions (os.date checks them before calling) */
size_t strftime(char *s, size_t n, const char *f, const struct tm *t) {
  Out o = { s, n, 0 };
  char tmp[64];
  for (; *f; f++) {
    if (*f != '%') { put(&o, *f); continue; }
    f++;
    if (*f == 'E' || *f == 'O') f++;
    int h12 = t->tm_hour % 12 ? t->tm_hour % 12 : 12;
    tmp[0] = 0;
    switch (*f) {
    case 'a': snprintf(tmp, sizeof tmp, "%.3s", WDAY[t->tm_wday % 7]); break;
    case 'A': snprintf(tmp, sizeof tmp, "%s", WDAY[t->tm_wday % 7]); break;
    case 'b': case 'h': snprintf(tmp, sizeof tmp, "%.3s", MON[t->tm_mon % 12]); break;
    case 'B': snprintf(tmp, sizeof tmp, "%s", MON[t->tm_mon % 12]); break;
    case 'c':
      snprintf(tmp, sizeof tmp, "%.3s %.3s %2d %02d:%02d:%02d %d", WDAY[t->tm_wday % 7],
               MON[t->tm_mon % 12], t->tm_mday, t->tm_hour, t->tm_min, t->tm_sec, t->tm_year + 1900);
      break;
    case 'C': snprintf(tmp, sizeof tmp, "%02d", (t->tm_year + 1900) / 100); break;
    case 'd': snprintf(tmp, sizeof tmp, "%02d", t->tm_mday); break;
    case 'D': case 'x':
      snprintf(tmp, sizeof tmp, "%02d/%02d/%02d", t->tm_mon + 1, t->tm_mday, t->tm_year % 100);
      break;
    case 'e': snprintf(tmp, sizeof tmp, "%2d", t->tm_mday); break;
    case 'F': snprintf(tmp, sizeof tmp, "%d-%02d-%02d", t->tm_year + 1900, t->tm_mon + 1, t->tm_mday); break;
    case 'g': case 'G': case 'V': case 'U': case 'W': {
      /* weeks: Sunday- / Monday-based, and ISO 8601 */
      int wd = t->tm_wday, yd = t->tm_yday;
      if (*f == 'U') { snprintf(tmp, sizeof tmp, "%02d", (yd + 7 - wd) / 7); break; }
      if (*f == 'W') { snprintf(tmp, sizeof tmp, "%02d", (yd + 7 - (wd + 6) % 7) / 7); break; }
      int y = t->tm_year + 1900, iso = (wd + 6) % 7; /* Monday = 0 */
      int w = (yd - iso + 10) / 7;
      int jan1 = ((iso - yd) % 7 + 7) % 7;
      if (w < 1) {
        y--;
        int prev = ((jan1 - 365 - leap_year(y)) % 7 + 7) % 7;
        w = iso_weeks(prev, leap_year(y));
      } else if (w > iso_weeks(jan1, leap_year(y))) {
        w = 1;
        y++;
      }
      if (*f == 'V') snprintf(tmp, sizeof tmp, "%02d", w);
      else if (*f == 'G') snprintf(tmp, sizeof tmp, "%d", y);
      else snprintf(tmp, sizeof tmp, "%02d", y % 100);
      break;
    }
    case 'H': snprintf(tmp, sizeof tmp, "%02d", t->tm_hour); break;
    case 'I': snprintf(tmp, sizeof tmp, "%02d", h12); break;
    case 'j': snprintf(tmp, sizeof tmp, "%03d", t->tm_yday + 1); break;
    case 'm': snprintf(tmp, sizeof tmp, "%02d", t->tm_mon + 1); break;
    case 'M': snprintf(tmp, sizeof tmp, "%02d", t->tm_min); break;
    case 'n': snprintf(tmp, sizeof tmp, "\n"); break;
    case 'p': snprintf(tmp, sizeof tmp, "%s", t->tm_hour < 12 ? "AM" : "PM"); break;
    case 'r': snprintf(tmp, sizeof tmp, "%02d:%02d:%02d %s", h12, t->tm_min, t->tm_sec, t->tm_hour < 12 ? "AM" : "PM"); break;
    case 'R': snprintf(tmp, sizeof tmp, "%02d:%02d", t->tm_hour, t->tm_min); break;
    case 'S': snprintf(tmp, sizeof tmp, "%02d", t->tm_sec); break;
    case 't': snprintf(tmp, sizeof tmp, "\t"); break;
    case 'T': case 'X': snprintf(tmp, sizeof tmp, "%02d:%02d:%02d", t->tm_hour, t->tm_min, t->tm_sec); break;
    case 'u': snprintf(tmp, sizeof tmp, "%d", t->tm_wday ? t->tm_wday : 7); break;
    case 'w': snprintf(tmp, sizeof tmp, "%d", t->tm_wday); break;
    case 'y': snprintf(tmp, sizeof tmp, "%02d", t->tm_year % 100); break;
    case 'Y': snprintf(tmp, sizeof tmp, "%d", t->tm_year + 1900); break;
    case 'z': case 'Z': break;
    case '%': snprintf(tmp, sizeof tmp, "%%"); break;
    default: if (!*f) f--; break;
    }
    put_n(&o, tmp, strlen(tmp));
  }
  if (o.len >= n) return 0;
  s[o.len] = 0;
  return o.len;
}

/* ---- setjmp / longjmp: the runtime LLVM's Wasm SjLj lowering
   (-mllvm -wasm-enable-sjlj) calls, after emscripten's and wasi-libc's.
   LLVM 18 and older call saveSetjmp / testSetjmp / getTempRet0, newer
   ones __wasm_setjmp / __wasm_setjmp_test; both throw with
   __wasm_longjmp. ---- */
typedef struct { uint32_t id, label; } SjEntry;
static uint32_t sj_id, temp_ret0;

SjEntry *saveSetjmp(uint32_t *env, uint32_t label, SjEntry *table, uint32_t size) {
  *env = ++sj_id;
  for (;;) {
    for (uint32_t i = 0; i < size; i++) {
      if (table[i].id == 0) {
        table[i].id = sj_id;
        table[i].label = label;
        table[i + 1].id = 0;
        temp_ret0 = size;
        return table;
      }
    }
    size *= 2;
    table = realloc(table, sizeof(SjEntry) * (size + 1));
    if (!table) __builtin_trap();
  }
}
uint32_t testSetjmp(uint32_t id, SjEntry *table, uint32_t size) {
  for (uint32_t i = 0; i < size && table[i].id; i++)
    if (table[i].id == id) return table[i].label;
  return 0;
}
uint32_t getTempRet0(void) { return temp_ret0; }
void setTempRet0(uint32_t v) { temp_ret0 = v; }

struct jmp_buf_impl { void *func_invocation_id; uint32_t label; };
void __wasm_setjmp(void *env, uint32_t label, void *func_invocation_id) {
  struct jmp_buf_impl *jb = env;
  jb->func_invocation_id = func_invocation_id;
  jb->label = label;
}
uint32_t __wasm_setjmp_test(void *env, void *func_invocation_id) {
  struct jmp_buf_impl *jb = env;
  return jb->func_invocation_id == func_invocation_id ? jb->label : 0;
}
static struct { void *env; int val; } lj_arg;
_Noreturn void __wasm_longjmp(void *env, int val) {
  lj_arg.env = env;
  lj_arg.val = val ? val : 1;
  __builtin_wasm_throw(1, &lj_arg);
}
