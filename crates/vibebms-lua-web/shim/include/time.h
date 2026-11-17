#pragma once
#include <stddef.h>
typedef long long time_t;
typedef long clock_t;
#define CLOCKS_PER_SEC 1000000L
struct tm { int tm_sec, tm_min, tm_hour, tm_mday, tm_mon, tm_year, tm_wday, tm_yday, tm_isdst; };
time_t time(time_t *); clock_t clock(void); double difftime(time_t, time_t); time_t mktime(struct tm *);
struct tm *localtime(const time_t *); struct tm *gmtime(const time_t *);
size_t strftime(char *, size_t, const char *, const struct tm *);
