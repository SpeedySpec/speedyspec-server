/* PCRE2's build settings, matching the static PHP's bundled PCRE2 (php-src ext/pcre/pcre2lib/config.h)
   without its PHP includes. build.rs defines SUPPORT_UNICODE, SUPPORT_PCRE2_8 and SUPPORT_JIT. */

#ifndef HEAP_LIMIT
#define HEAP_LIMIT 20000000
#endif

#ifndef PARENS_NEST_LIMIT
#define PARENS_NEST_LIMIT 250
#endif

#ifndef MATCH_LIMIT
#define MATCH_LIMIT 10000000
#endif

#ifndef MATCH_LIMIT_DEPTH
#define MATCH_LIMIT_DEPTH MATCH_LIMIT
#endif

#ifndef MAX_NAME_COUNT
#define MAX_NAME_COUNT 10000
#endif

#ifndef MAX_NAME_SIZE
#define MAX_NAME_SIZE 128
#endif

/* LF. */
#ifndef NEWLINE_DEFAULT
#define NEWLINE_DEFAULT 2
#endif

#ifndef LINK_SIZE
#define LINK_SIZE 2
#endif

#ifndef MAX_VARLOOKBEHIND
#define MAX_VARLOOKBEHIND 255
#endif

#define PCRE2_EXPORT
