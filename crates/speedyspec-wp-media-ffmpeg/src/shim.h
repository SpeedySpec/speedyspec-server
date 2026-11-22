/*
 * The one call the runtime makes into FFmpeg: describe the file open on `fd`, within limits, as plain structs this
 * shim owns. Nothing FFmpeg allocated escapes; ss_free releases everything ss_probe returned.
 */
#ifndef SPEEDYSPEC_MEDIA_SHIM_H
#define SPEEDYSPEC_MEDIA_SHIM_H

#include <stdint.h>

enum {
    SS_OK = 0,
    SS_UNRECOGNIZED = 1,
    SS_TRUNCATED = 2,
    SS_LIMIT_BYTES = 3,
    SS_LIMIT_STREAMS = 4,
    SS_TIMEOUT = 5,
    SS_IO = 6,
    SS_NOMEM = 7,
};

typedef struct {
    int64_t max_bytes;
    int64_t deadline_ms;
    int32_t max_streams;
    int64_t max_picture_bytes;
    int64_t max_tag_bytes;
} ss_limits;

typedef struct {
    char *key;
    char *value;
} ss_tag;

typedef struct {
    int32_t media_type; /* 0 video, 1 audio, 2 data, 3 subtitle, 4 attachment, -1 unknown */
    char *codec_name;
    char *profile;
    int32_t width, height, sample_rate, channels;
    char *channel_layout;
    int32_t bits_per_raw_sample, bits_per_coded_sample;
    int64_t bit_rate;
    int32_t fr_num, fr_den;
    char *pix_fmt;
    char *sample_fmt;
    int32_t attached_pic;
    uint8_t *picture;
    int64_t picture_size;
    ss_tag *tags;
    int32_t tag_count;
} ss_stream;

typedef struct {
    char *format_name;
    int64_t duration_us; /* INT64_MIN when unknown */
    int64_t bit_rate;
    int64_t bytes_read;
    ss_tag *tags;
    int32_t tag_count;
    ss_stream *streams;
    int32_t stream_count;
} ss_result;

void ss_global_init(int64_t max_alloc);
int32_t ss_probe(int fd, const ss_limits *limits, ss_result **out);
void ss_free(ss_result *result);

#endif
